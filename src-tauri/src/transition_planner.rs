// LLM-assisted transition planning: refines the heuristic candidate list
// already computed client-side (`suggestTransitionPoints` in
// src/lib/captions.js -- speaker changes, silence gaps, prosody-driven
// emphasis, pace shifts) using the local LLM's read of the transcript's
// actual *semantic* content, picking a real editorial reason and one of
// video_transitions.rs's four effects for whichever candidates genuinely
// warrant one.
//
// Deliberately NOT free generation -- the LLM only ever chooses among
// candidate times it is *given*, never invents a new one. This was found
// necessary by direct testing against the real local model (Qwen2.5-
// 0.5B-Instruct via llm.rs), not assumed: an earlier design that let the
// model freely propose its own timestamps was tested against several
// real synthetic transcripts and, twice, returned a `time` that did not
// match any real segment boundary at all -- once literally a segment's
// *end* time rather than a start. Constraining the task to "classify one
// of these N already-real candidates" instead of "invent a timestamp"
// closes that failure mode structurally, and was re-verified clean
// across 5 real test runs (3 different transcripts, including 2 repeats
// for consistency) with zero invalid times returned. `plan_transitions`
// below still validates every returned time against the real candidate
// list as a defensive backstop regardless -- never trusting the model's
// echo blindly, the same "verify, then still validate" discipline this
// session applied to `sanitize_hashtag`/`sanitize_emoji` in
// content_ideas.rs for a different untrusted-output failure mode.
//
// A second, real, honestly-documented limitation found in the same
// testing (not fixed, matching content_ideas.rs's own Tamil-comprehension
// and music_gen.rs's own mood-matching precedent for owning a small
// model's real limits rather than silently shipping around them): asked
// to judge a plain step-by-step list with no real narrative shift (a
// baking recipe, a coding tutorial), this 0.5B model does not reliably
// return "drop everything" even with a worked few-shot example
// demonstrating exactly that -- it tends to keep at least one candidate
// with an invented-sounding reason. This is mitigated architecturally,
// not eliminated: every output here is still a plain, dismissable
// `VideoTransition` the user can remove with one click via Timeline.jsx's
// existing popover (never auto-committed to an override or anything
// harder to undo), and the candidates being judged are already
// pre-filtered by the heuristic -- a false positive here is never worse
// than what the heuristic alone would already have suggested, just with
// a (sometimes wrong) LLM-authored reason attached instead of a bare
// signal name.

use serde::{Deserialize, Serialize};

use crate::captions::speaker_id_at;
use crate::diarize::SpeakerSegment;
use crate::pipeline::WordTimestamp;
use crate::video_transitions::TransitionEffect;

/// One candidate point handed in from `suggestTransitionPoints` (JS) --
/// `reasons`/`speaker_id` aren't used directly in the prompt (the LLM
/// reasons from the real transcript text instead, not these signal
/// names), only `time`, which anchors this candidate to the real,
/// already-computed transcript segment it falls within.
#[derive(Debug, Clone, Deserialize)]
pub struct TransitionCandidate {
    pub time: f64,
}

/// One entry the LLM chose to keep, with its own authored reason and
/// picked effect -- `effect` is the real `TransitionEffect` enum (not a
/// raw string), so an out-of-library value can't reach the frontend even
/// in principle; llama-server's own JSON-schema-constrained generation
/// already keeps the model from producing anything outside the 4 real
/// variants at the token level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitionPlanEntry {
    pub time: f64,
    pub reason: String,
    pub effect: TransitionEffect,
}

#[derive(Debug, Serialize)]
struct TranscriptSegmentForPrompt {
    start: f64,
    end: f64,
    text: String,
    speaker: Option<u8>,
    silence_before: f64,
    silence_after: f64,
}

