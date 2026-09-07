// Burns styled captions onto a video by generating an ASS (Advanced
// SubStation Alpha) subtitle file from word-level timestamps, then running
// it through ffmpeg's `ass` filter (libass). ASS is used instead of plain
// `drawtext` because its per-word `\k`/`\t` tags and real Style entries
// (background boxes, bold/italic, letter spacing, drop shadow) are what
// make TikTok-style caption presets possible without generating one
// drawtext filter per word.

use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::AppHandle;

use crate::diarize::SpeakerSegment;
use crate::ffmpeg::{best_encoder, probe_duration_seconds, run_with_progress};
use crate::pipeline::WordTimestamp;
use crate::segments::{burn_captions_segmented, recommended_segment_count};
use crate::util::{cli_path, unique_temp_path, verify_transcript_is_fresh};

/// Fixed per-speaker accent-color palette for the cascade active-word
/// pop, used in place of `style.accent_color` when diarization data is
/// available — indexed by `speaker_id % SPEAKER_COLORS.len()`.
const SPEAKER_COLORS: &[&str] = &["#FFE600", "#00E5FF", "#FF4D8D", "#7CFF6B"];

/// Which speaker (if any) is talking at `time`, based on the segment
/// that contains it. `pub(crate)` so transition_planner.rs can reuse it
/// for the same lookup instead of a second copy.
pub(crate) fn speaker_id_at(speakers: &[SpeakerSegment], time: f64) -> Option<u8> {
    speakers.iter().find(|s| time >= s.start && time < s.end).map(|s| s.speaker_id)
}

/// Per-word vocal-emphasis bucket from `analyze_prosody` (audio-only —
/// RMS energy percentiles within this clip, see media_ai/prosody.py).
/// Fresh, minimal type local to this module — not the deleted vision
/// feature's `Intensity`, which was per-segment and description-derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WordIntensity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProsodyWord {
    pub start: f64,
    pub end: f64,
    pub intensity: WordIntensity,
}

/// How close (in seconds) a rendered word's own start time must be to a
/// `ProsodyWord`'s start to count as "the same word" — timestamps can
/// drift slightly between when prosody was analyzed and when captions
/// are burned (e.g. the transcript was hand-edited in between).
const PROSODY_MATCH_TOLERANCE_SECONDS: f64 = 0.05;

/// Nearest-start-time prosody match for a word being rendered at `time`.
/// Defaults to `Medium` (the original fixed-scale behavior) when no
/// prosody data is available or nothing matches closely enough —
/// including whenever `prosody` is empty.
fn prosody_intensity_at(prosody: &[ProsodyWord], time: f64) -> WordIntensity {
    prosody
        .iter()
        .filter(|p| (p.start - time).abs() <= PROSODY_MATCH_TOLERANCE_SECONDS)
        .min_by(|a, b| (a.start - time).abs().partial_cmp(&(b.start - time).abs()).unwrap())
        .map(|p| p.intensity)
        .unwrap_or(WordIntensity::Medium)
}

#[derive(Debug, Clone, Deserialize)]
pub struct CaptionStyle {
    pub font_family: String,
    pub font_size: u32,
    pub text_color: String,    // "#RRGGBB"
    pub outline_color: String, // "#RRGGBB"
    pub position: String,      // "bottom" | "middle" | "top"
    // "none" | "fade" | "pop" | "karaoke" | "bounce" | "typewriter" | "highlight" | "slide" | "zoom"
    // (ignored in cascade mode — its per-word spotlight is always on, see build_cascade_text)
    pub animation: String,
    #[serde(default = "default_words_per_line")]
    pub words_per_line: usize,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub letter_spacing: f32,
    #[serde(default = "default_text_transform")]
    pub text_transform: String, // "none" | "uppercase" | "lowercase" | "capitalize"
    #[serde(default = "default_background")]
    pub background: String, // "none" | "box"
    #[serde(default = "default_background_color")]
    pub background_color: String, // "#RRGGBB"
    #[serde(default = "default_background_opacity")]
    pub background_opacity: f32, // 0-100
    #[serde(default)]
    pub shadow_size: f32,
    // "classic" (one styled line per chunk, existing behavior) | "cascade"
    // (rolling 2-line window: the chunk just spoken shrinks to `text_color`
    // above, the chunk being spoken now is shown large in `accent_color`
    // below — the "big word" TikTok/CapCut preset).
    #[serde(default = "default_style_mode")]
    pub style_mode: String,
    #[serde(default = "default_accent_color")]
    pub accent_color: String, // "#RRGGBB", cascade mode's "current chunk" color
}

/// A different `CaptionStyle` applied to just one time range of the
/// video, layered on top of the whole-video base style -- e.g. a cold
/// open in one theme, the rest in another. Non-overlap between overrides
/// is enforced by the frontend at creation time (never here), so the
/// style-timeline algorithm below never has to arbitrate a tie between
/// two overrides covering the same instant.
#[derive(Debug, Clone, Deserialize)]
pub struct CaptionStyleOverride {
    pub start: f64,
    pub end: f64,
    pub style: CaptionStyle,
}

fn default_words_per_line() -> usize {
    4
}
fn default_style_mode() -> String {
    "classic".to_string()
}
fn default_accent_color() -> String {
    "#FFE600".to_string()
}
fn default_text_transform() -> String {
    "none".to_string()
}
fn default_background() -> String {
    "none".to_string()
}
fn default_background_color() -> String {
    "#000000".to_string()
}
fn default_background_opacity() -> f32 {
    70.0
}

/// "#RRGGBB" -> ASS's "&HAABBGGRR" (blue-green-red order; ASS alpha is
/// inverted from normal "opacity" — 00 = fully opaque, FF = fully
/// transparent).
fn parse_hex_rgb(hex: &str) -> Result<(&str, &str, &str), String> {
    let hex = hex.trim_start_matches('#');
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("Invalid color '{hex}' — expected a 6-digit hex color like #FFFFFF"));
    }
    Ok((&hex[0..2], &hex[2..4], &hex[4..6]))
}

fn hex_to_ass_color_with_opacity(hex: &str, opacity_percent: f32) -> Result<String, String> {
    let (r, g, b) = parse_hex_rgb(hex)?;
    let alpha_byte = ((1.0 - (opacity_percent.clamp(0.0, 100.0) / 100.0)) * 255.0).round() as u8;
    Ok(format!("&H{alpha_byte:02X}{b}{g}{r}").to_uppercase())
}

fn hex_to_ass_color(hex: &str) -> Result<String, String> {
    hex_to_ass_color_with_opacity(hex, 100.0)
}

