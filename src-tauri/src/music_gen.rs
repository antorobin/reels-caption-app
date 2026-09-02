// AI-generated background music, contextual to the video's own spoken
// content -- lets "Music ducking" (ducking.rs/DuckingPanel.jsx) offer a
// generated bed as an alternative to picking an existing music file,
// without changing the ducking pipeline itself at all: this module's whole
// job ends at handing back a WAV path that covers the full video duration,
// which is exactly what `duck_music` already expects from an uploaded file.
//
// Model: Meta's MusicGen-Small (`facebook/musicgen-small`, official
// Hugging Face `transformers` model, text-conditioned, 300M params,
// CPU-capable). TinyMusician (a smaller/faster distillation of MusicGen)
// was considered first -- ruled out because it has no public checkpoint,
// pip package, or GitHub implementation as of this writing, just a Sept
// 2025 arXiv paper (confirmed via web search, not assumed).
//
// Runs in the existing "tts" conda env rather than a new one: that env
// already has torch+transformers+scipy (see tts.rs's own module doc
// comment) for MMS-TTS, which is exactly what MusicGen needs too --
// reuses `tts::resolve_tts_prefix`/`python_exe`/`tts_script_path` directly
// (promoted to `pub(crate)` there) instead of a second, duplicate conda
// resolution. Verified directly against the real local "tts" env before
// writing any of this wiring: transformers 5.15.1 already supports
// `MusicgenForConditionalGeneration` (added upstream in 4.31), so no
// version bump was even needed here.
//
// Two-command shape (suggest, then finalize) rather than one "generate and
// apply" call -- a user asked directly for this after trying the original
// single-shot version: pick from a few options with an audible preview of
// each, and only have the *chosen* one actually become the active bed,
// rather than committing to whatever one prompt the LLM happened to write
// first. `suggest_background_music` generates several *short* preview
// clips (cheap to audition); `finalize_background_music` re-generates only
// the one the user actually picked, at the real target length. Real,
// measured CPU generation speed (~12x slower than real-time, see
// `MAX_DIRECT_GENERATION_SECONDS`'s own comment) is exactly why this stays
// two phases instead of generating every candidate at full length --
// auditioning 3 full-length beds before picking one would be 3x the wait
// for 2 that get thrown away.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::pipeline::WordTimestamp;
use crate::tts::{python_exe, resolve_tts_prefix, tts_script_path};
use crate::util::{cli_path, unique_temp_path};

/// CPU generation time scales with how much audio is requested -- capping
/// how much a single call ever generates directly keeps that bounded.
/// Beyond this, a bed this long is generated once and looped
/// (`loop_audio_to_duration` in ffmpeg.rs) rather than generating the full
/// length directly. Measured directly on this project's own dev machine
/// (not assumed): MusicGen-Small on CPU runs roughly **12x slower than
/// real-time** -- a 20-second clip took ~4m12s wall-clock with the model
/// already warm/cached (no download in that run). 30s was the original
/// guess before that measurement; 15s keeps the worst case around 3
/// minutes, which is what actually informed lowering it here.
const MAX_DIRECT_GENERATION_SECONDS: f64 = 15.0;

/// How many distinct style suggestions to offer at once -- and how many
/// preview clips that means generating sequentially before the user picks
/// one. Kept small on purpose given the ~12x-real-time CPU cost above: 3
/// previews at `PREVIEW_SECONDS` each is already a real wait, not a knob to
/// casually raise.
const SUGGESTION_COUNT: usize = 3;
/// Preview clips are short on purpose -- long enough to judge whether a
/// style fits, short enough that auditioning `SUGGESTION_COUNT` of them
/// doesn't itself take as long as just generating one full bed would.
const PREVIEW_SECONDS: f64 = 6.0;

