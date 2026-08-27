// Speech-to-text: NVIDIA Parakeet (English) + Indic Whisper fine-tunes (Indian languages),
// replacing the WhisperX-based pipeline this app used before.
//
// Why: the agenda for this rewrite was "excellent STT for English + Indian
// languages, optimized for CPU speed on a laptop, with reliable word-level
// alignment." WhisperX's generic multilingual faster-whisper model is not
// tuned for Indian languages specifically; NVIDIA Parakeet (TDT 0.6B v2,
// CC-BY-4.0) is both faster and more accurate than Whisper-family models on
// English per NVIDIA's own published benchmarks, and runs through the
// `onnx-asr` package — pure ONNX Runtime, no PyTorch/CTranslate2 needed at
// all for the English path. For Indian languages: AI4Bharat's IndicWhisper
// (whisper-medium fine-tunes) was the original plan, but its checkpoint
// (~1.4GB) consistently OOM'd during CTranslate2 conversion on this
// project's target laptop-class hardware. Tamil currently uses
// `vasista22/whisper-tamil-small` instead — IIT Madras's Apache-2.0
// fine-tune of whisper-small (244M params, ~1/3 the size), the same model
// this project used successfully in its earlier whisper.cpp era. Verified
// end-to-end on real Tamil audio: clean, monotonically increasing word
// timestamps and an accurate transcript. Swap in a whisper-medium-based
// checkpoint per-language later if a target machine has the RAM for it —
// nothing here is Tamil- or whisper-small-specific.
//
// Word timestamps for the Indic path come straight from faster-whisper's own
// native decoder output (`word_timestamps=True`), not a separate
// forced-alignment step. An earlier version of this pipeline reused
// WhisperX's `align()` function to forced-align against a wav2vec2 CTC
// model, reasoning that Whisper's native cross-attention timestamps drift
// on long-form audio (a real, confirmed problem this project hit before,
// on a different model). Real usage on Tamil audio surfaced the actual bug
// that reasoning missed: faster-whisper was producing occasional huge
// (20-30s), unsplit segments for this model, and forced-aligning a giant
// block of un-punctuated text against that much dense speech is exactly
// where CTC alignment falls apart — both the misaligned highlighting and
// the "missing" words users saw (never dropped by the ASR; they just
// failed to get a placeable timestamp during alignment and were silently
// filtered out). The real fix is capping faster-whisper's own decode
// window (`chunk_length` in indic_transcribe.py — not a VAD setting, which
// only affects what survives the pre-decode silence-stripping pass and has
// no effect on segment length): verified directly on a real 60s Tamil
// clip, every word timestamp landed under 1.1s once capped, versus an
// 11-second single-word artifact uncapped. This also means WhisperX and
// NLTK are no longer dependencies anywhere in this pipeline. Parakeet
// never needed a separate alignment step either: `onnx_asr`'s
// `with_timestamps()` adapter returns per-token timestamps directly,
// merged into words in parakeet_transcribe.py the same way this project
// has always merged sub-word ASR tokens into words.
//
// Language scope is deliberately narrower than the old "English / Tamil /
// Other (auto-detect)" — Parakeet is English-only and each Indic language
// needs its own converted checkpoint, so there is no remaining generic
// multilingual fallback to route "Other Language" to. Only Tamil is wired
// up so far; extending to another Indic language means adding its entry to
// INDIC_LANGUAGES below and running the same convert-to-CTranslate2 step
// (see resources/stt-models/README.md).
//
// Setup (not bundled — see README): a conda environment named "stt":
//   conda create -n stt python=3.10 -y
//   conda run -n stt pip install faster-whisper onnx-asr[cpu,hub] soundfile

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::pipeline::{WordTimestamp, PROGRESS_EVENT};
use crate::util::{cli_path, emit_progress, unique_temp_path};

pub const STT_ENV_NAME: &str = "stt";

static STT_PREFIX_CACHE: tokio::sync::OnceCell<PathBuf> = tokio::sync::OnceCell::const_new();

async fn resolve_stt_prefix() -> Result<PathBuf, String> {
    let prefix = STT_PREFIX_CACHE
        .get_or_try_init(|| async {
            let conda = crate::conda_util::resolve_conda_env(
                STT_ENV_NAME,
                &["python", "-c", "import onnx_asr, faster_whisper"],
                "REELS_CAPTION_APP_STT_CONDA_PATH",
            )
            .await?;
            crate::conda_util::resolve_conda_env_prefix(&conda, STT_ENV_NAME).await
        })
        .await?;
    Ok(prefix.clone())
}

