// Shared plumbing for the "media-ai" conda env (mediapipe + librosa +
// scipy + numpy — deliberately no PyTorch/pyannote/scikit-learn, unlike
// the deleted vision.rs feature) used by voice-over sync, prosody
// analysis, and speaker diarization.
//
// Unlike vision.rs's persistent worker process (justified there by a
// multi-second-to-minutes model load worth amortizing across calls),
// every media-ai script here is a fast, one-shot CPU operation — a
// fresh `conda run` subprocess per call is simple and fast enough.
//
// Setup (not bundled — see README): a conda environment named "media-ai":
//   conda create -n media-ai python=3.11 -y
//   conda run -n media-ai pip install -qU mediapipe librosa scipy numpy

use std::path::PathBuf;
use tauri::AppHandle;
use tokio::process::Command;

use crate::util::{cli_path, emit_progress};

pub const MEDIA_AI_ENV_NAME: &str = "media-ai";

pub async fn resolve_media_ai_conda() -> Result<String, String> {
    crate::conda_util::resolve_conda_env(
        MEDIA_AI_ENV_NAME,
        // `mp.solutions` is an attribute set at package-init time, not a
        // real importable submodule — `import mediapipe.solutions.x` fails
        // even when `mp.solutions.x` works fine as attribute access.
        // Confirmed empirically against the pinned mediapipe==0.10.9 build.
        &["python", "-c", "import mediapipe as mp; mp.solutions.face_mesh; import librosa, scipy, numpy"],
        "REELS_CAPTION_APP_MEDIA_AI_CONDA_PATH",
    )
    .await
}

/// Resolves a script under `src-tauri/media_ai/` at compile time — a
/// plain source-file path, not a bundled resource (dev-machine-only for
/// now, same scope cut the deleted vision feature made for its own
/// worker script).
pub fn media_ai_script_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("media_ai").join(name)
}

/// Runs a media-ai Python script as a one-shot subprocess and returns its
/// trimmed stdout (expected to be a single JSON line — each script's own
/// caller parses it into whatever shape it needs). Emits a simple
/// start/done progress pair rather than fine-grained percent — these
/// scripts finish in seconds, so a percent-parsing protocol isn't worth
/// building (unlike the STT engine/ffmpeg, which can run for minutes).
pub async fn run_media_ai_script(
    app: &AppHandle,
    script: &str,
    args: Vec<String>,
    event_name: &str,
    stage: &str,
) -> Result<String, String> {
    let conda = resolve_media_ai_conda().await?;
    let script_path_arg = cli_path(&media_ai_script_path(script));

    emit_progress(app, event_name, stage, None, Some(format!("Running {script}…")));

    let mut full_args =
        vec!["run".to_string(), "-n".to_string(), MEDIA_AI_ENV_NAME.to_string(), "python".to_string(), script_path_arg];
    full_args.extend(args);

    let output = Command::new(&conda)
        .args(&full_args)
        .env("PYTHONIOENCODING", "utf-8")
        .output()
        .await
        .map_err(|e| format!("Failed to run {script} via conda: {e}"))?;

    if !output.status.success() {
        return Err(format!("{script} failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    emit_progress(app, event_name, stage, Some(100.0), None);

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