// This went through four real, verified-against-the-actual-model
// iterations before shipping -- not guessed:
// 1. A plain zero-shot instruction ("suggest N genuinely different
//    instrumental styles...") asking for a `count`-item JSON array: Qwen
//    2.5-0.5B-Instruct just echoed fragments of the instruction itself back
//    ("purely instrumental", "no vocals", "no lyrics") as if they were the
//    three suggestions -- the same small-model failure mode already
//    documented for tts.rs's emotion classifier (which needed few-shot
//    examples to work at all), just worse here since asking for an array of
//    genuinely distinct items is a harder structural task than single-label
//    classification.
// 2. Adding one worked few-shot example (matching tts.rs's fix) produced
//    real, distinct-sounding prompts -- but a second test transcript (a
//    memorial/tribute video) came back with one suggestion literally
//    describing "a soft, emotive vocal, like a lead singer", directly
//    violating "no vocals" -- a real functional bug, not cosmetic: handed
//    to MusicGen, that prompt would push it toward generating something
//    vocal-like instead of a purely instrumental bed.
// 3. Strengthening the no-vocals instruction alone fixed the vocal leakage
//    but collapsed all 3 suggestions into near-duplicates of each other
//    (three "piano with cello" variations, three "drumbeat with heavy
//    bassline" variations) -- diversity and constraint-following traded off
//    against each other at this model size. Explicitly naming three
//    different instrumentation *categories* to fill (one per suggestion,
//    below) fixed both at once: verified across three different-mood test
//    transcripts (a DIY project, a memorial tribute, a race countdown), no
//    vocal-word leakage in any of them, and three structurally distinct
//    genres every time.
// 4. Requested directly: factor in the detected spoken language's own
//    film/popular music culture (Tamil cinema, English-language albums,
//    etc), not just content/mood. Added a `Language:` field alongside the
//    transcript and a third few-shot example (Tamil, leaning Carnatic/
//    Kollywood-style instrumentation) -- verified directly against a real
//    Tamil transcript from earlier in this project's own testing, and it
//    correctly leaned South Indian/Carnatic-style instrumentation without
//    prompting for it explicitly beyond the language label. Also
//    discovered, isolating it directly rather than guessing: a heavily
//    **code-switched** (Tamil+English mid-sentence) transcript can derail
//    this 0.5B model badly regardless of the language label given -- one
//    real test transcript about cooking made it describe the recipe itself
//    ("warm biryani, comforting") instead of music entirely. A plain-
//    English translation of the exact same content only degraded mildly
//    (occasional food-adjacent adjectives like "spicy" bleeding into a
//    genre description, still basically usable). This is a real limitation
//    of this specific lightweight local model's non-English/code-switched
//    comprehension -- not fixed here (would need a translation step this
//    app doesn't have), just documented honestly rather than silently
//    shipped as if it always works.
// 5. Requested directly, again: go further than film/classical and name
//    *regional folk* styles too (e.g. Gana -- rhythmic, percussion-driven
//    street/folk music from North Chennai, associated with working-class
//    and mass-appeal themes). Named it explicitly in the instruction and
//    swapped the Tamil example's percussion suggestion to demonstrate it.
//    Verified against a real energetic, working-class-story Tamil
//    transcript: the model correctly picked up on Gana for that specific
//    content, and a second, calmer/devotional Tamil transcript correctly
//    still favored Carnatic/Kollywood instead (it isn't just parroting
//    Gana regardless of content). One honest caveat found in the same
//    test: on a harder transcript, the model occasionally mangled "Gana"
//    into a garbled non-word ("Ghaannar-style") rather than misusing it --
//    cosmetically rough in the UI's displayed suggestion text, but the
//    surrounding descriptive words (percussion, rhythm, street music) still
//    carry real meaning for MusicGen's own generation, so this doesn't
//    break the actual audio the way the vocal-leakage bug above would have.
//
// Mood-matching for any *one* suggestion still isn't perfect at this model
// size (occasionally too mellow for a high-energy transcript) -- acceptable
// given the whole point of offering 3 auditioned options is letting a human
// pick the one that actually fits, not trusting one AI guess to nail it.
const MUSIC_PROMPT_SYSTEM: &str = r#"You are a music supervisor choosing instrumental background music for a short-form video, given its spoken language and transcript. Suggest exactly 3 instrumental styles that each fit the transcript's specific content and mood, one from each of these three categories so they stay genuinely different:
1. Acoustic/organic (real instruments: guitar, piano, strings, etc.)
2. Electronic/synth-based
3. Percussion/rhythm-driven

