// Live dictation: "text appears as you speak" -- via periodic
// re-transcription of the growing captured buffer, reusing the exact same
// segment-then-classify-then-route-then-merge logic `mixed_language.rs`
// already proves out for whole videos (so Tamil+English code-switching
// works here too, for free), not a true incremental streaming recognizer.
//
// Why not true streaming (this project's first attempt, since replaced):
// genuinely incremental streaming ASR needs a model architecturally built
// for it, and no such model exists for Tamil (or Tamil+English
// code-switching) today -- confirmed via k2-fsa/sherpa-onnx's own project
// discussion (#3199) asking exactly this question. A prior version of this
// module used `sherpa-onnx` + `cpal` for true streaming, which worked but
// was English-only.
//
// Why this ISN'T just "call mixed_language::transcribe_with_language_detection
// every 2.5s" (a real bug found and fixed the hard way): that function
// spawns a fresh Python process -- and reloads the whole ASR model from
// disk -- for every single call. Fine for a one-shot batch transcription,
// but paying full model-load cost every ~2.5s completely defeated the
// point of "live": a single update cycle could easily take far longer than
// the interval itself, which is exactly why it didn't feel real-time.
// `LiveWorker` below fixes this the correct way: one persistent Python
// process (`stt/live_worker.py`) that loads the language-ID model,
// Parakeet, and the Tamil checkpoint ONCE and stays resident across the
// whole app session (not just one dictation session -- started lazily on
// first use, then reused), serving repeated requests over stdin/stdout
// with no reload cost. The segmentation/merge orchestration itself is
// unchanged and still runs in Rust (`mixed_language::subdivide_long_segments`/
// `resolve_segment_languages`/`merge_into_chunks`, made `pub(crate)` for
// this to reuse directly) -- only the two actual model-inference calls
// route through the worker instead of stt.rs's one-shot scripts. The main
// batch pipeline (pipeline.rs) is completely untouched by any of this; it
// still uses the one-shot scripts directly, since it only pays that cost
// once per video anyway and doesn't need a resident process for it.
//
// Trade-off that remains even with a warm worker: text still grows every
// ~2.5s (a full re-transcription of everything captured so far), not
// instantly per word the way true streaming would. Re-transcribing the
// whole growing buffer each cycle (rather than just the newest slice)
// avoids boundary-cutting artifacts a naive "transcribe only the latest
// chunk" approach would hit, at the cost of doing some redundant work as a
// session gets longer -- an acceptable trade for what's meant to be short
// dictation sessions (a caption or two, not a lecture).
//
// Mic capture is still native (`cpal`, direct OS audio-device access, no
// browser API) -- that part of the original brief ("use the OS's own
// capability") didn't need to change, only the recognition engine did.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, Command};

use crate::mixed_language::{self, LanguageChunk};
use crate::pipeline::WordTimestamp;
use crate::stt;
use crate::util::{cli_path, unique_temp_path};

const CHUNK_INTERVAL: Duration = Duration::from_millis(2500);
const CAPTURE_SAMPLE_RATE: i32 = 16000;

/// Plain linear-interpolation resampler -- good enough for speech
/// recognition preprocessing (not hi-fi audio), and avoids pulling in a
/// dedicated resampling crate for one small job.
fn resample_linear(input: &[f32], input_rate: i32, output_rate: i32) -> Vec<f32> {
    if input_rate == output_rate || input.is_empty() {
        return input.to_vec();
    }
    let ratio = output_rate as f64 / input_rate as f64;
    let out_len = ((input.len() as f64) * ratio).round() as usize;
    let mut output = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = i as f64 / ratio;
        let idx = src_pos as usize;
        let frac = src_pos - idx as f64;
        let a = *input.get(idx).unwrap_or(&0.0) as f64;
        let b = *input.get(idx + 1).unwrap_or(&(a as f32)) as f64;
        output.push((a + (b - a) * frac) as f32);
    }
    output
}

struct LiveDictationHandle {
    stop_flag: Arc<AtomicBool>,
    capture_thread: std::thread::JoinHandle<()>,
    transcribe_task: tauri::async_runtime::JoinHandle<Vec<WordTimestamp>>,
}