/// Groups `words` into sentence-like segments, breaking after a word
/// ending in `.`/`!`/`?` (or at the very last word, however it ends) --
/// the same "what a human would call one sentence" granularity as the
/// user-provided example this prompt was designed and verified against.
fn segment_words(words: &[WordTimestamp]) -> Vec<(f64, f64, String)> {
    let mut segments = Vec::new();
    let mut seg_start_idx = 0;
    for (i, w) in words.iter().enumerate() {
        let ends_sentence = w.word.trim_end().ends_with(['.', '!', '?']);
        let is_last = i == words.len() - 1;
        if ends_sentence || is_last {
            let start = words[seg_start_idx].start;
            let end = w.end;
            let text = words[seg_start_idx..=i].iter().map(|w| w.word.as_str()).collect::<Vec<_>>().join(" ");
            segments.push((start, end, text));
            seg_start_idx = i + 1;
        }
    }
    segments
}

fn build_segments(words: &[WordTimestamp], speakers: &[SpeakerSegment]) -> Vec<TranscriptSegmentForPrompt> {
    let raw = segment_words(words);
    raw.iter()
        .enumerate()
        .map(|(i, (start, end, text))| {
            let silence_before = if i == 0 { 0.0 } else { (start - raw[i - 1].1).max(0.0) };
            let silence_after = if i + 1 < raw.len() { (raw[i + 1].0 - end).max(0.0) } else { 0.0 };
            TranscriptSegmentForPrompt {
                start: *start,
                end: *end,
                text: text.clone(),
                speaker: speaker_id_at(speakers, *start),
                silence_before,
                silence_after,
            }
        })
        .collect()
}

// Verified against the real local model across 5 test runs (3 distinct
// synthetic transcripts: a real narrative arc, a plain step-by-step
// tutorial, and a hardship-to-success arc; 2 of the runs were exact
// repeats checking consistency) before this exact wording was settled on
// -- see this module's own doc comment above for the two real findings
// that shaped it (the free-generation timestamp-hallucination bug this
// candidate-constrained design exists to close, and the honestly-kept
// "doesn't reliably say nothing" limitation on flat/list-style content).
const TRANSITION_PLAN_SYSTEM: &str = r#"You are a video editor deciding which of a few candidate moments in a spoken video actually deserve a transition effect, and which effect fits best. You are given the video's transcript (split into timed segments, each with start time, end time, text, speaker number, and the silence gap before/after it) and a list of candidate times already flagged by timing analysis (a pause, a speaker change, a pace shift, or vocal emphasis).

For each candidate time, decide using the transcript's actual content: does this moment genuinely mark a topic change, a tonal/emotional shift (e.g. struggle to success), or a meaningful time jump ("three years later")? If a candidate is just a mechanical pause or speaker change with no real narrative shift, DROP it -- not every candidate deserves an effect. It is correct and common to drop most or all of the candidates for a plain step-by-step list with no narrative shift at all.

For each candidate you keep, choose exactly one of these four real effects based on which best fits WHY that moment deserves one:
- "zoom-punch": a quick zoom-in, for a moment of emphasis, excitement, or a rising highlight
- "flash-cut": a quick white flash, for a hard cut between two different speakers or a natural pause/breath
- "shake": a jarring jolt, for a sudden or abrupt shift in energy or pace
- "color-pulse": a brief desaturate pulse, for a tonal or mood shift, or a slowdown

CRITICAL: the "time" value in your answer must be copied EXACTLY from the given candidate list -- never a different number, never invented.

Respond with only the JSON object: {"transitions": [{"time": ..., "reason": ..., "effect": ...}, ...]} -- omit any candidate you decide to drop.

Example 1 (a real narrative arc):
Segments: [{"start": 0.0, "end": 18.2, "text": "I started my company in 2020.", "speaker": 0, "silence_before": 0.0, "silence_after": 0.3}, {"start": 18.5, "end": 35.0, "text": "The first six months were extremely difficult.", "speaker": 0, "silence_before": 0.3, "silence_after": 0.4}, {"start": 35.4, "end": 52.0, "text": "Then we got our first customer.", "speaker": 0, "silence_before": 0.4, "silence_after": 0.2}, {"start": 52.1, "end": 68.0, "text": "Today we have more than 100 customers.", "speaker": 0, "silence_before": 0.2, "silence_after": 0.0}]
Candidate times: [18.5, 35.4, 52.1]
Transitions: {"transitions": [{"time": 35.4, "reason": "hardship turns into the first customer win", "effect": "zoom-punch"}, {"time": 52.1, "reason": "jumps forward in time to today", "effect": "color-pulse"}]}

