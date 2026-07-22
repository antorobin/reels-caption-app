// Burns styled captions onto a video by generating an ASS (Advanced
// SubStation Alpha) subtitle file from word-level timestamps, then running
// it through ffmpeg's `ass` filter (libass). ASS is used instead of plain
// `drawtext` because its per-word `\k`/`\t` tags and real Style entries
// (background boxes, bold/italic, letter spacing, drop shadow) are what
// make TikTok-style caption presets possible without generating one
// drawtext filter per word.

use serde::Deserialize;
use std::path::Path;
use tauri::AppHandle;

use crate::ffmpeg::{best_encoder, probe_duration_seconds, run_with_progress};
use crate::pipeline::WordTimestamp;
use crate::segments::{burn_captions_segmented, recommended_segment_count};
use crate::util::{cli_path, unique_temp_path};

#[derive(Debug, Clone, Deserialize)]
pub struct CaptionStyle {
    pub font_family: String,
    pub font_size: u32,
    pub text_color: String,    // "#RRGGBB"
    pub outline_color: String, // "#RRGGBB"
    pub position: String,      // "bottom" | "middle" | "top"
    // "none" | "fade" | "pop" | "karaoke" | "bounce" | "typewriter" | "highlight"
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

/// A user-placed piece of text pinned to a specific time range — separate
/// from the auto-generated transcript captions (e.g. a "SALE!" sticker or
/// "link in bio" callout at a chosen moment), with its own independent
/// styling and (like transcript captions) its own animation preset.
/// Rendered on ASS layer 1 (vs. the transcript captions' layer 0) so it
/// always draws on top if the two overlap in time.
#[derive(Debug, Clone, Deserialize)]
pub struct CustomTextOverlay {
    pub id: String,
    pub text: String,
    pub start: f64,
    pub end: f64,
    pub position: String, // "bottom" | "middle" | "top"
    pub font_family: String,
    pub font_size: u32,
    pub text_color: String,    // "#RRGGBB"
    pub outline_color: String, // "#RRGGBB"
    #[serde(default = "default_animation")]
    pub animation: String,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub letter_spacing: f32,
    #[serde(default = "default_text_transform")]
    pub text_transform: String,
    #[serde(default = "default_background")]
    pub background: String,
    #[serde(default = "default_background_color")]
    pub background_color: String,
    #[serde(default = "default_background_opacity")]
    pub background_opacity: f32,
    #[serde(default)]
    pub shadow_size: f32,
}

fn default_animation() -> String {
    "none".to_string()
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
        "Style: {name},{font_family},{font_size},{primary},{primary},{outline},{back_colour},{bold_flag},{italic_flag},0,0,100,100,{letter_spacing},0,{border_style},3,{shadow_size},{alignment},40,40,{margin_v},1\n"
    ))
}

/// Splits `text` into evenly-timed word spans across `[start, end]`. Used
/// to give custom overlays (which only have one start/end for their whole
/// text) the same per-word timing structure transcript caption chunks
/// have, so both can share the same word-level animation code.
fn synthesize_word_spans(text: &str, start: f64, end: f64) -> Vec<WordTimestamp> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }
    let span = (end - start).max(0.01);
    let step = span / words.len() as f64;
    words
        .iter()
        .enumerate()
        .map(|(i, w)| WordTimestamp { word: (*w).to_string(), start: start + step * i as f64, end: start + step * (i as f64 + 1.0) })
        .collect()
}

fn group_words_into_lines(words: &[WordTimestamp], words_per_line: usize) -> Vec<&[WordTimestamp]> {
    let words_per_line = words_per_line.max(1);
    words.chunks(words_per_line).collect()
}