pub struct LiveDictationState(Mutex<Option<LiveDictationHandle>>);

impl Default for LiveDictationState {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}

/// Runs on a dedicated OS thread for the capture session's whole life --
/// `cpal::Stream` isn't safely movable across threads on every platform,
/// so it's built and dropped on this one thread only, communicating with
/// the rest of the app purely by appending into the shared `buffer` and
/// polling `stop_flag`.
fn run_capture(buffer: Arc<Mutex<Vec<f32>>>, stop_flag: Arc<AtomicBool>) -> Result<(), String> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or("No microphone (default input device) was found.".to_string())?;
    let config = device.default_input_config().map_err(|e| format!("Couldn't read the microphone's default config: {e}"))?;
    let input_sample_rate = config.sample_rate().0 as i32;
    let channels = config.channels() as usize;

    let err_fn = |e| eprintln!("cpal input stream error: {e}");

    macro_rules! build_stream_for {
        ($sample_type:ty, $to_f32:expr) => {{
            let buffer = Arc::clone(&buffer);
            device.build_input_stream(
                &config.clone().into(),
                move |data: &[$sample_type], _: &cpal::InputCallbackInfo| {
                    let mono: Vec<f32> = data
                        .chunks(channels.max(1))
                        .map(|frame| {
                            let sum: f32 = frame.iter().map(|s| $to_f32(*s)).sum();
                            sum / frame.len().max(1) as f32
                        })
                        .collect();
                    let resampled = resample_linear(&mono, input_sample_rate, CAPTURE_SAMPLE_RATE);
                    if let Ok(mut buf) = buffer.lock() {
                        buf.extend(resampled);
                    }
                },
                err_fn,
                None,
            )
        }};
    }

    let input_stream = match config.sample_format() {
        cpal::SampleFormat::F32 => build_stream_for!(f32, |s: f32| s),
        cpal::SampleFormat::I16 => build_stream_for!(i16, |s: i16| s as f32 / i16::MAX as f32),
        cpal::SampleFormat::U16 => build_stream_for!(u16, |s: u16| (s as f32 - 32768.0) / 32768.0),
        other => return Err(format!("Unsupported microphone sample format: {other:?}")),
    }
    .map_err(|e| format!("Couldn't open the microphone: {e}"))?;

    input_stream.play().map_err(|e| format!("Couldn't start listening to the microphone: {e}"))?;

    while !stop_flag.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(input_stream);
    Ok(())
}

/// Writes raw f32 PCM samples to a WAV file via the already-bundled
/// ffmpeg, matching every other audio-format conversion in this codebase
/// (e.g. `mic_recording.rs`) rather than hand-rolling a WAV writer.
async fn write_wav(samples: &[f32]) -> Result<PathBuf, String> {
    let raw_path = unique_temp_path("live-dictation-raw", "pcm");
    let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    std::fs::write(&raw_path, &bytes).map_err(|e| format!("Couldn't write raw audio buffer: {e}"))?;

    let wav_path = unique_temp_path("live-dictation", "wav");
    let output = Command::new(crate::bin_paths::ffmpeg_path())
        .args([
            "-y",
            "-f",
            "f32le",
            "-ar",
            &CAPTURE_SAMPLE_RATE.to_string(),
            "-ac",
            "1",
            "-i",
            &cli_path(&raw_path),
        ])
        .arg(cli_path(&wav_path))
        .output()
        .await
        .map_err(|e| format!("Failed to run ffmpeg to package the recording as WAV: {e}"))?;

    let _ = std::fs::remove_file(&raw_path);

    if !output.status.success() {
        let _ = std::fs::remove_file(&wav_path);
        return Err(format!("Couldn't package the recording as WAV: {}", String::from_utf8_lossy(&output.stderr)));
    }
    Ok(wav_path)
}

/// One running `live_worker.py` process -- a persistent stdin/stdout JSON
/// line protocol (see that script's own doc comment), talked to strictly
/// request-then-response since only one call is ever in flight at a time
/// from `run_periodic_transcription`'s loop.
struct LiveWorkerProcess {
    _child: Child,
    stdin: ChildStdin,
    stdout_lines: Lines<BufReader<tokio::process::ChildStdout>>,
}

