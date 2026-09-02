// Text-to-speech voiceover generation: Piper (English) + MMS-TTS (Indian
// languages) -- the same "small, dedicated, CPU-friendly model per
// language" shape as stt.rs, for the same reasons (see stt.rs's module doc
// comment): both are lightweight VITS-family models chosen specifically to
// fit this project's laptop-CPU, RAM-constrained target hardware, over
// heavier alternatives.
//
// Piper (~63MB per voice, ONNX Runtime) is about as fast as neural TTS
// gets on CPU -- confirmed directly: a warm process synthesizes close to
// real-time, though a fresh process pays a one-time ~10s ONNX graph
// warm-up on its first call. MMS-TTS (Meta, per-language VITS checkpoints
// via `transformers`+torch, downloaded on demand -- no manual
// CTranslate2-style conversion step needed) is meaningfully slower --
// confirmed directly: consistently ~3x slower than real-time, with no
// warm-up speedup the way Piper has. That's still fine for generating a
// voiceover file (not a live/interactive use case), and uses Meta's own
// officially-published checkpoints rather than a community-converted ONNX
// export of uncertain freshness -- the same reasoning that favored an
// official GGUF over a third-party quant for llm.rs.
//
// Language scope mirrors stt.rs: English always available, Indian
// languages limited to whatever TTS_INDIC_LANGUAGES lists a Meta MMS-TTS
// mapping for (see mms_synthesize.py's LANGUAGE_TO_MMS_CODE) -- adding a
// language means adding its entry to both.
//
// Setup (not bundled — see README): a conda environment named "tts":
//   conda create -n tts python=3.10 -y
//   conda run -n tts pip install piper-tts
//   conda run -n tts pip install torch --index-url https://download.pytorch.org/whl/cpu
//   conda run -n tts pip install transformers scipy

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::util::{cli_path, unique_temp_path};

pub const TTS_ENV_NAME: &str = "tts";

static TTS_PREFIX_CACHE: tokio::sync::OnceCell<PathBuf> = tokio::sync::OnceCell::const_new();

/// `pub(crate)` -- `music_gen.rs` reuses this directly rather than
/// duplicating conda-env resolution, since MusicGen generation lives in
/// this exact same "tts" env (already has `torch`+`transformers`+`scipy`,
/// which is all it needs too). Its own probe (`import piper, transformers`)
/// still passes for that use -- it's just checking the env is real and has
/// the shared dependency, not that every caller imports every package.
pub(crate) async fn resolve_tts_prefix() -> Result<PathBuf, String> {
    let prefix = TTS_PREFIX_CACHE
        .get_or_try_init(|| async {
            let conda = crate::conda_util::resolve_conda_env(
                TTS_ENV_NAME,
                &["python", "-c", "import piper, transformers"],
                "REELS_CAPTION_APP_TTS_CONDA_PATH",
            )
            .await?;
            crate::conda_util::resolve_conda_env_prefix(&conda, TTS_ENV_NAME).await
        })
        .await?;
    Ok(prefix.clone())
}

pub(crate) fn python_exe(prefix: &Path) -> PathBuf {
    prefix.join("python.exe")
}

pub(crate) fn tts_script_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tts").join(name)
}

/// Default English Piper voice (female) — used whenever the fallback tier
/// can't determine a closer gender match (no reference audio, or pitch
/// tracking too unconfident).
pub const DEFAULT_ENGLISH_VOICE: &str = "en_US-lessac-medium";
/// Male English Piper voice, bundled alongside the default so the
/// gender-matched fallback tier has a real second option — see
/// `voice_clone::detect_reference_gender`.
pub const MALE_ENGLISH_VOICE: &str = "en_US-hfc_male-medium";

/// Where a bundled Piper voice (`voice_id`, e.g. "en_US-lessac-medium")
/// lives. Small enough (~63MB each) to bundle like the fonts/llama.cpp
/// resources, unlike the STT/media-ai models which are left to
/// per-machine setup.
fn piper_voice_path(app: &AppHandle, voice_id: &str) -> Result<PathBuf, String> {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let candidate = resource_dir.join("tts-models").join("en").join(format!("{voice_id}.onnx"));
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    let dev_candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("tts-models")
        .join("en")
        .join(format!("{voice_id}.onnx"));
    if dev_candidate.exists() {
        return Ok(dev_candidate);
    }
    Err(format!("Piper voice '{voice_id}' not found in resources/tts-models/en/ — see README's voiceover setup section"))
}

#[derive(Debug, Deserialize)]
struct TtsOutput {
    output_path: String,
}

