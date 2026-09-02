// Full pipeline: extract audio from a video (ffmpeg) -> transcribe it with
// word-level timestamps -> return the words as JSON so React can render
// them and drive the caption-burning step.
//
// Transcription + alignment happen in one call to stt.rs, which routes to
// NVIDIA Parakeet (English) or an Indic Whisper fine-tune + WhisperX's
// alignment (Indian languages) — see stt.rs's module doc comment for why.
// There's no more `ModelQuality` speed/accuracy tier: unlike the old
// whisper.cpp GGML files and WhisperX's swappable faster-whisper sizes,
// each language here has exactly one model (Parakeet TDT 0.6B for English,
// one Indic Whisper checkpoint per Indian language), already chosen and
// quantized for good CPU speed — there's no smaller/larger variant to
// trade off.
//
// No manual language selection anywhere in this pipeline: the spoken
// language (or languages -- see mixed_language.rs) is detected
// automatically and routed to whichever model(s) match. A detected
// language this app has no model for is a clear error, not a silent
// misroute.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::AppHandle;
use tokio::process::Command;

use crate::util::{cli_path, emit_progress, record_fresh_transcript, unique_temp_path};

pub(crate) const PROGRESS_EVENT: &str = "pipeline-progress";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordTimestamp {
    pub word: String,
    pub start: f64, // seconds
    pub end: f64,   // seconds
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineResult {
    pub words: Vec<WordTimestamp>,
    pub detected_language: Option<String>,
}

/// Extracts mono 16kHz PCM WAV audio from `video_path`. Returns the path
/// to the extracted audio file.
pub(crate) async fn extract_audio(app: &AppHandle, video_path: &str, project_id: &str) -> Result<PathBuf, String> {
    emit_progress(app, PROGRESS_EVENT, project_id, "extracting_audio", None, None);

    let audio_path = unique_temp_path("audio", "wav");
    let audio_path_arg = cli_path(&audio_path);

    let output = Command::new(crate::bin_paths::ffmpeg_path())
        .args(["-y", "-i", video_path, "-vn", "-acodec", "pcm_s16le", "-ar", "16000", "-ac", "1"])
        .arg(&audio_path_arg)
        .output()
        .await
        .map_err(|e| format!("Failed to run ffmpeg: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "ffmpeg failed to extract audio: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(audio_path)
}

/// Detects the spoken language(s) of `audio_path` and transcribes it with
/// the matching model(s) -- see `mixed_language.rs`'s
/// `transcribe_with_language_detection` for how this both handles an
/// ordinary single-language video (the common case, one fast whole-file
/// STT call) and one where different stretches are in different supported
/// languages, with no separate mode to opt into either way.
async fn transcribe_with_auto_language(
    app: &AppHandle,
    audio_path: &Path,
    project_id: &str,
) -> Result<(Vec<WordTimestamp>, Option<String>), String> {
    // Held for the whole STT call -- this loads its own model into memory
    // independently per call (see concurrency.rs's own doc comment), so
    // it's exactly the kind of operation the heavy-ML gate exists for now
    // that more than one project can be transcribing at once.
    let _permit = crate::concurrency::acquire_heavy_ml().await;
    crate::mixed_language::transcribe_with_language_detection(app, audio_path, project_id).await
}

#[tauri::command]
pub async fn run_pipeline(
    app: AppHandle,
    video_path: String,
    normalize_slang: bool,
    project_id: String,
) -> Result<PipelineResult, String> {
    // Speculative prefetch: hardware-encoder detection (see ffmpeg.rs)
    // does a handful of trial encodes and is cached for the app's
    // lifetime, but that means the *first* burn pays for it synchronously.
    // Kick it off now, in the background, so by the time the user has
    // reviewed the transcript and clicked "burn captions" it's already
    // resolved — pure latency-hiding, doesn't change what work happens or
    // slow down transcription (it's not competing for the same resource).
    tokio::spawn(async { crate::ffmpeg::best_encoder().await });

    // A silent video (real content here: B-roll meant to get a generated
    // voice-over, not a recording error -- see VoiceoverSection.jsx) has
    // nothing to extract or transcribe. Previously this reached ffmpeg's
    // own audio-extraction command anyway, which failed with "Output file
    // does not contain any stream" -- a raw ffmpeg stack trace as the
    // very first thing a user saw after picking a video, now that the
    // pipeline auto-runs on upload instead of behind a manual button. An
    // empty transcript is a normal, expected result here, not an error.
    if !crate::ffmpeg::has_audio_stream(&video_path).await {
        record_fresh_transcript(&app, &video_path)?;
        return Ok(PipelineResult { words: vec![], detected_language: None });
    }

    let audio_path = extract_audio(&app, &video_path, &project_id).await?;
    let result = transcribe_with_auto_language(&app, &audio_path, &project_id).await;
    let _ = tokio::fs::remove_file(&audio_path).await;
    let (mut words, detected_language) = result?;

    if normalize_slang {
        crate::slang::normalize_words(&mut words);
    }

    // Record that video_path, at its current on-disk state, now has a
    // matching, freshly-generated transcript — burn_captions checks this
    // before burning, regardless of which session/process calls it.
    record_fresh_transcript(&app, &video_path)?;

    Ok(PipelineResult { words, detected_language })
}

/// Transcribes an already-extracted audio file directly -- no ffmpeg
/// extraction step, unlike `run_pipeline`. Used to get real word-level
/// timestamps for a generated/uploaded/mic-recorded voiceover (a WAV file
/// already) so its captions can match what it actually says, instead of
/// continuing to burn the original video's own transcript once the
/// voiceover has replaced its audio. Both STT engines already handle
/// arbitrary input sample rates internally (same as they already do for
/// ffmpeg-extracted audio), so no resampling step is needed here even
/// though Piper (22050Hz) and MMS-TTS (16000Hz) output different rates.
///
/// Deliberately does NOT go through `transcribe_with_auto_language` (the
/// video-oriented, chop-into-many-small-segments language detector
/// `run_pipeline` uses) -- every caller of this command hands it a single
/// speaker's single continuous take (a voiceover recording, never a
/// multi-speaker video), which is exactly the case
/// `transcribe_solo_recording_with_language_detection` exists for. See its
/// own doc comment for the real, on-device bug this replaced (a genuinely
/// Tamil mic recording silently mis-routed to the English transcriber and
/// coming back empty).
#[tauri::command]
pub async fn transcribe_audio_file(app: AppHandle, audio_path: String, project_id: String) -> Result<PipelineResult, String> {
    let _permit = crate::concurrency::acquire_heavy_ml().await;
    let (words, detected_language) =
        crate::mixed_language::transcribe_solo_recording_with_language_detection(&app, Path::new(&audio_path), &project_id)
            .await?;
    Ok(PipelineResult { words, detected_language })
}
