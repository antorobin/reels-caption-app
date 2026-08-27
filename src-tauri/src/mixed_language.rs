// Language detection + transcription strategy for `pipeline.rs`'s
// `run_pipeline`/`transcribe_audio_file` -- handles both an ordinary
// single-language video AND one where different speakers (or the same
// speaker, code-switching mid-sentence) use different supported languages
// within one recording, e.g. one person in Tamil, another in English.
//
// This always segments and classifies first (see `detect_language_chunks`
// below) rather than only detecting language once for the whole file --
// there's no separate "mixed language" mode a user has to opt into. The
// earlier version of this feature was exactly that opt-in checkbox; it was
// removed once it became clear the segment-and-classify approach costs
// little enough (one ffmpeg silencedetect pass + one batched, single
// model-load Python call) to just always run, and a single-language video
// naturally collapses to one chunk anyway, taking the fast single-call
// transcription path with no slicing overhead at all. Detecting reliably
// is genuinely worth doing unconditionally: `stt::detect_spoken_language`'s
// old single whole-file call had its own blind spot even for ordinary
// single-language long-form audio (Whisper's language-ID only examines
// roughly the first ~30 seconds of whatever it's handed), which this
// segment-based approach also happens to fix as a side effect.
//
// Segmentation happens language-agnostically, via ffmpeg's own silence
// detection (`ffmpeg::detect_speech_segments`) directly on the waveform --
// not from word-level timestamps the way `jumpcuts.rs`'s silence-gap logic
// works. Getting word timestamps first would require already having
// transcribed the file in *some* one language, which is exactly the
// chicken-and-egg problem this module exists to avoid.
//
// Quality/robustness note (this is the part that took iterating on):
// classifying every raw silence-bounded segment and transcribing each one
// independently would fragment the job into a lot of short clips -- each
// STT call gets less audio context, which measurably hurts accuracy (see
// stt.rs's own doc comment on short-segment artifacts encountered
// elsewhere in this project). So segments that resolve to the same
// language are merged into one contiguous chunk *before* transcribing, and
// a segment too short or too low-confidence to trust on its own inherits
// its neighbor's resolved language instead of forcing a split -- both
// specifically to keep every real STT call working on the longest
// reasonably-possible same-language span, without making the underlying
// segmentation itself any coarser (a genuine language switch, even a
// quick one, still gets its own chunk).

use std::path::Path;
use tauri::AppHandle;

use crate::ffmpeg;
use crate::pipeline::{WordTimestamp, PROGRESS_EVENT};
use crate::stt;
use crate::util::{cli_path, emit_progress, unique_temp_path};

/// Below this, there's truly not enough audio for the language-ID model to
/// say anything meaningful -- always smoothed over, regardless of what
/// confidence it happened to report.
const MIN_DURATION_SECONDS: f64 = 0.35;
/// A segment at or above this confidence is trusted at *any* duration past
/// the floor above -- a short but unambiguous detection (this project's own
/// benchmark: 98.7% on English, 91-95% on Tamil) shouldn't be overridden
/// just because the clip is brief. Getting this wrong was a real bug found
/// via a live test video full of short back-and-forth phrases ("Start",
/// "Thank you"): an earlier version of this function required BOTH >=1.2s
/// duration AND >=55% confidence to ever trust a segment's own language,
/// so every short genuine utterance -- which is most of a video like that
/// one -- always inherited whatever its neighbor was labeled, and a short
/// English word sitting next to Tamil speech got transcribed as Tamil
/// (the model didn't switch languages; it default to whatever Tamil-vs-
/// English phonetic reading of that word its checkpoint knows, since a
/// language-specific fine-tune only ever outputs its own script).
const HIGH_CONFIDENCE_THRESHOLD: f64 = 0.70;
/// A segment below high confidence can still be trusted if it's long
/// enough to have given the model real material to work with.
const MODERATE_CONFIDENCE_THRESHOLD: f64 = 0.55;
const MODERATE_CONFIDENCE_MIN_DURATION: f64 = 0.8;