/// Tauri-managed, app-lifetime state -- the worker is started lazily on
/// first use and then kept alive across every live-dictation session
/// after that (not torn down when a single session stops), which is the
/// entire point: pay the model-load cost once per app run, not once per
/// ~2.5s update. It dies for free when the app process exits, the same
/// way every other child process here does (`proc_cleanup.rs`'s Windows
/// Job Object already covers this -- no extra cleanup needed).
pub struct LiveWorkerState(tokio::sync::Mutex<Option<LiveWorkerProcess>>);

impl Default for LiveWorkerState {
    fn default() -> Self {
        Self(tokio::sync::Mutex::new(None))
    }
}

const WORKER_STARTUP_TIMEOUT: Duration = Duration::from_secs(120);

/// Loading the actual models (Parakeet + Whisper lang-ID + optionally
/// Tamil) genuinely takes real time -- measured directly against this
/// project's own conda env, ~20-25s on ordinary hardware, more on a slower
/// disk/CPU. Without some signal, that whole window looks indistinguishable
/// from "live dictation is just broken and never shows anything," which is
/// exactly the confusion a user hit on a first real test. `loading: true`
/// fires only when a fresh worker actually needs to start (the fast path,
/// `guard.is_some()` above, never fires it), so every session after the
/// first in one app run sees none of this.
async fn ensure_live_worker(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<LiveWorkerState>();
    let mut guard = state.0.lock().await;
    if guard.is_some() {
        return Ok(());
    }

    let _ = app.emit("live-dictation-worker-status", serde_json::json!({"loading": true}));
    let outcome = spawn_live_worker(app).await;
    let _ = app.emit("live-dictation-worker-status", serde_json::json!({"loading": false}));

    match outcome {
        Ok(process) => {
            *guard = Some(process);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

async fn spawn_live_worker(app: &AppHandle) -> Result<LiveWorkerProcess, String> {
    let python = stt::stt_python_exe().await?;
    let script = stt::live_worker_script_path();
    let tamil_dir = stt::tamil_model_dir_if_available(app);

    let mut command = Command::new(&python);
    command.arg(cli_path(&script));
    if let Some(dir) = &tamil_dir {
        command.arg("--tamil-model-dir").arg(cli_path(dir));
    }
    command.env("PYTHONIOENCODING", "utf-8").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command.spawn().map_err(|e| {
        format!(
            "Couldn't start the live-dictation worker ({e}). Set up the 'stt' conda environment (see README) -- \
             or if it's already installed somewhere this couldn't find, set REELS_CAPTION_APP_STT_CONDA_PATH."
        )
    })?;

    let stdin = child.stdin.take().ok_or("live-dictation worker has no stdin")?;
    let stdout = child.stdout.take().ok_or("live-dictation worker has no stdout")?;
    let stderr = child.stderr.take().ok_or("live-dictation worker has no stderr")?;

    // Drains the worker's own log/error output in the background for as
    // long as it lives, so it never blocks on a full stderr pipe buffer
    // and so a startup failure's real error text ends up somewhere visible
    // (this process's own stderr) rather than silently discarded.
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            eprintln!("[live-dictation worker] {line}");
        }
    });

    let mut stdout_lines = BufReader::new(stdout).lines();
    let ready_line = tokio::time::timeout(WORKER_STARTUP_TIMEOUT, stdout_lines.next_line())
        .await
        .map_err(|_| {
            "Timed out waiting for the live-dictation worker to finish loading its models (2 minutes).".to_string()
        })?
        .map_err(|e| format!("Failed reading from the live-dictation worker during startup: {e}"))?
        .ok_or_else(|| "The live-dictation worker exited before it finished starting up.".to_string())?;
    if !ready_line.contains("\"ready\"") {
        return Err(format!("Unexpected startup response from the live-dictation worker: {ready_line}"));
    }

    Ok(LiveWorkerProcess { _child: child, stdin, stdout_lines })
}

