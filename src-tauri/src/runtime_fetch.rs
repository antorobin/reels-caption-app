// Runtime component manager: fetches the pieces the *slim* installer
// leaves out -- a minimal ffmpeg, the Python envs, the local LLM -- into
// `~/.reels-caption-app/runtime/`, from GitHub Release assets (the same
// hosting `model_fetch.rs` already uses for optional model files).
//
// Every resolver in this crate (`bin_paths.rs`, `python_env.rs`,
// `llm.rs`, `tts.rs`) checks `~/.reels-caption-app/runtime/<unpack_to>/`
// alongside its bundled-resource path, so a *downloaded* component is
// found exactly like a *bundled* one. The only difference between the
// slim and full installer is what's already present on first launch --
// not the code path a feature takes to find it.
//
// Manifest: `components.json`, compiled in via `include_str!` as the
// baseline, overridden at runtime by a hosted copy (so component
// URLs/versions can move without an app release). Downloads are
// SHA-256-gated and resumable (HTTP Range); each installed component
// leaves a `runtime/.markers/<id>-<version>` file so a re-launch knows
// what's already there.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::process::Stdio;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::AppHandle;
use tokio::process::Command;

use crate::util::{cli_path, emit_progress};

const EMBEDDED_MANIFEST: &str = include_str!("../components.json");

/// Set to `1` to let an unpublished component (empty `sha256` in the
/// manifest) install without verification -- local testing only.
const ALLOW_UNVERIFIED_ENV: &str = "REELS_CAPTION_APP_ALLOW_UNVERIFIED_COMPONENTS";

/// `windows-x64` | `macos-arm64` | `macos-x64` | `linux-x64` for the host
/// this build is running on. The manifest carries one `Component` entry
/// per (id, platform); everything below filters to this.
pub fn current_platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", _) => "windows-x64",
        ("macos", "aarch64") => "macos-arm64",
        ("macos", _) => "macos-x64",
        ("linux", _) => "linux-x64",
        _ => "unknown",
    }
}

fn default_platform() -> String {
    "windows-x64".to_string()
}

#[derive(Deserialize, Clone)]
pub struct Component {
    pub id: String,
    pub tier: String, // "core" | "on-demand"
    /// Which host this entry is for. Defaulted for back-compat with a
    /// manifest that predates multi-platform packs.
    #[serde(default = "default_platform")]
    pub platform: String,
    pub version: String,
    pub url: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub size: u64,
    /// Relative to `runtime/`. The archive's contents extract directly
    /// into this dir; archives may carry leaf dirs (`python-voice` carries
    /// `tts/ media-ai/ voice-clone/`).
    pub unpack_to: String,
    #[serde(default)]
    pub needed_for: Vec<String>,
}

#[derive(Deserialize)]
pub struct Manifest {
    #[allow(dead_code)]
    pub schema: u32,
    #[serde(default)]
    pub manifest_url: String,
    pub components: Vec<Component>,
}

#[derive(Serialize)]
pub struct ComponentStatus {
    pub id: String,
    pub tier: String,
    pub version: String,
    pub size: u64,
    pub installed: bool,
    pub needed_for: Vec<String>,
}

fn reels_caption_app_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default();
    PathBuf::from(home).join(".reels-caption-app")
}

/// The root every resolver checks for downloaded runtime pieces.
pub fn runtime_dir() -> PathBuf {
    reels_caption_app_dir().join("runtime")
}

fn markers_dir() -> PathBuf {
    runtime_dir().join(".markers")
}

fn marker_path(c: &Component) -> PathBuf {
    markers_dir().join(format!("{}-{}", c.id, c.version))
}

/// True once `c` (this exact version) has been fully installed.
pub fn is_installed(c: &Component) -> bool {
    marker_path(c).exists()
}

fn embedded_manifest() -> Manifest {
    serde_json::from_str(EMBEDDED_MANIFEST).expect("compiled-in components.json is valid")
}

/// The hosted manifest if reachable and parseable, else the compiled-in
/// baseline. Never fails -- a missing/broken hosted copy just falls back.
async fn load_manifest() -> Manifest {
    let base = embedded_manifest();
    if base.manifest_url.is_empty() {
        return base;
    }
    match reqwest::get(&base.manifest_url).await {
        Ok(resp) if resp.status().is_success() => match resp.text().await {
            Ok(body) => serde_json::from_str(&body).unwrap_or(base),
            Err(_) => base,
        },
        _ => base,
    }
}

fn to_status(c: &Component) -> ComponentStatus {
    ComponentStatus {
        id: c.id.clone(),
        tier: c.tier.clone(),
        version: c.version.clone(),
        size: c.size,
        installed: is_installed(c),
        needed_for: c.needed_for.clone(),
    }
}

/// Every component for this host and whether it's installed -- drives the
/// Settings "Manage components" pane and the per-feature download gates.
#[tauri::command]
pub async fn list_runtime_components() -> Vec<ComponentStatus> {
    let here = current_platform();
    load_manifest().await.components.iter().filter(|c| c.platform == here).map(to_status).collect()
}

/// The `tier: "core"` components for this host not yet installed -- what
/// the first-run setup screen downloads before the editor opens. Empty
/// vec => ready (also the case on a platform with no packs published
/// yet, which then behaves like the pre-pack slim build).
#[tauri::command]
pub async fn missing_core_components() -> Vec<ComponentStatus> {
    let here = current_platform();
    load_manifest()
        .await
        .components
        .iter()
        .filter(|c| c.platform == here && c.tier == "core" && !is_installed(c))
        .map(to_status)
        .collect()
}

fn stable_part_path(id: &str) -> PathBuf {
    std::env::temp_dir().join(format!("reels-caption-app-component-{id}.part"))
}