/// Extra silence kept on each side of a chunk's extracted audio slice so
/// the STT model isn't handed audio abruptly clipped mid-word right at a
/// boundary. Segment boundaries already sit inside detected silence (that
/// is what made them boundaries), so this only widens into more of that
/// same silence, never into neighboring speech.
const CHUNK_PADDING_SECONDS: f64 = 0.15;

struct LanguageChunk {
    start: f64,
    end: f64,
    language: String,
}

/// Whisper's encoder works on a fixed ~30-second context window; asking
/// its language-ID call to classify a much longer stretch in one go
/// silently ignores everything past roughly that window rather than
/// erroring -- confirmed directly on a real bilingual test file: ffmpeg's
/// silencedetect found no qualifying gap across a continuous ~48-second
/// take (people don't reliably pause >=0.5s between languages), so the
/// whole thing became ONE raw segment, and `detect_language()` on that
/// whole segment reported "Tamil" purely because the first ~10s happened
/// to open in Tamil -- the ~37 seconds of confidently-English speech that
/// followed were never actually examined. Capping how long a single
/// language-ID call ever looks at fixes this: any segment longer than this
/// gets split into equal sub-windows purely for classification, so a
/// language change deep inside one long silence-free stretch still gets
/// caught. The later same-language merge step (`merge_into_chunks`) still
/// recombines same-language sub-windows into one contiguous transcription
/// chunk afterward, so this doesn't fragment the final transcription --
/// only how finely language is *sampled* across a long stretch.
const MAX_SEGMENT_SECONDS_FOR_LANGUAGE_ID: f64 = 8.0;

fn subdivide_long_segments(segments: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    let mut result = Vec::with_capacity(segments.len());
    for (start, end) in segments {
        let duration = end - start;
        if duration <= MAX_SEGMENT_SECONDS_FOR_LANGUAGE_ID {
            result.push((start, end));
            continue;
        }
        let pieces = (duration / MAX_SEGMENT_SECONDS_FOR_LANGUAGE_ID).ceil() as usize;
        let piece_len = duration / pieces as f64;
        for i in 0..pieces {
            let piece_start = start + (i as f64) * piece_len;
            let piece_end = if i + 1 == pieces { end } else { start + ((i + 1) as f64) * piece_len };
            result.push((piece_start, piece_end));
        }
    }
    result
}

/// First pass over the raw, per-segment language guesses: decide each
/// segment's *trusted* language, then fill every untrusted segment from
/// the nearest trusted neighbor -- looking both backward (continue the
/// preceding trusted language, the common case) and forward (for untrusted
/// segments that come *before* the first trusted one, which backward-only
/// filling couldn't reach and would otherwise leave stuck on a bogus/
/// unsupported raw guess forever). That "stuck forever" case was a second
/// real bug found alongside the confidence-threshold one above: a leading
/// segment too short/uncertain to trust, with no prior trusted language to
/// inherit yet, kept its own (sometimes "unknown", sometimes wrong)
/// language, and `transcribe_chunk` silently skips a chunk whose language
/// isn't one this app supports -- which showed up as multi-second gaps
/// with no words at all where that audio should have been transcribed.
/// Doesn't merge anything yet -- see `merge_into_chunks`.
fn resolve_segment_languages(raw: Vec<(f64, f64, String, f64)>) -> Vec<(f64, f64, String)> {
    let n = raw.len();

    let trust: Vec<Option<String>> = raw
        .iter()
        .map(|(start, end, language, probability)| {
            let duration = end - start;
            if !stt::is_supported_language_code(language) || duration < MIN_DURATION_SECONDS {
                return None;
            }
            let trustworthy = *probability >= HIGH_CONFIDENCE_THRESHOLD
                || (*probability >= MODERATE_CONFIDENCE_THRESHOLD && duration >= MODERATE_CONFIDENCE_MIN_DURATION);
            trustworthy.then(|| language.clone())
        })
        .collect();

    let mut forward_filled: Vec<Option<String>> = Vec::with_capacity(n);
    let mut last_trusted: Option<String> = None;
    for t in &trust {
        if t.is_some() {
            last_trusted = t.clone();
        }
        forward_filled.push(last_trusted.clone());
    }

    let mut backward_filled: Vec<Option<String>> = vec![None; n];
    let mut next_trusted: Option<String> = None;
    for i in (0..n).rev() {
        if trust[i].is_some() {
            next_trusted = trust[i].clone();
        }
        backward_filled[i] = next_trusted.clone();
    }

    raw.into_iter()
        .enumerate()
        .map(|(i, (start, end, raw_language, _probability))| {
            let resolved = trust[i]
                .clone()
                .or_else(|| forward_filled[i].clone())
                .or_else(|| backward_filled[i].clone())
                // Only reachable if literally nothing in the entire video
                // ever earned trust -- transcribe_chunk skips this
                // gracefully rather than erroring the whole run.
                .unwrap_or(raw_language);
            (start, end, resolved)
        })
        .collect()
}

