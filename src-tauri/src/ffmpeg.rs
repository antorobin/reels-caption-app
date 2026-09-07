// Async ffmpeg invocation with live progress reporting, shared by the
// audio-extraction and caption-burning steps. Uses `tokio::process` instead
// of `std::process` so a long re-encode doesn't block the thread handling
// Tauri's IPC — that was the original cause of the UI freezing during
// "Burn captions".

use std::process::Stdio;
use tauri::AppHandle;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;

use crate::util::{cli_path, emit_progress};

/// Best-effort media duration lookup via ffprobe, used to turn ffmpeg's
/// raw "time processed so far" progress into a percentage. Returns `None`
/// (rather than an error) if ffprobe is missing or the duration can't be
/// parsed — progress events still fire, just without a percent.
pub async fn probe_duration_seconds(path: &str) -> Option<f64> {
    let output = Command::new(crate::bin_paths::ffprobe_path())
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", path])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout).trim().parse::<f64>().ok()
}

/// Best-effort width/height/fps lookup, needed only when video
/// transitions are requested (see video_transitions.rs) -- their filter
/// graph needs the real frame size to crop a zoom back down exactly and
/// to size the flash overlay's solid-color source, unlike ASS captions
/// (which scale via PlayResX/Y regardless of actual resolution). Returns
/// `None` on any probe failure or an unparseable/zero frame rate --
/// callers treat that the same as "no transitions requested" rather than
/// failing the whole burn over a bonus visual effect.
pub async fn probe_video_dimensions(path: &str) -> Option<(u32, u32, f64)> {
    let output = Command::new(crate::bin_paths::ffprobe_path())
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,r_frame_rate",
            "-of",
            "csv=p=0",
            path,
        ])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    let mut parts = text.trim().split(',');
    let width: u32 = parts.next()?.parse().ok()?;
    let height: u32 = parts.next()?.parse().ok()?;
    let mut fps_parts = parts.next()?.split('/'); // e.g. "30/1" or "30000/1001"
    let num: f64 = fps_parts.next()?.parse().ok()?;
    let den: f64 = fps_parts.next().unwrap_or("1").parse().ok()?;
    if den == 0.0 {
        return None;
    }
    Some((width, height, num / den))
}

/// Whether `path` has at least one audio stream -- checked before
/// extracting audio for transcription, since a silent video (real content
/// here: B-roll meant to get a generated voice-over, not a recording
/// error) previously hit ffmpeg's own "Output file does not contain any
/// stream" error and surfaced a raw stack trace as the very first thing a
/// user saw after picking a video. Returns `false` (not an error) on any
/// probe failure -- callers already treat "no audio" as a normal, expected
/// case to route around, not a reason to fail outright.
pub async fn has_audio_stream(path: &str) -> bool {
    let output = Command::new(crate::bin_paths::ffprobe_path())
        .args(["-v", "error", "-select_streams", "a", "-show_entries", "stream=index", "-of", "csv=p=0", path])
        .output()
        .await;

    match output {
        Ok(o) => o.status.success() && !String::from_utf8_lossy(&o.stdout).trim().is_empty(),
        Err(_) => false,
    }
}

/// Repeats a shorter audio file to fill `target_duration_seconds` --
/// `music_gen.rs` needs this because generating a bed directly at the
/// video's full length is capped (real CPU generation time scales with
/// requested length), so a longer video gets a shorter bed looped instead.
/// `-stream_loop -1` on the input repeats it indefinitely; `-t` then cuts
/// the output to the exact target length regardless of how evenly the loop
/// count divides into it. `-c copy` avoids a needless re-encode of already-
/// decoded PCM. Known v1 trade-off, not addressed here: the loop point
/// itself isn't crossfaded, so a hard seam can be audible on a very
/// short/percussive bed.
pub async fn loop_audio_to_duration(input_path: &std::path::Path, output_path: &std::path::Path, target_duration_seconds: f64) -> Result<(), String> {
    let output = Command::new(crate::bin_paths::ffmpeg_path())
        .args([
            "-y".to_string(),
            "-stream_loop".to_string(),
            "-1".to_string(),
            "-i".to_string(),
            cli_path(input_path),
            "-t".to_string(),
            target_duration_seconds.to_string(),
            "-c".to_string(),
            "copy".to_string(),
            cli_path(output_path),
        ])
        .output()
        .await
        .map_err(|e| format!("Failed to run ffmpeg to loop the music bed: {e}"))?;

    if !output.status.success() {
        return Err(format!("Failed to loop the music bed to length: {}", String::from_utf8_lossy(&output.stderr)));
    }
    Ok(())
}

