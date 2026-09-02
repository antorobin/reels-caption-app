// Loudness normalization — pure ffmpeg, no Python dependency. Two-pass
// `loudnorm`: a measure pass (parses the JSON block ffmpeg prints to
// stderr after "Parsed_loudnorm") followed by an apply pass fed the
// measured values (`linear=true`), the standard documented ffmpeg
// loudnorm workflow for accurate results (a single-pass loudnorm only
// approximates).

use serde::Deserialize;
use tauri::AppHandle;
use tokio::process::Command;

use crate::ffmpeg::{probe_duration_seconds, run_with_progress};

/// -14 LUFS is the widely-used target for social/short-form platforms
/// (Spotify/YouTube/Instagram/TikTok-class) — distinct from broadcast
/// standards (-16 to -24) used in other contexts.
const DEFAULT_TARGET_LUFS: f64 = -14.0;
const TRUE_PEAK_LIMIT: f64 = -1.5;
const LOUDNESS_RANGE: f64 = 11.0;

#[derive(Debug, Deserialize)]
struct LoudnormMeasurement {
    input_i: String,
    input_tp: String,
    input_lra: String,
    input_thresh: String,
    target_offset: String,
}

async fn measure_loudness(input_path: &str, target_lufs: f64) -> Result<LoudnormMeasurement, String> {
    let filter = format!("loudnorm=I={target_lufs}:TP={TRUE_PEAK_LIMIT}:LRA={LOUDNESS_RANGE}:print_format=json");
    let output = Command::new(crate::bin_paths::ffmpeg_path())
        .args(["-i", input_path, "-af", &filter, "-f", "null", "-"])
        .output()
        .await
        .map_err(|e| format!("Failed to run ffmpeg loudnorm measure pass: {e}"))?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    let start = stderr.rfind('{');
    let end = stderr.rfind('}');
    let (Some(start), Some(end)) = (start, end) else {
        return Err("Couldn't find loudnorm measurement JSON in ffmpeg output".to_string());
    };
    if end < start {
        return Err("Couldn't find loudnorm measurement JSON in ffmpeg output".to_string());
    }

    serde_json::from_str(&stderr[start..=end]).map_err(|e| format!("Couldn't parse loudnorm measurement: {e}"))
}

pub async fn normalize_loudness(app: &AppHandle, input_path: &str, output_path: &str, target_lufs: f64) -> Result<(), String> {
    let measured = measure_loudness(input_path, target_lufs).await?;

    let filter = format!(
        "loudnorm=I={target_lufs}:TP={TRUE_PEAK_LIMIT}:LRA={LOUDNESS_RANGE}:measured_I={}:measured_TP={}:measured_LRA={}:measured_thresh={}:offset={}:linear=true",
        measured.input_i, measured.input_tp, measured.input_lra, measured.input_thresh, measured.target_offset
    );

    let duration = probe_duration_seconds(input_path).await;
    let args = vec![
        "-y".to_string(),
        "-i".to_string(),
        input_path.to_string(),
        "-af".to_string(),
        filter,
        "-c:v".to_string(),
        "copy".to_string(),
        output_path.to_string(),
    ];

    // `input_path` doubles as the scoping key here, not a real project id --
    // LoudnessPanel.jsx is a self-contained, foreground-only tool with no
    // background routing (confirmed: no App.jsx callback, writes to a
    // user-chosen save() path), so there's no real concurrent-listener
    // collision this needs to guard against the way burn/duck/jump-cut do.
    run_with_progress(app, "loudness-progress", input_path, "normalizing_loudness", args, duration).await
}

#[tauri::command]
pub async fn normalize_audio(app: AppHandle, video_path: String, output_path: String, target_lufs: Option<f64>) -> Result<String, String> {
    let _permit = crate::concurrency::acquire_encode().await;
    normalize_loudness(&app, &video_path, &output_path, target_lufs.unwrap_or(DEFAULT_TARGET_LUFS)).await?;
    Ok(output_path)
}