/// Downloads and installs one component by id. Resumable and
/// SHA-256-verified; emits `runtime-component-progress` events carrying
/// the component id as the `project_id` slot so the UI can route them.
#[tauri::command]
pub async fn download_runtime_component(app: AppHandle, id: String) -> Result<(), String> {
    let here = current_platform();
    let manifest = load_manifest().await;
    let component = manifest
        .components
        .iter()
        .find(|c| c.id == id && c.platform == here)
        .ok_or_else(|| format!("No '{id}' runtime component for this platform ({here})"))?
        .clone();

    if is_installed(&component) {
        return Ok(());
    }

    let allow_unverified = std::env::var(ALLOW_UNVERIFIED_ENV).map(|v| v == "1").unwrap_or(false);
    if component.sha256.is_empty() && !allow_unverified {
        return Err(format!(
            "Component '{id}' has no published checksum yet. It can't be installed until the release pipeline builds it."
        ));
    }

    emit_progress(&app, "runtime-component-progress", &id, "downloading", Some(0.0), None);

    let part_path = stable_part_path(&id);
    let mut resume_from: u64 = std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);

    let client = reqwest::Client::new();
    let mut request = client.get(&component.url);
    if resume_from > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={resume_from}-"));
    }
    let response = request.send().await.map_err(|e| format!("Failed to start download: {e}"))?;

    // 206 => server honoured the Range and we append; anything else 2xx =>
    // it sent the whole file, so start over.
    let resuming = response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    if !resuming {
        resume_from = 0;
        let _ = std::fs::remove_file(&part_path);
    }
    if !response.status().is_success() {
        return Err(format!(
            "Download failed: HTTP {} for {} — the release asset may be missing or renamed.",
            response.status(),
            component.url
        ));
    }

    let total = response.content_length().unwrap_or(0) + resume_from;

    let mut hasher = Sha256::new();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(resuming)
        .write(true)
        .open(&part_path)
        .map_err(|e| format!("Couldn't open {}: {e}", part_path.display()))?;

    if resuming {
        // Re-hash what's already on disk so the final digest covers the
        // whole file.
        file.seek(SeekFrom::Start(0)).map_err(|e| format!("seek failed: {e}"))?;
        let mut buf = vec![0u8; 1 << 20];
        let mut left = resume_from;
        while left > 0 {
            let want = left.min(buf.len() as u64) as usize;
            let read = file.read(&mut buf[..want]).map_err(|e| format!("re-hash read failed: {e}"))?;
            if read == 0 {
                break;
            }
            hasher.update(&buf[..read]);
            left -= read as u64;
        }
        file.seek(SeekFrom::End(0)).map_err(|e| format!("seek failed: {e}"))?;
    }

    let mut response = response;
    let mut received = resume_from;
    let mut last_percent = -1.0;
    while let Some(chunk) = response.chunk().await.map_err(|e| format!("Download interrupted: {e}"))? {
        file.write_all(&chunk).map_err(|e| format!("Couldn't write to disk: {e}"))?;
        hasher.update(&chunk);
        received += chunk.len() as u64;
        if total > 0 {
            let percent = (received as f64 / total as f64 * 100.0).clamp(0.0, 100.0);
            if percent - last_percent >= 1.0 {
                last_percent = percent;
                emit_progress(&app, "runtime-component-progress", &id, "downloading", Some(percent), None);
            }
        }
    }
    drop(file);

    if !component.sha256.is_empty() {
        let digest = format!("{:x}", hasher.finalize());
        if !digest.eq_ignore_ascii_case(&component.sha256) {
            let _ = std::fs::remove_file(&part_path);
            return Err(format!(
                "Checksum mismatch for '{id}' — expected {}, got {digest}. Download discarded.",
                component.sha256
            ));
        }
    }

    emit_progress(&app, "runtime-component-progress", &id, "extracting", Some(0.0), None);

    let dest = runtime_dir().join(&component.unpack_to);
    std::fs::create_dir_all(&dest).map_err(|e| format!("Couldn't create {}: {e}", dest.display()))?;

    // bsdtar (Windows 10+) and GNU tar both auto-detect gzip/zstd here.
    let output = Command::new("tar")
        .args(["-xf".to_string(), cli_path(&part_path), "-C".to_string(), cli_path(&dest)])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("Failed to run tar (expected preinstalled on this OS): {e}"))?;

    if !output.status.success() {
        return Err(format!("Extraction failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    let _ = std::fs::remove_file(&part_path);
    std::fs::create_dir_all(markers_dir()).map_err(|e| format!("Couldn't create markers dir: {e}"))?;
    std::fs::write(marker_path(&component), component.version.as_bytes())
        .map_err(|e| format!("Couldn't write install marker: {e}"))?;

    emit_progress(&app, "runtime-component-progress", &id, "done", Some(100.0), None);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_manifest_parses_and_has_the_core_pair() {
        let m = embedded_manifest();
        assert_eq!(m.schema, 1);
        let ids: Vec<&str> = m.components.iter().map(|c| c.id.as_str()).collect();
        assert!(ids.contains(&"ffmpeg"));
        assert!(ids.contains(&"python-stt"));
        let core: Vec<&str> =
            m.components.iter().filter(|c| c.tier == "core").map(|c| c.id.as_str()).collect();
        assert_eq!(core, vec!["ffmpeg", "python-stt"]);
    }

    #[test]
    fn every_component_has_a_sane_unpack_target() {
        for c in embedded_manifest().components {
            assert!(!c.unpack_to.is_empty(), "{} has empty unpack_to", c.id);
            assert!(!c.unpack_to.starts_with('/') && !c.unpack_to.contains(".."), "{} unpack_to escapes runtime/", c.id);
            assert!(c.url.starts_with("https://"), "{} url must be https", c.id);
        }
    }
}