/// How sensitive `detect_speech_segments` is to what counts as "silence" --
/// anything quieter than this, for at least `SILENCE_MIN_DURATION_SECONDS`,
/// is treated as a gap between speech segments. Tuned for already-extracted
/// 16kHz mono PCM speech audio (see pipeline.rs's `extract_audio`), not
/// arbitrary source material.
const SILENCE_NOISE_THRESHOLD_DB: &str = "-35dB";
const SILENCE_MIN_DURATION_SECONDS: f64 = 0.5;

/// Language-agnostic speech segmentation via ffmpeg's own `silencedetect`
/// filter, run directly on the waveform -- deliberately NOT derived from
/// word-level timestamps the way `jumpcuts.rs`'s silence-gap logic is.
/// `mixed_language.rs` needs segment boundaries *before* any transcription
/// happens (it doesn't yet know what language each segment is in, which is
/// the whole point), so word timestamps -- which would require already
/// having transcribed the file in some one language -- aren't available yet
/// and wouldn't be trustworthy for the "wrong" segments anyway.
pub async fn detect_speech_segments(audio_path: &std::path::Path) -> Result<Vec<(f64, f64)>, String> {
    let audio_path_arg = cli_path(audio_path);
    let duration = probe_duration_seconds(&audio_path_arg)
        .await
        .ok_or("Couldn't determine audio duration for speech segmentation")?;

    let filter = format!("silencedetect=noise={SILENCE_NOISE_THRESHOLD_DB}:d={SILENCE_MIN_DURATION_SECONDS}");
    let output = Command::new(crate::bin_paths::ffmpeg_path())
        .args(["-i", &audio_path_arg, "-af", &filter, "-f", "null", "-"])
        .output()
        .await
        .map_err(|e| format!("Failed to run ffmpeg silencedetect: {e}"))?;

    // silencedetect reports on stderr regardless of success/failure of the
    // (nonexistent, "-f null") output -- not a real error signal here.
    let stderr_text = String::from_utf8_lossy(&output.stderr);

    let mut silences: Vec<(f64, f64)> = Vec::new();
    let mut pending_start: Option<f64> = None;
    for line in stderr_text.lines() {
        if let Some(rest) = line.split("silence_start: ").nth(1) {
            pending_start = rest.split_whitespace().next().and_then(|s| s.parse::<f64>().ok());
        } else if let Some(rest) = line.split("silence_end: ").nth(1) {
            if let Some(start) = pending_start.take() {
                let end = rest.split(|c: char| c == '|' || c.is_whitespace()).next().and_then(|s| s.parse::<f64>().ok());
                if let Some(end) = end {
                    silences.push((start, end));
                }
            }
        }
    }

    // Speech segments are the complement of the detected silences, bounded
    // by [0, duration].
    let mut segments = Vec::new();
    let mut cursor = 0.0;
    for (silence_start, silence_end) in silences {
        if silence_start > cursor {
            segments.push((cursor, silence_start));
        }
        cursor = silence_end.max(cursor);
    }
    if cursor < duration {
        segments.push((cursor, duration));
    }

    Ok(segments)
}

/// A hardware encoder is usually a much bigger win than any amount of CPU
/// thread/segment tuning — it's dedicated silicon instead of borrowed CPU
/// cycles, often 5-10x faster than even `libx264 -preset veryfast`. Detect
/// what this machine's ffmpeg build actually has available (checked once
/// and cached; this shells out and greps ~100+ lines of encoder listings,
/// not something to redo per burn job) and prefer it. `libass` subtitle
/// rendering itself stays on the CPU either way (there's no portable GPU
/// path for it), but the expensive part — encoding — gets offloaded.
///
/// `tokio::sync::OnceCell`, not `std::sync::OnceLock` — this needs an
/// *async* initializer (the hardware probe below spawns ffmpeg
/// subprocesses), and `OnceLock::get_or_init` only accepts a sync closure.
/// The previous check-then-await-then-`get_or_init` pattern this used to
/// be had a real race under concurrency (now possible now that more than
/// one project can process at once — see concurrency.rs): two callers
/// could both observe `get()` as `None` and both run the full multi-probe
/// sequence before either stored the result. `OnceCell::get_or_init`
/// guarantees exactly one initializing future ever runs even if several
/// callers race in — the same pattern already proven correct by
/// `tts.rs`'s `resolve_tts_prefix`.
static BEST_ENCODER: tokio::sync::OnceCell<Encoder> = tokio::sync::OnceCell::const_new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoder {
    Nvenc,
    Qsv,
    Amf,
    SoftwareX264,
}