/// Second pass: collapse consecutive same-language segments into one
/// contiguous chunk -- this is the step that actually protects
/// transcription quality, by maximizing how much audio each real STT call
/// gets to work with.
fn merge_into_chunks(resolved: Vec<(f64, f64, String)>) -> Vec<LanguageChunk> {
    let mut chunks: Vec<LanguageChunk> = Vec::new();
    for (start, end, language) in resolved {
        if let Some(last) = chunks.last_mut() {
            if last.language == language {
                last.end = end;
                continue;
            }
        }
        chunks.push(LanguageChunk { start, end, language });
    }
    chunks
}

async fn transcribe_chunk(
    app: &AppHandle,
    audio_path: &Path,
    total_duration: f64,
    chunk: &LanguageChunk,
) -> Result<Vec<WordTimestamp>, String> {
    if !stt::is_supported_language_code(&chunk.language) {
        // An unresolvable stretch (e.g. nothing in the whole file ever
        // earned a trusted language) -- skip it rather than failing the
        // entire mixed-language run over one unclassifiable chunk.
        return Ok(Vec::new());
    }

    let padded_start = (chunk.start - CHUNK_PADDING_SECONDS).max(0.0);
    let padded_end = (chunk.end + CHUNK_PADDING_SECONDS).min(total_duration);

    let slice_path = unique_temp_path("mixed-lang-chunk", "wav");
    let output = tokio::process::Command::new(crate::bin_paths::ffmpeg_path())
        .args([
            "-y".to_string(),
            "-i".to_string(),
            cli_path(audio_path),
            "-ss".to_string(),
            format!("{padded_start}"),
            "-to".to_string(),
            format!("{padded_end}"),
            "-c".to_string(),
            "copy".to_string(),
            cli_path(&slice_path),
        ])
        .output()
        .await
        .map_err(|e| format!("Failed to run ffmpeg to slice a language chunk: {e}"))?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&slice_path);
        return Err(format!("Failed to slice audio chunk: {}", String::from_utf8_lossy(&output.stderr)));
    }

    let result = stt::transcribe_and_align(app, &slice_path, &chunk.language).await;
    let _ = std::fs::remove_file(&slice_path);
    let (words, _echoed_language) = result?;

    // Words come back timed relative to the padded slice's own start --
    // shift them into the full audio's timeline, and drop anything that
    // landed in the padding itself. Real speech shouldn't be there --
    // padding only extends into already-detected silence -- but a
    // slightly imprecise silence boundary could still let a fragment
    // through, and keeping it risks a duplicate word once the *next*
    // chunk's own padding overlaps the same instant.
    let words = words
        .into_iter()
        .filter_map(|w| {
            let shifted_start = w.start + padded_start;
            let shifted_end = w.end + padded_start;
            if shifted_end < chunk.start - 0.02 || shifted_start > chunk.end + 0.02 {
                return None;
            }
            Some(WordTimestamp { word: w.word, start: shifted_start, end: shifted_end })
        })
        .collect();

    Ok(words)
}

