// Marketing content strategy: title/description/hook/hashtags/emoji
// suggestions, generated locally by Qwen2.5-0.5B-Instruct via llm.rs.
//
// Supersedes the earlier single-result `generate_content_ideas` (still the
// shape callers get back once they pick one option -- see
// `ContentStrategyOption`, identical fields to the old `ContentIdeas` plus
// `angle`) with `suggest_content_strategy`: returns 3 distinct angles
// instead of one fixed result, and accepts optional free-text `hints`
// (user-supplied themes/ideas to steer toward) alongside the transcript --
// previously there was no way to steer generation at all, and no
// transcript meant no output. `hints` alone (no transcript) is a real,
// supported input now: the from-scratch "generate a video with no upload"
// flow has no transcript yet and uses hints as its only seed.
//
// Also fixes a real existing gap: `detectedLanguage` was already computed
// by the pipeline and already threaded into `suggest_background_music`
// (music_gen.rs), but never into this feature -- every call here is now
// language-aware, reusing `llm_budget.rs`'s per-language token budgeting
// (factored out of music_gen.rs once this needed the identical logic).

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::AppHandle;

use crate::pipeline::WordTimestamp;

// Naming 3 fixed categories explicitly (rather than just asking for "3
// different strategies") is carried over from music_gen.rs's own hard-won
// lesson (its MUSIC_PROMPT_SYSTEM doc comment has the full history): a
// plain "give me N distinct things" ask lets this 0.5B model collapse into
// near-duplicates. Verified directly here too, across 4 real test cases
// (English transcript alone, transcript+hints, hints alone, Tamil
// transcript alone) before settling on this wording -- three real findings
// from that testing shaped it:
//
// 1. A first draft asked the model to also lock the 3 options into a
//    fixed output *order* ("Educational first, Entertainment second,
//    Aspirational third"). Verified directly that this instruction is
//    unreliable at this model size -- real generations still came back in
//    random order across repeated runs even with the instruction present.
//    Rather than keep fighting an unreliable constraint, the instruction
//    was dropped and the order requirement removed entirely: each
//    option's own `angle` field is what actually identifies it, so
//    nothing downstream needs the array order to mean anything.
// 2. Combining a transcript with hints occasionally produced near-
//    duplicate hooks across 2 of the 3 options (e.g. the exact same
//    sentence used twice) and, once, literal placeholder-looking text
//    ("gard", "mo") in the `emoji` field instead of real pictographs --
//    the model appears to blend fragments of this prompt's own few-shot
//    examples into unrelated real output under that combined input. The
//    explicit "every option's hook must be a completely different
//    sentence" and "emoji must always be real Unicode pictographs" lines
//    below measurably reduced but did not eliminate this at 0.5B scale --
//    `sanitize_emoji` below is the defensive backstop for the cases that
//    still get through, matching `sanitize_hashtag`'s existing role for a
//    different untrusted-output failure mode.
// 3. A Tamil transcript with no hints produced a fully coherent, well-
//    formed strategy every time -- but about an entirely different,
//    fabricated topic having nothing to do with the real transcript
//    (three separate test runs invented three different unrelated
//    topics: a furniture-shop marketplace, a coffee shop, a rainbow-
//    vegetable recipe -- none present in the actual transcript). This
//    matches music_gen.rs's own documented Tamil/code-switched
//    comprehension weakness in this same small model and was not fixable
//    by further prompt wording in the time spent here -- a real,
//    reproducible limitation of this specific lightweight local model on
//    Tamil input, not something this feature silently papers over. Tamil
//    input *with* hints wasn't separately broken by this -- the hints
//    still anchor the topic even when the transcript itself derails
//    comprehension -- so the practical mitigation is: encourage hints for
//    Tamil content rather than relying on transcript-only generation.
const SYSTEM_PROMPT: &str = "You are a marketing strategist creating short-form video content strategy. You may be given a spoken video's transcript, optional hints/themes the creator wants to emphasize, and the spoken language. Using whichever of these are actually given, invent exactly 3 distinct content strategy options, one from each category so they stay genuinely different:\n\
1. Educational/how-to -- position the video as teaching or explaining something\n\
2. Entertainment/relatable -- position the video as funny, relatable, or emotionally engaging\n\
3. Aspirational/emotional -- position the video as inspiring, motivational, or aspirational\n\
\n\
For each option, invent: a catchy title under 60 characters; a SHORT 1-2 sentence description that summarizes the angle in your own words -- never copy or closely paraphrase the transcript itself, and never describe individual details or steps, just the overall subject; a short, attention-grabbing hook line for the video's opening (different from the title); 3 to 6 relevant hashtags, each a single word or CamelCase phrase starting with # (no spaces inside a hashtag); and 1 to 4 actual emoji characters (real Unicode pictographs like \u{1F60A}, \u{1F634}, \u{1F4AA}, \u{1F525} -- never emoji names or words) that match that option's tone.\n\
\n\
If a transcript is given, ground every option in what's actually said. If hints/themes are given, let them steer the topic and angle of every option. If NO transcript is given at all, there is no existing video to summarize -- invent an entirely new video concept from the hints alone, and never write as if a video already exists.\n\
\n\
The 3 options must be meaningfully different from each other, even when a transcript and hints are both given about one narrow subject -- the whole point of 3 options is 3 real alternatives, not 3 copies with the words shuffled. Every option's hook must be a completely different sentence from the other two options' hooks -- never reuse the same hook, title, or hashtag word-for-word across options. Emoji must always be real Unicode pictograph characters (like the examples' \u{1F4AA}\u{1F338}\u{2615}\u{1F525}) -- never plain text or letters.\n\
\n\
Respond with only the JSON object: {\"options\": [{\"angle\": ..., \"title\": ..., \"description\": ..., \"hook\": ..., \"hashtags\": [...], \"emoji\": [...]}, ...]} with exactly 3 items in \"options\", one per category above (any order).\n\
\n\
Example (transcript given, no hints):\n\
Language: English\n\
Hints/themes to emphasize: (none)\n\
Transcript: today im gonna show you how i turned my garage into a home gym for under five hundred dollars using mostly stuff i found secondhand\n\
Strategies: {\"options\": [{\"angle\": \"Educational/how-to\", \"title\": \"Build a Home Gym for Under $500\", \"description\": \"A step-by-step budget breakdown for turning unused garage space into a real home gym.\", \"hook\": \"You don't need a gym membership -- you need a garage.\", \"hashtags\": [\"#HomeGym\", \"#BudgetFitness\", \"#DIY\", \"#GarageGym\"], \"emoji\": [\"\u{1F4AA}\", \"\u{1F3CB}\u{FE0F}\"]}, {\"angle\": \"Entertainment/relatable\", \"title\": \"I Turned My Garage Into a Gym (No Regrets)\", \"description\": \"A relatable, slightly chaotic journey of secondhand-shopping and DIY mistakes on the way to a real home gym.\", \"hook\": \"My garage used to hold junk. Now it holds my gains.\", \"hashtags\": [\"#GarageGym\", \"#DIYFail\", \"#Relatable\"], \"emoji\": [\"\u{1F602}\", \"\u{1F4AA}\"]}, {\"angle\": \"Aspirational/emotional\", \"title\": \"From Garage to Gains: My Fitness Comeback\", \"description\": \"A motivational story about reclaiming a neglected space to reclaim your health, on a real budget.\", \"hook\": \"This garage is where my comeback started.\", \"hashtags\": [\"#FitnessJourney\", \"#Motivation\", \"#Comeback\", \"#HomeGym\"], \"emoji\": [\"\u{1F525}\", \"\u{2728}\"]}]}\n\
\n\
Example (hints only, no transcript):\n\
Language: English\n\
Hints/themes to emphasize: a small neighborhood coffee shop just opened, cozy and handmade vibe\n\
Transcript: (none)\n\
Strategies: {\"options\": [{\"angle\": \"Educational/how-to\", \"title\": \"What Makes a Coffee Shop Actually Cozy\", \"description\": \"A look at the small, deliberate design choices behind a genuinely cozy neighborhood cafe.\", \"hook\": \"Cozy isn't an accident -- it's a design choice.\", \"hashtags\": [\"#CoffeeShop\", \"#CafeDesign\", \"#SmallBusiness\"], \"emoji\": [\"\u{2615}\", \"\u{1F3E0}\"]}, {\"angle\": \"Entertainment/relatable\", \"title\": \"POV: You Found Your New Favorite Coffee Shop\", \"description\": \"The relatable feeling of stumbling on a new neighborhood spot that instantly feels like home.\", \"hook\": \"I walked in for coffee. I stayed for the vibe.\", \"hashtags\": [\"#POV\", \"#CoffeeShop\", \"#NeighborhoodGem\"], \"emoji\": [\"\u{2615}\", \"\u{1F970}\"]}, {\"angle\": \"Aspirational/emotional\", \"title\": \"A Handmade Coffee Shop, Built With Love\", \"description\": \"The heart behind a new small business built to feel like a warm, handmade home away from home.\", \"hook\": \"Every cup here was made with someone's whole heart.\", \"hashtags\": [\"#SmallBusiness\", \"#HandmadeWithLove\", \"#SupportLocal\"], \"emoji\": [\"\u{2615}\", \"\u{2764}\u{FE0F}\"]}]}\n\
\n\
Respond with only the JSON object for the new input.";

