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
// language is detected automatically (`stt::detect_spoken_language`,
// `transcribe_with_auto_language` below) and routed to whichever model
// matches. A detected language this app has no model for is a clear error,
// not a silent misroute.

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
pub(crate) async fn extract_audio(app: &AppHandle, video_path: &str) -> Result<PathBuf, String> {
    emit_progress(app, PROGRESS_EVENT, "extracting_audio", None, None);

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

/// Detects the spoken language of `audio_path` (see `stt::detect_spoken_language`)
/// and transcribes it with the matching model, rejecting a detected
/// language this app has no model for with a clear error rather than a
/// confusing downstream failure.
async fn transcribe_with_auto_language(
    app: &AppHandle,
    audio_path: &Path,
) -> Result<(Vec<WordTimestamp>, Option<String>), String> {
    let (code, probability) = crate::stt::detect_spoken_language(app, audio_path).await?;
    if !crate::stt::is_supported_language_code(&code) {
        return Err(format!(
            "Detected spoken language '{code}' ({:.0}% confidence) isn't supported yet — \
             this app currently supports: {}.",
            probability * 100.0,
            crate::stt::supported_language_names().join(", ")
        ));
    }
    let (words, _echoed_language) = crate::stt::transcribe_and_align(app, audio_path, &code).await?;
    Ok((words, Some(crate::stt::language_name_for_code(&code))))
}

#[tauri::command]
pub async fn run_pipeline(
    app: AppHandle,
    video_path: String,
    normalize_slang: bool,
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

    let audio_path = extract_audio(&app, &video_path).await?;
    let result = transcribe_with_auto_language(&app, &audio_path).await;
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
/// timestamps for a generated/uploaded voiceover (a WAV file already) so
/// its captions can match what it actually says, instead of continuing to
/// burn the original video's own transcript once the voiceover has
/// replaced its audio. Both STT engines already handle arbitrary input
/// sample rates internally (same as they already do for ffmpeg-extracted
/// audio), so no resampling step is needed here even though Piper (22050Hz)
/// and MMS-TTS (16000Hz) output different rates.
#[tauri::command]
pub async fn transcribe_audio_file(app: AppHandle, audio_path: String) -> Result<PipelineResult, String> {
    let (words, detected_language) = transcribe_with_auto_language(&app, Path::new(&audio_path)).await?;
    Ok(PipelineResult { words, detected_language })
}
