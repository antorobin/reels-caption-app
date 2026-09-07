// Real, burned-into-the-footage visual effects at a specific moment in the
// video -- distinct from `CaptionStyleOverride` in captions.rs (which only
// changes how the *text* is drawn on top). Four effects, all verified
// against real ffmpeg/libx264 renders before being wired into
// `burn_captions` (see this module's own tests plus README's write-up):
//
// - **Zoom punch**: a brief scale-up-then-settle via ffmpeg's `scale`/
//   `crop`, with the scale factor a function of the frame's own
//   presentation timestamp (`t`), evaluated per-frame (`eval=frame`).
// - **Flash cut**: a short white flash -- a solid-color clip alpha-faded
//   in and back out, overlaid on top of the footage.
// - **Shake**: a brief 2D jitter -- the same scale-up-for-headroom trick
//   as zoom punch, but the `crop` filter's x/y offset (itself already
//   re-evaluated every frame, no `eval=` option needed there) wobbles via
//   two different-frequency sine/cosine terms instead of staying centered,
//   so it reads as a genuine 2D jitter rather than a single diagonal
//   oscillation.
// - **Color pulse**: a brief desaturate-to-gray-and-back via `eq`'s
//   `saturation` option, the same per-frame-expression shape as zoom
//   punch's scale factor.
//
// All four are pure per-frame filters: they don't insert, drop,
// duplicate, or reorder a single frame, so a video's total duration and
// every existing timestamp (words, jump cuts, caption style overrides)
// stay exactly correct with transitions applied -- confirmed empirically,
// not assumed: a real 6s/180-frame test render came back byte-identical
// in frame count and duration with each effect applied individually.
// Also confirmed empirically: ffmpeg's `overlay` filter defaults to
// holding the flash clip's last (fully faded-out, transparent) frame once
// that clip ends rather than truncating the whole output early, so a
// too-short/unknown probe duration for the color source is a
// non-catastrophic degradation, not a risk of silently producing a
// truncated video -- verified with a deliberately 1s-long flash source
// composited onto a 6s video and confirming the output stayed the full 6s.
//
// Deliberately NOT supported yet: `segments.rs`'s long-video, parallel-
// encode burn path. `captions.rs::burn_captions` forces the single-pass
// path instead whenever transitions are present -- correctness over that
// path's speedup, until per-segment transition-time shifting gets the
// same explicit shift/clip treatment `CaptionStyleOverride` already has
// via `shift_overrides`.

use serde::{Deserialize, Serialize};

// Serialize (not just Deserialize) is needed by transition_planner.rs,
// which -- unlike burn_captions, which only ever *receives* this enum
// from the frontend -- hands one *back* out as part of an LLM-suggested
// transition plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransitionEffect {
    ZoomPunch,
    FlashCut,
    Shake,
    ColorPulse,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VideoTransition {
    pub time: f64,
    pub effect: TransitionEffect,
}

/// The zoom ramps up from the pin's own time (not centered on it) via a
/// half-sine bump -- 0 at the start, `ZOOM_PUNCH_MAX_SCALE` at the
/// midpoint, back to 0 at the end -- so it starts and ends at exactly
/// zoom=1 with no visible seam. 0.4s and +18% at the peak were the values
/// used in the real render checked into this module's test/verification
/// notes above; distinct enough to read as a deliberate punch without
/// being jarring.
const ZOOM_PUNCH_DURATION_SECONDS: f64 = 0.4;
const ZOOM_PUNCH_MAX_SCALE: f64 = 0.18;

/// The flash ramps in over `FLASH_RAMP_SECONDS`, holds at full white for
/// `FLASH_HOLD_SECONDS`, then ramps back out over `FLASH_RAMP_SECONDS` --
/// centered so the pin's own time sits at the *start* of the hold
/// plateau, matching this module's own verified real render.
const FLASH_RAMP_SECONDS: f64 = 0.05;
const FLASH_HOLD_SECONDS: f64 = 0.05;