Let the spoken language's own film, regional folk, and popular music culture inform at least one of the three suggestions -- for example Tamil often pairs with Tamil cinema (Kollywood) style instrumentation, Carnatic classical instrumentation (veena, mridangam, nadaswaram), or Gana (rhythmic, percussion-driven Chennai street/folk music, common for energetic or working-class themes); Hindi pairs with Bollywood or Hindustani-style instrumentation (sitar, tabla), or regional folk like Bhangra; English pairs with mainstream Western pop/rock/album-production instrumentation, or regional folk/Americana where it fits. Only apply a specific cultural style where it genuinely fits the language given -- for English or an unspecified language, use whatever Western instrumentation fits the content.

Each suggestion is a single sentence describing purely instrumental sound with NO vocals, NO singing, NO rapping, and NO lyrics of any kind -- describe only instruments and rhythm, never a singer or vocal melody, suitable as a direct prompt to a text-to-music generation model.

Example:
Language: English
Transcript: Today I'm walking you through how I renovated my entire kitchen on a budget, from tearing out the old cabinets to installing new countertops myself.
Suggestions: {"prompts": ["Upbeat acoustic folk with strummed guitar, tambourine, and a steady clapping rhythm, cheerful and DIY-inspired", "Motivational synth-pop with bright arpeggiated synths and a driving electronic bassline, energetic and productive, in the style of a mainstream pop album production", "Warm lo-fi hip-hop beat with a tight drum groove, soft vinyl crackle, and hand percussion, relaxed and satisfying"]}

Example:
Language: English
Transcript: In loving memory of my grandmother, who taught me so much about kindness over the years.
Suggestions: {"prompts": ["Slow, tender solo piano with soft strings underneath, gentle and reflective", "Airy ambient synth pads with a slow-evolving texture, soft and nostalgic", "Sparse, hushed hand percussion with a distant soft mallet instrument, quiet and reverent"]}

Example:
Language: Tamil
Transcript: நாம் இன்று பேசப்போவது எப்படி இந்த புதிய திட்டத்தை வெற்றிகரமாக முடித்தோம் என்பது பற்றி.
Suggestions: {"prompts": ["Traditional Carnatic-style composition with a bright veena melody over a steady mridangam rhythm, devotional and uplifting", "Modern Tamil cinema (Kollywood) style electronic fusion with a nadaswaram-like synth lead over a driving bassline, energetic and celebratory", "Energetic Gana-style street rhythm with fast dholak and thavil percussion, raw and celebratory"]}

Respond with only the JSON object for the new transcript."#;

/// `llm.rs` runs `llama-server` with a 4096-token context -- and that's a
/// genuinely hard limit, not a soft one: confirmed directly (not assumed)
/// by sending an oversized real request and getting back a plain HTTP 400
/// (`"exceeds the available context size"`), not silent truncation or
/// degraded output. `content_ideas.rs`'s `suggest_content_strategy` has
/// this same unbounded-transcript shape and shares `llm_budget.rs`'s
/// budgeting logic for exactly this reason -- this module is what
/// originally surfaced the need for it.
///
/// Measured directly against the real `MUSIC_PROMPT_SYSTEM` constant via
/// `llama-server`'s own `/tokenize` endpoint: 720 tokens for the system
/// prompt alone, 741 once wrapped in ChatML tags plus the "Language: X /
/// Transcript: " prefix with no transcript body yet. Rounded up with
/// margin for a longer language label (e.g. "Tamil + English") and future
/// edits to the prompt text.
const SYSTEM_PROMPT_TOKEN_BUDGET: usize = 900;
/// Matches the `max_tokens` passed to `llm::complete` below -- has to be
/// reserved from the same context window, not free space on top of it.
const OUTPUT_TOKEN_BUDGET: usize = 400;

