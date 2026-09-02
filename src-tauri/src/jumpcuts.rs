// Removes silence and filler words ("um", "uh", ...) from a video —
// classic "jump cut" editing, automated. Reuses word-level timestamps
// we've already got from the STT step (no VAD model needed for silence:
// a "gap" is just the space between two consecutive words) and does the
// whole cut in a single ffmpeg pass via the `select`/`aselect` filters,
// rather than our segment-cut-and-concat machinery in segments.rs — that
// machinery exists to parallelize encoding a handful of large segments,
// but a jump-cut job can have dozens of small cuts, and `select` handles
// "keep these N time ranges, drop the rest" natively in one encode with
// no concat-boundary artifacts to worry about.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::ffmpeg::{best_encoder, probe_duration_seconds, run_with_progress};
use crate::pipeline::WordTimestamp;
use crate::util::{cli_path, record_fresh_transcript, unique_temp_path};

const PROGRESS_EVENT: &str = "jumpcut-progress";

// Deliberately conservative: only unambiguous filler interjections. Words
// like "like" or "so" are often meaningful, not filler, and misdetecting
// those would silently mangle someone's sentence — not an acceptable
// trade for an automated feature.
const FILLER_WORDS: &[&str] = &["um", "umm", "ummm", "uh", "uhh", "uhm", "erm", "er", "hmm", "hm"];