impl Encoder {
    pub fn codec_name(self) -> &'static str {
        match self {
            Encoder::Nvenc => "h264_nvenc",
            Encoder::Qsv => "h264_qsv",
            Encoder::Amf => "h264_amf",
            Encoder::SoftwareX264 => "libx264",
        }
    }

    /// Each encoder has its own preset/threading vocabulary — there's no
    /// shared `-preset veryfast` across hardware vendors.
    pub fn speed_args(self, threads: usize) -> Vec<String> {
        match self {
            Encoder::Nvenc => vec!["-preset".into(), "p1".into(), "-tune".into(), "hq".into()],
            Encoder::Qsv => vec!["-preset".into(), "veryfast".into()],
            Encoder::Amf => vec!["-quality".into(), "speed".into()],
            Encoder::SoftwareX264 => {
                vec!["-preset".into(), "veryfast".into(), "-threads".into(), threads.to_string()]
            }
        }
    }
}

/// `ffmpeg -encoders` only lists what the ffmpeg *build* was compiled
/// with support for — it says nothing about whether this machine actually
/// has working hardware/drivers for it. Confirmed empirically during
/// development: a build that lists h264_nvenc/h264_qsv/h264_amf all
/// "available" only actually worked for h264_qsv here (an Intel iGPU);
/// nvenc and amf both failed at encode time with no matching hardware.
/// So instead of trusting the list, actually try a trivial 1-frame encode
/// with each candidate and see if it succeeds.
async fn encoder_actually_works(codec: &str) -> bool {
    let output = Command::new(crate::bin_paths::ffmpeg_path())
        .args([
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=1:size=320x240:rate=5",
            "-frames:v",
            "1",
            "-c:v",
            codec,
            "-f",
            "null",
            "-",
        ])
        .output()
        .await;

    matches!(output, Ok(o) if o.status.success())
}

/// Detects and caches the best available H.264 encoder on this machine.
pub async fn best_encoder() -> Encoder {
    *BEST_ENCODER
        .get_or_init(|| async {
            if encoder_actually_works("h264_nvenc").await {
                Encoder::Nvenc
            } else if encoder_actually_works("h264_qsv").await {
                Encoder::Qsv
            } else if encoder_actually_works("h264_amf").await {
                Encoder::Amf
            } else {
                Encoder::SoftwareX264
            }
        })
        .await
}

/// Runs ffmpeg with `args` (everything except `-progress`, which this adds
/// itself), invoking `on_seconds` with the "time processed so far" (in
/// seconds) as ffmpeg reports it. stdout and stderr are drained
/// concurrently so a chatty stderr (ffmpeg's normal logging) can never
/// fill its pipe buffer and deadlock the child process while we're only
/// reading stdout.
pub async fn run_capturing_progress(
    args: Vec<String>,
    on_seconds: impl Fn(f64) + Send + 'static,
) -> Result<(), String> {
    let mut full_args = vec!["-progress".to_string(), "pipe:1".to_string(), "-nostats".to_string()];
    full_args.extend(args);

    let mut child = Command::new(crate::bin_paths::ffmpeg_path())
        .args(&full_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to run ffmpeg: {e}"))?;

    let stdout = child.stdout.take().expect("ffmpeg stdout was piped");
    let stderr = child.stderr.take().expect("ffmpeg stderr was piped");

    let progress_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Some(us) = line.strip_prefix("out_time_us=") else { continue };
            let Ok(us) = us.trim().parse::<f64>() else { continue };
            on_seconds((us / 1_000_000.0).max(0.0));
        }
    });

    let stderr_task = tokio::spawn(async move {
        let mut buf = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut buf).await;
        buf
    });

    let status = child.wait().await.map_err(|e| format!("Failed to wait on ffmpeg: {e}"))?;
    let _ = progress_task.await;
    let stderr_text = stderr_task.await.unwrap_or_default();

    if !status.success() {
        return Err(format!("ffmpeg failed: {stderr_text}"));
    }

    Ok(())
}

/// Convenience wrapper over [`run_capturing_progress`] for the common case
/// of a single ffmpeg job whose progress should go straight to the
/// frontend as a `stage`-labeled event under `event_name`.
pub async fn run_with_progress(
    app: &AppHandle,
    event_name: &str,
    project_id: &str,
    stage: &str,
    args: Vec<String>,
    duration_secs: Option<f64>,
) -> Result<(), String> {
    emit_progress(app, event_name, project_id, stage, Some(0.0), None);

    let app_progress = app.clone();
    let event_name_owned = event_name.to_string();
    let project_id_owned = project_id.to_string();
    let stage_owned = stage.to_string();
    run_capturing_progress(args, move |seconds| {
        let percent = duration_secs.filter(|d| *d > 0.0).map(|d| (seconds / d * 100.0).clamp(0.0, 100.0));
        emit_progress(&app_progress, &event_name_owned, &project_id_owned, &stage_owned, percent, None);
    })
    .await?;

    emit_progress(app, event_name, project_id, stage, Some(100.0), None);
    Ok(())
}