// `MUSIC_PROMPT_SYSTEM` above hardcodes "exactly 3" and enumerates 3
// specific categories by name -- verified directly that this rigid framing
// is what actually made the model follow both the vocals constraint and
// the diversity requirement at once (see that constant's own doc comment).
// `SUGGESTION_COUNT` and this schema are NOT independently adjustable from
// that prompt text; changing one without the other would silently break --
// this `debug_assert` is a tripwire for exactly that.
fn suggestions_json_schema(count: usize) -> serde_json::Value {
    debug_assert_eq!(count, 3, "MUSIC_PROMPT_SYSTEM hardcodes exactly 3 categories -- update both together");
    serde_json::json!({
        "type": "object",
        "properties": {
            "prompts": { "type": "array", "items": { "type": "string" }, "minItems": count, "maxItems": count }
        },
        "required": ["prompts"]
    })
}

#[derive(Debug, Deserialize)]
struct MusicPromptSuggestions {
    prompts: Vec<String>,
}

async fn derive_music_prompt_suggestions(
    app: &AppHandle,
    words: &[WordTimestamp],
    count: usize,
    language: Option<&str>,
) -> Result<Vec<String>, String> {
    // "unspecified" (not e.g. empty string) matches the wording
    // MUSIC_PROMPT_SYSTEM itself uses for "no language given" -- keeps the
    // model's own instructions and this call's input using the same term
    // for that case.
    let user_prompt = format!(
        "Language: {}\nTranscript: {}",
        language.unwrap_or("unspecified"),
        crate::llm_budget::transcript_text_for_prompt(words, language, SYSTEM_PROMPT_TOKEN_BUDGET, OUTPUT_TOKEN_BUDGET)
    );
    // 400 tokens was verified directly to be enough headroom for 3 full
    // sentence-length suggestions without truncating mid-JSON.
    let max_tokens = OUTPUT_TOKEN_BUDGET as i32;
    let raw = crate::llm::complete(app, MUSIC_PROMPT_SYSTEM, &user_prompt, max_tokens, 0.9, Some(suggestions_json_schema(count)))
        .await?;
    let parsed: MusicPromptSuggestions = serde_json::from_str(raw.trim())
        .map_err(|e| format!("Couldn't parse the model's music-suggestion output as JSON: {e} (raw: {raw})"))?;
    Ok(parsed.prompts)
}

/// Runs `generate_music.py` in the "tts" env -- same stdout-JSON-line
/// protocol, same stdout/stderr-drained-concurrently shape as
/// `tts.rs::run_tts_script`, just not sharing that function directly since
/// this script's args are a different shape (`--prompt`/`--duration`
/// rather than the TTS scripts' own arg sets).
async fn run_music_generation_script(prompt: &str, duration_seconds: f64, output_path: &PathBuf) -> Result<(), String> {
    // Held for the whole subprocess call -- MusicGen loads its own model
    // fresh per invocation (see concurrency.rs's own doc comment). Acquired
    // here, the single choke point both `suggest_background_music` (once
    // per preview, in its loop) and `finalize_background_music` (via
    // `generate_bed_for_duration`) already go through, rather than once
    // for an entire multi-preview `suggest_background_music` call -- that
    // would reserve a permit for the whole multi-minute round-trip instead
    // of letting other projects' work interleave between previews.
    let _permit = crate::concurrency::acquire_heavy_ml().await;
    let prefix = resolve_tts_prefix().await?;
    let python = python_exe(&prefix);
    let script = tts_script_path("generate_music.py");

    let mut child = Command::new(&python)
        .arg(cli_path(&script))
        .args(["--prompt", prompt, "--duration", &duration_seconds.to_string(), "--output", &cli_path(output_path)])
        .env("PYTHONIOENCODING", "utf-8")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "Music generation engine not found ({e}). Set up the 'tts' conda environment \
                 (see README) -- or if it's already installed somewhere this couldn't find, \
                 set REELS_CAPTION_APP_TTS_CONDA_PATH."
            )
        })?;

    let stdout = child.stdout.take().expect("music-gen stdout was piped");
    let stderr = child.stderr.take().expect("music-gen stderr was piped");

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

    let status = child.wait().await.map_err(|e| format!("Failed to wait on the music generation engine: {e}"))?;
    let stdout_text = stdout_task.await.unwrap_or_default();
    let stderr_text = stderr_task.await.unwrap_or_default();

    if !status.success() {
        return Err(format!("Music generation failed: {stderr_text}"));
    }

    // The script's stdout JSON echoes back `output_path`/`sample_rate`, but
    // this caller already knows the path it asked for (`output_path`, the
    // parameter above) -- parsing here is purely a "did it actually finish
    // and print well-formed success JSON, not silently write nothing or a
    // truncated file" check, not a value this function needs to extract.
    let last_line = stdout_text.lines().last().unwrap_or_default();
    serde_json::from_str::<serde_json::Value>(last_line)
        .map_err(|e| format!("Couldn't parse music generation output: {e} (raw: {stdout_text})"))?;
    Ok(())
}

