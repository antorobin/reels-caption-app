// Voice-over <-> mouth-movement cadence sync — see media_ai/vo_sync.py
// for the algorithm. `sync_voice_over` is the standalone-tool path: output
// is a muxed video file the user can separately load if they want captions
// on it. `compute_voiceover_offset` is the same underlying offset
// calculation without the mux step, used by VoiceoverPanel.jsx for live
// in-browser preview sync -- playing the generated voiceover alongside the
// original video file directly (see VideoPreview.jsx), rather than writing
// a new merged file just to preview it.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::media_ai::run_media_ai_script;
use crate::util::cli_path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoSyncResult {
    pub output_path: String,
    pub offset_seconds: f64,
}

#[derive(Debug, Deserialize)]
struct VoSyncScriptOutput {
    offset_seconds: f64,
    output_path: String,
}

#[derive(Debug, Deserialize)]
struct OffsetOnlyOutput {
    offset_seconds: f64,
}

#[tauri::command]
pub async fn compute_voiceover_offset(app: AppHandle, video_path: String, voiceover_path: String) -> Result<f64, String> {
    // `video_path` doubles as the scoping key -- this is a self-contained,
    // foreground-only tool (VoiceoverSection.jsx's live-preview sync, no
    // background routing), so there's no real concurrent-listener
    // collision to guard against, just the new required parameter to
    // satisfy (see media_ai.rs's own doc comment).
    let stdout = run_media_ai_script(
        &app,
        "vo_sync.py",
        vec!["--video".to_string(), video_path.clone(), "--voiceover".to_string(), voiceover_path, "--offset-only".to_string()],
        "vosync-progress",
        &video_path,
        "syncing",
    )
    .await?;

    let parsed: OffsetOnlyOutput =
        serde_json::from_str(&stdout).map_err(|e| format!("Couldn't parse vo_sync.py output: {e} (raw: {stdout})"))?;
    Ok(parsed.offset_seconds)
}

#[tauri::command]
pub async fn sync_voice_over(
    app: AppHandle,
    video_path: String,
    voiceover_path: String,
    output_path: String,
) -> Result<VoSyncResult, String> {
    let stdout = run_media_ai_script(
        &app,
        "vo_sync.py",
        vec![
            "--video".to_string(),
            video_path.clone(),
            "--voiceover".to_string(),
            voiceover_path,
            "--ffmpeg".to_string(),
            cli_path(crate::bin_paths::ffmpeg_path()),
            "--out".to_string(),
            output_path,
        ],
        "vosync-progress",
        &video_path,
        "syncing",
    )
    .await?;

    let parsed: VoSyncScriptOutput =
        serde_json::from_str(&stdout).map_err(|e| format!("Couldn't parse vo_sync.py output: {e} (raw: {stdout})"))?;

    Ok(VoSyncResult { output_path: parsed.output_path, offset_seconds: parsed.offset_seconds })
}
