// Full pipeline: extract audio from a video (ffmpeg) -> transcribe it with
// word-level timestamps (whisper-cli) -> return the words as JSON so React
// can render them and drive the caption-burning step.
//
// Both steps currently shell out to CLI binaries (README section 2). This
// is the "desktop today" path described in README section 7 — swap the
// whisper half for `whisper-rs` FFI (see the note in Cargo.toml) to unify
// desktop + mobile behind one code path.
//
// Both steps run via `tokio::process` and are `async fn` commands so a
// multi-second (or multi-minute) job doesn't block the thread handling
// Tauri's IPC — synchronous `std::process::Command` calls here used to
// freeze the whole window for the duration of the subprocess.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tauri::AppHandle;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;

use crate::model::resolve_whisper_model;
use crate::util::{cli_path, emit_progress, unique_temp_path};

const PROGRESS_EVENT: &str = "pipeline-progress";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WordTimestamp {
    pub word: String,
    pub start: f64, // seconds
    pub end: f64,   // seconds
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineResult {
    pub words: Vec<WordTimestamp>,
}

/// Extracts mono 16kHz PCM WAV audio from `video_path` — the format
/// whisper.cpp expects. Returns the path to the extracted audio file.
async fn extract_audio(app: &AppHandle, video_path: &str) -> Result<PathBuf, String> {
    emit_progress(app, PROGRESS_EVENT, "extracting_audio", None, None);

    let audio_path = unique_temp_path("audio", "wav");
    let audio_path_arg = cli_path(&audio_path);

    let output = Command::new("ffmpeg")
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

// --- whisper-cli JSON parsing -------------------------------------------
//
// Shape produced by `whisper-cli ... -ojf` (--output-json-full), which adds
// token-level timestamps to each segment. Field names match whisper.cpp's
// examples/main.cpp output_json() as of the 2024-era CLI. If your installed
// whisper.cpp version's JSON differs, adjust the struct fields below to
// match the `<output-file>.json` it actually produces.

#[derive(Debug, Deserialize)]
struct WhisperOffsets {
    from: i64, // ms
    to: i64,   // ms
}

#[derive(Debug, Deserialize)]
struct WhisperToken {
    text: String,
    offsets: WhisperOffsets,
}

#[derive(Debug, Deserialize)]
struct WhisperSegment {
    offsets: WhisperOffsets,
    text: String,
    #[serde(default)]
    tokens: Vec<WhisperToken>,
}

#[derive(Debug, Deserialize)]
struct WhisperJson {
    transcription: Vec<WhisperSegment>,
}

fn is_special_token(text: &str) -> bool {
    let t = text.trim();
    // whisper.cpp emits non-speech tokens in two styles depending on
    // context: bracketed (e.g. "[_TT_50]") and GPT-2-style special tokens
    // (e.g. "<|endoftext|>", "<|startoftranscript|>", "<|en|>").
    t.is_empty()
        || (t.starts_with('[') && t.ends_with(']'))
        || (t.starts_with("<|") && t.ends_with("|>"))
}

fn words_from_whisper_json(json: WhisperJson) -> Vec<WordTimestamp> {
    let mut words = Vec::new();

    for segment in json.transcription {
        if !segment.tokens.is_empty() {
            for token in segment.tokens {
                if is_special_token(&token.text) {
                    continue;
                }
                words.push(WordTimestamp {
                    word: token.text.trim().to_string(),
                    start: token.offsets.from as f64 / 1000.0,
                    end: token.offsets.to as f64 / 1000.0,
                });
            }
        } else {
            // Fall back to splitting the segment text evenly across its
            // time span, in case this whisper.cpp build ignores -sow and
            // never emits per-token offsets.
            let words_in_segment: Vec<&str> = segment.text.split_whitespace().collect();
            if words_in_segment.is_empty() {
                continue;
            }
            let start = segment.offsets.from as f64 / 1000.0;
            let end = segment.offsets.to as f64 / 1000.0;
            let span = (end - start).max(0.01);
            let step = span / words_in_segment.len() as f64;
            for (i, w) in words_in_segment.iter().enumerate() {
                words.push(WordTimestamp {
                    word: w.to_string(),
                    start: start + step * i as f64,
                    end: start + step * (i as f64 + 1.0),
                });
            }
        }
    }

    words
}

/// Parses a whisper.cpp `-pp` progress line, e.g.
/// "whisper_print_progress_callback: progress =  42%", into a percentage.
fn parse_whisper_progress_line(line: &str) -> Option<f64> {
    let rest = line.split("progress = ").nth(1)?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<f64>().ok()
}

/// whisper-cli defaults to 4 threads regardless of the machine. Encoder
/// throughput keeps improving up to ~8 threads on most CPUs but tends to
/// flatten out (or regress, from memory-bandwidth contention) past that,
/// so this caps out rather than handing it every logical core.
fn whisper_thread_count() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8)
}