/// Segments `audio_path` (language-agnostically) and returns the resolved,
/// merged same-language chunks covering it -- the shared first half of
/// both the fast single-language path and the multi-chunk path below.
async fn detect_language_chunks(app: &AppHandle, audio_path: &Path) -> Result<Vec<LanguageChunk>, String> {
    emit_progress(app, PROGRESS_EVENT, "segmenting", Some(0.0), None);
    let raw_segments = ffmpeg::detect_speech_segments(audio_path).await?;
    if raw_segments.is_empty() {
        return Ok(Vec::new());
    }
    let raw_segments = subdivide_long_segments(raw_segments);

    emit_progress(app, PROGRESS_EVENT, "detecting_languages", Some(0.0), None);
    let raw_with_language = stt::detect_spoken_languages_batch(audio_path, &raw_segments).await?;

    let resolved = resolve_segment_languages(raw_with_language);
    Ok(merge_into_chunks(resolved))
}

/// The always-on entry point `pipeline.rs` calls for every video --
/// detects language chunk(s) first, then either transcribes the whole file
/// in one call (a single chunk: an ordinary single-language video, the
/// common case, with no slicing overhead at all) or transcribes each
/// language-homogeneous chunk separately and stitches the results (more
/// than one chunk: a genuinely mixed-language recording).
pub(crate) async fn transcribe_with_language_detection(
    app: &AppHandle,
    audio_path: &Path,
) -> Result<(Vec<WordTimestamp>, Option<String>), String> {
    let chunks = detect_language_chunks(app, audio_path).await?;

    let [single] = chunks.as_slice() else {
        return transcribe_mixed_chunks(app, audio_path, &chunks).await;
    };

    if !stt::is_supported_language_code(&single.language) {
        return Err(format!(
            "Detected spoken language '{}' isn't supported yet -- this app currently supports: {}.",
            single.language,
            stt::supported_language_names().join(", ")
        ));
    }
    let (words, _echoed_language) = stt::transcribe_and_align(app, audio_path, &single.language).await?;
    Ok((words, Some(stt::language_name_for_code(&single.language))))
}

async fn transcribe_mixed_chunks(
    app: &AppHandle,
    audio_path: &Path,
    chunks: &[LanguageChunk],
) -> Result<(Vec<WordTimestamp>, Option<String>), String> {
    if chunks.is_empty() {
        return Ok((Vec::new(), None));
    }
    // The last chunk's end is a safe (if occasionally slightly
    // conservative, when the file ends in trailing silence) stand-in for
    // the audio's true total duration -- good enough for clamping chunk
    // padding below without a second ffprobe round-trip.
    let total_duration = chunks.last().map(|c| c.end).unwrap_or(0.0);

    let mut all_words = Vec::new();
    let mut languages_seen = std::collections::BTreeSet::new();
    let chunk_count = chunks.len().max(1);
    for (i, chunk) in chunks.iter().enumerate() {
        emit_progress(app, PROGRESS_EVENT, "transcribing", Some((i as f64 / chunk_count as f64) * 100.0), None);
        let words = transcribe_chunk(app, audio_path, total_duration, chunk).await?;
        if !words.is_empty() {
            languages_seen.insert(stt::language_name_for_code(&chunk.language));
        }
        all_words.extend(words);
    }
    emit_progress(app, PROGRESS_EVENT, "transcribing", Some(100.0), None);

    let detected_language =
        if languages_seen.is_empty() { None } else { Some(languages_seen.into_iter().collect::<Vec<_>>().join(" + ")) };

    Ok((all_words, detected_language))
}
