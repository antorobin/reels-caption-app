// Voice cloning: reshapes an already-synthesized voiceover clip's timbre to
// match the original video's own speaker, via OpenVoice V2's
// `ToneColorConverter` (MIT license, https://github.com/myshell-ai/OpenVoice).
// This is deliberately a post-process layered on top of tts.rs's existing
// Piper/MMS-TTS synthesis, not a cloning-native TTS engine replacing it --
// the converter reshapes acoustic timbre only, not linguistic content, so
// it works the same way regardless of the synthesized language (English or
// Tamil). A cloning-native model like XTTS was rejected specifically
// because it doesn't support Tamil at all.
//
// Its own dedicated conda env, not folded into `tts` or `media-ai`:
// OpenVoice's own requirements.txt pins old versions of dependencies both
// of those envs already carry newer copies of (faster-whisper==0.9.0,
// librosa==0.9.1, numpy==1.22.0), and drags in Chinese/Japanese
// text-normalization packages and gradio this app has no use for --
// isolating it avoids any version-conflict risk to the working stt/tts/
// media-ai envs.
//
// Setup (not bundled — see README's full setup section for the verified
// working install recipe, the checkpoint download, and two real gotchas
// found via direct testing: `pip install git+...` alone fails outright
// (OpenVoice's requirements.txt pins `faster-whisper==0.9.0`, which pulls
// an ancient `av==10.*` with no prebuilt Windows wheel and a source build
// that fails; `--no-deps` + installing its actual runtime deps with modern
// versions sidesteps this entirely) and the first real cloning call may
// need one manual "yes" to a `torch.hub` prompt trusting
// `snakers4/silero-vad` (cached per-machine after that, so this is truly
// one-time — verified directly: closing stdin, matching how this module's
// own subprocess call runs it, succeeds silently on a second call).

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::media_ai::run_media_ai_script;
use crate::util::{cli_path, unique_temp_path};

/// Where the OpenVoice V2 converter checkpoint (`config.json` +
/// `checkpoint.pth`) is expected to live -- not bundled (large, and
/// license-permitting-but-third-party, same treatment as the Indic Whisper
/// checkpoints). Same three-tier lookup as `stt::indic_model_dir`: a real
/// `tauri build`'s resource dir, then the dev-mode project-relative path,
/// then a per-machine cache dir so converting/downloading once doesn't need
/// repeating per build.
fn voice_clone_checkpoint_dir(app: &AppHandle) -> PathBuf {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let candidate = resource_dir.join("voice-clone-models").join("converter");
        if candidate.exists() {
            return candidate;
        }
    }
    let dev_candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("voice-clone-models")
        .join("converter");
    if dev_candidate.exists() {
        return dev_candidate;
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default();
    PathBuf::from(home).join(".reels-caption-app").join("voice-clone-models").join("converter")
}

pub const VOICE_CLONE_ENV_NAME: &str = "voice-clone";

static VOICE_CLONE_PREFIX_CACHE: tokio::sync::OnceCell<PathBuf> = tokio::sync::OnceCell::const_new();

async fn resolve_voice_clone_prefix() -> Result<PathBuf, String> {
    let prefix = VOICE_CLONE_PREFIX_CACHE
        .get_or_try_init(|| async {
            let conda = crate::conda_util::resolve_conda_env(
                VOICE_CLONE_ENV_NAME,
                &["python", "-c", "import openvoice, torch"],
                "REELS_CAPTION_APP_VOICE_CLONE_CONDA_PATH",
            )
            .await?;
            crate::conda_util::resolve_conda_env_prefix(&conda, VOICE_CLONE_ENV_NAME).await
        })
        .await?;
    Ok(prefix.clone())
}

fn python_exe(prefix: &Path) -> PathBuf {
    prefix.join("python.exe")
}

fn voice_clone_script_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("voice_clone").join(name)
}

/// How much of the video's own audio to hand to the tone-color extractor as
/// a reference clip -- long enough for a stable speaker embedding, short
/// enough to bound extraction/embedding time (this isn't the full clip,
/// just a sample of the speaker's voice). Also comfortably clears
/// OpenVoice's own minimum-length floor for `se_extractor.get_se()` --
/// confirmed directly: a ~5s clip failed with "input audio is too short,"
/// a ~15-17s clip worked cleanly.
const REFERENCE_CLIP_SECONDS: f64 = 30.0;