/// whisper.cpp's `-p`/`--processors` splits the audio into N chunks and
/// decodes them in N truly-parallel contexts — the same idea as the
/// video-segment parallelism, applied to transcription. Each chunk loses
/// cross-chunk context (a small accuracy cost right at chunk boundaries),
/// so this only kicks in for audio long enough that the wall-clock win is
/// clearly worth it; short clips (most reels) get `1` and behave exactly
/// as before.
fn whisper_processor_count(audio_duration_secs: Option<f64>) -> usize {
    audio_duration_secs.map(|d| (d / 60.0).floor() as usize).unwrap_or(1).clamp(1, 4)
}

/// Runs whisper-cli against `audio_path`, requesting word-level timestamps,
/// and returns the parsed words. Streams `-pp` progress lines from stderr
/// as "transcribing" progress events.
async fn transcribe(app: &AppHandle, audio_path: &Path) -> Result<Vec<WordTimestamp>, String> {
    let model_path = resolve_whisper_model(app)?;
    let output_stem = unique_temp_path("transcript", "out");
    let output_stem = output_stem.with_extension(""); // whisper-cli appends its own extension

    let model_path_arg = cli_path(&model_path);
    let audio_path_arg = cli_path(audio_path);
    let output_stem_arg = cli_path(&output_stem);

    let audio_duration = crate::ffmpeg::probe_duration_seconds(&audio_path_arg).await;
    let processors = whisper_processor_count(audio_duration);
    // Total thread budget divided across processors, so `p` parallel
    // contexts don't each independently try to grab up to 8 threads and
    // oversubscribe the machine.
    let threads_arg = (whisper_thread_count() / processors).max(1).to_string();
    let processors_arg = processors.to_string();

    emit_progress(app, PROGRESS_EVENT, "transcribing", Some(0.0), None);

    let mut child = Command::new("whisper-cli")
        .args([
            "-m",
            &model_path_arg,
            "-f",
            &audio_path_arg,
            "-t",
            &threads_arg, // default is 4 — use more of the machine's cores
            "-p",
            &processors_arg, // split long audio into this many parallel decode contexts
            "-ojf", // word-level timestamps in the JSON output
            "-sow", // split on word rather than token
            "-nt",  // don't print timestamps to stdout — we read the JSON file
            "-pp",  // print progress to stderr, which we parse below
            "-of",
            &output_stem_arg,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!("whisper-cli not found on PATH ({e}). Build whisper.cpp and add it to PATH — see README.")
        })?;

    let stderr = child.stderr.take().expect("whisper-cli stderr was piped");
    let app_progress = app.clone();
    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut full_text = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(percent) = parse_whisper_progress_line(&line) {
                // whisper.cpp's own progress callback can overshoot past
                // 100% on short clips (observed up to 272%) — clamp so the
                // UI never shows a nonsensical bar.
                emit_progress(&app_progress, PROGRESS_EVENT, "transcribing", Some(percent.clamp(0.0, 100.0)), None);
            }
            full_text.push_str(&line);
            full_text.push('\n');
        }
        full_text
    });

    let status = child.wait().await.map_err(|e| format!("Failed to wait on whisper-cli: {e}"))?;
    let stderr_text = stderr_task.await.unwrap_or_default();

    if !status.success() {
        return Err(format!("whisper-cli failed (model path passed: {model_path_arg}): {stderr_text}"));
    }

    emit_progress(app, PROGRESS_EVENT, "transcribing", Some(100.0), None);

    let json_path = output_stem.with_extension("json");
    let mut json_file = tokio::fs::File::open(&json_path)
        .await
        .map_err(|e| format!("Couldn't read whisper output JSON at {}: {e}", json_path.display()))?;
    let mut json_text = String::new();
    json_file
        .read_to_string(&mut json_text)
        .await
        .map_err(|e| format!("Couldn't read whisper output JSON at {}: {e}", json_path.display()))?;
    drop(json_file);
    let _ = tokio::fs::remove_file(&json_path).await;

    let parsed: WhisperJson =
        serde_json::from_str(&json_text).map_err(|e| format!("Couldn't parse whisper JSON output: {e}"))?;

    Ok(words_from_whisper_json(parsed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_token_level_json_into_words() {
        let json = r#"{
            "transcription": [
                {
                    "offsets": { "from": 0, "to": 1200 },
                    "text": " Hello world",
                    "tokens": [
                        { "text": " Hello", "offsets": { "from": 0, "to": 500 } },
                        { "text": " world", "offsets": { "from": 500, "to": 1200 } },
                        { "text": "[_TT_50]", "offsets": { "from": 1200, "to": 1200 } }
                    ]
                }
            ]
        }"#;
        let parsed: WhisperJson = serde_json::from_str(json).unwrap();
        let words = words_from_whisper_json(parsed);
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].word, "Hello");
        assert_eq!(words[0].start, 0.0);
        assert_eq!(words[0].end, 0.5);
        assert_eq!(words[1].word, "world");
        assert_eq!(words[1].end, 1.2);
    }

    #[test]
    fn filters_out_endoftext_special_token() {
        // Regression test: real whisper-cli -ojf output ends each segment's
        // token list with a "<|endoftext|>" marker, which must not leak
        // into the transcript (it would otherwise get burned into captions).
        let json = r#"{
            "transcription": [
                {
                    "offsets": { "from": 0, "to": 1000 },
                    "text": " Hi",
                    "tokens": [
                        { "text": " Hi", "offsets": { "from": 0, "to": 500 } },
                        { "text": "<|endoftext|>", "offsets": { "from": 1000, "to": 1000 } }
                    ]
                }
            ]
        }"#;
        let parsed: WhisperJson = serde_json::from_str(json).unwrap();
        let words = words_from_whisper_json(parsed);
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].word, "Hi");
    }

    #[test]
    fn falls_back_to_even_split_when_no_tokens() {
        let json = r#"{
            "transcription": [
                { "offsets": { "from": 0, "to": 2000 }, "text": "one two", "tokens": [] }
            ]
        }"#;
        let parsed: WhisperJson = serde_json::from_str(json).unwrap();
        let words = words_from_whisper_json(parsed);
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].word, "one");
        assert_eq!(words[0].start, 0.0);
        assert_eq!(words[0].end, 1.0);
        assert_eq!(words[1].word, "two");
        assert_eq!(words[1].start, 1.0);
        assert_eq!(words[1].end, 2.0);
    }

    #[test]
    fn is_special_token_filters_bracketed_and_empty() {
        assert!(is_special_token("[_TT_50]"));
        assert!(is_special_token("<|endoftext|>"));
        assert!(is_special_token("<|startoftranscript|>"));
        assert!(is_special_token("   "));
        assert!(is_special_token(""));
        assert!(!is_special_token(" hello"));
    }

    #[test]
    fn parses_whisper_progress_lines() {
        assert_eq!(
            parse_whisper_progress_line("whisper_print_progress_callback: progress =  42%"),
            Some(42.0)
        );
        assert_eq!(
            parse_whisper_progress_line("whisper_print_progress_callback: progress = 100%"),
            Some(100.0)
        );
        assert_eq!(parse_whisper_progress_line("some unrelated line"), None);
    }

    #[test]
    fn whisper_processor_count_stays_at_one_for_short_or_unknown_duration() {
        assert_eq!(whisper_processor_count(Some(30.0)), 1);
        assert_eq!(whisper_processor_count(Some(59.9)), 1);
        assert_eq!(whisper_processor_count(None), 1);
    }

    #[test]
    fn whisper_processor_count_scales_with_duration_up_to_cap() {
        assert_eq!(whisper_processor_count(Some(120.0)), 2);
        assert_eq!(whisper_processor_count(Some(600.0)), 4);
    }
}

#[tauri::command]
pub async fn run_pipeline(app: AppHandle, video_path: String) -> Result<PipelineResult, String> {
    // Speculative prefetch: hardware-encoder detection (see ffmpeg.rs)
    // does a handful of trial encodes and is cached for the app's
    // lifetime, but that means the *first* burn pays for it synchronously.
    // Kick it off now, in the background, so by the time the user has
    // reviewed the transcript and clicked "burn captions" it's already
    // resolved — pure latency-hiding, doesn't change what work happens or
    // slow down transcription (it's not competing for the same resource).
    tokio::spawn(async { crate::ffmpeg::best_encoder().await });

    let audio_path = extract_audio(&app, &video_path).await?;
    let words = transcribe(&app, &audio_path).await;
    let _ = tokio::fs::remove_file(&audio_path).await;

    Ok(PipelineResult { words: words? })
}
