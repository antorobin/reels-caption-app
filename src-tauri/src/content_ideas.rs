// Title/description/hook/hashtags/emoji suggestions, generated locally by
// Qwen2.5-0.5B-Instruct via llm.rs. Supersedes the earlier, narrower
// "hooks + hashtags" feature (which used Sarvam-1, a base/completion
// model) -- previously the Keywords panel (src/lib/keywords.js) used a
// pure-JS RAKE-lite extractor with no model at all, which still exists
// and still works with no setup; this is an additional, opt-in panel for
// creators who have the LLM service set up and want real generated copy,
// not just extracted keyphrases.
//
// Qwen2.5-0.5B-Instruct is genuinely instruction-tuned (unlike Sarvam-1),
// so this asks it directly for the fields we want and constrains
// generation with a JSON schema (via llm::complete's `json_schema` param)
// rather than parsing free-form text -- much less fragile than the old
// line-by-line "Hook:"/"Hashtags:" parsing, and it's what lets a single
// call return five structured fields reliably from a 0.5B model.

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::AppHandle;

use crate::pipeline::WordTimestamp;

const SYSTEM_PROMPT: &str = "You are a social-media assistant that writes short-form video metadata from a \
spoken transcript. Given the transcript, invent: a catchy title under 60 characters; a 1-2 sentence \
description; a short, attention-grabbing hook line for the video's opening (different from the title); 3 to 6 \
relevant hashtags, each a single word or CamelCase phrase starting with # (no spaces inside a hashtag); and 1 \
to 4 actual emoji characters (real Unicode pictographs like \u{1F60A}, \u{1F634}, \u{1F4AA}, \u{1F525} -- never \
emoji names or words) that match the video's emotional tone. Respond with only the JSON object.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentIdeas {
    pub title: String,
    pub description: String,
    pub hook: String,
    pub hashtags: Vec<String>,
    pub emoji: Vec<String>,
}

fn transcript_text(words: &[WordTimestamp]) -> String {
    words.iter().map(|w| w.word.as_str()).collect::<Vec<_>>().join(" ")
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

fn json_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "description": { "type": "string" },
            "hook": { "type": "string" },
            "hashtags": { "type": "array", "items": { "type": "string" }, "minItems": 3, "maxItems": 6 },
            "emoji": {
                "type": "array",
                "items": { "type": "string", "minLength": 1, "maxLength": 4 },
                "minItems": 1,
                "maxItems": 4
            }
        },
        "required": ["title", "description", "hook", "hashtags", "emoji"]
    })
}

#[tauri::command]
pub async fn generate_content_ideas(app: AppHandle, words: Vec<WordTimestamp>) -> Result<ContentIdeas, String> {
    if words.is_empty() {
        return Err("No transcript to generate content ideas from — run transcription first.".to_string());
    }

    let user_prompt = format!("Transcript: {}", transcript_text(&words));
    let raw = crate::llm::complete(&app, SYSTEM_PROMPT, &user_prompt, 300, 0.7, Some(json_schema())).await?;

    let mut ideas: ContentIdeas = serde_json::from_str(raw.trim())
        .map_err(|e| format!("Couldn't parse the model's output as JSON: {e} (raw: {raw})"))?;
    ideas.hashtags = ideas.hashtags.iter().map(|t| sanitize_hashtag(t)).collect();
    Ok(ideas)
}