/// Measured directly against the real `SYSTEM_PROMPT` constant above via
/// llama-server's own `/tokenize` endpoint (same methodology as
/// music_gen.rs's own measured budget): 1177 tokens once wrapped in
/// ChatML tags plus the "Language: X\nHints/themes to emphasize:
/// (none)\nTranscript: " prefix with no transcript body yet. Rounded up
/// with margin for a longer language label and future prompt edits.
const SYSTEM_PROMPT_TOKEN_BUDGET: usize = 1250;
/// Real generations across all 4 verification test cases (see
/// `SYSTEM_PROMPT`'s own doc comment) used 256-317 tokens for 3 full
/// options -- rounded up generously since a longer set of hashtags/hints
/// could plausibly push this higher.
const OUTPUT_TOKEN_BUDGET: usize = 600;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentStrategyOption {
    /// Which of the 3 named categories this option fills -- e.g.
    /// "Educational/how-to". Identifies the option for display; nothing
    /// depends on the array's own order (see SYSTEM_PROMPT's doc comment
    /// on why an output-order instruction was tried and dropped).
    pub angle: String,
    pub title: String,
    pub description: String,
    pub hook: String,
    pub hashtags: Vec<String>,
    pub emoji: Vec<String>,
}

/// A 0.5B model occasionally slips a space into a hashtag (e.g. "#Peanut
/// ButterEffect") even when told not to -- collapsing whitespace out and
/// re-adding a leading `#` is cheap insurance against that at this
/// untrusted-output boundary, without needing to reject/retry the whole
/// generation over one malformed tag.
fn sanitize_hashtag(tag: &str) -> String {
    let collapsed: String = tag.split_whitespace().collect();
    let trimmed = collapsed.trim_start_matches('#');
    format!("#{trimmed}")
}