async fn run_tts_script(args: Vec<String>) -> Result<PathBuf, String> {
    let prefix = resolve_tts_prefix().await?;
    let python = python_exe(&prefix);

    let mut child = Command::new(&python)
        .args(&args)
        .env("PYTHONIOENCODING", "utf-8")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "TTS engine not found ({e}). Set up the 'tts' conda environment \
                 (see README) — or if it's already installed somewhere this couldn't \
                 find, set REELS_CAPTION_APP_TTS_CONDA_PATH."
            )
        })?;

    let stdout = child.stdout.take().expect("tts stdout was piped");
    let stderr = child.stderr.take().expect("tts stderr was piped");

    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut full_text = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            full_text.push_str(&line);
            full_text.push('\n');
        }
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

    let status = child.wait().await.map_err(|e| format!("Failed to wait on TTS engine: {e}"))?;
    let stdout_text = stdout_task.await.unwrap_or_default();
    let stderr_text = stderr_task.await.unwrap_or_default();

    if !status.success() {
        return Err(format!("TTS engine failed: {stderr_text}"));
    }

    let last_line = stdout_text.lines().last().unwrap_or_default();
    let output: TtsOutput =
        serde_json::from_str(last_line).map_err(|e| format!("Couldn't parse TTS output: {e} (raw: {stdout_text})"))?;
    Ok(PathBuf::from(output.output_path))
}

async fn synthesize_english(
    app: &AppHandle,
    text: &str,
    voice_id: &str,
    output_path: &Path,
    length_scale: f32,
) -> Result<PathBuf, String> {
    let voice = piper_voice_path(app, voice_id)?;
    run_tts_script(vec![
        cli_path(&tts_script_path("piper_synthesize.py")),
        text.to_string(),
        "--model-path".to_string(),
        cli_path(&voice),
        "--output".to_string(),
        cli_path(output_path),
        "--length-scale".to_string(),
        length_scale.to_string(),
    ])
    .await
}

async fn synthesize_indic(text: &str, language: &str, output_path: &Path, speaking_rate: f32) -> Result<PathBuf, String> {
    run_tts_script(vec![
        cli_path(&tts_script_path("mms_synthesize.py")),
        text.to_string(),
        "--language".to_string(),
        language.to_string(),
        "--output".to_string(),
        cli_path(output_path),
        "--speaking-rate".to_string(),
        speaking_rate.to_string(),
    ])
    .await
}

/// Indian languages with a known MMS-TTS mapping (see
/// mms_synthesize.py's LANGUAGE_TO_MMS_CODE) -- kept as its own list
/// rather than reusing `stt::INDIC_LANGUAGES` directly, since STT and TTS
/// language coverage can diverge (a checkpoint existing for one direction
/// says nothing about the other).
pub const TTS_INDIC_LANGUAGES: &[(&str, &str)] = &[("ta", "Tamil")];

// --- Emotion-driven delivery (opt-in) -------------------------------------
//
// Not true video-driven lip-sync or pitch-matching (see VoiceoverPanel's
// own hint text) -- this classifies the *script's* emotional tone via the
// LLM service already used for content_ideas.rs, then applies a modest,
// hand-picked rate + pitch preset. Deliberately subtle adjustments: VITS
// voices degrade audibly at extreme length_scale/pitch values, so presets
// stay within a range confirmed by ear to still sound like the same voice,
// not a chipmunk/slowed-down effect.
//
// Rate has a native synthesis parameter in both engines, but with
// *opposite* conventions -- Piper's `length_scale` is inverted (>1 =
// slower) versus MMS-TTS's `speaking_rate` (>1 = faster) -- so each
// preset carries both, not one shared "rate" number. Pitch has no native
// synthesis-time control in either engine; it's a post-process step via
// ffmpeg's `rubberband` filter (formant-preserving, unlike the
// `asetrate`+`atempo` trick, and confirmed present in this project's
// bundled ffmpeg build).

const EMOTIONS: &[&str] = &["neutral", "excited", "somber", "urgent", "calm"];

// A plain instruction ("classify into one of: ...") was verified directly
// to fail badly at this model size -- a script literally saying "We just
// hit one million subscribers!" came back "calm", and every test script
// defaulted toward "calm" regardless of content, at both temperature=0.7
// and temperature=0 (so not a sampling-randomness issue -- a genuine
// capability gap at 0.5B params for 5-way classification). Concrete
// worked examples in the prompt (few-shot, not zero-shot) fixed it:
// verified directly, 4/5 correct on a fresh test set afterward, versus
// the earlier consistent "calm" bias. Small models lean heavily on
// in-context examples for tasks like this; don't strip these back down to
// a bare instruction without re-verifying against real scripts first.
const EMOTION_SYSTEM_PROMPT: &str = r#"Classify the emotional tone of a voiceover script into exactly one of: neutral, excited, somber, urgent, calm.

