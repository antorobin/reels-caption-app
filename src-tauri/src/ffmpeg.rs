// Async ffmpeg invocation with live progress reporting, shared by the
// audio-extraction and caption-burning steps. Uses `tokio::process` instead
// of `std::process` so a long re-encode doesn't block the thread handling
// Tauri's IPC — that was the original cause of the UI freezing during
// "Burn captions".

use std::process::Stdio;
use std::sync::OnceLock;
use tauri::AppHandle;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;

use crate::util::emit_progress;

/// Best-effort media duration lookup via ffprobe, used to turn ffmpeg's
/// raw "time processed so far" progress into a percentage. Returns `None`
/// (rather than an error) if ffprobe is missing or the duration can't be
/// parsed — progress events still fire, just without a percent.
pub async fn probe_duration_seconds(path: &str) -> Option<f64> {
    let output = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", path])
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout).trim().parse::<f64>().ok()
}

/// A hardware encoder is usually a much bigger win than any amount of CPU
/// thread/segment tuning — it's dedicated silicon instead of borrowed CPU
/// cycles, often 5-10x faster than even `libx264 -preset veryfast`. Detect
/// what this machine's ffmpeg build actually has available (checked once
/// and cached; this shells out and greps ~100+ lines of encoder listings,
/// not something to redo per burn job) and prefer it. `libass` subtitle
/// rendering itself stays on the CPU either way (there's no portable GPU
/// path for it), but the expensive part — encoding — gets offloaded.
static BEST_ENCODER: OnceLock<Encoder> = OnceLock::new();

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
    let output = Command::new("ffmpeg")
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
    if let Some(cached) = BEST_ENCODER.get() {
        return *cached;
    }

    let chosen = if encoder_actually_works("h264_nvenc").await {
        Encoder::Nvenc
    } else if encoder_actually_works("h264_qsv").await {
        Encoder::Qsv
    } else if encoder_actually_works("h264_amf").await {
        Encoder::Amf
    } else {
        Encoder::SoftwareX264
    };

    *BEST_ENCODER.get_or_init(|| chosen)
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

    let mut child = Command::new("ffmpeg")
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
    stage: &str,
    args: Vec<String>,
    duration_secs: Option<f64>,
) -> Result<(), String> {
    emit_progress(app, event_name, stage, Some(0.0), None);

    let app_progress = app.clone();
    let event_name_owned = event_name.to_string();
    let stage_owned = stage.to_string();
    run_capturing_progress(args, move |seconds| {
        let percent = duration_secs.filter(|d| *d > 0.0).map(|d| (seconds / d * 100.0).clamp(0.0, 100.0));
        emit_progress(&app_progress, &event_name_owned, &stage_owned, percent, None);
    })
    .await?;

    emit_progress(app, event_name, stage, Some(100.0), None);
    Ok(())
}