/// Defensive backstop for the real (if uncommon) failure mode found during
/// verification: an `emoji` slot occasionally comes back as plain ASCII
/// text (e.g. "gard", "mo") instead of an actual pictograph, seemingly
/// when the model blends fragments of this prompt's own few-shot examples
/// into an unrelated real generation (see `SYSTEM_PROMPT`'s doc comment,
/// finding 2). Detected by checking for at least one character outside
/// the ASCII range -- every real emoji this prompt asks for is well above
/// ASCII, while the garbled failures observed were plain lowercase
/// letters -- and swapped for a small, tone-neutral fallback rather than
/// failing the whole generation over one bad slot.
fn sanitize_emoji(emoji: &str) -> String {
    if emoji.chars().any(|c| !c.is_ascii()) {
        emoji.to_string()
    } else {
        "\u{2728}".to_string() // ✨, a safe generic fallback
    }
}

const SCHEMA_MAX_TITLE_CHARS: u64 = 80;
const SCHEMA_MAX_DESCRIPTION_CHARS: u64 = 300;
const SCHEMA_MAX_HOOK_CHARS: u64 = 120;

const TARGET_TITLE_CHARS: usize = 60;
const TARGET_DESCRIPTION_CHARS: usize = 200;
const TARGET_HOOK_CHARS: usize = 90;

/// `SUGGESTION_COUNT` isn't independently adjustable from `SYSTEM_PROMPT`
/// above, which hardcodes "exactly 3" and enumerates 3 specific
/// categories by name -- same tripwire music_gen.rs uses for its own
/// hardcoded suggestion count, for the same reason (verified directly
/// that this rigid framing, not a variable count, is what keeps this
/// small model from collapsing into near-duplicates).
const SUGGESTION_COUNT: usize = 3;

fn option_json_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "angle": { "type": "string" },
            "title": { "type": "string", "maxLength": SCHEMA_MAX_TITLE_CHARS },
            "description": { "type": "string", "maxLength": SCHEMA_MAX_DESCRIPTION_CHARS },
            "hook": { "type": "string", "maxLength": SCHEMA_MAX_HOOK_CHARS },
            "hashtags": { "type": "array", "items": { "type": "string" }, "minItems": 3, "maxItems": 6 },
            "emoji": {
                "type": "array",
                "items": { "type": "string", "minLength": 1, "maxLength": 4 },
                "minItems": 1,
                "maxItems": 4
            }
        },
        "required": ["angle", "title", "description", "hook", "hashtags", "emoji"]
    })
}

