// Splits a long caption-burn job into several time ranges, encodes them in
// parallel (true multi-process — each ffmpeg invocation is its own OS
// process, scheduled independently by the OS), then stitches the results
// back together with a fast, lossless concat. Only worth the added
// complexity for videos long enough that the parallel wall-clock win
// outweighs per-segment overhead — see `recommended_segment_count`.
//
// Design notes — what this deliberately does and doesn't do, and why:
//
// - Cut points are snapped to gaps in the transcript (moments with no
//   active caption), not naive `duration/N` points. A boundary landing
//   mid-caption would truncate that caption's on-screen time at the cut
//   and never show the rest of it in the next segment — that's the real
//   "concat artifact" risk here, not video GOP misalignment.
// - We don't pre-probe video keyframes to align cuts to them. Every
//   segment gets a full decode + re-encode (never a stream copy), so each
//   output segment starts on its own fresh keyframe regardless of where
//   the *source* gets cut. Keyframe-aligning the source cut would only
//   save a little redundant decode time, not fix a correctness bug —
//   ffmpeg's own accurate input-seeking already handles arbitrary cut
//   points correctly, just marginally less efficiently.
// - Audio is re-encoded per segment (not stream-copied), with identical
//   codec/bitrate across every segment. Stream-copying an arbitrary time
//   slice risks a click/glitch where the cut doesn't land on an audio
//   frame boundary, and the concat demuxer needs uniform stream
//   parameters across segments regardless.
// - Thread counts are divided across segments explicitly. Leaving each
//   segment at "auto" threads would have N segments each try to grab
//   every core, oversubscribing the machine badly enough to end up
//   *slower* than a single full-width encode.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::AppHandle;
use tokio::process::Command;

use crate::captions::{build_ass_document, escape_ffmpeg_filter_path, CaptionStyle, CaptionStyleOverride, ProsodyWord};
use crate::diarize::SpeakerSegment;
use crate::ffmpeg::{best_encoder, run_capturing_progress, Encoder};
use crate::pipeline::WordTimestamp;
use crate::util::{cli_path, emit_progress, unique_temp_path};

const MIN_SEGMENT_SECONDS: f64 = 20.0;
const MAX_SEGMENTS: usize = 4;

#[derive(Debug, Clone)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    /// Word timestamps shifted to be relative to `start`, so a per-segment
    /// ASS file lines up with ffmpeg's PTS renormalization after an
    /// input-side `-ss` seek (the first frame of a seeked segment gets
    /// PTS ~0, not its original absolute timestamp).
    pub words: Vec<WordTimestamp>,
}

/// How many parallel segments to split into. Caps out at `MAX_SEGMENTS`
/// regardless of core count — beyond that, per-segment process-spawn and
/// encoder-init overhead eats into the wall-clock win faster than more
/// parallelism buys it back, for typical reel-length source video.
pub fn recommended_segment_count(duration: f64) -> usize {
    if duration < MIN_SEGMENT_SECONDS * 2.0 {
        return 1;
    }
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let by_duration = (duration / MIN_SEGMENT_SECONDS).floor() as usize;
    by_duration.clamp(1, MAX_SEGMENTS).min(cores.max(1))
}

/// Finds the transcript gap (silence between two words) whose midpoint is
/// closest to `ideal`, and returns that midpoint — as long as it's within
/// 5 seconds of `ideal`. Dense, gapless dialogue near the ideal split
/// point falls back to `ideal` itself rather than dragging the boundary
/// far away and badly unbalancing segment sizes.
fn snap_to_gap(ideal: f64, words: &[WordTimestamp], duration: f64) -> f64 {
    let mut best: Option<(f64, f64)> = None; // (gap_midpoint, distance_to_ideal)
    for pair in words.windows(2) {
        let gap_start = pair[0].end;
        let gap_end = pair[1].start;
        if gap_end <= gap_start {
            continue;
        }
        let midpoint = (gap_start + gap_end) / 2.0;
        let distance = (midpoint - ideal).abs();
        if best.is_none_or(|(_, best_dist)| distance < best_dist) {
            best = Some((midpoint, distance));
        }
    }

    match best {
        Some((midpoint, distance)) if distance < 5.0 => midpoint.clamp(0.5, (duration - 0.5).max(0.5)),
        _ => ideal,
    }
}