async fn live_worker_request(app: &AppHandle, request: serde_json::Value) -> Result<serde_json::Value, String> {
    let state = app.state::<LiveWorkerState>();
    let mut guard = state.0.lock().await;
    let process = guard.as_mut().ok_or("The live-dictation worker isn't running.".to_string())?;

    let mut line = serde_json::to_string(&request).map_err(|e| format!("Couldn't serialize a live-dictation request: {e}"))?;
    line.push('\n');
    process.stdin.write_all(line.as_bytes()).await.map_err(|e| format!("Couldn't write to the live-dictation worker: {e}"))?;
    process.stdin.flush().await.map_err(|e| format!("Couldn't flush to the live-dictation worker: {e}"))?;

    let response_line = process
        .stdout_lines
        .next_line()
        .await
        .map_err(|e| format!("Failed reading from the live-dictation worker: {e}"))?
        .ok_or_else(|| "The live-dictation worker closed its output unexpectedly.".to_string())?;
    serde_json::from_str(&response_line).map_err(|e| format!("Couldn't parse the live-dictation worker's response: {e}"))
}

fn worker_error(response: &serde_json::Value) -> Option<String> {
    response.get("error").and_then(|e| e.as_str()).map(|s| s.to_string())
}

async fn live_detect_languages(
    app: &AppHandle,
    audio_path: &Path,
    segments: &[(f64, f64)],
) -> Result<Vec<(f64, f64, String, f64)>, String> {
    let request = serde_json::json!({
        "op": "detect_languages",
        "audio_path": cli_path(audio_path),
        "segments": segments,
    });
    let response = live_worker_request(app, request).await?;
    if let Some(err) = worker_error(&response) {
        return Err(err);
    }

    #[derive(Deserialize)]
    struct Detected {
        start: f64,
        end: f64,
        language: String,
        probability: f64,
    }
    let results: Vec<Detected> = serde_json::from_value(response.get("results").cloned().unwrap_or_default())
        .map_err(|e| format!("Unexpected detect_languages response shape: {e}"))?;
    Ok(results.into_iter().map(|d| (d.start, d.end, d.language, d.probability)).collect())
}

async fn live_transcribe(app: &AppHandle, audio_path: &Path, language: &str) -> Result<Vec<WordTimestamp>, String> {
    let request = serde_json::json!({
        "op": "transcribe",
        "audio_path": cli_path(audio_path),
        "language": language,
    });
    let response = live_worker_request(app, request).await?;
    if let Some(err) = worker_error(&response) {
        return Err(err);
    }

    #[derive(Deserialize)]
    struct WordOut {
        word: String,
        start: f64,
        end: f64,
    }
    let words: Vec<WordOut> = serde_json::from_value(response.get("words").cloned().unwrap_or_default())
        .map_err(|e| format!("Unexpected transcribe response shape: {e}"))?;
    Ok(words.into_iter().map(|w| WordTimestamp { word: w.word, start: w.start, end: w.end }).collect())
}

/// Same padding/shift/filter logic as `mixed_language.rs`'s own
/// `transcribe_chunk` -- kept in sync deliberately, just calling
/// `live_transcribe` (the persistent worker) instead of
/// `stt::transcribe_and_align` (a fresh one-shot process) at the end.
const CHUNK_PADDING_SECONDS: f64 = 0.15;

async fn live_transcribe_chunk(
    app: &AppHandle,
    audio_path: &Path,
    total_duration: f64,
    chunk: &LanguageChunk,
) -> Result<Vec<WordTimestamp>, String> {
    let padded_start = (chunk.start - CHUNK_PADDING_SECONDS).max(0.0);
    let padded_end = (chunk.end + CHUNK_PADDING_SECONDS).min(total_duration);

    let slice_path = unique_temp_path("live-lang-chunk", "wav");
    let output = Command::new(crate::bin_paths::ffmpeg_path())
        .args([
            "-y".to_string(),
            "-i".to_string(),
            cli_path(audio_path),
            "-ss".to_string(),
            format!("{padded_start}"),
            "-to".to_string(),
            format!("{padded_end}"),
            "-c".to_string(),
            "copy".to_string(),
            cli_path(&slice_path),
        ])
        .output()
        .await
        .map_err(|e| format!("Failed to run ffmpeg to slice a live chunk: {e}"))?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&slice_path);
        return Err(format!("Failed to slice live audio chunk: {}", String::from_utf8_lossy(&output.stderr)));
    }

    let result = live_transcribe(app, &slice_path, &chunk.language).await;
    let _ = std::fs::remove_file(&slice_path);
    let words = result?;

    Ok(words
        .into_iter()
        .filter_map(|w| {
            let shifted_start = w.start + padded_start;
            let shifted_end = w.end + padded_start;
            if shifted_end < chunk.start - 0.02 || shifted_start > chunk.end + 0.02 {
                return None;
            }
            Some(WordTimestamp { word: w.word, start: shifted_start, end: shifted_end })
        })
        .collect())
}