Example 2 (a plain step-by-step tutorial -- correct answer drops every candidate):
Segments: [{"start": 0.0, "end": 6.0, "text": "First, open your terminal and navigate to the project folder.", "speaker": 0, "silence_before": 0.0, "silence_after": 0.1}, {"start": 6.1, "end": 12.0, "text": "Then run npm install to get all the dependencies.", "speaker": 0, "silence_before": 0.1, "silence_after": 0.1}, {"start": 12.1, "end": 18.0, "text": "After that, run npm run dev to start the local server.", "speaker": 0, "silence_before": 0.1, "silence_after": 0.1}, {"start": 18.1, "end": 24.0, "text": "And finally, open localhost 3000 in your browser to see it running.", "speaker": 0, "silence_before": 0.1, "silence_after": 0.0}]
Candidate times: [6.1, 12.1, 18.1]
Transitions: {"transitions": []}

Respond with only the JSON object for the new segments and candidates."#;

/// Measured directly (not estimated) via llama-server's own `/tokenize`
/// endpoint against `TRANSITION_PLAN_SYSTEM` above, wrapped in ChatML
/// plus the "Segments: " prefix with no body yet: 1020 tokens. Rounded up
/// with margin for future prompt edits, matching music_gen.rs/
/// content_ideas.rs's own precedent for this constant.
const SYSTEM_PROMPT_TOKEN_BUDGET: usize = 1100;
/// `suggestTransitionPoints` (captions.js) already caps its own output at
/// 12 candidates (`MAX_SUGGESTIONS`) -- generous headroom per entry at
/// that count, well within what real test responses (2-4 entries) needed.
const OUTPUT_TOKEN_BUDGET: usize = 500;
/// How close (in seconds) an LLM-returned time must land to a real
/// candidate to count as a match -- floating-point round-tripping through
/// JSON, not a real tolerance for "close enough but different."
const TIME_MATCH_TOLERANCE_SECONDS: f64 = 0.01;

fn transition_plan_json_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "transitions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "time": { "type": "number" },
                        "reason": { "type": "string" },
                        "effect": { "type": "string", "enum": ["zoom-punch", "flash-cut", "shake", "color-pulse"] }
                    },
                    "required": ["time", "reason", "effect"]
                }
            }
        },
        "required": ["transitions"]
    })
}

#[derive(Debug, Deserialize)]
struct RawTransitionPlan {
    transitions: Vec<TransitionPlanEntry>,
}