/// Pulls a bounded reference clip of `video_path`'s own audio, for voice
/// cloning to extract a target speaker embedding from. Independent of
/// `pipeline::extract_audio` (whose output is ephemeral, already deleted by
/// the time a voiceover is generated) -- same "pull what you need
/// yourself" pattern `vo_sync.py`'s Rust caller already uses. Callers must
/// check `ffmpeg::has_audio_stream` first; this doesn't re-check.
pub async fn extract_reference_clip(video_path: &str) -> Result<PathBuf, String> {
    let reference_path = unique_temp_path("voice-reference", "wav");
    let reference_path_arg = cli_path(&reference_path);

    let output = Command::new(crate::bin_paths::ffmpeg_path())
        .args([
            "-y",
            "-i",
            video_path,
            "-t",
            &REFERENCE_CLIP_SECONDS.to_string(),
            "-vn",
            "-acodec",
            "pcm_s16le",
            "-ar",
            "16000",
            "-ac",
            "1",
        ])
        .arg(&reference_path_arg)
        .output()
        .await
        .map_err(|e| format!("Failed to run ffmpeg: {e}"))?;

    if !output.status.success() {
        return Err(format!(
            "ffmpeg failed to extract a reference clip: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(reference_path)
}

#[derive(Debug, Deserialize)]
struct GenderOutput {
    gender: Option<String>,
}

/// Rough gender classification of a reference clip's speaker, via median
/// pitch (librosa.pyin) in the existing `media-ai` env -- used only by the
/// gender-matched fallback tier when real cloning isn't possible. `None`
/// (rather than a guess) when pitch tracking couldn't get a confident read.
pub async fn detect_reference_gender(app: &AppHandle, reference_wav: &Path) -> Result<Option<String>, String> {
    // `reference_wav`'s own path doubles as the scoping key -- a quick,
    // fast media-ai utility step (see media_ai.rs's own doc comment) run
    // ahead of the actual heavy voiceover synthesis, not itself part of
    // the background-routed heavy work that needs a real project id.
    let reference_wav_str = cli_path(reference_wav);
    let stdout = run_media_ai_script(
        app,
        "detect_gender.py",
        vec![reference_wav_str.clone()],
        "voice-clone-progress",
        &reference_wav_str,
        "detecting_gender",
    )
    .await?;
    let parsed: GenderOutput =
        serde_json::from_str(&stdout).map_err(|e| format!("Couldn't parse detect_gender.py output: {e} (raw: {stdout})"))?;
    Ok(parsed.gender)
}

#[derive(Debug, Deserialize)]
struct CloneOutput {
    output_path: String,
}

/// Reshapes `source_wav` (already-synthesized voiceover audio) so its
/// timbre matches the speaker in `reference_wav` (a clip of the original
/// video's own audio), via OpenVoice V2's `ToneColorConverter`. Content and
/// language are untouched -- only acoustic timbre changes.
pub async fn clone_voice(app: &AppHandle, source_wav: &Path, reference_wav: &Path) -> Result<PathBuf, String> {
    let checkpoint_dir = voice_clone_checkpoint_dir(app);
    if !checkpoint_dir.exists() {
        return Err(format!(
            "No OpenVoice V2 converter checkpoint found at {} — see README's voice cloning setup section.",
            checkpoint_dir.display()
        ));
    }

    let prefix = resolve_voice_clone_prefix().await?;
    let python = python_exe(&prefix);
    let output_path = unique_temp_path("voice-cloned", "wav");

    let mut child = Command::new(&python)
        .args([
            cli_path(&voice_clone_script_path("clone_voice.py")),
            "--source".to_string(),
            cli_path(source_wav),
            "--reference".to_string(),
            cli_path(reference_wav),
            "--checkpoint-dir".to_string(),
            cli_path(&checkpoint_dir),
            "--output".to_string(),
            cli_path(&output_path),
        ])
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "Voice-cloning engine not found ({e}). Set up the 'voice-clone' conda \
                 environment (see README) — or if it's already installed somewhere this \
                 couldn't find, set REELS_CAPTION_APP_VOICE_CLONE_CONDA_PATH."
            )
        })?;

    let stdout = child.stdout.take().expect("voice-clone stdout was piped");
    let stderr = child.stderr.take().expect("voice-clone stderr was piped");

    let stdout_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut full_text = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            full_text.push_str(&line);
            full_text.push('\n');
        }
        full_text
    });
    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut full_text = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            full_text.push_str(&line);
            full_text.push('\n');
        }
        full_text
    });

    let status = child.wait().await.map_err(|e| format!("Failed to wait on voice-cloning engine: {e}"))?;
    let stdout_text = stdout_task.await.unwrap_or_default();
    let stderr_text = stderr_task.await.unwrap_or_default();

    if !status.success() {
        return Err(format!("Voice cloning failed: {stderr_text}"));
    }

    let last_line = stdout_text.lines().last().unwrap_or_default();
    let parsed: CloneOutput = serde_json::from_str(last_line)
        .map_err(|e| format!("Couldn't parse voice-cloning output: {e} (raw: {stdout_text})"))?;
    Ok(PathBuf::from(parsed.output_path))
}