#[derive(Debug, Clone, Deserialize)]
pub struct JumpCutOptions {
    /// Gaps between words shorter than this are left alone — real speech
    /// has natural micro-pauses; only genuinely dead air gets cut.
    pub min_silence_seconds: f64,
    pub remove_filler_words: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct JumpCutResult {
    pub output_path: String,
    pub words: Vec<WordTimestamp>,
    pub removed_seconds: f64,
    pub cut_count: usize,
}

fn normalize_word(word: &str) -> String {
    word.trim().trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

fn is_filler_word(word: &str) -> bool {
    FILLER_WORDS.contains(&normalize_word(word).as_str())
}

/// A little silence left at each cut boundary reads as a natural pause;
/// zero-gap butt-cuts between words sound unnaturally rushed.
const KEEP_PADDING_SECONDS: f64 = 0.08;

/// Pure planning function: given word timestamps and the whole video's
/// duration, decides which `(start, end)` ranges to *keep*. Silence gaps
/// longer than `min_silence_seconds` and (optionally) filler-word spans
/// are treated as cuts; everything else survives.
pub fn compute_keep_ranges(words: &[WordTimestamp], duration: f64, options: &JumpCutOptions) -> Vec<(f64, f64)> {
    // Silence-gap cuts get shrunk inward before removal so a little
    // natural pause survives at each edge (a hard butt-cut between two
    // words sounds unnaturally rushed). Filler-word cuts deliberately do
    // NOT get this treatment — padding them the same way would leave an
    // audible fragment of the "um" itself at each edge, defeating the
    // point of removing it.
    let mut silence_cuts: Vec<(f64, f64)> = Vec::new();
    let mut filler_cuts: Vec<(f64, f64)> = Vec::new();

    if let Some(first) = words.first() {
        if first.start > options.min_silence_seconds {
            silence_cuts.push((0.0, first.start));
        }
    }

    for pair in words.windows(2) {
        let gap = pair[1].start - pair[0].end;
        if gap > options.min_silence_seconds {
            silence_cuts.push((pair[0].end, pair[1].start));
        }
    }

    if options.remove_filler_words {
        for word in words {
            if is_filler_word(&word.word) {
                filler_cuts.push((word.start, word.end));
            }
        }
    }

    let mut all_cuts: Vec<(f64, f64)> = silence_cuts
        .into_iter()
        .map(|(s, e)| (s + KEEP_PADDING_SECONDS, e - KEEP_PADDING_SECONDS))
        .filter(|(s, e)| e > s)
        .collect();
    all_cuts.extend(filler_cuts);
    all_cuts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    let mut merged: Vec<(f64, f64)> = Vec::new();
    for (s, e) in all_cuts {
        if let Some(last) = merged.last_mut() {
            if s <= last.1 {
                last.1 = last.1.max(e);
                continue;
            }
        }
        merged.push((s, e));
    }

    // Keep ranges = complement of the merged cuts within [0, duration].
    let mut keep = Vec::new();
    let mut cursor = 0.0;
    for (s, e) in merged {
        if s > cursor {
            keep.push((cursor, s));
        }
        cursor = cursor.max(e);
    }
    if cursor < duration {
        keep.push((cursor, duration));
    }

    keep
}

/// Remaps word timestamps onto the new, shortened timeline that results
/// from splicing out everything not in `keep_ranges`. A word that isn't
/// *fully* inside some keep range (a removed filler word, or one that
/// bordered a cut closely enough for padding to clip it) is dropped —
/// correct, since it won't be in the output video either.
pub fn remap_words(words: &[WordTimestamp], keep_ranges: &[(f64, f64)]) -> Vec<WordTimestamp> {
    let mut result = Vec::new();
    let mut new_cursor = 0.0;

    for (start, end) in keep_ranges {
        for word in words {
            if word.start >= *start && word.end <= *end {
                result.push(WordTimestamp {
                    word: word.word.clone(),
                    start: new_cursor + (word.start - start),
                    end: new_cursor + (word.end - start),
                });
            }
        }
        new_cursor += end - start;
    }

    result
}

fn build_select_expr(keep_ranges: &[(f64, f64)]) -> String {
    keep_ranges.iter().map(|(s, e)| format!("between(t,{s:.3},{e:.3})")).collect::<Vec<_>>().join("+")
}

#[tauri::command]
pub async fn remove_silence_and_fillers(
    app: AppHandle,
    video_path: String,
    words: Vec<WordTimestamp>,
    options: JumpCutOptions,
    project_id: String,
) -> Result<JumpCutResult, String> {
    if words.is_empty() {
        return Err("No transcript to work from — run the transcription pipeline first.".to_string());
    }

    let _permit = crate::concurrency::acquire_encode().await;

    let duration = probe_duration_seconds(&video_path)
        .await
        .ok_or("Couldn't determine the video's duration.".to_string())?;

    let keep_ranges = compute_keep_ranges(&words, duration, &options);
    let kept_duration: f64 = keep_ranges.iter().map(|(s, e)| e - s).sum();
    let removed_seconds = (duration - kept_duration).max(0.0);
    let cut_count = keep_ranges.len().saturating_sub(1) + usize::from(keep_ranges.first().is_some_and(|(s, _)| *s > 0.0));

    if removed_seconds < 0.3 {
        return Err("No silence or filler words long enough to cut were found.".to_string());
    }

    let select_expr = build_select_expr(&keep_ranges);
    let encoder = best_encoder().await;

    // This is an intermediate editing step (style + burn still come
    // later), so the trimmed video goes to a temp file rather than
    // interrupting the flow with a save dialog — same treatment as the
    // audio-extraction step.
    let output_path = unique_temp_path("trimmed", "mp4");
    let output_path_arg = cli_path(&output_path);

    let mut args = vec![
        "-y".to_string(),
        "-i".to_string(),
        cli_path(std::path::Path::new(&video_path)),
        "-vf".to_string(),
        format!("select='{select_expr}',setpts=N/FRAME_RATE/TB"),
        "-af".to_string(),
        format!("aselect='{select_expr}',asetpts=N/SR/TB"),
    ];
    args.extend(encoder.speed_args(0));
    args.push("-c:v".to_string());
    args.push(encoder.codec_name().to_string());
    args.push("-c:a".to_string());
    args.push("aac".to_string());
    args.push("-b:a".to_string());
    args.push("160k".to_string());
    args.push(output_path_arg.clone());

    run_with_progress(&app, PROGRESS_EVENT, &project_id, "trimming", args, Some(kept_duration)).await?;

    // The trimmed output is a brand-new file with remapped word timings
    // that are correct for it *right now* — record that so burn_captions
    // trusts it without requiring a redundant re-transcription.
    record_fresh_transcript(&app, &output_path_arg)?;

    Ok(JumpCutResult {
        output_path: output_path_arg,
        words: remap_words(&words, &keep_ranges),
        removed_seconds,
        cut_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(w: &str, start: f64, end: f64) -> WordTimestamp {
        WordTimestamp { word: w.to_string(), start, end }
    }

    fn options(min_silence: f64, remove_fillers: bool) -> JumpCutOptions {
        JumpCutOptions { min_silence_seconds: min_silence, remove_filler_words: remove_fillers }
    }

    #[test]
    fn is_filler_word_matches_known_fillers_case_and_punctuation_insensitively() {
        assert!(is_filler_word("um"));
        assert!(is_filler_word("Um,"));
        assert!(is_filler_word("UHH"));
        assert!(!is_filler_word("humble"));
        assert!(!is_filler_word("like")); // deliberately not treated as filler
    }

    #[test]
    fn compute_keep_ranges_leaves_normal_speech_untouched() {
        let words = vec![word("hello", 0.0, 0.5), word("world", 0.6, 1.0)];
        let keep = compute_keep_ranges(&words, 1.0, &options(0.6, false));
        // The 0.1s gap between "hello" and "world" is well under the 0.6s
        // threshold, so nothing should be cut.
        assert_eq!(keep, vec![(0.0, 1.0)]);
    }

    #[test]
    fn compute_keep_ranges_cuts_long_interior_silence() {
        let words = vec![word("hello", 0.0, 0.5), word("world", 3.0, 3.5)];
        let keep = compute_keep_ranges(&words, 4.0, &options(0.6, false));
        // Gap is 0.5..3.0 (2.5s), well over threshold. Padding keeps a
        // sliver of silence on each side of the cut.
        assert_eq!(keep.len(), 2);
        assert_eq!(keep[0], (0.0, 0.5 + KEEP_PADDING_SECONDS));
        assert_eq!(keep[1], (3.0 - KEEP_PADDING_SECONDS, 4.0));
    }

    #[test]
    fn compute_keep_ranges_trims_leading_silence() {
        let words = vec![word("hello", 2.0, 2.5)];
        let keep = compute_keep_ranges(&words, 3.0, &options(0.6, false));
        assert_eq!(keep.len(), 2);
        // A tiny pre-roll sliver survives from the padding (leaves a hint
        // of breathing room rather than starting mid-cut) — harmless, a
        // couple of frames at most.
        assert_eq!(keep[0], (0.0, KEEP_PADDING_SECONDS));
        assert!((keep[1].0 - (2.0 - KEEP_PADDING_SECONDS)).abs() < 1e-9);
        assert_eq!(keep[1].1, 3.0);
    }

    #[test]
    fn compute_keep_ranges_removes_filler_words_when_enabled() {
        let words = vec![word("so", 0.0, 0.3), word("um", 0.4, 0.7), word("yeah", 0.8, 1.2)];
        let keep = compute_keep_ranges(&words, 1.2, &options(0.6, true));
        // "um" at 0.4-0.7 should be excised even though the surrounding
        // gaps are all under the silence threshold.
        let total_kept: f64 = keep.iter().map(|(s, e)| e - s).sum();
        assert!(total_kept < 1.2, "expected some time removed for the filler word");
        for (s, e) in &keep {
            assert!(*e <= 0.4 + 1e-6 || *s >= 0.7 - 1e-6, "kept range {s}-{e} overlaps the filler word");
        }
    }

    #[test]
    fn compute_keep_ranges_ignores_filler_words_when_disabled() {
        let words = vec![word("um", 0.4, 0.7), word("yeah", 0.8, 1.2)];
        let keep = compute_keep_ranges(&words, 1.2, &options(0.6, false));
        assert_eq!(keep, vec![(0.0, 1.2)]);
    }

    #[test]
    fn remap_words_shifts_onto_the_new_shortened_timeline() {
        let words = vec![word("hello", 0.0, 0.5), word("world", 3.0, 3.5)];
        let keep_ranges = vec![(0.0, 0.58), (2.92, 4.0)];
        let remapped = remap_words(&words, &keep_ranges);
        assert_eq!(remapped.len(), 2);
        assert_eq!(remapped[0].word, "hello");
        assert_eq!(remapped[0].start, 0.0);
        // Second range starts at new_cursor = 0.58 (first range's length);
        // "world" was at 3.0, i.e. 0.08 into the second keep range.
        assert!((remapped[1].start - 0.66).abs() < 1e-9);
    }

    #[test]
    fn remap_words_drops_words_that_were_cut() {
        let words = vec![word("um", 0.0, 0.3), word("hello", 0.5, 1.0)];
        // Simulate "um" having been excised: only the second word's range survives.
        let keep_ranges = vec![(0.42, 1.0)];
        let remapped = remap_words(&words, &keep_ranges);
        assert_eq!(remapped.len(), 1);
        assert_eq!(remapped[0].word, "hello");
    }
}