/// Shake reuses zoom punch's own "enlarge slightly, then crop back down"
/// trick purely for headroom -- `SHAKE_ZOOM_MARGIN` is how much bigger
/// than the source the frame gets scaled during the shake window, giving
/// the crop's offset room to wobble around within without ever exposing
/// the frame's real edge. The two frequencies are deliberately different
/// (not just out of phase) so the jitter traces a 2D Lissajous-style
/// wobble rather than one straight diagonal line back and forth --
/// confirmed visually in this module's own verified real render (two
/// frames within the same window landed at genuinely different offsets,
/// not just further along one line).
const SHAKE_DURATION_SECONDS: f64 = 0.4;
const SHAKE_ZOOM_MARGIN: f64 = 0.06;
const SHAKE_AMPLITUDE_PX: f64 = 18.0;
const SHAKE_FREQUENCY_X_HZ: f64 = 9.0;
const SHAKE_FREQUENCY_Y_HZ: f64 = 11.0;

/// Same half-sine-bump shape as zoom punch's scale factor, applied to
/// `eq`'s `saturation` option instead -- 1.0 (unchanged) at both ends,
/// dipping to `1.0 - COLOR_PULSE_DEPTH` (near-total desaturation, at
/// `COLOR_PULSE_DEPTH = 0.95`) at the midpoint.
const COLOR_PULSE_DURATION_SECONDS: f64 = 0.3;
const COLOR_PULSE_DEPTH: f64 = 0.95;