/// The live-dictation counterpart to
/// `mixed_language::transcribe_with_language_detection` -- identical
/// segment → classify → merge → route → stitch shape (reusing that
/// module's own segmentation/merge functions directly), but every model
/// call goes through the persistent worker above instead of a fresh
/// one-shot process.
async fn live_transcribe_with_language_detection(app: &AppHandle, audio_path: &Path) -> Result<Vec<WordTimestamp>, String> {
    ensure_live_worker(app).await?;

    let raw_segments = crate::ffmpeg::detect_speech_segments(audio_path).await?;
    if raw_segments.is_empty() {
        return Ok(Vec::new());
    }
    let raw_segments = mixed_language::subdivide_long_segments(raw_segments);

    let raw_with_language = live_detect_languages(app, audio_path, &raw_segments).await?;
    let resolved = mixed_language::resolve_segment_languages(raw_with_language);
    let chunks = mixed_language::merge_into_chunks(resolved);
    if chunks.is_empty() {
        return Ok(Vec::new());
    }

    let total_duration = chunks.last().map(|c| c.end).unwrap_or(0.0);
    let mut all_words = Vec::new();
    for chunk in &chunks {
        if !stt::is_supported_language_code(&chunk.language) {
            continue;
        }
        all_words.extend(live_transcribe_chunk(app, audio_path, total_duration, chunk).await?);
    }
    Ok(all_words)
}

#[derive(Clone, Serialize)]
struct LivePartial {
    text: String,
}

/// Runs on Tauri's own async runtime for the capture session's whole life:
/// wakes up every `CHUNK_INTERVAL`, re-transcribes everything captured so
/// far through the exact same code-switching-aware pipeline a whole video
/// goes through, and emits the growing result. Once `stop_flag` is set,
/// does one final pass and returns its `Vec<WordTimestamp>` as this task's
/// result -- `stop_live_dictation` awaits this task directly to get it.
async fn run_periodic_transcription(
    app: AppHandle,
    buffer: Arc<Mutex<Vec<f32>>>,
    stop_flag: Arc<AtomicBool>,
) -> Vec<WordTimestamp> {
    let mut last_emitted = String::new();
    loop {
        tokio::time::sleep(CHUNK_INTERVAL).await;
        let stopping = stop_flag.load(Ordering::SeqCst);

        let snapshot = buffer.lock().map(|b| b.clone()).unwrap_or_default();
        if snapshot.is_empty() {
            if stopping {
                return Vec::new();
            }
            continue;
        }

        let words = match write_wav(&snapshot).await {
            Ok(wav_path) => {
                let result = live_transcribe_with_language_detection(&app, &wav_path).await;
                let _ = std::fs::remove_file(&wav_path);
                match result {
                    Ok(words) => words,
                    Err(e) => {
                        eprintln!("Live dictation chunk transcription failed: {e}");
                        Vec::new()
                    }
                }
            }
            Err(e) => {
                eprintln!("Live dictation couldn't package a chunk: {e}");
                Vec::new()
            }
        };

        let text = words.iter().map(|w| w.word.as_str()).collect::<Vec<_>>().join(" ");
        if text != last_emitted {
            last_emitted = text.clone();
            let _ = app.emit("live-dictation-partial", LivePartial { text });
        }

        if stopping {
            return words;
        }
    }
}