fn python_exe(prefix: &Path) -> PathBuf {
    prefix.join("python.exe")
}

fn stt_script_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("stt").join(name)
}

/// Where a converted (CTranslate2, int8) Tamil Whisper checkpoint for
/// `language` is expected to live. Not bundled (large, per-language) — see
/// `resources/stt-models/README.md` for the download+convert steps. Checks,
/// in order: the packaged resource dir (a real `tauri build`), then
/// `resources/stt-models/<language>/` directly under the project (dev mode,
/// where `resource_dir()` doesn't point at `src-tauri/resources/`), then finally
/// `~/.reels-caption-app/stt-models/<language>/` so a machine that converted
/// a model once doesn't need to redo it per build.
fn indic_model_dir(app: &AppHandle, language: &str) -> PathBuf {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let candidate = resource_dir.join("stt-models").join(language);
        if candidate.exists() {
            return candidate;
        }
    }
    let dev_candidate =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources").join("stt-models").join(language);
    if dev_candidate.exists() {
        return dev_candidate;
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default();
    PathBuf::from(home).join(".reels-caption-app").join("stt-models").join(language)
}

#[derive(Debug, Deserialize)]
struct SttWord {
    word: String,
    start: f64,
    end: f64,
}

#[derive(Debug, Deserialize)]
struct SttOutput {
    words: Vec<SttWord>,
    language: Option<String>,
}

async fn run_stt_script(
    app: &AppHandle,
    args: Vec<String>,
) -> Result<SttOutput, String> {
    let prefix = resolve_stt_prefix().await?;
    let python = python_exe(&prefix);

    emit_progress(app, PROGRESS_EVENT, "transcribing", Some(0.0), None);

    let mut child = Command::new(&python)
        .args(&args)
        .env("PYTHONIOENCODING", "utf-8")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "STT engine not found ({e}). Set up the 'stt' conda environment \
                 (see README) — or if it's already installed somewhere this couldn't \
                 find, set REELS_CAPTION_APP_STT_CONDA_PATH."
            )
        })?;

    let stdout = child.stdout.take().expect("stt stdout was piped");
    let stderr = child.stderr.take().expect("stt stderr was piped");

    let app_clone = app.clone();
    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut full_text = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            full_text.push_str(&line);
            full_text.push('\n');
        }
        let _ = &app_clone;
        full_text
    });

    let stdout_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut full_text = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            full_text.push_str(&line);
            full_text.push('\n');
        }
        full_text
    });

    let status = child.wait().await.map_err(|e| format!("Failed to wait on STT engine: {e}"))?;
    let stdout_text = stdout_task.await.unwrap_or_default();
    let stderr_text = stderr_task.await.unwrap_or_default();

    if !status.success() {
        return Err(format!("STT engine failed: {stderr_text}"));
    }

    emit_progress(app, PROGRESS_EVENT, "transcribing", Some(100.0), None);

    let last_line = stdout_text.lines().last().unwrap_or_default();
    serde_json::from_str(last_line).map_err(|e| format!("Couldn't parse STT output: {e} (raw: {stdout_text})"))
}

async fn transcribe_english(app: &AppHandle, audio_path: &Path) -> Result<SttOutput, String> {
    run_stt_script(app, vec![cli_path(&stt_script_path("parakeet_transcribe.py")), cli_path(audio_path)]).await
}

async fn transcribe_indic(app: &AppHandle, audio_path: &Path, language: &str) -> Result<SttOutput, String> {
    let model_dir = indic_model_dir(app, language);
    if !model_dir.exists() {
        return Err(format!(
            "No converted Indic Whisper model found for '{language}' at {} — see \
             resources/stt-models/README.md for the download+convert steps.",
            model_dir.display()
        ));
    }
    run_stt_script(
        app,
        vec![
            cli_path(&stt_script_path("indic_transcribe.py")),
            cli_path(audio_path),
            "--language".to_string(),
            language.to_string(),
            "--model-dir".to_string(),
            cli_path(&model_dir),
        ],
    )
    .await
}

/// Transcribes `audio_path`, routing to Parakeet for English or an Indic
/// Whisper fine-tune for a supported Indian language. `language` must be
/// `"en"` or one of `INDIC_LANGUAGES`.
pub const INDIC_LANGUAGES: &[(&str, &str)] = &[("ta", "Tamil")];

/// True for any language code this pipeline can actually transcribe
/// (English, or one of `INDIC_LANGUAGES`) — used to reject an
/// auto-detected language this app has no model for, with a clear error
/// rather than a confusing downstream failure.
pub fn is_supported_language_code(code: &str) -> bool {
    code == "en" || INDIC_LANGUAGES.iter().any(|(c, _)| *c == code)
}