Examples:
Script: We just hit one million subscribers, thank you all so much!
Emotion: excited

Script: In loving memory of those we lost this year.
Emotion: somber

Script: Evacuate the building immediately, this is not a drill.
Emotion: urgent

Script: Take a slow breath in and let it out gently.
Emotion: calm

Script: The meeting is scheduled for 3pm on Thursday.
Emotion: neutral

Respond with only the JSON object for the new script."#;

#[derive(Deserialize)]
struct EmotionClassification {
    emotion: String,
}

async fn classify_emotion(app: &AppHandle, text: &str) -> Result<String, String> {
    let schema = json!({
        "type": "object",
        "properties": { "emotion": { "type": "string", "enum": EMOTIONS } },
        "required": ["emotion"]
    });
    let user_prompt = format!("Script: {text}");
    // Low temperature: this is a single-best-label task, not creative
    // generation -- see llm::complete's doc comment for why this alone
    // didn't fix the misclassification (the few-shot examples above did).
    let raw = crate::llm::complete(app, EMOTION_SYSTEM_PROMPT, &user_prompt, 30, 0.1, Some(schema)).await?;
    let parsed: EmotionClassification = serde_json::from_str(raw.trim())
        .map_err(|e| format!("Couldn't parse emotion classification: {e} (raw: {raw})"))?;
    Ok(parsed.emotion)
}

struct EmotionPreset {
    piper_length_scale: f32,
    mms_speaking_rate: f32,
    pitch_semitones: f32,
}

impl Default for EmotionPreset {
    fn default() -> Self {
        Self { piper_length_scale: 1.0, mms_speaking_rate: 1.0, pitch_semitones: 0.0 }
    }
}

fn emotion_preset(emotion: &str) -> EmotionPreset {
    match emotion {
        "excited" => EmotionPreset { piper_length_scale: 0.9, mms_speaking_rate: 1.1, pitch_semitones: 1.5 },
        "somber" => EmotionPreset { piper_length_scale: 1.15, mms_speaking_rate: 0.87, pitch_semitones: -1.5 },
        "urgent" => EmotionPreset { piper_length_scale: 0.85, mms_speaking_rate: 1.18, pitch_semitones: 1.0 },
        "calm" => EmotionPreset { piper_length_scale: 1.1, mms_speaking_rate: 0.91, pitch_semitones: -0.5 },
        _ => EmotionPreset::default(),
    }
}