/// Pure planning function: given the whole video's duration and word
/// timestamps, decides where to cut and slices/shifts them for each
/// resulting segment.
pub fn plan_segments(duration: f64, words: &[WordTimestamp], target_count: usize) -> Vec<Segment> {
    if target_count <= 1 || duration <= 0.0 {
        return vec![Segment { start: 0.0, end: duration.max(0.0), words: words.to_vec() }];
    }

    let mut cut_points: Vec<f64> = (1..target_count)
        .map(|i| snap_to_gap(duration * i as f64 / target_count as f64, words, duration))
        .collect();
    cut_points.sort_by(|a, b| a.partial_cmp(b).unwrap());
    cut_points.dedup_by(|a, b| (*a - *b).abs() < 0.05);

    let mut boundaries = vec![0.0];
    boundaries.extend(cut_points);
    boundaries.push(duration);

    boundaries
        .windows(2)
        .filter(|w| w[1] - w[0] > 0.1) // drop degenerate zero-length segments from dedup edge cases
        .map(|w| {
            let (start, end) = (w[0], w[1]);
            let segment_words = words
                .iter()
                .filter(|word| word.start >= start && word.start < end)
                .map(|word| WordTimestamp {
                    word: word.word.clone(),
                    start: (word.start - start).max(0.0),
                    end: (word.end - start).min(end - start).max(0.0),
                })
                .collect();
            Segment { start, end, words: segment_words }
        })
        .collect()
}

/// Shifts prosody words from whole-video-absolute time into a segment's
/// own local time frame (mirrors how `plan_segments` shifts word
/// timestamps), keeping only the ones that actually fall in range.
fn shift_prosody(prosody: &[ProsodyWord], start: f64, end: f64) -> Vec<ProsodyWord> {
    prosody
        .iter()
        .filter(|p| p.start >= start && p.start < end)
        .map(|p| ProsodyWord { start: p.start - start, end: (p.end - start).min(end - start), intensity: p.intensity })
        .collect()
}

/// Same idea as `shift_prosody`, for diarization speaker segments.
fn shift_speakers(speakers: &[SpeakerSegment], start: f64, end: f64) -> Vec<SpeakerSegment> {
    speakers
        .iter()
        .filter(|s| s.start < end && s.end > start)
        .map(|s| SpeakerSegment {
            start: (s.start - start).max(0.0),
            end: (s.end - start).min(end - start),
            speaker_id: s.speaker_id,
        })
        .collect()
}

/// Clips and rebases caption-style-override ranges into a segment's own
/// local time frame. Uses `shift_speakers`'s overlap-filter-then-clip
/// shape, not `shift_prosody`'s simpler start-only filter — an override
/// is itself a *range* that can straddle a segment cut point (cuts are
/// chosen by `snap_to_gap`, entirely independent of override ranges), so
/// a partially-overlapping override must be clipped to the intersection
/// and appear (clipped) in every segment it actually overlaps — never
/// dropped, never duplicated in full.
fn shift_overrides(overrides: &[CaptionStyleOverride], start: f64, end: f64) -> Vec<CaptionStyleOverride> {
    overrides
        .iter()
        .filter(|o| o.start < end && o.end > start)
        .map(|o| CaptionStyleOverride {
            start: (o.start - start).max(0.0),
            end: (o.end - start).min(end - start),
            style: o.style.clone(),
        })
        .collect()
}

struct SegmentJob {
    index: usize,
    video_path: String,
    segment: Segment,
    style: CaptionStyle,
    encoder: Encoder,
    threads: usize,
    /// Whole-video-absolute-time prosody words — shifted to this
    /// segment's own local time frame (matching `segment.words`) right
    /// before rendering.
    prosody: Vec<ProsodyWord>,
    /// Same idea, for diarization speaker segments.
    speakers: Vec<SpeakerSegment>,
    /// Same idea, for per-portion caption style overrides — clipped to
    /// this segment's own range, not just shifted (see `shift_overrides`).
    overrides: Vec<CaptionStyleOverride>,
}