/// Human-readable names of every language this app supports, for display
/// (the "Supported languages" line in the UI) and for error messages when
/// an auto-detected language isn't one of them.
pub fn supported_language_names() -> Vec<&'static str> {
    let mut names = vec!["English"];
    names.extend(INDIC_LANGUAGES.iter().map(|(_, name)| *name));
    names
}

/// Maps a language code back to its display name (falls back to the raw
/// code for anything unrecognized, though callers should have already
/// rejected those via `is_supported_language_code`).
pub fn language_name_for_code(code: &str) -> String {
    if code == "en" {
        return "English".to_string();
    }
    INDIC_LANGUAGES
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, name)| name.to_string())
        .unwrap_or_else(|| code.to_string())
}

#[tauri::command]
pub fn get_supported_languages() -> Vec<&'static str> {
    supported_language_names()
}

#[derive(Debug, Deserialize)]
struct SegmentLanguageResult {
    start: f64,
    end: f64,
    language: String,
    probability: f64,
}

/// Identifies the spoken language of each of `segments` via a small
/// pre-converted Whisper-tiny checkpoint (`Systran/faster-whisper-tiny`),
/// used purely for its `detect_language()` call -- never for actual
/// transcription, which stays on Parakeet/the per-language Indic Whisper
/// fine-tunes above. Loaded once and run over every segment within a
/// single process (`detect_language_segments.py`) rather than once per
/// segment, which `mixed_language.rs` can have dozens of and would
/// otherwise pay full model-load time (a meaningful fraction of a second
/// each) for repeatedly. Runs in the same "stt" conda env (it already
/// depends on faster-whisper) -- no new environment needed. Verified
/// directly on this project's own test audio: 98.7% confidence on
/// English, 91-95% on Tamil.
pub async fn detect_spoken_languages_batch(
    audio_path: &Path,
    segments: &[(f64, f64)],
) -> Result<Vec<(f64, f64, String, f64)>, String> {
    if segments.is_empty() {
        return Ok(Vec::new());
    }
    let prefix = resolve_stt_prefix().await?;
    let python = python_exe(&prefix);

    let segments_json_path = unique_temp_path("lang-segments", "json");
    let segments_json = serde_json::to_string(segments).map_err(|e| format!("Couldn't serialize segments: {e}"))?;
    std::fs::write(&segments_json_path, segments_json).map_err(|e| format!("Couldn't write segments file: {e}"))?;

    let result = Command::new(&python)
        .args([
            cli_path(&stt_script_path("detect_language_segments.py")),
            cli_path(audio_path),
            "--segments".to_string(),
            cli_path(&segments_json_path),
        ])
        .env("PYTHONIOENCODING", "utf-8")
        .env("HF_HUB_DISABLE_SYMLINKS", "1")
        .output()
        .await
        .map_err(|e| {
            format!(
                "Language-detection engine not found ({e}). Set up the 'stt' conda \
                 environment (see README) — or if it's already installed somewhere \
                 this couldn't find, set REELS_CAPTION_APP_STT_CONDA_PATH."
            )
        });

    let _ = std::fs::remove_file(&segments_json_path);
    let output = result?;

    if !output.status.success() {
        return Err(format!("Segment language detection failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    let stdout_text = String::from_utf8_lossy(&output.stdout);
    let last_line = stdout_text.lines().last().unwrap_or_default();
    let parsed: Vec<SegmentLanguageResult> = serde_json::from_str(last_line)
        .map_err(|e| format!("Couldn't parse segment language-detection output: {e} (raw: {stdout_text})"))?;

    Ok(parsed.into_iter().map(|r| (r.start, r.end, r.language, r.probability)).collect())
}

pub async fn transcribe_and_align(
    app: &AppHandle,
    audio_path: &Path,
    language: &str,
) -> Result<(Vec<WordTimestamp>, Option<String>), String> {
    let output = if language == "en" {
        transcribe_english(app, audio_path).await?
    } else if INDIC_LANGUAGES.iter().any(|(code, _)| *code == language) {
        transcribe_indic(app, audio_path, language).await?
    } else {
        return Err(format!(
            "Unsupported language code '{language}' — this pipeline supports English (en) and: {}",
            INDIC_LANGUAGES.iter().map(|(_, name)| *name).collect::<Vec<_>>().join(", ")
        ));
    };

    let words = output.words.into_iter().map(|w| WordTimestamp { word: w.word, start: w.start, end: w.end }).collect();
    Ok((words, output.language))
}