#[tauri::command]
pub async fn suggest_transition_plan(
    app: tauri::AppHandle,
    words: Vec<WordTimestamp>,
    speakers: Vec<SpeakerSegment>,
    candidates: Vec<TransitionCandidate>,
    language: Option<String>,
) -> Result<Vec<TransitionPlanEntry>, String> {
    if words.is_empty() || candidates.is_empty() {
        return Ok(vec![]); // nothing to plan from -- not an error, just nothing to suggest
    }

    // A transcript longer than the budget is truncated from the start
    // rather than sampled (unlike transcript_text_for_prompt's begin/
    // middle/end split) -- segments need to stay one contiguous run to
    // read as a real transcript; candidates past the truncation point are
    // dropped below rather than judged with no surrounding context. A
    // real, accepted scope cut for unusually long videos, not addressed
    // further here -- this app's own target content (short-form reels)
    // is comfortably within budget in the common case.
    let word_budget = crate::llm_budget::transcript_word_budget(language.as_deref(), SYSTEM_PROMPT_TOKEN_BUDGET, OUTPUT_TOKEN_BUDGET);
    let budgeted_words: &[WordTimestamp] = if words.len() > word_budget { &words[..word_budget] } else { &words };

    let segments = build_segments(budgeted_words, &speakers);
    let Some(last_segment) = segments.last() else {
        return Ok(vec![]);
    };
    let coverage_end = last_segment.end;

    let valid_times: Vec<f64> = candidates.iter().map(|c| c.time).filter(|t| *t <= coverage_end).collect();
    if valid_times.is_empty() {
        return Ok(vec![]);
    }

    let segments_json = serde_json::to_string(&segments).map_err(|e| format!("Couldn't serialize transcript segments: {e}"))?;
    let candidates_json = serde_json::to_string(&valid_times).map_err(|e| format!("Couldn't serialize candidate times: {e}"))?;
    let user_prompt = format!("Segments: {segments_json}\nCandidate times: {candidates_json}");

    let raw = crate::llm::complete(
        &app,
        TRANSITION_PLAN_SYSTEM,
        &user_prompt,
        OUTPUT_TOKEN_BUDGET as i32,
        0.2, // low temperature -- this is a structured classification task over given candidates, not creative generation
        Some(transition_plan_json_schema()),
    )
    .await?;
    let parsed: RawTransitionPlan = serde_json::from_str(raw.trim())
        .map_err(|e| format!("Couldn't parse the model's transition-plan output as JSON: {e} (raw: {raw})"))?;

    // Defensive validation, confirmed necessary by direct testing against
    // the real model (see this module's own doc comment): even
    // explicitly constrained to candidate times only, and even after
    // that constraint was verified clean across 5 fresh test runs, this
    // is still never trusted blindly -- anything that doesn't match a
    // real candidate within a tiny floating-point tolerance is dropped,
    // and duplicate times (also seen in real testing -- one response
    // repeated the same time twice with the same reason/effect) are
    // deduped, keeping the first occurrence.
    let mut seen_times: Vec<f64> = Vec::new();
    let validated: Vec<TransitionPlanEntry> = parsed
        .transitions
        .into_iter()
        .filter(|entry| {
            let matches_candidate = valid_times.iter().any(|t| (t - entry.time).abs() < TIME_MATCH_TOLERANCE_SECONDS);
            let is_duplicate = seen_times.iter().any(|t| (t - entry.time).abs() < TIME_MATCH_TOLERANCE_SECONDS);
            if matches_candidate && !is_duplicate {
                seen_times.push(entry.time);
                true
            } else {
                false
            }
        })
        .collect();

    Ok(validated)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(w: &str, start: f64, end: f64) -> WordTimestamp {
        WordTimestamp { word: w.to_string(), start, end }
    }

    #[test]
    fn segment_words_breaks_on_sentence_punctuation() {
        let words = vec![
            word("Hello", 0.0, 0.3),
            word("there.", 0.3, 0.6),
            word("How", 1.0, 1.2),
            word("are", 1.2, 1.4),
            word("you?", 1.4, 1.7),
        ];
        let segments = segment_words(&words);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0], (0.0, 0.6, "Hello there.".to_string()));
        assert_eq!(segments[1], (1.0, 1.7, "How are you?".to_string()));
    }

    #[test]
    fn segment_words_closes_a_trailing_segment_with_no_terminal_punctuation() {
        let words = vec![word("just", 0.0, 0.2), word("trailing", 0.2, 0.5), word("words", 0.5, 0.8)];
        let segments = segment_words(&words);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], (0.0, 0.8, "just trailing words".to_string()));
    }

    #[test]
    fn build_segments_computes_real_silence_gaps_and_speaker_ids() {
        let words = vec![word("Hi.", 0.0, 0.5), word("Bye.", 2.0, 2.5)];
        let speakers =
            vec![SpeakerSegment { start: 0.0, end: 1.0, speaker_id: 0 }, SpeakerSegment { start: 1.5, end: 3.0, speaker_id: 1 }];
        let segments = build_segments(&words, &speakers);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].silence_before, 0.0);
        assert!((segments[0].silence_after - 1.5).abs() < 1e-9);
        assert_eq!(segments[0].speaker, Some(0));
        assert_eq!(segments[1].silence_before, 1.5);
        assert_eq!(segments[1].silence_after, 0.0);
        assert_eq!(segments[1].speaker, Some(1));
    }

    #[test]
    fn json_schema_only_allows_the_four_real_effect_names() {
        let schema = transition_plan_json_schema();
        let enum_values = schema["properties"]["transitions"]["items"]["properties"]["effect"]["enum"].as_array().unwrap();
        let names: Vec<&str> = enum_values.iter().map(|v| v.as_str().unwrap()).collect();
        assert_eq!(names, vec!["zoom-punch", "flash-cut", "shake", "color-pulse"]);
    }
}
