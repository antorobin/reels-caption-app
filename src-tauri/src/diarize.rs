// Lightweight speaker diarization — no pyannote/torch. Candidate
// speaker-turn boundaries come from jumpcuts.rs's own silence-gap
// detection (already does exactly this, just reused with a larger
// min-gap); media_ai/diarize.py clusters those segments by MFCC
// voice-print via a hand-rolled numpy k-means.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::jumpcuts::{compute_keep_ranges, JumpCutOptions};
use crate::media_ai::run_media_ai_script;
use crate::pipeline::WordTimestamp;
use crate::util::{cli_path, unique_temp_path};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerSegment {
    pub start: f64,
    pub end: f64,
    pub speaker_id: u8,
}

/// Larger than the jump-cut default (0.6s) — speaker turns are naturally
/// longer pauses than mid-sentence breaths.
const DIARIZE_MIN_GAP_SECONDS: f64 = 0.9;

#[tauri::command]
pub async fn diarize_speakers(
    app: AppHandle,
    video_path: String,
    words: Vec<WordTimestamp>,
    project_id: String,
) -> Result<Vec<SpeakerSegment>, String> {
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let duration = words.last().map(|w| w.end).unwrap_or(0.0);
    let options = JumpCutOptions { min_silence_seconds: DIARIZE_MIN_GAP_SECONDS, remove_filler_words: false };
    let segments = compute_keep_ranges(&words, duration, &options);

    let audio_path = crate::pipeline::extract_audio(&app, &video_path, &project_id).await?;

    let segments_json_path = unique_temp_path("diarize-segments", "json");
    let segments_json =
        serde_json::to_string(&segments).map_err(|e| format!("Couldn't serialize segments for diarization: {e}"))?;
    std::fs::write(&segments_json_path, segments_json).map_err(|e| format!("Couldn't write diarization segments file: {e}"))?;

    let stdout = run_media_ai_script(
        &app,
        "diarize.py",
        vec![
            "--audio".to_string(),
            cli_path(&audio_path),
            "--segments".to_string(),
            cli_path(&segments_json_path),
        ],
        "diarize-progress",
        &project_id,
        "diarizing",
    )
    .await;

    let _ = std::fs::remove_file(&audio_path);
    let _ = std::fs::remove_file(&segments_json_path);
    let stdout = stdout?;

    serde_json::from_str(&stdout).map_err(|e| format!("Couldn't parse diarize.py output: {e} (raw: {stdout})"))
}