#[tauri::command]
pub fn start_live_dictation(app: AppHandle, state: tauri::State<LiveDictationState>) -> Result<(), String> {
    let previous = {
        let mut guard = state.0.lock().map_err(|_| "Dictation state lock was poisoned".to_string())?;
        guard.take()
    };
    if let Some(existing) = previous {
        // Force-end whatever was there instead of refusing to start (this
        // used to return "A dictation session is already running." and stop
        // there) -- a stale prior session left running by a bug elsewhere
        // (or one from before this process's own fixes were loaded, in a
        // long-lived dev session) didn't just block the *error message*;
        // its `cpal` stream was still holding the microphone open, so a
        // *new* session's own stream could fail to open or silently
        // capture nothing at all -- which looked exactly like "live
        // dictation never shows any text," not like a session conflict.
        existing.stop_flag.store(true, Ordering::SeqCst);
        // `run_capture` polls its stop flag every 100ms, so this is a
        // short, bounded wait -- not the kind of open-ended "could take
        // forever" wait `stop_live_dictation` deliberately avoids for the
        // final transcription pass below. Waiting here (rather than firing
        // and forgetting) is what actually guarantees the mic is free
        // before the new capture thread tries to open it.
        let _ = existing.capture_thread.join();
        // The old transcription task's result is moot now -- this session
        // was superseded, not gracefully finished -- so drop it rather than
        // let it keep running and later emit a stale "live-dictation-final"
        // into whatever new session is now active.
        existing.transcribe_task.abort();
    }

    // Kick off model loading immediately rather than waiting for the first
    // `CHUNK_INTERVAL` tick to discover the worker isn't running yet --
    // overlaps first-time startup cost with the user already speaking
    // instead of adding it as extra silent wait before any text can
    // appear. Best-effort/fire-and-forget: if this fails, the periodic
    // loop's own `ensure_live_worker` call will surface the real error.
    let warm_up_app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = ensure_live_worker(&warm_up_app).await;
    });

    let stop_flag = Arc::new(AtomicBool::new(false));
    let buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));

    let capture_stop_flag = Arc::clone(&stop_flag);
    let capture_buffer = Arc::clone(&buffer);
    let capture_thread = std::thread::spawn(move || {
        if let Err(e) = run_capture(capture_buffer, capture_stop_flag) {
            eprintln!("Live dictation capture thread ended with an error: {e}");
        }
    });

    let transcribe_app = app.clone();
    let transcribe_stop_flag = Arc::clone(&stop_flag);
    let transcribe_task =
        tauri::async_runtime::spawn(run_periodic_transcription(transcribe_app, buffer, transcribe_stop_flag));

    let mut guard = state.0.lock().map_err(|_| "Dictation state lock was poisoned".to_string())?;
    *guard = Some(LiveDictationHandle { stop_flag, capture_thread, transcribe_task });
    Ok(())
}

#[derive(Clone, Serialize)]
struct LiveFinal {
    words: Option<Vec<WordTimestamp>>,
    error: Option<String>,
}

/// Signals the session to stop and returns immediately -- deliberately
/// does NOT wait for the final transcription pass to finish, which can
/// take a while (or, if a chunk transcription is genuinely stuck, forever)
/// and would otherwise leave the calling UI blocked on this command with
/// no way to back out. The actual final `Vec<WordTimestamp>` (or an error)
/// arrives later via a `"live-dictation-final"` event once the background
/// task actually finishes -- callers (DictationHud.jsx,
/// LiveDictationPanel.jsx) show a "Finishing…" state with its own
/// cancel/close affordance in the meantime rather than waiting on this
/// command's own return.
#[tauri::command]
pub fn stop_live_dictation(app: AppHandle, state: tauri::State<LiveDictationState>) -> Result<(), String> {
    let handle = {
        let mut guard = state.0.lock().map_err(|_| "Dictation state lock was poisoned".to_string())?;
        guard.take().ok_or("No dictation session is running.".to_string())?
    };

    handle.stop_flag.store(true, Ordering::SeqCst);

    tauri::async_runtime::spawn(async move {
        let outcome = handle.transcribe_task.await;
        let _ = tokio::task::spawn_blocking(move || handle.capture_thread.join()).await;

        let payload = match outcome {
            Ok(words) => LiveFinal { words: Some(words), error: None },
            Err(e) => LiveFinal { words: None, error: Some(format!("Live dictation transcription task panicked: {e}")) },
        };
        let _ = app.emit("live-dictation-final", payload);
    });

    Ok(())
}