/// Pitch-shifts `path` in place by `semitones` (formant-preserving, tempo
/// unchanged) via ffmpeg's `rubberband` filter. A no-op for 0 semitones
/// (the "neutral" preset) rather than running ffmpeg for an identity
/// transform.
async fn apply_pitch_shift(path: &Path, semitones: f32) -> Result<(), String> {
    if semitones == 0.0 {
        return Ok(());
    }
    let factor = 2f32.powf(semitones / 12.0);
    let shifted_path = path.with_extension("pitched.wav");

    let output = Command::new(crate::bin_paths::ffmpeg_path())
        .args(["-y", "-i", &cli_path(path), "-af", &format!("rubberband=pitch={factor}"), &cli_path(&shifted_path)])
        .output()
        .await
        .map_err(|e| format!("Failed to run pitch-shift: {e}"))?;

    if !output.status.success() {
        return Err(format!("Pitch-shift failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    std::fs::rename(&shifted_path, path).map_err(|e| format!("Couldn't finalize pitch-shifted audio: {e}"))?;
    Ok(())
}

#[derive(Serialize)]
pub struct VoiceoverResult {
    pub output_path: String,
    pub emotion: Option<String>,
    /// "cloned" | "gender_matched" | "default" — what voice-matching
    /// tier actually produced this audio, see voice_clone.rs's module doc
    /// comment for the full decision order.
    pub voice_match: String,
    /// Set whenever the tier isn't "cloned", explaining why (no video
    /// audio to reference, cloning engine unavailable, cloning itself
    /// failed) -- shown to the user rather than silently downgrading.
    pub voice_match_note: Option<String>,
}

/// Voice-cloning + gender-matched-fallback orchestration for the English
/// path (Tamil has no second voice to gender-match against — see
/// voice_clone.rs's module doc comment — so it only ever gets "cloned" or
/// "default"). Runs before synthesis (to pick the closest-matching Piper
/// voice up front) and again after (to attempt real cloning on top);
/// returns the Piper voice id to synthesize with plus a closure-free
/// summary of what reference audio, if any, is available for cloning.
struct VoiceReference {
    reference_clip: Option<PathBuf>,
    english_voice_id: &'static str,
    note: Option<String>,
}

async fn resolve_voice_reference(app: &AppHandle, video_path: Option<&str>) -> VoiceReference {
    let Some(video_path) = video_path else {
        return VoiceReference {
            reference_clip: None,
            english_voice_id: DEFAULT_ENGLISH_VOICE,
            note: Some("No video loaded to match a voice to.".to_string()),
        };
    };
    if !crate::ffmpeg::has_audio_stream(video_path).await {
        return VoiceReference {
            reference_clip: None,
            english_voice_id: DEFAULT_ENGLISH_VOICE,
            note: Some("This clip has no original audio to match a voice to.".to_string()),
        };
    }

    let reference_clip = match crate::voice_clone::extract_reference_clip(video_path).await {
        Ok(path) => path,
        Err(e) => {
            return VoiceReference {
                reference_clip: None,
                english_voice_id: DEFAULT_ENGLISH_VOICE,
                note: Some(format!("Couldn't extract a reference clip from the video: {e}")),
            };
        }
    };

    let english_voice_id = match crate::voice_clone::detect_reference_gender(app, &reference_clip).await {
        Ok(Some(gender)) if gender == "male" => MALE_ENGLISH_VOICE,
        _ => DEFAULT_ENGLISH_VOICE,
    };

    VoiceReference { reference_clip: Some(reference_clip), english_voice_id, note: None }
}

#[tauri::command]
pub async fn generate_voiceover(
    app: AppHandle,
    text: String,
    language: String,
    auto_emotion: bool,
    video_path: Option<String>,
) -> Result<VoiceoverResult, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("No text to synthesize — type a script first.".to_string());
    }

    let emotion = if auto_emotion { Some(classify_emotion(&app, text).await?) } else { None };
    let preset = emotion.as_deref().map(emotion_preset).unwrap_or_default();

    // Resolved up front (before synthesis) so the English path can already
    // pick the closer-matching preset voice as its base -- if cloning then
    // fails below, that gender-matched synthesis is what's kept, with no
    // second synthesis pass needed.
    let voice_ref = resolve_voice_reference(&app, video_path.as_deref()).await;

    let output_path = unique_temp_path("voiceover", "wav");

    // Held through synthesis AND voice cloning below (both load their own
    // model into memory per call -- OpenVoice for cloning, Piper/MMS-TTS
    // for synthesis) -- one acquire for the whole heavy section, not a
    // second acquire-drop-reacquire around cloning, which would let
    // another queued caller's synthesis start mid-function and interleave
    // with this call's own model loads.
    let _permit = crate::concurrency::acquire_heavy_ml().await;

    let result_path = if language == "en" {
        synthesize_english(&app, text, voice_ref.english_voice_id, &output_path, preset.piper_length_scale).await?
    } else if TTS_INDIC_LANGUAGES.iter().any(|(code, _)| *code == language) {
        synthesize_indic(text, &language, &output_path, preset.mms_speaking_rate).await?
    } else {
        return Err(format!(
            "Unsupported language code '{language}' — this voiceover feature supports English (en) and: {}",
            TTS_INDIC_LANGUAGES.iter().map(|(_, name)| *name).collect::<Vec<_>>().join(", ")
        ));
    };

    let (final_path, voice_match, voice_match_note) = match &voice_ref.reference_clip {
        Some(reference_clip) => match crate::voice_clone::clone_voice(&app, &result_path, reference_clip).await {
            Ok(cloned_path) => {
                let _ = tokio::fs::remove_file(&result_path).await;
                (cloned_path, "cloned".to_string(), None)
            }
            Err(e) => {
                let tier = if language == "en" && voice_ref.english_voice_id == MALE_ENGLISH_VOICE {
                    "gender_matched"
                } else {
                    "default"
                };
                (result_path, tier.to_string(), Some(format!("Couldn't clone the original speaker's voice: {e}")))
            }
        },
        None => {
            let tier = if language == "en" && voice_ref.english_voice_id == MALE_ENGLISH_VOICE {
                "gender_matched"
            } else {
                "default"
            };
            (result_path, tier.to_string(), voice_ref.note.clone())
        }
    };

    apply_pitch_shift(&final_path, preset.pitch_semitones).await?;

    Ok(VoiceoverResult {
        output_path: final_path.to_string_lossy().to_string(),
        emotion,
        voice_match,
        voice_match_note,
    })
}