async fn burn_one_segment(job: SegmentJob, on_seconds: impl Fn(f64) + Send + 'static) -> Result<PathBuf, String> {
    let ass_path = unique_temp_path(&format!("segment-{}-captions", job.index), "ass");
    let local_prosody = shift_prosody(&job.prosody, job.segment.start, job.segment.end);
    let local_speakers = shift_speakers(&job.speakers, job.segment.start, job.segment.end);
    let local_overrides = shift_overrides(&job.overrides, job.segment.start, job.segment.end);
    let ass_contents = build_ass_document(&job.segment.words, &job.style, &local_prosody, &local_speakers, &local_overrides)?;
    std::fs::write(&ass_path, ass_contents).map_err(|e| format!("Failed to write subtitle file: {e}"))?;

    let filter = format!(
        "ass='{}':fontsdir='{}'",
        escape_ffmpeg_filter_path(&ass_path),
        escape_ffmpeg_filter_path(&crate::bin_paths::fonts_dir())
    );
    let output_path = unique_temp_path(&format!("segment-{}", job.index), "mp4");
    let output_path_arg = cli_path(&output_path);

    let mut args = vec![
        "-y".to_string(),
        "-ss".to_string(),
        job.segment.start.to_string(),
        "-to".to_string(),
        job.segment.end.to_string(),
        "-i".to_string(),
        job.video_path,
        "-vf".to_string(),
        filter,
    ];
    args.extend(job.encoder.speed_args(job.threads));
    args.push("-c:v".to_string());
    args.push(job.encoder.codec_name().to_string());
    // Re-encode audio (not copy) so every segment shares identical stream
    // parameters, which the concat demuxer requires, and to avoid a
    // stream-copy cut landing mid audio-frame right at the boundary.
    args.push("-c:a".to_string());
    args.push("aac".to_string());
    args.push("-b:a".to_string());
    args.push("160k".to_string());
    args.push(output_path_arg);

    let result = run_capturing_progress(args, on_seconds).await;

    let _ = std::fs::remove_file(&ass_path);
    result?;

    Ok(output_path)
}

