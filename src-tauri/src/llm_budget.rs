// Shared per-language token-budgeting helpers for prompts built from a
// transcript, going into Qwen2.5-0.5B-Instruct's fixed 4096-token context
// (llm.rs's `--ctx-size 4096`, confirmed a hard limit -- an oversized
// request gets a plain HTTP 400, not silent truncation). First written for
// music_gen.rs's `suggest_background_music` (its own doc comments there
// have the full measurement history and the real failure modes this
// budgeting was built to avoid); factored out here once
// content_ideas.rs's `suggest_content_strategy` needed the identical
// logic, so the two features' token-cost constants can't silently drift
// apart the way two independent copies eventually would.

use crate::pipeline::WordTimestamp;

pub const CONTEXT_SIZE_TOKENS: usize = 4096;

/// How many *tokens* one spoken word costs Qwen's tokenizer -- measured
/// directly (not estimated): a 120-word English sample tokenized to
/// exactly 120 tokens (~1.0/word); a 17-word Tamil sample tokenized to 146
/// tokens (~8.6/word). Qwen's tokenizer is built around Latin/CJK text;
/// Tamil script falls back to a much less efficient encoding, so a
/// moderate Tamil transcript can hit the context limit at a fraction of
/// the word count an English one would. Adding another non-Latin language
/// later should re-measure its own ratio via the same `/tokenize` check
/// rather than assuming it behaves like Tamil.
pub const ENGLISH_TOKENS_PER_WORD: f64 = 1.3; // measured 1.0, padded for punctuation/mixed content
pub const TAMIL_TOKENS_PER_WORD: f64 = 9.0; // measured 8.6, padded for safety margin

pub fn transcript_text(words: &[WordTimestamp]) -> String {
    words.iter().map(|w| w.word.as_str()).collect::<Vec<_>>().join(" ")
}

fn tokens_per_word(language: Option<&str>) -> f64 {
    if language.is_some_and(|l| l.contains("Tamil")) { TAMIL_TOKENS_PER_WORD } else { ENGLISH_TOKENS_PER_WORD }
}

/// How many transcript *words* fit in what's left of the context window
/// after reserving `system_prompt_tokens` (the caller's own system prompt
/// as wrapped in ChatML, measured directly via llama-server's own
/// `/tokenize` endpoint -- see each caller's own doc comment for its
/// measured number) and `output_tokens` (the caller's own `max_tokens`
/// budget passed to `llm::complete` -- reserved from the same context
/// window, not free space on top of it).
pub fn transcript_word_budget(language: Option<&str>, system_prompt_tokens: usize, output_tokens: usize) -> usize {
    let transcript_token_budget = CONTEXT_SIZE_TOKENS.saturating_sub(system_prompt_tokens).saturating_sub(output_tokens);
    ((transcript_token_budget as f64) / tokens_per_word(language)).floor() as usize
}

/// A transcript within budget goes through whole; a longer one is sampled
/// from the beginning, middle, and end (40/20/40 split) rather than simply
/// cut off at the budget -- most uses of a full transcript (style, topic,
/// summary) should reflect the *whole* thing, and truncating from the end
/// alone would only ever show the model an intro.
pub fn transcript_text_for_prompt(
    words: &[WordTimestamp],
    language: Option<&str>,
    system_prompt_tokens: usize,
    output_tokens: usize,
) -> String {
    let budget = transcript_word_budget(language, system_prompt_tokens, output_tokens);
    if words.len() <= budget {
        return transcript_text(words);
    }

    let join = |slice: &[WordTimestamp]| slice.iter().map(|w| w.word.as_str()).collect::<Vec<_>>().join(" ");

    let first_n = budget * 2 / 5;
    let last_n = budget * 2 / 5;
    let middle_n = budget.saturating_sub(first_n + last_n);
    let middle_start = (words.len().saturating_sub(middle_n)) / 2;

    format!(
        "{} ... {} ... {}",
        join(&words[..first_n]),
        join(&words[middle_start..middle_start + middle_n]),
        join(&words[words.len() - last_n..])
    )
}