/// Renders one caption line's text with `animation` applied to `words`
/// (already text-transformed). `\t`/`\k` timing tags are relative to the
/// dialogue event's own Start, so every offset here is computed relative
/// to `words[0].start`, regardless of whether `words` came from real
/// transcript timestamps or [`synthesize_word_spans`].
fn build_animated_text(words: &[WordTimestamp], animation: &str, text_transform: &str) -> String {
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

/// The "previous chunk" line in cascade mode shrinks to this fraction of
/// `font_size` — tuned by eye against the reference "big word" preset
/// (current word large + accent color, trailing phrase small + base color).
const CASCADE_PREV_SCALE: f32 = 0.6;

/// Renders one cascade-mode dialogue: the just-finished chunk (if any)
/// small and in `text_color` on its own line, followed by the chunk
/// currently being spoken large and in `accent_color`. `animation` still
/// applies to the current line's per-word timing (karaoke/highlight/etc),
/// layered under the accent-color override.
fn build_cascade_text(prev: Option<&[WordTimestamp]>, current: &[WordTimestamp], style: &CaptionStyle) -> Result<String, String> {
    let prev_color = hex_to_ass_override_color(&style.text_color)?;
    let accent_color = hex_to_ass_override_color(&style.accent_color)?;
    let prev_size = (style.font_size as f32 * CASCADE_PREV_SCALE).round().max(1.0) as u32;

    let mut out = String::new();
    if let Some(prev_words) = prev {
        if !prev_words.is_empty() {
            let prev_text: Vec<String> = prev_words.iter().map(|w| apply_text_transform(&w.word, &style.text_transform)).collect();
            out.push_str(&format!(
                "{{\\fs{prev_size}\\c{prev_color}}}{}\\N",
                escape_ass_text(&prev_text.join(" "))
            ));
        }
    }
    out.push_str(&format!("{{\\fs{}\\c{accent_color}}}", style.font_size));
    out.push_str(&build_animated_text(current, &style.animation, &style.text_transform));
    Ok(out)
}

pub(crate) fn build_ass_document(
    words: &[WordTimestamp],
    style: &CaptionStyle,
    custom_overlays: &[CustomTextOverlay],
) -> Result<String, String> {
    let mut doc = String::new();
    doc.push_str("[Script Info]\n");
    doc.push_str("ScriptType: v4.00+\n");
    doc.push_str("PlayResX: 1920\n");
    doc.push_str("PlayResY: 1080\n");
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
    for (i, overlay) in custom_overlays.iter().enumerate() {
        doc.push_str(&build_style_line(
            &format!("Overlay{i}"),
            &overlay.font_family,
            overlay.font_size,
            &overlay.text_color,
            &overlay.outline_color,
            &overlay.position,
            overlay.bold,
            overlay.italic,
            overlay.letter_spacing,
            &overlay.background,
            &overlay.background_color,
            overlay.background_opacity,
            overlay.shadow_size,
        )?);
    }
    doc.push('\n');

    doc.push_str("[Events]\n");
    doc.push_str("Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n");

    let chunks = group_words_into_lines(words, style.words_per_line);
    for (i, chunk) in chunks.iter().enumerate() {
        let (Some(first), Some(last)) = (chunk.first(), chunk.last()) else {
            continue;
        };
        let start = ass_timestamp(first.start);
        let end = ass_timestamp(last.end);
        let text = if style.style_mode == "cascade" {
            let prev = if i > 0 { Some(chunks[i - 1]) } else { None };
            build_cascade_text(prev, chunk, style)?
        } else {
            build_animated_text(chunk, &style.animation, &style.text_transform)
        };
        doc.push_str(&format!("Dialogue: 0,{start},{end},Default,,0,0,0,,{text}\n"));
    }

    for (i, overlay) in custom_overlays.iter().enumerate() {
        let start = ass_timestamp(overlay.start);
        let end = ass_timestamp(overlay.end.max(overlay.start + 0.1));
        let spans = synthesize_word_spans(&overlay.text, overlay.start, overlay.end);
        let text = build_animated_text(&spans, &overlay.animation, &overlay.text_transform);
        doc.push_str(&format!("Dialogue: 1,{start},{end},Overlay{i},,0,0,0,,{text}\n"));
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

    fn overlay(text: &str, start: f64, end: f64) -> CustomTextOverlay {
        CustomTextOverlay {
            id: "1".to_string(),
            text: text.to_string(),
            start,
            end,
            position: "top".to_string(),
            font_family: "Impact".to_string(),
            font_size: 80,
            text_color: "#FFFF00".to_string(),
            outline_color: "#000000".to_string(),
            animation: "none".to_string(),
            bold: false,
            italic: false,
            letter_spacing: 0.0,
            text_transform: "none".to_string(),
            background: "none".to_string(),
            background_color: "#000000".to_string(),
            background_opacity: 70.0,
            shadow_size: 0.0,
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
        let text = build_animated_text(&words, "karaoke", "none");
        assert!(text.contains("{\\k50}hello"));
        assert!(text.contains("{\\k70}world"));
    }

    #[test]
    fn build_animated_text_none_is_plain_joined_words() {
        let words = vec![word("hello", 0.0, 0.5), word("world", 0.5, 1.2)];
        assert_eq!(build_animated_text(&words, "none", "none"), "hello world");
    }

    #[test]
    fn build_animated_text_applies_transform_before_animating() {
        let words = vec![word("hello", 0.0, 0.5)];
        assert_eq!(build_animated_text(&words, "none", "uppercase"), "HELLO");
    }

    #[test]
    fn build_animated_text_highlight_transitions_each_word_at_its_own_offset() {
        let words = vec![word("hello", 0.0, 0.5), word("world", 0.5, 1.2)];
        let text = build_animated_text(&words, "highlight", "none");
        assert!(text.contains("\\t(0,150,\\c)}hello"));
        assert!(text.contains("\\t(500,650,\\c)}world")); // "world" starts 500ms into the chunk
    }

    #[test]
    fn build_animated_text_typewriter_reveals_each_character() {
        let words = vec![word("hi", 0.0, 0.5)];
        let text = build_animated_text(&words, "typewriter", "none");
        assert!(text.contains("\\alpha&HFF&"));
        assert!(text.contains('h'));
        assert!(text.contains('i'));
    }

    #[test]
    fn synthesize_word_spans_distributes_time_evenly() {
        let spans = synthesize_word_spans("one two three four", 10.0, 14.0);
        assert_eq!(spans.len(), 4);
        assert_eq!(spans[0].start, 10.0);
        assert_eq!(spans[0].end, 11.0);
        assert_eq!(spans[3].start, 13.0);
        assert_eq!(spans[3].end, 14.0);
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
        let doc = build_ass_document(&words, &s, &[]).unwrap();
        let dialogue_lines: Vec<&str> = doc.lines().filter(|l| l.starts_with("Dialogue:")).collect();
        assert_eq!(dialogue_lines.len(), 2); // 4 words + 1 word = 2 chunks
        assert!(dialogue_lines[0].contains("one two three four"));
        assert!(dialogue_lines[1].contains("five"));
    }

    #[test]
    fn build_cascade_text_omits_prev_line_when_none() {
        let current = vec![word("sometimes", 0.0, 0.5)];
        let s = style("none");
        let text = build_cascade_text(None, &current, &s).unwrap();
        assert!(!text.contains("\\N"));
        assert!(text.contains(&format!("\\fs{}", s.font_size)));
        assert!(text.contains("sometimes"));
    }

    #[test]
    fn build_cascade_text_renders_prev_small_and_current_accent_colored() {
        let prev = vec![word("at", 0.0, 0.2), word("night", 0.2, 0.4)];
        let current = vec![word("sometimes", 0.4, 0.9)];
        let mut s = style("none");
        s.font_size = 64;
        s.text_color = "#FFFFFF".to_string();
        s.accent_color = "#FFE600".to_string();
        let text = build_cascade_text(Some(&prev), &current, &s).unwrap();

        // Prev line: smaller font, base text color, ends with a line break.
        assert!(text.contains(&format!("\\fs{}", (64.0_f32 * CASCADE_PREV_SCALE).round() as u32)));
        assert!(text.contains("&HFFFFFF&")); // white override color for prev
        assert!(text.contains("at night\\N"));
        // Current line: full font size, accent color.
        assert!(text.contains("\\fs64"));
        assert!(text.contains("&H00E6FF&")); // BGR override for #FFE600
        assert!(text.contains("sometimes"));
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
        let doc = build_ass_document(&words, &s, &[]).unwrap();
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
        assert!(build_ass_document(&[word("hi", 0.0, 0.1)], &s, &[]).is_err());
    }

    #[test]
    fn build_ass_document_includes_custom_overlays_on_a_higher_layer_with_their_own_style() {
        let words = vec![word("hello", 0.0, 1.0)];
        let overlays = vec![overlay("SALE!", 0.5, 2.0)];
        let doc = build_ass_document(&words, &style("none"), &overlays).unwrap();

        let style_lines: Vec<&str> = doc.lines().filter(|l| l.starts_with("Style:")).collect();
        assert_eq!(style_lines.len(), 2);
        assert!(style_lines[0].starts_with("Style: Default,"));
        assert!(style_lines[1].starts_with("Style: Overlay0,"));

        let dialogue_lines: Vec<&str> = doc.lines().filter(|l| l.starts_with("Dialogue:")).collect();
        assert_eq!(dialogue_lines.len(), 2);
        assert!(dialogue_lines[0].starts_with("Dialogue: 0,")); // transcript caption, layer 0
        assert!(dialogue_lines[1].starts_with("Dialogue: 1,")); // overlay, layer 1 (draws on top)
        assert!(dialogue_lines[1].contains(",Overlay0,"));
        assert!(dialogue_lines[1].contains("SALE!"));
    }

    #[test]
    fn build_ass_document_works_with_only_custom_overlays_no_transcript() {
        let overlays = vec![overlay("Link in bio", 0.0, 2.0)];
        let doc = build_ass_document(&[], &style("none"), &overlays).unwrap();
        let dialogue_lines: Vec<&str> = doc.lines().filter(|l| l.starts_with("Dialogue:")).collect();
        assert_eq!(dialogue_lines.len(), 1);
        assert!(dialogue_lines[0].contains("Link in bio"));
    }
}

#[tauri::command]
pub async fn burn_captions(
    app: AppHandle,
    video_path: String,
    words: Vec<WordTimestamp>,
    style: CaptionStyle,
    custom_overlays: Vec<CustomTextOverlay>,
    output_path: String,
) -> Result<String, String> {
    if words.is_empty() && custom_overlays.is_empty() {
        return Err(
            "Nothing to burn — run the transcription pipeline or add custom text first.".to_string()
        );
    }

    let duration = probe_duration_seconds(&video_path).await;
    let segment_count = duration.map(recommended_segment_count).unwrap_or(1);

    // Long enough to be worth the segmented-parallel-encode complexity?
    // (See segments.rs for why: cut points snapped to transcript gaps,
    // per-segment audio re-encode, right-sized thread division, fast
    // lossless concat.) Otherwise fall through to the simple single-pass
    // path below, which is both the common case (typical reels are short)
    // and the tested fallback if a duration probe or segmentation ever
    // misbehaves.
    if segment_count > 1 {
        if let Some(duration) = duration {
            let result = burn_captions_segmented(
                &app,
                &video_path,
                &words,
                &style,
                &custom_overlays,
                &output_path,
                duration,
                segment_count,
            )
            .await;
            return result.map(|_| output_path);
        }
    }

    let ass_path = unique_temp_path("captions", "ass");
    let ass_contents = build_ass_document(&words, &style, &custom_overlays)?;
    std::fs::write(&ass_path, ass_contents).map_err(|e| format!("Failed to write subtitle file: {e}"))?;

    let filter = format!("ass='{}'", escape_ffmpeg_filter_path(&ass_path));
    let encoder = best_encoder().await;

    let mut args = vec!["-y".to_string(), "-i".to_string(), video_path, "-vf".to_string(), filter];
    // Burning text onto a video needs decode + re-encode no matter what
    // (filters can't stream-copy). Prefer a hardware encoder when this
    // machine's ffmpeg build has one (usually 5-10x faster than even the
    // fastest software preset); otherwise fall back to libx264 at a fast
    // preset rather than paying its slow default "medium" search.
    args.extend(encoder.speed_args(0)); // 0 threads = let libx264 auto-detect; no oversubscription risk for a single job
    args.push("-c:v".to_string());
    args.push(encoder.codec_name().to_string());
    args.push("-c:a".to_string());
    args.push("copy".to_string());
    args.push(output_path.clone());

    let result = run_with_progress(&app, "burn-progress", "burning", args, duration).await;

    let _ = std::fs::remove_file(&ass_path);
    result?;

    Ok(output_path)
}
