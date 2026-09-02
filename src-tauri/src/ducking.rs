// Auto background-music ducking — pure ffmpeg, reusing the transcript
// already computed (no new audio-analysis dependency needed). Speech
// windows come directly from the existing WordTimestamp list.

use tauri::AppHandle;

use crate::ffmpeg::{probe_duration_seconds, run_with_progress};
use crate::pipeline::WordTimestamp;

/// Seconds of silence between words short enough to stay inside one
/// continuous "speech window" — merges word-to-word micro-pauses so the
/// music doesn't pop back up and immediately duck again between every
/// word.
const SPEECH_MERGE_GAP_SECONDS: f64 = 0.6;
/// Fade duration (seconds) at each speech window's edges, so ducking
/// doesn't click in/out abruptly.
const DUCK_RAMP_SECONDS: f64 = 0.3;

/// Coalesces consecutive words into merged speech spans — the natural
/// inverse of jumpcuts.rs's silence-gap detection (that finds silence to
/// cut; this finds speech to duck under), built directly rather than by
/// awkwardly inverting `compute_keep_ranges`' output.
pub fn speech_windows_from_words(words: &[WordTimestamp]) -> Vec<(f64, f64)> {
    let mut windows: Vec<(f64, f64)> = Vec::new();
    for w in words {
        if let Some(last) = windows.last_mut() {
            if w.start - last.1 <= SPEECH_MERGE_GAP_SECONDS {
                last.1 = w.end.max(last.1);
                continue;
            }
        }
        windows.push((w.start, w.end));
    }
    windows
}

/// Builds a single ffmpeg-expression-language formula for "music volume
/// factor at time t": 1.0 (full volume) outside every speech window,
/// dipping to `duck_level` in the middle of each one with a
/// `DUCK_RAMP_SECONDS` fade at each edge — a trapezoid, via `min` of two
/// independent edge-distance ramps (0 at each edge, rising to 1 in the
/// middle), the standard way to build a fade-in/fade-out shape in one
/// self-contained expression. Same "one expression covers the whole
/// timeline, `if(between(...))` chain" approach as the (now-deleted)
/// zoom-filter code used for its per-window ramps.
fn build_ducking_volume_expr(windows: &[(f64, f64)], duck_level: f64) -> String {
    if windows.is_empty() {
        return "1".to_string();
    }
    let amount = 1.0 - duck_level;
    let mut expr = "1".to_string();
    for (start, end) in windows {
        let edge = format!(
            "min(min(max((t-{start})/{DUCK_RAMP_SECONDS},0),1),min(max(({end}-t)/{DUCK_RAMP_SECONDS},0),1))"
        );
        let dip = format!("1-{amount}*{edge}");
        expr = format!("if(between(t,{start},{end}),{dip},{expr})");
    }
    expr
}

/// Mixes `music_path` under `video_path`'s existing audio, ducking the
/// music during speech windows. Video is untouched (`-c:v copy` — only
/// the audio filter graph runs).
///
/// The `eval=frame` on `volume` is not optional: ffmpeg's `volume`
/// filter, like `crop` (see the zoom-filter fix earlier in this
/// project's history), only re-evaluates its expression once at init
/// unless told otherwise — a time-varying duck level would otherwise
/// silently freeze at whatever it evaluates to at t=0 and never animate.
pub async fn duck_music_bed(
    app: &AppHandle,
    video_path: &str,
    music_path: &str,
    words: &[WordTimestamp],
    duck_level: f64,
    output_path: &str,
    project_id: &str,
) -> Result<(), String> {
    let windows = speech_windows_from_words(words);
    let expr = build_ducking_volume_expr(&windows, duck_level);
    let filter = format!(
        "[1:a]volume='{expr}':eval=frame[ducked];[0:a][ducked]amix=inputs=2:duration=first:dropout_transition=0:normalize=0[aout]"
    );

    let duration = probe_duration_seconds(video_path).await;
    let args = vec![
        "-y".to_string(),
        "-i".to_string(),
        video_path.to_string(),
        "-i".to_string(),
        music_path.to_string(),
        "-filter_complex".to_string(),
        filter,
        "-map".to_string(),
        "0:v".to_string(),
        "-map".to_string(),
        "[aout]".to_string(),
        "-c:v".to_string(),
        "copy".to_string(),
        "-c:a".to_string(),
        "aac".to_string(),
        output_path.to_string(),
    ];

    run_with_progress(app, "ducking-progress", project_id, "ducking_music", args, duration).await
}

#[tauri::command]
pub async fn duck_music(
    app: AppHandle,
    video_path: String,
    music_path: String,
    words: Vec<WordTimestamp>,
    duck_level: f64,
    output_path: String,
    project_id: String,
) -> Result<String, String> {
    let _permit = crate::concurrency::acquire_encode().await;
    duck_music_bed(&app, &video_path, &music_path, &words, duck_level, &output_path, &project_id).await?;
    Ok(output_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(w: &str, start: f64, end: f64) -> WordTimestamp {
        WordTimestamp { word: w.to_string(), start, end }
    }

    #[test]
    fn speech_windows_from_words_merges_close_words_into_one_window() {
        let words = vec![word("a", 0.0, 0.3), word("b", 0.4, 0.7), word("c", 0.8, 1.1)];
        let windows = speech_windows_from_words(&words);
        assert_eq!(windows, vec![(0.0, 1.1)]);
    }

    #[test]
    fn speech_windows_from_words_splits_on_a_long_gap() {
        let words = vec![word("a", 0.0, 0.3), word("b", 5.0, 5.3)];
        let windows = speech_windows_from_words(&words);
        assert_eq!(windows, vec![(0.0, 0.3), (5.0, 5.3)]);
    }

    #[test]
    fn build_ducking_volume_expr_returns_full_volume_for_no_windows() {
        assert_eq!(build_ducking_volume_expr(&[], 0.25), "1");
    }

    #[test]
    fn build_ducking_volume_expr_includes_eval_ready_between_and_duck_level() {
        let expr = build_ducking_volume_expr(&[(2.0, 8.0)], 0.25);
        assert!(expr.contains("between(t,2,8)"));
        assert!(expr.contains("0.75")); // amount = 1 - duck_level
    }
}
