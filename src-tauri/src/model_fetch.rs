// Fetches optional, per-machine model checkpoints (Tamil transcription,
// OpenVoice cloning) that aren't bundled into the installer, straight from
// this repo's GitHub Release -- so an installed app can offer a one-click
// "download these now" instead of a manual conda/curl dance.
//
// Ships a much smaller, dedicated archive (`optional-models.tar.gz`,
// ~360MB) than the developer-facing `npm run fetch-resources` one
// (`dev-resources.tar.gz`, ~1.1GB, see scripts/fetch-dev-resources.mjs) --
// that larger one also includes ffmpeg/llama.cpp/Piper, which are already
// bundled into every install via tauri.conf.json's `bundle.resources`, so
// re-fetching them here would be pure waste for an end user.
//
// This deliberately does NOT set up the `stt`/`tts`/`media-ai`/
// `voice-clone` conda environments themselves -- those need real Python
// packages installed (torch, transformers, faster-whisper, openvoice...),
// a meaningfully bigger scope (bundling a Python distribution) this
// project has kept out of scope so far. This only closes the "the model
// *file* isn't there" gap; the conda setup is still a documented manual
// step (README sections 2, 2.4, 2.6, 2.7).
//
// Extraction shells out to the system `tar` (bundled with Windows 10+,
// macOS, and every mainstream Linux distro) rather than adding a
// tar/gzip-decoding Rust dependency just for this one archive -- same
// reasoning as scripts/fetch-dev-resources.mjs's own doc comment.

use serde::Serialize;
use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;
use tauri::AppHandle;
use tokio::process::Command;

use crate::util::{cli_path, emit_progress, unique_temp_path};

const RELEASE_ASSET_URL: &str =
    "https://github.com/antorobin/reels-caption-app/releases/latest/download/optional-models.tar.gz";

/// Where the fetched checkpoints land -- the exact same per-machine
/// fallback directory `stt::indic_model_dir` and
/// `voice_clone::voice_clone_checkpoint_dir` already check as their last
/// resort, so nothing downstream needs to know this download ever
/// happened.
fn per_machine_models_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default();
    PathBuf::from(home).join(".reels-caption-app")
}

#[derive(Serialize)]
pub struct OptionalModelStatus {
    pub tamil_transcription: bool,
    pub voice_cloning: bool,
}

impl OptionalModelStatus {
    fn any_missing(&self) -> bool {
        !self.tamil_transcription || !self.voice_cloning
    }
}

/// Checked on app load (see App.jsx) to decide whether to show the
/// "download optional models" prompt at all -- most installs that already
/// ran `npm run fetch-resources` (developers) or a previous download
/// (returning users) should see nothing.
#[tauri::command]
pub fn check_optional_models() -> OptionalModelStatus {
    let base = per_machine_models_dir();
    OptionalModelStatus {
        tamil_transcription: base.join("stt-models").join("ta").join("model.bin").exists(),
        voice_cloning: base.join("voice-clone-models").join("converter").join("checkpoint.pth").exists(),
    }
}

#[tauri::command]
pub fn optional_models_missing() -> bool {
    check_optional_models().any_missing()
}

#[tauri::command]
pub async fn download_optional_models(app: AppHandle) -> Result<(), String> {
    let archive_path = unique_temp_path("optional-models", "tar.gz");

    // This is a one-time, app-wide optional-model download -- never
    // per-project (nothing about it is scoped to any one project), so
    // there's no real id to pass here, just the empty-string placeholder
    // `emit_progress`'s signature now requires everywhere else.
    emit_progress(&app, "optional-models-progress", "", "downloading", Some(0.0), None);

    let response = reqwest::get(RELEASE_ASSET_URL).await.map_err(|e| format!("Failed to start download: {e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Download failed: HTTP {} — the release asset may be missing or renamed.",
            response.status()
        ));
    }
    let total = response.content_length().unwrap_or(0);

    let mut response = response;
    let mut file = std::fs::File::create(&archive_path).map_err(|e| format!("Couldn't create temp file: {e}"))?;
    let mut received: u64 = 0;
    let mut last_reported_percent = -1.0;

    while let Some(chunk) = response.chunk().await.map_err(|e| format!("Download interrupted: {e}"))? {
        file.write_all(&chunk).map_err(|e| format!("Couldn't write download to disk: {e}"))?;
        received += chunk.len() as u64;
        if total > 0 {
            let percent = (received as f64 / total as f64 * 100.0).clamp(0.0, 100.0);
            // Only emit on whole-percent steps -- this loop runs once per
            // network chunk (potentially thousands of times for a 360MB
            // download), and the frontend only needs coarse updates.
            if percent - last_reported_percent >= 1.0 {
                last_reported_percent = percent;
                emit_progress(&app, "optional-models-progress", "", "downloading", Some(percent), None);
            }
        }
    }
    drop(file);

    emit_progress(&app, "optional-models-progress", "", "extracting", Some(0.0), None);

    let dest = per_machine_models_dir();
    std::fs::create_dir_all(&dest).map_err(|e| format!("Couldn't create {}: {e}", dest.display()))?;

    let output = Command::new("tar")
        .args(["-xzf".to_string(), cli_path(&archive_path), "-C".to_string(), cli_path(&dest)])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| format!("Failed to run tar (expected to be preinstalled on this OS): {e}"))?;

    let _ = std::fs::remove_file(&archive_path);

    if !output.status.success() {
        return Err(format!("Extraction failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    emit_progress(&app, "optional-models-progress", "", "done", Some(100.0), None);
    Ok(())
}