/// Generates a bed for one already-chosen `prompt`, capped/looped to cover
/// `video_path`'s full duration -- the shared tail end of both
/// `suggest_background_music` (at `PREVIEW_SECONDS`, per candidate) and
/// `finalize_background_music` (at the real target length, for the one
/// the user picked).
async fn generate_bed_for_duration(prompt: &str, generation_seconds: f64, target_duration: f64) -> Result<PathBuf, String> {
    let bed_path = unique_temp_path("generated-music-bed", "wav");
    run_music_generation_script(prompt, generation_seconds, &bed_path).await?;

    if target_duration > generation_seconds {
        let looped_path = unique_temp_path("generated-music", "wav");
        crate::ffmpeg::loop_audio_to_duration(&bed_path, &looped_path, target_duration).await?;
        let _ = std::fs::remove_file(&bed_path);
        Ok(looped_path)
    } else {
        Ok(bed_path)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MusicSuggestion {
    pub prompt: String,
    pub preview_path: String,
}

#[derive(Clone, Serialize)]
struct SuggestionsProgress {
    project_id: String,
    stage: &'static str, // "deriving_prompts" | "generating_preview"
    index: Option<usize>, // 1-based, only set during "generating_preview"
    total: usize,
}

#[tauri::command]
pub async fn suggest_background_music(
    app: AppHandle,
    words: Vec<WordTimestamp>,
    language: Option<String>,
    project_id: String,
) -> Result<Vec<MusicSuggestion>, String> {
    if words.is_empty() {
        return Err("No transcript yet -- run transcription first so suggestions can match the speech.".to_string());
    }

    let _ = app.emit(
        "music-suggestions-progress",
        SuggestionsProgress {
            project_id: project_id.clone(),
            stage: "deriving_prompts",
            index: None,
            total: SUGGESTION_COUNT,
        },
    );
    let prompts = derive_music_prompt_suggestions(&app, &words, SUGGESTION_COUNT, language.as_deref()).await?;

    let mut suggestions = Vec::with_capacity(prompts.len());
    for (i, prompt) in prompts.into_iter().enumerate() {
        let _ = app.emit(
            "music-suggestions-progress",
            SuggestionsProgress {
                project_id: project_id.clone(),
                stage: "generating_preview",
                index: Some(i + 1),
                total: SUGGESTION_COUNT,
            },
        );
        let preview_path = unique_temp_path("music-preview", "wav");
        run_music_generation_script(&prompt, PREVIEW_SECONDS, &preview_path).await?;
        suggestions.push(MusicSuggestion { prompt, preview_path: cli_path(&preview_path) });
    }

    Ok(suggestions)
}

#[derive(Debug, Clone, Serialize)]
pub struct GeneratedMusic {
    pub output_path: String,
    pub prompt: String,
}

/// Re-generates the *chosen* suggestion's prompt at the real target
/// length -- the preview clip `suggest_background_music` made for it was
/// deliberately short (`PREVIEW_SECONDS`), just for auditioning, so this
/// isn't a duplicate of work already done; it's the one candidate actually
/// worth paying the full generation cost for.
#[tauri::command]
pub async fn finalize_background_music(video_path: String, prompt: String) -> Result<GeneratedMusic, String> {
    let duration = crate::ffmpeg::probe_duration_seconds(&video_path)
        .await
        .ok_or("Couldn't determine the video's duration.".to_string())?;

    let generation_seconds = duration.min(MAX_DIRECT_GENERATION_SECONDS);
    let final_path = generate_bed_for_duration(&prompt, generation_seconds, duration).await?;

    Ok(GeneratedMusic { output_path: cli_path(&final_path), prompt })
}