/// ASS Style-line SecondaryColour for `animation: "karaoke"` — the color a
/// word shows *before* its own `\k` duration elapses; PrimaryColour (the
/// style's own `text_color`) is what it switches to once "sung". A muted
/// gray, opaque (alpha byte 00), matching the same pre-word gray the
/// `"highlight"` animation already uses via its inline `\c&H808080&`
/// override (see `build_animated_text`) — same visual grammar, one fixed
/// constant instead of a duplicated literal.
///
/// Verified directly against real libass output (not assumed from the ASS
/// spec): a throwaway ffmpeg render with deliberately distinct Primary/
/// Secondary colors showed Secondary before a word's `\k` time and Primary
/// after, with the burned frame otherwise byte-identical either side of
/// that boundary. Before this fix, `build_style_line` passed the same
/// value for both fields — Secondary == Primary means no visible sweep at
/// all, only inert `\k` timing tags; confirmed via the same kind of render,
/// reproducing today's code path, byte-identical before vs. after.
const KARAOKE_SECONDARY_COLOUR: &str = "&H00808080";

/// Inline override-tag color (`\c&HBBGGRR&`) — distinct from the Style-line
/// format above, which carries a leading alpha byte and no delimiter.
fn hex_to_ass_override_color(hex: &str) -> Result<String, String> {
    let (r, g, b) = parse_hex_rgb(hex)?;
    Ok(format!("&H{b}{g}{r}&").to_uppercase())
}

fn ass_alignment(position: &str) -> u8 {
    // Numpad-style ASS alignment codes, center column only.
    match position {
        "top" => 8,
        "middle" => 5,
        _ => 2, // bottom
    }
}

fn ass_margin_v(position: &str) -> u32 {
    match position {
        "middle" => 0,
        _ => 60,
    }
}

const PLAY_RES_X: f64 = 1920.0;
const PLAY_RES_Y: f64 = 1080.0;

/// Approximate anchor point for `position`+`margin_v`, used by the "slide"
/// animation's `\move`. `\move` takes full control of a line's position —
/// it doesn't compose with the Style's alignment/margin auto-placement —
/// so this reproduces where that auto-placement would have put it, close
/// enough that the line settles in the same spot "none"/"fade"/etc. do,
/// just arrived at via a slide. `ass_alignment` only ever produces 2/5/8
/// (center-column), so horizontal center (PlayResX/2) is always correct.
fn ass_move_target(position: &str, margin_v: u32) -> (f64, f64) {
    let x = PLAY_RES_X / 2.0;
    let y = match position {
        "top" => margin_v as f64,
        "middle" => PLAY_RES_Y / 2.0,
        _ => PLAY_RES_Y - margin_v as f64, // bottom
    };
    (x, y)
}

fn ass_timestamp(seconds: f64) -> String {
    let total_cs = (seconds.max(0.0) * 100.0).round() as u64;
    let cs = total_cs % 100;
    let total_s = total_cs / 100;
    let s = total_s % 60;
    let total_m = total_s / 60;
    let m = total_m % 60;
    let h = total_m / 60;
    format!("{h}:{m:02}:{s:02}.{cs:02}")
}

fn escape_ass_text(text: &str) -> String {
    text.replace('\\', "\\\\").replace('{', "\\{").replace('}', "\\}")
}