fn json_schema() -> serde_json::Value {
    debug_assert_eq!(SUGGESTION_COUNT, 3, "SYSTEM_PROMPT hardcodes exactly 3 categories -- update both together");
    json!({
        "type": "object",
        "properties": {
            "options": {
                "type": "array",
                "items": option_json_schema(),
                "minItems": SUGGESTION_COUNT,
                "maxItems": SUGGESTION_COUNT
            }
        },
        "required": ["options"]
    })
}

#[derive(Debug, Deserialize)]
struct ContentStrategyResponse {
    options: Vec<ContentStrategyOption>,
}

/// Title/description/hook/hashtags/emoji strategy options, generated
/// locally by Qwen2.5-0.5B-Instruct -- 3 distinct angles (see
/// `SYSTEM_PROMPT`) rather than one fixed result, optionally steered by
/// free-text `hints` and aware of the transcript's spoken `language`
/// (matching `suggest_background_music`'s own signature shape).
///
/// `words` and `hints` aren't both required -- only both empty is an
/// error. An empty `words` with real `hints` is the from-scratch,
/// no-video-upload seed case: there's no transcript yet, so hints are the
/// only input, and `SYSTEM_PROMPT` has its own explicit instruction for
/// inventing a new concept rather than claiming to summarize a video that
/// doesn't exist.
#[tauri::command]
pub async fn suggest_content_strategy(
    app: AppHandle,
    words: Vec<WordTimestamp>,
    hints: Option<String>,
    language: Option<String>,
) -> Result<Vec<ContentStrategyOption>, String> {
    let hints = hints.filter(|h| !h.trim().is_empty());
    if words.is_empty() && hints.is_none() {
        return Err(
            "Nothing to generate a strategy from -- run transcription first, or give a hint/theme.".to_string()
        );
    }

    let transcript = if words.is_empty() {
        "(none)".to_string()
    } else {
        crate::llm_budget::transcript_text_for_prompt(
            &words,
            language.as_deref(),
            SYSTEM_PROMPT_TOKEN_BUDGET,
            OUTPUT_TOKEN_BUDGET,
        )
    };
    let user_prompt = format!(
        "Language: {}\nHints/themes to emphasize: {}\nTranscript: {}",
        language.as_deref().unwrap_or("unspecified"),
        hints.as_deref().unwrap_or("(none)"),
        transcript
    );

    let raw = crate::llm::complete(&app, SYSTEM_PROMPT, &user_prompt, OUTPUT_TOKEN_BUDGET as i32, 0.8, Some(json_schema()))
        .await?;

    let parsed: ContentStrategyResponse = serde_json::from_str(raw.trim())
        .map_err(|e| format!("Couldn't parse the model's output as JSON: {e} (raw: {raw})"))?;

    Ok(parsed
        .options
        .into_iter()
        .map(|mut option| {
            option.hashtags = option.hashtags.iter().map(|t| sanitize_hashtag(t)).collect();
            option.emoji = option.emoji.iter().map(|e| sanitize_emoji(e)).collect();
            option.title = trim_to_sentence(&option.title, TARGET_TITLE_CHARS);
            option.description = trim_to_sentence(&option.description, TARGET_DESCRIPTION_CHARS);
            option.hook = trim_to_sentence(&option.hook, TARGET_HOOK_CHARS);
            option
        })
        .collect())
}

/// Trims `text` to at most `max_len` characters, preferring to cut at the
/// last complete sentence within that budget rather than a hard character
/// cut -- verified directly against real (bad) model output before
/// trusting this: a schema-level `maxLength` alone produced things like
/// "...even sending alerts when" (cut mid-sentence), while running this
/// over the same text lands on "...sensors." (a real, complete sentence).
/// Falls back to the last word boundary (marked with an ellipsis, so it's
/// honest about being cut) only when there's no sentence-ending
/// punctuation at all within the budget -- real ASR transcripts often have
/// none, and a description built from one might not either.
fn trim_to_sentence(text: &str, max_len: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max_len {
        return text.to_string();
    }
    let prefix: String = text.chars().take(max_len).collect();
    if let Some(cut) = prefix.rfind(['.', '!', '?']) {
        return prefix[..=cut].trim().to_string();
    }
    match prefix.rfind(' ') {
        Some(last_space) => format!("{}…", prefix[..last_space].trim()),
        None => format!("{}…", prefix.trim()),
    }
}