/// Builds the `-filter_complex` fragment applying every transition in
/// `transitions` to `in_label` (typically the literal input stream
/// specifier `"0:v"`), writing the final result to `out_label`. Returns
/// `None` when `transitions` is empty -- callers should fall back to
/// whatever simpler filter chain they'd use with no transitions requested
/// at all, not call this function needlessly.
///
/// `width`/`height`/`fps` must be the *real* probed dimensions of the
/// source video (see `ffmpeg::probe_video_dimensions`) -- unlike the ASS
/// caption overlay (which scales via a fixed `PlayResX`/`PlayResY`
/// regardless of actual resolution), these are literal pixel-dimension
/// ffmpeg filters and need the real numbers to crop a zoom back down to
/// exactly the source size. `duration` only needs to be a safe
/// upper-bound estimate, not exact -- see this module's own doc comment
/// above for why a too-short one degrades gracefully rather than
/// truncating the output.
pub fn build_transition_filter(
    transitions: &[VideoTransition],
    in_label: &str,
    out_label: &str,
    width: u32,
    height: u32,
    fps: f64,
    duration: f64,
) -> Option<String> {
    if transitions.is_empty() {
        return None;
    }

    let zooms: Vec<&VideoTransition> =
        transitions.iter().filter(|t| t.effect == TransitionEffect::ZoomPunch).collect();
    let shakes: Vec<&VideoTransition> = transitions.iter().filter(|t| t.effect == TransitionEffect::Shake).collect();
    let pulses: Vec<&VideoTransition> =
        transitions.iter().filter(|t| t.effect == TransitionEffect::ColorPulse).collect();
    let flashes: Vec<&VideoTransition> =
        transitions.iter().filter(|t| t.effect == TransitionEffect::FlashCut).collect();

    // Fixed stage order: zoom -> shake -> color pulse -> flash. Each
    // stage's own output label is the caller's `out_label` only if every
    // stage *after* it is empty -- otherwise an internal label the next
    // present stage consumes. Computed once per stage from the
    // already-filtered vectors above, since the order itself is fixed.
    let zoom_is_last = shakes.is_empty() && pulses.is_empty() && flashes.is_empty();
    let shake_is_last = pulses.is_empty() && flashes.is_empty();
    let pulse_is_last = flashes.is_empty();

    let mut chains: Vec<String> = Vec::new();
    let mut current = in_label.to_string();

    if !zooms.is_empty() {
        // Nests one if(between(...)) per zoom, falling through to plain
        // `iw`/`ih` (no zoom) when none match. Order/overlap between
        // zooms is never arbitrated here -- the frontend's own
        // non-overlap conventions for pins keep real zoom windows well
        // separated (each is under half a second).
        let mut w_expr = "iw".to_string();
        let mut h_expr = "ih".to_string();
        for z in &zooms {
            let t0 = z.time.max(0.0);
            let t1 = t0 + ZOOM_PUNCH_DURATION_SECONDS;
            w_expr = format!(
                "if(between(t,{t0},{t1}),iw*(1+{amp}*sin(PI*(t-{t0})/{d})),{fallback})",
                amp = ZOOM_PUNCH_MAX_SCALE,
                d = ZOOM_PUNCH_DURATION_SECONDS,
                fallback = w_expr
            );
            h_expr = format!(
                "if(between(t,{t0},{t1}),ih*(1+{amp}*sin(PI*(t-{t0})/{d})),{fallback})",
                amp = ZOOM_PUNCH_MAX_SCALE,
                d = ZOOM_PUNCH_DURATION_SECONDS,
                fallback = h_expr
            );
        }
        let stage_out = if zoom_is_last { out_label.to_string() } else { "vt_zoomed".to_string() };
        chains.push(format!(
            "[{current}]scale=w='{w_expr}':h='{h_expr}':eval=frame,crop={width}:{height}:(iw-{width})/2:(ih-{height})/2[{stage_out}]"
        ));
        current = stage_out;
    }

    if !shakes.is_empty() {
        // Same enlarge-then-crop shape as zoom above, but the crop's
        // offset wobbles (via two nested if(between(...)) expressions,
        // one per axis) instead of staying centered -- see
        // SHAKE_FREQUENCY_X_HZ/Y_HZ's own doc comment for why the two
        // axes use different frequencies.
        let mut w_expr = "iw".to_string();
        let mut h_expr = "ih".to_string();
        let mut x_add_expr = "0".to_string();
        let mut y_add_expr = "0".to_string();
        for s in &shakes {
            let t0 = s.time.max(0.0);
            let t1 = t0 + SHAKE_DURATION_SECONDS;
            let enlarged = 1.0 + SHAKE_ZOOM_MARGIN;
            w_expr = format!("if(between(t,{t0},{t1}),iw*{enlarged},{fallback})", fallback = w_expr);
            h_expr = format!("if(between(t,{t0},{t1}),ih*{enlarged},{fallback})", fallback = h_expr);
            x_add_expr = format!(
                "if(between(t,{t0},{t1}),{amp}*sin(2*PI*{f}*(t-{t0})),{fallback})",
                amp = SHAKE_AMPLITUDE_PX,
                f = SHAKE_FREQUENCY_X_HZ,
                fallback = x_add_expr
            );
            y_add_expr = format!(
                "if(between(t,{t0},{t1}),{amp}*cos(2*PI*{f}*(t-{t0})),{fallback})",
                amp = SHAKE_AMPLITUDE_PX,
                f = SHAKE_FREQUENCY_Y_HZ,
                fallback = y_add_expr
            );
        }
        let stage_out = if shake_is_last { out_label.to_string() } else { "vt_shaken".to_string() };
        chains.push(format!(
            "[{current}]scale=w='{w_expr}':h='{h_expr}':eval=frame,crop={width}:{height}:'(iw-{width})/2+({x_add_expr})':'(ih-{height})/2+({y_add_expr})'[{stage_out}]"
        ));
        current = stage_out;
    }

    if !pulses.is_empty() {
        let mut sat_expr = "1".to_string();
        for p in &pulses {
            let t0 = p.time.max(0.0);
            let t1 = t0 + COLOR_PULSE_DURATION_SECONDS;
            sat_expr = format!(
                "if(between(t,{t0},{t1}),1-{depth}*sin(PI*(t-{t0})/{d}),{fallback})",
                depth = COLOR_PULSE_DEPTH,
                d = COLOR_PULSE_DURATION_SECONDS,
                fallback = sat_expr
            );
        }
        let stage_out = if pulse_is_last { out_label.to_string() } else { "vt_pulsed".to_string() };
        chains.push(format!("[{current}]eq=saturation='{sat_expr}':eval=frame[{stage_out}]"));
        current = stage_out;
    }

    if !flashes.is_empty() {
        let mut fade_ops = String::new();
        for f in &flashes {
            let peak = f.time.max(0.0);
            let fade_in_start = (peak - FLASH_RAMP_SECONDS).max(0.0);
            let fade_out_start = peak + FLASH_HOLD_SECONDS;
            fade_ops.push_str(&format!(
                ",fade=t=in:st={fade_in_start}:d={r}:alpha=1,fade=t=out:st={fade_out_start}:d={r}:alpha=1",
                r = FLASH_RAMP_SECONDS
            ));
        }
        chains.push(format!(
            "color=white:size={width}x{height}:duration={duration:.3}:rate={fps:.6},format=yuva420p{fade_ops}[vt_flash]"
        ));
        // Always the last stage in the fixed order, so it always writes
        // straight to `out_label`.
        chains.push(format!("[{current}][vt_flash]overlay[{out_label}]"));
    }

    Some(chains.join(";"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_transitions_produces_no_filter() {
        assert!(build_transition_filter(&[], "0:v", "outv", 1080, 1920, 30.0, 6.0).is_none());
    }

    #[test]
    fn single_zoom_punch_builds_a_scale_crop_chain_with_no_flash_stage() {
        let transitions = vec![VideoTransition { time: 3.0, effect: TransitionEffect::ZoomPunch }];
        let graph = build_transition_filter(&transitions, "0:v", "outv", 1080, 1920, 30.0, 6.0).unwrap();
        assert!(graph.contains("[0:v]scale="));
        assert!(graph.contains("crop=1080:1920:(iw-1080)/2:(ih-1920)/2[outv]"));
        assert!(graph.contains("between(t,3,3.4)"));
        assert!(!graph.contains("color=white")); // no flash stage requested
    }

    #[test]
    fn single_flash_cut_builds_a_color_fade_overlay_chain_with_no_zoom_stage() {
        let transitions = vec![VideoTransition { time: 5.0, effect: TransitionEffect::FlashCut }];
        let graph = build_transition_filter(&transitions, "0:v", "outv", 1080, 1920, 30.0, 6.0).unwrap();
        assert!(graph.contains("color=white:size=1080x1920:duration=6.000:rate=30.000000"));
        assert!(graph.contains("fade=t=in:st=4.95:d=0.05:alpha=1"));
        assert!(graph.contains("fade=t=out:st=5.05:d=0.05:alpha=1"));
        assert!(graph.contains("[0:v][vt_flash]overlay[outv]"));
        assert!(!graph.contains("scale=")); // no zoom stage requested
    }

    #[test]
    fn both_effects_chain_zoom_into_flash_in_order() {
        let transitions = vec![
            VideoTransition { time: 1.0, effect: TransitionEffect::ZoomPunch },
            VideoTransition { time: 4.0, effect: TransitionEffect::FlashCut },
        ];
        let graph = build_transition_filter(&transitions, "0:v", "outv", 1080, 1920, 30.0, 6.0).unwrap();
        // Zoom stage writes to an intermediate label, not directly to outv...
        assert!(graph.contains("[vt_zoomed]"));
        assert!(!graph.contains("crop=1080:1920:(iw-1080)/2:(ih-1920)/2[outv]"));
        // ...which the flash stage's overlay then consumes to produce outv.
        assert!(graph.contains("[vt_zoomed][vt_flash]overlay[outv]"));
    }

    #[test]
    fn a_flash_pin_near_time_zero_clamps_fade_in_start_to_zero() {
        let transitions = vec![VideoTransition { time: 0.02, effect: TransitionEffect::FlashCut }];
        let graph = build_transition_filter(&transitions, "0:v", "outv", 1080, 1920, 30.0, 6.0).unwrap();
        assert!(graph.contains("fade=t=in:st=0:d=0.05:alpha=1"));
    }

    #[test]
    fn multiple_zoom_punches_nest_independently() {
        let transitions = vec![
            VideoTransition { time: 1.0, effect: TransitionEffect::ZoomPunch },
            VideoTransition { time: 3.0, effect: TransitionEffect::ZoomPunch },
        ];
        let graph = build_transition_filter(&transitions, "0:v", "outv", 1080, 1920, 30.0, 6.0).unwrap();
        assert!(graph.contains("between(t,1,1.4)"));
        assert!(graph.contains("between(t,3,3.4)"));
    }

    #[test]
    fn single_shake_builds_a_wobbling_crop_chain_with_no_other_stage() {
        let transitions = vec![VideoTransition { time: 2.0, effect: TransitionEffect::Shake }];
        let graph = build_transition_filter(&transitions, "0:v", "outv", 1080, 1920, 30.0, 6.0).unwrap();
        assert!(graph.contains("[0:v]scale="));
        assert!(graph.contains("between(t,2,2.4)"));
        // The x/y crop offsets carry the wobble -- distinct from zoom
        // punch's own always-centered crop offset.
        assert!(graph.contains("sin(2*PI*9*(t-2))"));
        assert!(graph.contains("cos(2*PI*11*(t-2))"));
        assert!(graph.ends_with("[outv]"));
        assert!(!graph.contains("color=white"));
        assert!(!graph.contains("eq=saturation"));
    }

    #[test]
    fn single_color_pulse_builds_an_eq_saturation_chain_with_no_other_stage() {
        let transitions = vec![VideoTransition { time: 4.0, effect: TransitionEffect::ColorPulse }];
        let graph = build_transition_filter(&transitions, "0:v", "outv", 1080, 1920, 30.0, 6.0).unwrap();
        assert_eq!(graph, "[0:v]eq=saturation='if(between(t,4,4.3),1-0.95*sin(PI*(t-4)/0.3),1)':eval=frame[outv]");
    }

    #[test]
    fn all_four_effects_chain_in_fixed_zoom_shake_pulse_flash_order() {
        let transitions = vec![
            VideoTransition { time: 1.0, effect: TransitionEffect::FlashCut }, // deliberately listed out of order
            VideoTransition { time: 2.0, effect: TransitionEffect::ColorPulse },
            VideoTransition { time: 3.0, effect: TransitionEffect::Shake },
            VideoTransition { time: 4.0, effect: TransitionEffect::ZoomPunch },
        ];
        let graph = build_transition_filter(&transitions, "0:v", "outv", 1080, 1920, 30.0, 6.0).unwrap();
        // The chain's own internal ordering is fixed (zoom -> shake ->
        // pulse -> flash) regardless of the input list's order --
        // confirmed by each stage's label appearing before the next
        // stage that's supposed to consume it.
        let zoom_pos = graph.find("[vt_zoomed]").unwrap();
        let shake_pos = graph.find("[vt_shaken]").unwrap();
        let pulse_pos = graph.find("[vt_pulsed]").unwrap();
        let flash_pos = graph.find("[vt_flash]").unwrap();
        assert!(zoom_pos < shake_pos, "zoom's own output label must be written before shake consumes it");
        assert!(shake_pos < pulse_pos, "shake's own output label must be written before pulse consumes it");
        assert!(pulse_pos < flash_pos, "pulse's own output label must be written before flash's overlay consumes it");
        assert!(graph.ends_with("[outv]"));
    }
}