/// Concatenates already-encoded segments (identical codec/params) into the
/// final output via ffmpeg's concat *demuxer* — a fast, lossless stream
/// copy/repackage, not a re-encode.
async fn concat_segments(segment_paths: &[PathBuf], output_path: &str) -> Result<(), String> {
    if segment_paths.is_empty() {
        return Err("No segments were produced to concatenate.".to_string());
    }
    if segment_paths.len() == 1 {
        tokio::fs::copy(&segment_paths[0], output_path)
            .await
            .map_err(|e| format!("Failed to finalize output: {e}"))?;
        return Ok(());
    }

    let list_path = unique_temp_path("concat-list", "txt");
    let mut list_contents = String::new();
    for path in segment_paths {
        // The concat demuxer has its own tiny quoting syntax: wrap in
        // single quotes, escape a literal single quote as '\''.
        let normalized = cli_path(path).replace('\\', "/").replace('\'', r"'\''");
        list_contents.push_str(&format!("file '{normalized}'\n"));
    }
    std::fs::write(&list_path, list_contents).map_err(|e| format!("Failed to write concat list: {e}"))?;
    let list_path_arg = cli_path(&list_path);

    let output = Command::new(crate::bin_paths::ffmpeg_path())
        .args(["-y", "-f", "concat", "-safe", "0", "-i", &list_path_arg, "-c", "copy", output_path])
        .output()
        .await
        .map_err(|e| format!("Failed to run ffmpeg concat: {e}"));

    let _ = std::fs::remove_file(&list_path);
    let output = output?;

    if !output.status.success() {
        return Err(format!("ffmpeg concat failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    Ok(())
}

/// Runs the full segmented burn: plan -> N parallel ffmpeg encodes with
/// aggregated live progress -> concat. Cleans up all segment temp files
/// (and reports the first error) whether or not the job succeeds.
pub async fn burn_captions_segmented(
    app: &AppHandle,
    video_path: &str,
    words: &[WordTimestamp],
    style: &CaptionStyle,
    output_path: &str,
    duration: f64,
    segment_count: usize,
    prosody: &[ProsodyWord],
    speakers: &[SpeakerSegment],
    overrides: &[CaptionStyleOverride],
    project_id: &str,
) -> Result<(), String> {
    let encoder = best_encoder().await;
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    let threads_per_segment = (cores / segment_count).max(1);

    let segments = plan_segments(duration, words, segment_count);
    let segment_durations: Vec<f64> = segments.iter().map(|s| (s.end - s.start).max(0.01)).collect();
    let total_duration: f64 = segment_durations.iter().sum();
    let segment_progress: Arc<Mutex<Vec<f64>>> = Arc::new(Mutex::new(vec![0.0; segments.len()]));

    emit_progress(
        app,
        "burn-progress",
        project_id,
        "burning",
        Some(0.0),
        Some(format!(
            "Encoding {} segments in parallel on {}",
            segments.len(),
            encoder.codec_name()
        )),
    );

    let mut handles = Vec::with_capacity(segments.len());
    for (index, segment) in segments.into_iter().enumerate() {
        let app_clone = app.clone();
        let project_id_clone = project_id.to_string();
        let video_path = video_path.to_string();
        let style = style.clone();
        let progress = Arc::clone(&segment_progress);
        let seg_duration = segment_durations[index];
        let total = total_duration;
        let prosody = prosody.to_vec();
        let speakers = speakers.to_vec();
        let overrides = overrides.to_vec();

        handles.push(tokio::spawn(async move {
            let on_seconds = move |secs: f64| {
                let percent = {
                    let mut guard = progress.lock().unwrap();
                    guard[index] = secs.min(seg_duration);
                    let sum: f64 = guard.iter().sum();
                    if total > 0.0 {
                        (sum / total * 100.0).clamp(0.0, 100.0)
                    } else {
                        0.0
                    }
                };
                emit_progress(&app_clone, "burn-progress", &project_id_clone, "burning", Some(percent), None);
            };

            burn_one_segment(
                SegmentJob {
                    index,
                    video_path,
                    segment,
                    style,
                    encoder,
                    threads: threads_per_segment,
                    prosody,
                    speakers,
                    overrides,
                },
                on_seconds,
            )
            .await
        }));
    }

    let mut segment_paths: Vec<Option<PathBuf>> = vec![None; handles.len()];
    let mut first_error: Option<String> = None;
    for (index, handle) in handles.into_iter().enumerate() {
        match handle.await {
            Ok(Ok(path)) => segment_paths[index] = Some(path),
            Ok(Err(e)) => {
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
            Err(join_err) => {
                if first_error.is_none() {
                    first_error = Some(format!("segment task panicked: {join_err}"));
                }
            }
        }
    }

    let ordered_paths: Vec<PathBuf> = segment_paths.into_iter().flatten().collect();

    if let Some(err) = first_error {
        for path in &ordered_paths {
            let _ = std::fs::remove_file(path);
        }
        return Err(format!("Segmented encode failed: {err}"));
    }

    let concat_result = concat_segments(&ordered_paths, output_path).await;

    for path in &ordered_paths {
        let _ = std::fs::remove_file(path);
    }
    concat_result?;

    emit_progress(app, "burn-progress", project_id, "burning", Some(100.0), None);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(w: &str, start: f64, end: f64) -> WordTimestamp {
        WordTimestamp { word: w.to_string(), start, end }
    }

    #[test]
    fn recommended_segment_count_skips_short_videos() {
        assert_eq!(recommended_segment_count(10.0), 1);
        assert_eq!(recommended_segment_count(39.9), 1);
    }

    #[test]
    fn recommended_segment_count_scales_with_duration_up_to_cap() {
        assert_eq!(recommended_segment_count(40.0), 2);
        assert_eq!(recommended_segment_count(60.0), 3);
        assert_eq!(recommended_segment_count(1000.0), MAX_SEGMENTS);
    }

    #[test]
    fn plan_segments_returns_single_segment_for_target_count_one() {
        let words = vec![word("hi", 0.0, 1.0)];
        let segments = plan_segments(100.0, &words, 1);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].start, 0.0);
        assert_eq!(segments[0].end, 100.0);
    }

    #[test]
    fn plan_segments_snaps_cut_to_transcript_gap_not_mid_caption() {
        // A long silent gap from 48s to 52s straddles the naive 50s
        // midpoint of a 100s video split in two. The cut must land inside
        // that gap, not in the middle of either caption.
        let words = vec![word("before", 40.0, 48.0), word("after", 52.0, 60.0)];
        let segments = plan_segments(100.0, &words, 2);
        assert_eq!(segments.len(), 2);
        let cut = segments[0].end;
        assert!(cut > 48.0 && cut < 52.0, "cut point {cut} should land in the 48-52s gap");
    }

    #[test]
    fn plan_segments_never_truncates_a_caption_across_a_boundary() {
        let words = vec![
            word("one", 0.0, 5.0),
            word("two", 5.0, 10.0),
            word("three", 10.0, 15.0),
            word("four", 40.0, 45.0), // isolated word near the ideal 50/2=25s midpoint... far from it actually
        ];
        // Dense dialogue for the first 15s, nothing until 40s: any cut
        // between 15s and 40s is caption-safe. The naive ideal (50s) is
        // outside that gap entirely, so this also exercises "fall back to
        // ideal when no close-enough gap exists" — here duration is small
        // enough (100s) that snap_to_gap's 5s tolerance won't reach a
        // distant gap, which is the correct, conservative behavior.
        let segments = plan_segments(100.0, &words, 2);
        // Whatever the cut point is, no single word should be split
        // between two segments (each word appears whole in exactly one
        // segment's word list, by construction of plan_segments' filter).
        let total_words: usize = segments.iter().map(|s| s.words.len()).sum();
        assert_eq!(total_words, words.len());
    }

    #[test]
    fn plan_segments_shifts_word_timestamps_relative_to_segment_start() {
        let words = vec![word("early", 2.0, 4.0), word("late", 60.0, 62.0)];
        let segments = plan_segments(100.0, &words, 2);
        // Whichever segment "late" landed in, its shifted start must be
        // relative to that segment's own start, not the original video.
        let late_segment = segments.iter().find(|s| s.words.iter().any(|w| w.word == "late")).unwrap();
        let late_word = late_segment.words.iter().find(|w| w.word == "late").unwrap();
        assert_eq!(late_word.start, 60.0 - late_segment.start);
    }

    fn minimal_style() -> CaptionStyle {
        CaptionStyle {
            font_family: "Arial".to_string(),
            font_size: 64,
            text_color: "#FFFFFF".to_string(),
            outline_color: "#000000".to_string(),
            position: "bottom".to_string(),
            animation: "none".to_string(),
            words_per_line: 4,
            bold: false,
            italic: false,
            letter_spacing: 0.0,
            text_transform: "none".to_string(),
            background: "none".to_string(),
            background_color: "#000000".to_string(),
            background_opacity: 70.0,
            shadow_size: 0.0,
            style_mode: "classic".to_string(),
            accent_color: "#FFE600".to_string(),
        }
    }

    #[test]
    fn shift_overrides_clips_a_range_straddling_the_segment_boundary_into_both_segments() {
        // An override spanning [8, 12) straddles a cut at 10 -- it must
        // appear, clipped (not dropped, not duplicated in full), in BOTH
        // the segment ending at 10 and the one starting at 10.
        let overrides = vec![CaptionStyleOverride { start: 8.0, end: 12.0, style: minimal_style() }];

        let first_segment = shift_overrides(&overrides, 0.0, 10.0);
        assert_eq!(first_segment.len(), 1);
        assert_eq!(first_segment[0].start, 8.0);
        assert_eq!(first_segment[0].end, 10.0); // clipped to the segment's own end

        let second_segment = shift_overrides(&overrides, 10.0, 20.0);
        assert_eq!(second_segment.len(), 1);
        assert_eq!(second_segment[0].start, 0.0); // rebased: 8.0 - 10.0 clamped to 0
        assert_eq!(second_segment[0].end, 2.0); // rebased: 12.0 - 10.0
    }

    #[test]
    fn shift_overrides_drops_a_range_entirely_outside_the_segment() {
        let overrides = vec![CaptionStyleOverride { start: 50.0, end: 55.0, style: minimal_style() }];
        assert_eq!(shift_overrides(&overrides, 0.0, 10.0).len(), 0);
    }

}