fn apply_text_transform(text: &str, transform: &str) -> String {
    match transform {
        "uppercase" => text.to_uppercase(),
        "lowercase" => text.to_lowercase(),
        "capitalize" => text
            .split(' ')
            .map(|w| {
                let mut chars = w.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
        _ => text.to_string(),
    }
}

/// Builds a single named `[V4+ Styles]` line. Structural properties (font,
/// colors, box vs. outline, bold/italic, letter spacing, shadow) live here
/// rather than as inline override tags, since ASS only allows a handful of
/// things (color, scale, alpha, position) to be overridden per dialogue
/// line — background boxes and bold/italic are Style-only.
#[allow(clippy::too_many_arguments)]
fn build_style_line(
    name: &str,
    font_family: &str,
    font_size: u32,
    text_color: &str,
    outline_color: &str,
    position: &str,
    bold: bool,
    italic: bool,
    letter_spacing: f32,
    background: &str,
    background_color: &str,
    background_opacity: f32,
    shadow_size: f32,
) -> Result<String, String> {
    let primary = hex_to_ass_color(text_color)?;
    let outline = hex_to_ass_color(outline_color)?;
    let alignment = ass_alignment(position);
    let margin_v = ass_margin_v(position);
    let bold_flag = if bold { -1 } else { 0 };
    let italic_flag = if italic { -1 } else { 0 };
    // BorderStyle 3 = opaque box behind the text (the "boxed caption" look
    // most captioning tools default to); 1 = classic outline + shadow.
    let is_box = background == "box";
    let border_style = if is_box { 3 } else { 1 };
    let back_colour =
        if is_box { hex_to_ass_color_with_opacity(background_color, background_opacity)? } else { "&H00000000".to_string() };

    Ok(format!(
        "Style: {name},{font_family},{font_size},{primary},{KARAOKE_SECONDARY_COLOUR},{outline},{back_colour},{bold_flag},{italic_flag},0,0,100,100,{letter_spacing},0,{border_style},3,{shadow_size},{alignment},40,40,{margin_v},1\n"
    ))
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PhraseBreak {
    None,
    Clause,   // , ; :
    Sentence, // . ? !
}

fn classify_punctuation(word: &str) -> Option<PhraseBreak> {
    match word {
        "." | "?" | "!" => Some(PhraseBreak::Sentence),
        "," | ";" | ":" => Some(PhraseBreak::Clause),
        _ => None,
    }
}

/// WhisperX emits sentence/clause punctuation (`.` `,` `?` `!` `;` `:`)
/// as their own word-timestamp entries. Left alone, mechanical N-word
/// chunking treats a lone "." as a word and can orphan it at the start of
/// the next line. This merges each punctuation token onto the word before
/// it (extending that word's end time to cover it) and records what kind
/// of break follows — the signal `group_words_into_phrases` breaks on.
fn merge_punctuation(words: &[WordTimestamp]) -> Vec<(WordTimestamp, PhraseBreak)> {
    let mut merged: Vec<(WordTimestamp, PhraseBreak)> = Vec::new();
    for w in words {
        if let Some(break_kind) = classify_punctuation(&w.word) {
            if let Some(last) = merged.last_mut() {
                last.0.word.push_str(&w.word);
                last.0.end = w.end;
                last.1 = break_kind;
            }
            // Punctuation with nothing preceding it can't attach anywhere — drop it.
            continue;
        }
        merged.push((w.clone(), PhraseBreak::None));
    }
    merged
}

/// Groups words into caption chunks at sentence/clause punctuation instead
/// of a blind word count, so a chunk boundary lands where the sentence
/// actually pauses. `max_words` is still a hard cap — fast, comma-free
/// speech (common in real transcripts) can run many words between any
/// punctuation at all, so this is a "break early if there's a natural
/// place to, otherwise don't run on forever" rule rather than true
/// semantic phrase selection.
fn group_words_into_phrases(words: &[WordTimestamp], max_words: usize) -> Vec<Vec<WordTimestamp>> {
    let max_words = max_words.max(1);
    let mut chunks = Vec::new();
    let mut current: Vec<WordTimestamp> = Vec::new();
    for (word, break_kind) in merge_punctuation(words) {
        current.push(word);
        if break_kind != PhraseBreak::None || current.len() >= max_words {
            chunks.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Renders one caption line's text with `animation` applied to `words`
/// (already text-transformed). `\t`/`\k` timing tags are relative to the
/// dialogue event's own Start, so every offset here is computed relative
/// to `words[0].start`.
fn build_animated_text(words: &[WordTimestamp], animation: &str, text_transform: &str, position: &str) -> String {
    if words.is_empty() {
        return String::new();
    }
    let chunk_start = words[0].start;
    let words: Vec<WordTimestamp> =
        words.iter().map(|w| WordTimestamp { word: apply_text_transform(&w.word, text_transform), ..w.clone() }).collect();
    let joined = || words.iter().map(|w| w.word.as_str()).collect::<Vec<_>>().join(" ");
    let offset_ms = |w: &WordTimestamp| ((w.start - chunk_start) * 1000.0).round().max(0.0) as i64;

    match animation {
        "karaoke" => words
            .iter()
            .map(|w| {
                let duration_cs = ((w.end - w.start).max(0.01) * 100.0).round() as u64;
                format!("{{\\k{duration_cs}}}{} ", escape_ass_text(&w.word))
            })
            .collect::<String>()
            .trim_end()
            .to_string(),

        "pop" => format!("{{\\fscx60\\fscy60\\t(0,120,\\fscx100\\fscy100)}}{}", escape_ass_text(&joined())),

        "fade" => format!("{{\\fad(150,150)}}{}", escape_ass_text(&joined())),

        // Whole line overshoots past full size then settles — a spring/bounce feel.
        "bounce" => format!(
            "{{\\fscx60\\fscy60\\t(0,120,\\fscx115\\fscy115)\\t(120,220,\\fscx100\\fscy100)}}{}",
            escape_ass_text(&joined())
        ),

        // Slides in from just off its resting position (direction depends
        // on `position` — down into a top caption, up into a bottom one)
        // with a quick fade, rather than popping in place.
        "slide" => {
            let margin_v = ass_margin_v(position);
            let (x, y) = ass_move_target(position, margin_v);
            let from_y = if position == "top" { y - 60.0 } else { y + 60.0 };
            format!(
                "{{\\move({x},{from_y},{x},{y})\\alpha&HFF&\\t(0,220,\\alpha&H00&)}}{}",
                escape_ass_text(&joined())
            )
        }

        // Opposite of "pop": starts oversized and slightly transparent,
        // settles down to full size and opacity — a camera-zoom-out feel.
        "zoom" => format!(
            "{{\\fscx160\\fscy160\\alpha&H80&\\t(0,180,\\fscx100\\fscy100\\alpha&H00&)}}{}",
            escape_ass_text(&joined())
        ),

        // Each word pops from a dim gray to its real color right as it's
        // "spoken" — a discrete per-word emphasis, distinct from karaoke's
        // continuous fill-sweep.
        "highlight" => words
            .iter()
            .map(|w| {
                let start_ms = offset_ms(w);
                format!("{{\\c&H808080&\\t({start_ms},{},\\c)}}{} ", start_ms + 150, escape_ass_text(&w.word))
            })
            .collect::<String>()
            .trim_end()
            .to_string(),

        // Characters appear one at a time across each word's own span —
        // classic typewriter reveal.
        "typewriter" => {
            let mut out = String::new();
            for w in &words {
                let chars: Vec<char> = w.word.chars().collect();
                if chars.is_empty() {
                    continue;
                }
                let word_span_ms = ((w.end - w.start).max(0.01) * 1000.0) as i64;
                let step_ms = (word_span_ms / chars.len() as i64).max(1);
                let word_offset_ms = offset_ms(w);
                for (i, c) in chars.iter().enumerate() {
                    let reveal_at = word_offset_ms + step_ms * i as i64;
                    out.push_str(&format!(
                        "{{\\alpha&HFF&\\t({reveal_at},{},\\alpha&H00&)}}{}",
                        reveal_at + 60,
                        escape_ass_text(&c.to_string())
                    ));
                }
                out.push(' ');
            }
            out.trim_end().to_string()
        }

        _ => escape_ass_text(&joined()),
    }
}

/// The "previous phrase" line in cascade mode shrinks to this fraction of
/// `font_size` — tuned by eye against the reference "big word" preset.
const CASCADE_PREV_SCALE: f32 = 0.6;
/// How much bigger than `font_size` the word currently being spoken pops to.
const CASCADE_ACTIVE_SCALE: f32 = 1.15;
/// Ramp speed (ms) for the per-word *size* pop only — kept short enough to
/// read as a snap, not a slow grow. Color is a separate, effectively
/// instant transition (see below): a highlight that visibly fades in is
/// the thing that reads as "out of sync with the voice," even though the
/// underlying word timestamp is correct — the size pop is just polish and
/// doesn't carry that same sync expectation.
const CASCADE_SCALE_POP_MS: i64 = 60;

/// Per-intensity active-word pop scale. Empty/no-match prosody data
/// means every word reads as `Medium`, which maps to the same value
/// `CASCADE_ACTIVE_SCALE` always was — a burn with no prosody data
/// behaves exactly as before.
fn active_scale_for_intensity(intensity: WordIntensity) -> f32 {
    match intensity {
        WordIntensity::Low => 1.05,
        WordIntensity::Medium => CASCADE_ACTIVE_SCALE,
        WordIntensity::High => 1.35,
    }
}

/// Renders one cascade-mode dialogue: the just-finished phrase (if any)
/// shrinks to `text_color` on its own line, followed by the phrase
/// currently being spoken — shown at full size in `text_color`, except the
/// exact word being spoken *right now* pops to `accent_color` and
/// slightly larger, then reverts once it's done. Color switches instantly
/// at the word's own start/end (a 1ms `\t` window — libass has no true
/// zero-duration transform); only the size pop eases. A moving per-word
/// spotlight, not a whole-phrase color swap — `style.animation` is
/// intentionally ignored here, since this per-word pop *is* the
/// animation.
fn build_cascade_text(
    prev: Option<&[WordTimestamp]>,
    current: &[WordTimestamp],
    style: &CaptionStyle,
    prosody: &[ProsodyWord],
    speakers: &[SpeakerSegment],
) -> Result<String, String> {
    let prev_color = hex_to_ass_override_color(&style.text_color)?;
    let accent_color = hex_to_ass_override_color(&style.accent_color)?;
    // Precomputed once (fallible parsing needs `?`, which the per-word
    // closure below can't use) — empty when there's no diarization data,
    // so every word falls back to the style's own accent_color unchanged.
    let speaker_colors: Vec<String> =
        SPEAKER_COLORS.iter().map(|c| hex_to_ass_override_color(c)).collect::<Result<_, _>>()?;
    let prev_size = (style.font_size as f32 * CASCADE_PREV_SCALE).round().max(1.0) as u32;
    let base_size = style.font_size;

    let mut out = String::new();
    if let Some(prev_words) = prev {
        if !prev_words.is_empty() {
            let prev_text: Vec<String> = prev_words.iter().map(|w| apply_text_transform(&w.word, &style.text_transform)).collect();
            out.push_str(&format!("{{\\fs{prev_size}\\c{prev_color}}}{}\\N", escape_ass_text(&prev_text.join(" "))));
        }
    }

    let chunk_start = current.first().map(|w| w.start).unwrap_or(0.0);
    let current_text: String = current
        .iter()
        .map(|w| {
            let text = apply_text_transform(&w.word, &style.text_transform);
            let start_ms = ((w.start - chunk_start) * 1000.0).round().max(0.0) as i64;
            let end_ms = (((w.end - chunk_start) * 1000.0).round().max(0.0) as i64).max(start_ms + 1);
            let active_scale = active_scale_for_intensity(prosody_intensity_at(prosody, w.start));
            let active_size = (style.font_size as f32 * active_scale).round().max(1.0) as u32;
            let word_accent_color = speaker_id_at(speakers, w.start)
                .map(|id| speaker_colors[id as usize % speaker_colors.len()].as_str())
                .unwrap_or(&accent_color);
            format!(
                "{{\\fs{base_size}\\c{prev_color}\\t({start_ms},{},\\c{word_accent_color})\\t({start_ms},{},\\fs{active_size})\\t({end_ms},{},\\c{prev_color})\\t({end_ms},{},\\fs{base_size})}}{} ",
                start_ms + 1,
                start_ms + CASCADE_SCALE_POP_MS,
                end_ms + 1,
                end_ms + CASCADE_SCALE_POP_MS,
                escape_ass_text(&text)
            )
        })
        .collect::<String>()
        .trim_end()
        .to_string();
    out.push_str(&current_text);
    Ok(out)
}

/// One slice of the video's timeline that renders with one particular
/// style -- the base style fills every gap around/between the (already
/// sorted, non-overlapping) `overrides`. Together the returned pieces
/// exactly partition `[0, +infinity)` with no gaps or overlaps; the open
/// upper bound means the *last* piece always covers "the rest of the
/// video" without needing to know its actual duration here.
struct StylePiece<'a> {
    start: f64,
    end: f64,
    style: &'a CaptionStyle,
    name: String,
}

fn build_style_timeline<'a>(sorted_overrides: &'a [CaptionStyleOverride], base: &'a CaptionStyle) -> Vec<StylePiece<'a>> {
    let mut pieces = Vec::new();
    let mut cursor = 0.0_f64;
    for (i, ov) in sorted_overrides.iter().enumerate() {
        if ov.start > cursor {
            pieces.push(StylePiece { start: cursor, end: ov.start, style: base, name: "Default".to_string() });
        }
        pieces.push(StylePiece { start: ov.start, end: ov.end, style: &ov.style, name: format!("Override{i}") });
        cursor = cursor.max(ov.end);
    }
    pieces.push(StylePiece { start: cursor, end: f64::INFINITY, style: base, name: "Default".to_string() });
    pieces.retain(|p| p.end > p.start);
    pieces
}

/// One caption chunk, tagged with which `StylePiece` (by index into the
/// timeline `build_style_timeline` returned) it was chunked under --
/// `piece_index` is what lets cascade mode's "previous phrase" line know
/// whether the prior chunk actually belongs to the *same* style piece
/// (see `build_ass_document` below) rather than stitching the tail of one
/// override onto the head of the next.
struct ChunkPiece<'a> {
    piece_index: usize,
    chunk: Vec<WordTimestamp>,
    style: &'a CaptionStyle,
    style_name: String,
}

/// Chunks each `StylePiece`'s own word slice independently, with *that
/// piece's own* `words_per_line` -- an override with a different
/// `words_per_line` (or a different `style_mode` entirely) than the base
/// genuinely changes how its portion of the transcript breaks into
/// caption chunks, not just their color/font. A word belongs to whichever
/// piece its own *start* time falls in (never split across two pieces),
/// matching the same half-open-interval convention `plan_segments`/
/// `shift_speakers` already use elsewhere in this codebase.
fn build_chunk_timeline<'a>(words: &[WordTimestamp], pieces: &'a [StylePiece<'a>]) -> Vec<ChunkPiece<'a>> {
    let mut out = Vec::new();
    for (piece_index, piece) in pieces.iter().enumerate() {
        let piece_words: Vec<WordTimestamp> =
            words.iter().filter(|w| w.start >= piece.start && w.start < piece.end).cloned().collect();
        if piece_words.is_empty() {
            continue;
        }
        for chunk in group_words_into_phrases(&piece_words, piece.style.words_per_line) {
            out.push(ChunkPiece { piece_index, chunk, style: piece.style, style_name: piece.name.clone() });
        }
    }
    out
}

pub(crate) fn build_ass_document(
    words: &[WordTimestamp],
    style: &CaptionStyle,
    prosody: &[ProsodyWord],
    speakers: &[SpeakerSegment],
    overrides: &[CaptionStyleOverride],
) -> Result<String, String> {
    let mut doc = String::new();
    doc.push_str("[Script Info]\n");
    doc.push_str("ScriptType: v4.00+\n");
    doc.push_str(&format!("PlayResX: {}\n", PLAY_RES_X as i64));
    doc.push_str(&format!("PlayResY: {}\n", PLAY_RES_Y as i64));
    doc.push_str("WrapStyle: 0\n");
    doc.push_str("ScaledBorderAndShadow: yes\n\n");

    doc.push_str("[V4+ Styles]\n");
    doc.push_str(
        "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n",
    );
    doc.push_str(&build_style_line(
        "Default",
        &style.font_family,
        style.font_size,
        &style.text_color,
        &style.outline_color,
        &style.position,
        style.bold,
        style.italic,
        style.letter_spacing,
        &style.background,
        &style.background_color,
        style.background_opacity,
        style.shadow_size,
    )?);

    let mut sorted_overrides = overrides.to_vec();
    sorted_overrides.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap());
    for (i, ov) in sorted_overrides.iter().enumerate() {
        doc.push_str(&build_style_line(
            &format!("Override{i}"),
            &ov.style.font_family,
            ov.style.font_size,
            &ov.style.text_color,
            &ov.style.outline_color,
            &ov.style.position,
            ov.style.bold,
            ov.style.italic,
            ov.style.letter_spacing,
            &ov.style.background,
            &ov.style.background_color,
            ov.style.background_opacity,
            ov.style.shadow_size,
        )?);
    }
    doc.push('\n');

    doc.push_str("[Events]\n");
    doc.push_str("Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n");

    // Every override's own Style line is emitted unconditionally above,
    // even if (per the filtering below) it ends up covering zero words --
    // an unreferenced named Style is harmless to libass, not worth the
    // extra bookkeeping to suppress.
    let pieces = build_style_timeline(&sorted_overrides, style);
    let chunk_pieces = build_chunk_timeline(words, &pieces);
    for (i, cp) in chunk_pieces.iter().enumerate() {
        let (Some(first), Some(last)) = (cp.chunk.first(), cp.chunk.last()) else {
            continue;
        };
        let start = ass_timestamp(first.start);
        let end = ass_timestamp(last.end);
        let text = if cp.style.style_mode == "cascade" {
            // "prev" only reaches back within this same style piece --
            // never stitches the tail of one override onto the head of
            // the next (or the base style's own head), since
            // build_cascade_text renders `prev` using `cp.style`, which
            // would misrender a `prev` chunk that actually belongs to a
            // different piece.
            let prev = if i > 0 && chunk_pieces[i - 1].piece_index == cp.piece_index {
                Some(chunk_pieces[i - 1].chunk.as_slice())
            } else {
                None
            };
            build_cascade_text(prev, &cp.chunk, cp.style, prosody, speakers)?
        } else {
            build_animated_text(&cp.chunk, &cp.style.animation, &cp.style.text_transform, &cp.style.position)
        };
        let style_name = &cp.style_name;
        doc.push_str(&format!("Dialogue: 0,{start},{end},{style_name},,0,0,0,,{text}\n"));
    }

    Ok(doc)
}

/// ffmpeg's filtergraph parser treats `:` and `\` specially, and on Windows
/// a drive-letter path is full of both (`C:\Users\...`). Normalize to
/// forward slashes, then escape the remaining colon.
pub(crate) fn escape_ffmpeg_filter_path(path: &Path) -> String {
    let normalized = cli_path(path).replace('\\', "/");
    normalized.replace(':', "\\:")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(w: &str, start: f64, end: f64) -> WordTimestamp {
        WordTimestamp { word: w.to_string(), start, end }
    }

    fn style(animation: &str) -> CaptionStyle {
        CaptionStyle {
            font_family: "Arial".to_string(),
            font_size: 64,
            text_color: "#FFFFFF".to_string(),
            outline_color: "#000000".to_string(),
            position: "bottom".to_string(),
            animation: animation.to_string(),
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
    fn hex_to_ass_color_swaps_to_bgr() {
        assert_eq!(hex_to_ass_color("#FF8800").unwrap(), "&H000088FF");
        assert_eq!(hex_to_ass_color("00ff00").unwrap(), "&H0000FF00");
        assert_eq!(hex_to_ass_color("#000000").unwrap(), "&H00000000");
    }

    #[test]
    fn hex_to_ass_color_rejects_bad_input() {
        assert!(hex_to_ass_color("not-a-color").is_err());
        assert!(hex_to_ass_color("#FFF").is_err());
    }

    #[test]
    fn hex_to_ass_color_with_opacity_inverts_percent_to_ass_alpha() {
        // 100% opaque -> alpha byte 00; 0% opaque (fully transparent) -> FF.
        assert_eq!(hex_to_ass_color_with_opacity("#000000", 100.0).unwrap(), "&H00000000");
        assert_eq!(hex_to_ass_color_with_opacity("#000000", 0.0).unwrap(), "&HFF000000");
        // 70% opacity -> alpha byte round((1-0.7)*255) = 77 = 0x4D.
        assert_eq!(hex_to_ass_color_with_opacity("#000000", 70.0).unwrap(), "&H4D000000");
    }

    #[test]
    fn ass_timestamp_formats_hmscs() {
        assert_eq!(ass_timestamp(0.0), "0:00:00.00");
        assert_eq!(ass_timestamp(1.02), "0:00:01.02"); // rounds to nearest centisecond
        assert_eq!(ass_timestamp(61.5), "0:01:01.50");
        assert_eq!(ass_timestamp(3661.25), "1:01:01.25");
        assert_eq!(ass_timestamp(-5.0), "0:00:00.00"); // clamped, never negative
    }

    #[test]
    fn escape_ffmpeg_filter_path_handles_windows_drive_paths() {
        let path = Path::new(r"C:\Users\me\captions.ass");
        assert_eq!(escape_ffmpeg_filter_path(path), "C\\:/Users/me/captions.ass");
    }

    #[test]
    fn apply_text_transform_variants() {
        assert_eq!(apply_text_transform("Hello World", "uppercase"), "HELLO WORLD");
        assert_eq!(apply_text_transform("Hello World", "lowercase"), "hello world");
        assert_eq!(apply_text_transform("hello world", "capitalize"), "Hello World");
        assert_eq!(apply_text_transform("Hello World", "none"), "Hello World");
    }

    #[test]
    fn build_animated_text_karaoke_includes_k_tags_for_every_word() {
        let words = vec![word("hello", 0.0, 0.5), word("world", 0.5, 1.2)];
        let text = build_animated_text(&words, "karaoke", "none", "bottom");
        assert!(text.contains("{\\k50}hello"));
        assert!(text.contains("{\\k70}world"));
    }

    #[test]
    fn build_animated_text_none_is_plain_joined_words() {
        let words = vec![word("hello", 0.0, 0.5), word("world", 0.5, 1.2)];
        assert_eq!(build_animated_text(&words, "none", "none", "bottom"), "hello world");
    }

    #[test]
    fn build_animated_text_applies_transform_before_animating() {
        let words = vec![word("hello", 0.0, 0.5)];
        assert_eq!(build_animated_text(&words, "none", "uppercase", "bottom"), "HELLO");
    }

    #[test]
    fn build_animated_text_highlight_transitions_each_word_at_its_own_offset() {
        let words = vec![word("hello", 0.0, 0.5), word("world", 0.5, 1.2)];
        let text = build_animated_text(&words, "highlight", "none", "bottom");
        assert!(text.contains("\\t(0,150,\\c)}hello"));
        assert!(text.contains("\\t(500,650,\\c)}world")); // "world" starts 500ms into the chunk
    }

    #[test]
    fn build_animated_text_typewriter_reveals_each_character() {
        let words = vec![word("hi", 0.0, 0.5)];
        let text = build_animated_text(&words, "typewriter", "none", "bottom");
        assert!(text.contains("\\alpha&HFF&"));
        assert!(text.contains('h'));
        assert!(text.contains('i'));
    }

    #[test]
    fn build_animated_text_slide_moves_toward_the_resting_position() {
        let words = vec![word("hi", 0.0, 0.5)];
        let bottom = build_animated_text(&words, "slide", "none", "bottom");
        // Bottom: PlayResX/2=960, resting y = 1080-60=1020, slides up from 1020+60=1080.
        assert!(bottom.contains("\\move(960,1080,960,1020)"));

        let top = build_animated_text(&words, "slide", "none", "top");
        // Top: resting y = margin_v(60), slides down from 60-60=0.
        assert!(top.contains("\\move(960,0,960,60)"));
    }

    #[test]
    fn build_animated_text_zoom_starts_oversized_and_settles_to_full_size() {
        let words = vec![word("hi", 0.0, 0.5)];
        let text = build_animated_text(&words, "zoom", "none", "bottom");
        assert!(text.contains("\\fscx160\\fscy160"));
        assert!(text.contains("\\t(0,180,\\fscx100\\fscy100"));
    }

    #[test]
    fn build_style_line_uses_box_border_style_when_background_is_box() {
        let line = build_style_line("Default", "Arial", 64, "#FFFFFF", "#000000", "bottom", true, false, 2.0, "box", "#FF0000", 50.0, 3.0)
            .unwrap();
        // BorderStyle field (3 = box) and Bold flag (-1 = true) should show up positionally.
        assert!(line.contains(",-1,0,0,0,100,100,2,0,3,3,3,2,40,40,60,1"));
        // BackColour should be the box color at 50% opacity: alpha byte round((1-0.5)*255)=128=0x80.
        assert!(line.contains("&H800000FF"));
    }

    #[test]
    fn build_style_line_uses_outline_border_style_when_background_is_none() {
        let line = build_style_line("Default", "Arial", 64, "#FFFFFF", "#000000", "bottom", false, false, 0.0, "none", "#FF0000", 50.0, 0.0)
            .unwrap();
        assert!(line.contains(",0,0,0,0,100,100,0,0,1,3,0,2,40,40,60,1"));
    }

    #[test]
    fn build_style_line_secondary_colour_differs_from_primary_so_karaoke_is_visible() {
        // Regression test for a real bug: SecondaryColour used to equal
        // PrimaryColour, so `\k` karaoke tags carried timing but produced
        // no visible color sweep at all (confirmed via a real libass
        // render — see KARAOKE_SECONDARY_COLOUR's doc comment). Style-line
        // colour fields are positional: Name,Font,Size,Primary,Secondary,...
        let line = build_style_line("Default", "Arial", 64, "#FFFFFF", "#000000", "bottom", false, false, 0.0, "none", "#FF0000", 50.0, 0.0)
            .unwrap();
        let fields: Vec<&str> = line.trim_end().trim_start_matches("Style: ").split(',').collect();
        let (primary, secondary) = (fields[3], fields[4]);
        assert_ne!(primary, secondary);
        assert_eq!(secondary, KARAOKE_SECONDARY_COLOUR);
    }

    #[test]
    fn merge_punctuation_attaches_sentence_end_to_preceding_word() {
        let words = vec![word("hi", 0.0, 0.5), word(".", 0.5, 0.5), word("bye", 0.6, 1.0)];
        let merged = merge_punctuation(&words);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].0.word, "hi.");
        assert_eq!(merged[0].0.end, 0.5); // extended to cover the punctuation token
        assert_eq!(merged[0].1, PhraseBreak::Sentence);
        assert_eq!(merged[1].0.word, "bye");
        assert_eq!(merged[1].1, PhraseBreak::None);
    }

    #[test]
    fn merge_punctuation_classifies_clause_vs_sentence() {
        let words = vec![word("a", 0.0, 0.1), word(",", 0.1, 0.1), word("b", 0.2, 0.3), word("!", 0.3, 0.3)];
        let merged = merge_punctuation(&words);
        assert_eq!(merged[0].1, PhraseBreak::Clause);
        assert_eq!(merged[1].1, PhraseBreak::Sentence);
    }

    #[test]
    fn group_words_into_phrases_breaks_at_sentence_end_before_hitting_the_cap() {
        // "hi." ends a sentence after just 1 word — should not wait for max_words=4.
        let words = vec![word("hi", 0.0, 0.5), word(".", 0.5, 0.5), word("bye", 0.6, 1.0), word("there", 1.0, 1.4)];
        let chunks = group_words_into_phrases(&words, 4);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 1);
        assert_eq!(chunks[0][0].word, "hi.");
        assert_eq!(chunks[1].len(), 2);
    }

    #[test]
    fn group_words_into_phrases_falls_back_to_max_words_cap_when_no_punctuation() {
        let words = vec![word("a", 0.0, 0.1), word("b", 0.1, 0.2), word("c", 0.2, 0.3), word("d", 0.3, 0.4), word("e", 0.4, 0.5)];
        let chunks = group_words_into_phrases(&words, 4);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 4);
        assert_eq!(chunks[1].len(), 1);
    }

    #[test]
    fn build_ass_document_emits_one_dialogue_line_per_chunk() {
        let words = vec![
            word("one", 0.0, 0.2),
            word("two", 0.2, 0.4),
            word("three", 0.4, 0.6),
            word("four", 0.6, 0.8),
            word("five", 0.8, 1.0),
        ];
        let mut s = style("none");
        s.words_per_line = 4;
        let doc = build_ass_document(&words, &s, &[], &[], &[]).unwrap();
        let dialogue_lines: Vec<&str> = doc.lines().filter(|l| l.starts_with("Dialogue:")).collect();
        assert_eq!(dialogue_lines.len(), 2); // 4 words + 1 word = 2 chunks
        assert!(dialogue_lines[0].contains("one two three four"));
        assert!(dialogue_lines[1].contains("five"));
    }

    #[test]
    fn build_cascade_text_omits_prev_line_when_none() {
        let current = vec![word("sometimes", 0.0, 0.5)];
        let s = style("none");
        let text = build_cascade_text(None, &current, &s, &[], &[]).unwrap();
        assert!(!text.contains("\\N"));
        assert!(text.contains(&format!("\\fs{}", s.font_size)));
        assert!(text.contains("sometimes"));
    }

    #[test]
    fn build_cascade_text_renders_prev_small_and_pops_the_active_word() {
        let prev = vec![word("at", 0.0, 0.2), word("night", 0.2, 0.4)];
        let current = vec![word("sometimes", 0.4, 0.9)];
        let mut s = style("none");
        s.font_size = 64;
        s.text_color = "#FFFFFF".to_string();
        s.accent_color = "#FFE600".to_string();
        let text = build_cascade_text(Some(&prev), &current, &s, &[], &[]).unwrap();

        // Prev line: smaller font, base text color, ends with a line break.
        assert!(text.contains(&format!("\\fs{}", (64.0_f32 * CASCADE_PREV_SCALE).round() as u32)));
        assert!(text.contains("at night\\N"));

        // The current word starts at base size/color, pops to a bigger
        // accent-colored size while it's being spoken, then reverts — it's
        // never statically colored for the whole phrase. Color switches
        // instantly (1ms window) right at the word's own start/end, so the
        // highlight never visibly lags the spoken word; only the size pop
        // eases.
        let active_size = (64.0_f32 * CASCADE_ACTIVE_SCALE).round() as u32;
        assert!(text.contains("\\fs64\\c&HFFFFFF&\\t(0,1,\\c&H00E6FF&)"));
        assert!(text.contains(&format!("\\t(0,{CASCADE_SCALE_POP_MS},\\fs{active_size})")));
        assert!(text.contains("\\t(500,501,\\c&HFFFFFF&)"));
        assert!(text.contains(&format!("\\t(500,{},\\fs64)", 500 + CASCADE_SCALE_POP_MS)));
        assert!(text.contains("sometimes"));
    }

    #[test]
    fn build_cascade_text_pops_each_word_independently() {
        let current = vec![word("couple", 0.0, 0.3), word("reasons", 0.3, 0.8)];
        let s = style("none");
        let text = build_cascade_text(None, &current, &s, &[], &[]).unwrap();
        // Each word gets its own [start,end] instant-color window — not
        // one shared color block for the whole phrase.
        assert!(text.contains("\\t(0,1,"));
        assert!(text.contains("\\t(300,301,"));
        assert!(text.contains("couple"));
        assert!(text.contains("reasons"));
    }

    #[test]
    fn build_ass_document_cascade_mode_produces_two_line_dialogue_from_second_chunk_on() {
        let words = vec![
            word("at", 0.0, 0.2),
            word("night", 0.2, 0.4),
            word("sometimes", 0.4, 0.9),
        ];
        let mut s = style("none");
        s.style_mode = "cascade".to_string();
        s.words_per_line = 2;
        let doc = build_ass_document(&words, &s, &[], &[], &[]).unwrap();
        let dialogue_lines: Vec<&str> = doc.lines().filter(|l| l.starts_with("Dialogue:")).collect();
        assert_eq!(dialogue_lines.len(), 2);
        assert!(!dialogue_lines[0].contains("\\N")); // first chunk: no previous line yet
        assert!(dialogue_lines[1].contains("at night\\N")); // second chunk: first chunk trails above
        assert!(dialogue_lines[1].contains("sometimes"));
    }

    #[test]
    fn build_ass_document_rejects_invalid_color() {
        let mut s = style("none");
        s.text_color = "bogus".to_string();
        assert!(build_ass_document(&[word("hi", 0.0, 0.1)], &s, &[], &[], &[]).is_err());
    }

    #[test]
    fn build_ass_document_with_zero_overrides_is_byte_identical_to_no_overrides_param() {
        // Regression guarantee for the overrides feature: an empty
        // overrides list must produce exactly what this function always
        // produced before overrides existed — one "Default" style, one
        // global chunking pass.
        let words = vec![word("one", 0.0, 0.2), word("two", 0.2, 0.4), word("three", 0.4, 0.6)];
        let s = style("none");
        let with_empty_overrides = build_ass_document(&words, &s, &[], &[], &[]).unwrap();
        let dialogue_lines: Vec<&str> = with_empty_overrides.lines().filter(|l| l.starts_with("Dialogue:")).collect();
        assert_eq!(dialogue_lines.len(), 1);
        assert!(dialogue_lines[0].starts_with("Dialogue: 0,0:00:00.00,0:00:00.60,Default,"));
        // Exactly one Style line -- no stray "OverrideN" line when there
        // are no overrides to emit one for.
        assert_eq!(with_empty_overrides.lines().filter(|l| l.starts_with("Style:")).count(), 1);
    }

    #[test]
    fn build_ass_document_override_gets_its_own_style_and_chunking() {
        // Base style: 4 words/line. Override (covering "three four"):
        // a distinct style with 1 word/line -- proves both the separate
        // Style line AND the range-aware re-chunking (an override with a
        // different words_per_line genuinely changes how its portion
        // breaks into chunks, not just its color/font).
        let words = vec![
            word("one", 0.0, 0.2),
            word("two", 0.2, 0.4),
            word("three", 0.4, 0.6),
            word("four", 0.6, 0.8),
            word("five", 0.8, 1.0),
        ];
        let base = style("none");
        let mut override_style = style("none");
        override_style.words_per_line = 1;
        override_style.text_color = "#FF0000".to_string();
        let overrides = vec![CaptionStyleOverride { start: 0.4, end: 0.8, style: override_style }];

        let doc = build_ass_document(&words, &base, &[], &[], &overrides).unwrap();

        // Two Style lines: Default (base) + Override0.
        let style_lines: Vec<&str> = doc.lines().filter(|l| l.starts_with("Style:")).collect();
        assert_eq!(style_lines.len(), 2);
        assert!(style_lines.iter().any(|l| l.starts_with("Style: Default,")));
        // Override0's PrimaryColour is #FF0000 as an ASS Style-line colour:
        // 8-hex &HAABBGGRR (leading alpha byte 00), same form every other
        // Style line here uses -- not the alpha-less inline `\c` form.
        assert!(style_lines.iter().any(|l| l.starts_with("Style: Override0,") && l.contains("&H000000FF")));

        // "one two" (base, 4/line but only 2 words before the override
        // starts) then "three" and "four" as their own 1-word chunks
        // (override's words_per_line=1), then "five" back on Default.
        let dialogue_lines: Vec<&str> = doc.lines().filter(|l| l.starts_with("Dialogue:")).collect();
        assert_eq!(dialogue_lines.len(), 4);
        assert!(dialogue_lines[0].contains(",Default,") && dialogue_lines[0].contains("one two"));
        assert!(dialogue_lines[1].contains(",Override0,") && dialogue_lines[1].contains("three"));
        assert!(dialogue_lines[2].contains(",Override0,") && dialogue_lines[2].contains("four"));
        assert!(dialogue_lines[3].contains(",Default,") && dialogue_lines[3].contains("five"));
    }
}

#[tauri::command]
pub async fn burn_captions(
    app: AppHandle,
    video_path: String,
    words: Vec<WordTimestamp>,
    style: CaptionStyle,
    output_path: String,
    prosody: Vec<ProsodyWord>,
    speakers: Vec<SpeakerSegment>,
    overrides: Vec<CaptionStyleOverride>,
    video_transitions: Vec<crate::video_transitions::VideoTransition>,
    voiceover_path: Option<String>,
    voiceover_offset_seconds: Option<f64>,
    project_id: String,
) -> Result<String, String> {
    if words.is_empty() {
        return Err("Nothing to burn — run the transcription pipeline first.".to_string());
    }

    // Disk-persisted check (not just frontend state) — holds even if this
    // call comes from a different session/process than the one that
    // transcribed video_path. See record_fresh_transcript's doc comment.
    // Still valid with a voiceover active: this checks video_path's own
    // on-disk fingerprint, not which words are being burned.
    verify_transcript_is_fresh(&app, &video_path)?;

    // Held for the whole burn, however many segments it ends up using --
    // acquired once around this *outer* call, not per segment, since
    // segments.rs already parallelizes internally within one burn (each
    // segment competing separately for the same 2 encode permits would
    // make one burn contend with itself). Gates the hardware encoder's
    // driver-enforced concurrent-session cap (see concurrency.rs), not
    // CPU/RAM the way the heavy-ML gate does.
    let _permit = crate::concurrency::acquire_encode().await;

    let duration = probe_duration_seconds(&video_path).await;
    let segment_count = duration.map(recommended_segment_count).unwrap_or(1);

    // Long enough to be worth the segmented-parallel-encode complexity?
    // (See segments.rs for why: cut points snapped to transcript gaps,
    // per-segment audio re-encode, right-sized thread division, fast
    // lossless concat.) Otherwise fall through to the simple single-pass
    // path below, which is both the common case (typical reels are short)
    // and the tested fallback if a duration probe or segmentation ever
    // misbehaves. A voiceover forces the single-pass path regardless of
    // length — segments.rs slices the *original* audio per segment, with
    // no notion of a separate voiceover file/offset to slice in lockstep;
    // correctness matters more here than the parallel-encode speedup. Any
    // video transitions do too, for the same reason: segments.rs has no
    // shift/clip treatment for a transition's absolute timestamp across a
    // segment boundary the way CaptionStyleOverride's shift_overrides
    // does — see video_transitions.rs's own doc comment.
    if segment_count > 1 && voiceover_path.is_none() && video_transitions.is_empty() {
        if let Some(duration) = duration {
            let result = burn_captions_segmented(
                &app,
                &video_path,
                &words,
                &style,
                &output_path,
                duration,
                segment_count,
                &prosody,
                &speakers,
                &overrides,
                &project_id,
            )
            .await;
            return result.map(|_| output_path);
        }
    }

    let ass_path = unique_temp_path("captions", "ass");
    let ass_contents = build_ass_document(&words, &style, &prosody, &speakers, &overrides)?;
    std::fs::write(&ass_path, ass_contents).map_err(|e| format!("Failed to write subtitle file: {e}"))?;

    let ass_filter = format!(
        "ass='{}':fontsdir='{}'",
        escape_ffmpeg_filter_path(&ass_path),
        escape_ffmpeg_filter_path(crate::bin_paths::fonts_dir())
    );
    let encoder = best_encoder().await;

    // Probed here (before `video_path` is moved into `args` below), only
    // when transitions were actually requested — the caption burn itself
    // has never needed real pixel dimensions (ASS scales via a fixed
    // PlayResX/Y), so a probe failure here degrades to "no transitions"
    // rather than failing the whole burn over a bonus visual effect. See
    // video_transitions.rs's own doc comment for why a generous/wrong
    // `duration` fallback here is safe rather than risking a truncated
    // output.
    let transition_filter = if video_transitions.is_empty() {
        None
    } else {
        match crate::ffmpeg::probe_video_dimensions(&video_path).await {
            Some((width, height, fps)) => crate::video_transitions::build_transition_filter(
                &video_transitions,
                "0:v",
                "vt_out",
                width,
                height,
                fps,
                duration.unwrap_or(9999.0),
            ),
            None => None,
        }
    };
    let used_transition_filter = transition_filter.is_some();

    let mut args = vec!["-y".to_string(), "-i".to_string(), video_path];
    // A voiceover replaces the video's own audio track entirely — same
    // `-itsoffset` timing convention as vosync.rs's mux (positive =
    // voiceover starts later), applied to a second input rather than
    // `-c:a copy`-ing the first input's own audio.
    let has_voiceover = voiceover_path.is_some();
    if let Some(vo_path) = voiceover_path {
        args.push("-itsoffset".to_string());
        args.push(voiceover_offset_seconds.unwrap_or(0.0).to_string());
        args.push("-i".to_string());
        args.push(vo_path);
    }
    if let Some(transition_graph) = transition_filter {
        // Transitions render first, so the ASS overlay (the captions)
        // stays fixed on screen rather than zooming/flashing along with
        // the footage — chained into one filter_complex graph rather than
        // two separate ffmpeg passes, which would re-encode twice, the
        // second pass compounding the first's compression artifacts.
        args.push("-filter_complex".to_string());
        args.push(format!("{transition_graph};[vt_out]{ass_filter}[final]"));
        args.push("-map".to_string());
        args.push("[final]".to_string());
    } else {
        args.push("-vf".to_string());
        args.push(ass_filter);
    }
    // Burning text onto a video needs decode + re-encode no matter what
    // (filters can't stream-copy). Prefer a hardware encoder when this
    // machine's ffmpeg build has one (usually 5-10x faster than even the
    // fastest software preset); otherwise fall back to libx264 at a fast
    // preset rather than paying its slow default "medium" search.
    args.extend(encoder.speed_args(0)); // 0 threads = let libx264 auto-detect; no oversubscription risk for a single job
    args.push("-c:v".to_string());
    args.push(encoder.codec_name().to_string());
    // `-filter_complex` disables ffmpeg's automatic per-type stream
    // selection entirely (unlike plain `-vf`), so once it's used every
    // stream this output needs — audio included — must be mapped
    // explicitly, not just the video the filtergraph itself produced.
    if has_voiceover {
        if !used_transition_filter {
            args.push("-map".to_string());
            args.push("0:v".to_string());
        }
        args.push("-map".to_string());
        args.push("1:a".to_string());
        args.push("-c:a".to_string());
        args.push("aac".to_string());
        args.push("-shortest".to_string());
    } else {
        if used_transition_filter {
            args.push("-map".to_string());
            args.push("0:a".to_string());
        }
        args.push("-c:a".to_string());
        args.push("copy".to_string());
    }
    args.push(output_path.clone());

    let result = run_with_progress(&app, "burn-progress", &project_id, "burning", args, duration).await;

    let _ = std::fs::remove_file(&ass_path);
    result?;

    Ok(output_path)
}

/// Extracts audio, runs `media_ai/prosody.py` (per-word RMS-energy
/// percentile bucketing), returns the result for the caller to hold onto
/// and pass into `burn_captions`.
#[tauri::command]
pub async fn analyze_prosody(
    app: AppHandle,
    video_path: String,
    words: Vec<WordTimestamp>,
    project_id: String,
) -> Result<Vec<ProsodyWord>, String> {
    let audio_path = crate::pipeline::extract_audio(&app, &video_path, &project_id).await?;

    let words_json_path = unique_temp_path("prosody-words", "json");
    let words_json =
        serde_json::to_string(&words).map_err(|e| format!("Couldn't serialize words for prosody analysis: {e}"))?;
    std::fs::write(&words_json_path, words_json).map_err(|e| format!("Couldn't write prosody words file: {e}"))?;

    let stdout = crate::media_ai::run_media_ai_script(
        &app,
        "prosody.py",
        vec!["--audio".to_string(), cli_path(&audio_path), "--words".to_string(), cli_path(&words_json_path)],
        "prosody-progress",
        &project_id,
        "analyzing_prosody",
    )
    .await;

    let _ = std::fs::remove_file(&audio_path);
    let _ = std::fs::remove_file(&words_json_path);
    let stdout = stdout?;

    serde_json::from_str(&stdout).map_err(|e| format!("Couldn't parse prosody.py output: {e} (raw: {stdout})"))
}
