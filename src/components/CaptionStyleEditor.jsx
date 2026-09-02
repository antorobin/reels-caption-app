import { useEffect, useState } from "react";
import { CAPTION_THEMES, defaultTheme } from "../lib/themes.js";

export const FONT_OPTIONS = [
  "Arial",
  "Verdana",
  "Georgia",
  "Impact",
  "Courier New",
  "Noto Sans Tamil",
  "Montserrat",
  "Bebas Neue",
  "Poppins",
  "Anton",
  "Playfair Display",
];
export const POSITION_OPTIONS = [
  { value: "bottom", label: "Bottom" },
  { value: "middle", label: "Middle" },
  { value: "top", label: "Top" },
];
export const ANIMATION_OPTIONS = [
  { value: "none", label: "None (static)" },
  { value: "fade", label: "Fade in/out" },
  { value: "pop", label: "Pop (scale in)" },
  { value: "bounce", label: "Bounce" },
  { value: "slide", label: "Slide in" },
  { value: "zoom", label: "Zoom out" },
  { value: "karaoke", label: "Karaoke fill" },
  { value: "highlight", label: "Highlight (word pop-in)" },
  { value: "typewriter", label: "Typewriter" },
];

// CSS class per animation, used to give the classic-mode preview real
// motion instead of a static frame — mirrors the ASS effect in
// captions.rs closely enough to convey the actual feel (karaoke/highlight/
// typewriter are inherently per-word/per-character reveals, harder to
// fake convincingly as a simple looping CSS class, so they stay static).
function classicPreviewAnimationClass(animation, position) {
  switch (animation) {
    case "fade":
      return "anim-fade";
    case "pop":
      return "anim-pop";
    case "bounce":
      return "anim-bounce";
    case "zoom":
      return "anim-zoom";
    case "slide":
      return position === "top" ? "anim-slide-down" : "anim-slide-up";
    default:
      return "";
  }
}

// Cycles which word is "active" so cascade-mode previews show the real
// moving spotlight (see build_cascade_text in captions.rs) instead of a
// static frame.
function useCyclingIndex(length, intervalMs) {
  const [index, setIndex] = useState(0);
  useEffect(() => {
    if (length <= 1) return undefined;
    const id = setInterval(() => setIndex((i) => (i + 1) % length), intervalMs);
    return () => clearInterval(id);
  }, [length, intervalMs]);
  return index;
}

const SAMPLE_TEXT_BY_TRANSFORM = {
  none: "captions look like this",
  uppercase: "CAPTIONS LOOK LIKE THIS",
  lowercase: "captions look like this",
  capitalize: "Captions Look Like This",
};

const SAMPLE_CASCADE_BY_TRANSFORM = {
  none: { prev: "there are a", current: "couple of reasons" },
  uppercase: { prev: "THERE ARE A", current: "COUPLE OF REASONS" },
  lowercase: { prev: "there are a", current: "couple of reasons" },
  capitalize: { prev: "There Are A", current: "Couple Of Reasons" },
};

// A small live-rendered swatch of what a theme actually looks/feels like —
// real font, colors, case, background, and (for classic themes) a looping
// CSS animation — rather than a plain text label.
function ThemeCardPreview({ themeStyle }) {
  if (themeStyle.style_mode === "cascade") {
    const sample = SAMPLE_CASCADE_BY_TRANSFORM[themeStyle.text_transform] ?? SAMPLE_CASCADE_BY_TRANSFORM.none;
    return (
      <CascadeSpotlightPreview
        style={themeStyle}
        prevWords={sample.prev.split(" ")}
        currentWords={sample.current.split(" ")}
        fontScale={0.5}
      />
    );
  }

  const sampleText = SAMPLE_TEXT_BY_TRANSFORM[themeStyle.text_transform] ?? SAMPLE_TEXT_BY_TRANSFORM.none;
  const fontSize = Math.min(themeStyle.font_size, 44) * 0.6;
  return (
    <span
      key={themeStyle.animation}
      className={classicPreviewAnimationClass(themeStyle.animation, themeStyle.position)}
      style={{
        display: "inline-block",
        fontFamily: themeStyle.font_family,
        fontSize: `${fontSize}px`,
        fontWeight: themeStyle.bold ? "bold" : "normal",
        fontStyle: themeStyle.italic ? "italic" : "normal",
        letterSpacing: `${themeStyle.letter_spacing}px`,
        color: themeStyle.text_color,
        WebkitTextStroke: `1px ${themeStyle.outline_color}`,
        textShadow: themeStyle.shadow_size > 0 ? `${themeStyle.shadow_size}px ${themeStyle.shadow_size}px 4px rgba(0,0,0,0.8)` : "none",
        backgroundColor:
          themeStyle.background === "box"
            ? `${themeStyle.background_color}${Math.round((themeStyle.background_opacity / 100) * 255)
                .toString(16)
                .padStart(2, "0")}`
            : "transparent",
        padding: themeStyle.background === "box" ? "3px 8px" : 0,
      }}
    >
      {sampleText}
    </span>
  );
}

export function CascadeSpotlightPreview({ style, prevWords, currentWords, fontScale = 1 }) {
  const activeIndex = useCyclingIndex(currentWords.length, 550);
  const baseSize = Math.min(style.font_size, 48) * fontScale;
  const prevSize = baseSize * 0.6;
  const activeSize = baseSize * 1.15;
  const stroke = `1px ${style.outline_color}`;
  const textTransform = style.text_transform && style.text_transform !== "none" ? style.text_transform : "none";

  return (
    <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 4, textTransform }}>
      {prevWords.length > 0 && (
        <span
          style={{
            fontFamily: style.font_family,
            fontSize: `${prevSize}px`,
            fontWeight: style.bold ? "bold" : "normal",
            fontStyle: style.italic ? "italic" : "normal",
            color: style.text_color,
            WebkitTextStroke: stroke,
          }}
        >
          {prevWords.join(" ")}
        </span>
      )}
      <span>
        {currentWords.map((w, i) => (
          <span
            key={i}
            style={{
              display: "inline-block",
              fontFamily: style.font_family,
              fontSize: `${i === activeIndex ? activeSize : baseSize}px`,
              fontWeight: style.bold ? "bold" : "normal",
              fontStyle: style.italic ? "italic" : "normal",
              color: i === activeIndex ? style.accent_color : style.text_color,
              WebkitTextStroke: stroke,
              transition: "font-size 150ms ease, color 150ms ease",
            }}
          >
            {w}
            {i < currentWords.length - 1 ? " " : ""}
          </span>
        ))}
      </span>
    </div>
  );
}
export const TEXT_TRANSFORM_OPTIONS = [
  { value: "none", label: "As typed" },
  { value: "uppercase", label: "UPPERCASE" },
  { value: "lowercase", label: "lowercase" },
  { value: "capitalize", label: "Capitalize Each Word" },
];
export const BACKGROUND_OPTIONS = [
  { value: "none", label: "None (outline only)" },
  { value: "box", label: "Solid box" },
];
export const STYLE_MODE_OPTIONS = [
  { value: "classic", label: "Classic (one styled line)" },
  { value: "cascade", label: "Cascade (big current word/phrase)" },
];

export function defaultCaptionStyle() {
  return { ...defaultTheme().style };
}

// Ordered by first appearance in CAPTION_THEMES rather than alphabetically,
// so the filter pill row reads in the same "boldest/most common first"
// order the theme cards themselves are authored in.
const THEME_CATEGORIES = [...new Set(CAPTION_THEMES.map((t) => t.category))];

// Key-order-independent equality for two style objects -- CaptionStyle is
// always a flat object of primitives, so comparing sorted [key, value]
// pairs is enough; no need for a general deep-equal library for this shape.
function stylesAreEqual(a, b) {
  const aKeys = Object.keys(a).sort();
  const bKeys = Object.keys(b).sort();
  if (aKeys.length !== bKeys.length) return false;
  return aKeys.every((k, i) => k === bKeys[i] && a[k] === b[k]);
}

// "<theme name>" if `style` is still exactly that theme's own stock style,
// "<theme name> (Custom)" the moment any field has been hand-tweaked away
// from it -- computed fresh every time from `themeId` + `style` rather than
// a separately-stored flag, so it can never drift out of sync with the
// actual style values (see library.rs's DefaultCaptionStyle doc comment,
// which this mirrors on the Rust side).
function displayNameFor(themeId, style) {
  const baseTheme = CAPTION_THEMES.find((t) => t.id === themeId);
  if (!baseTheme) return "Custom";
  return stylesAreEqual(style, baseTheme.style) ? baseTheme.name : `${baseTheme.name} (Custom)`;
}

function CaptionStyleEditor({ style, onChange, themeId, onThemeIdChange, onResetToFactoryDefault }) {
  const [categoryFilter, setCategoryFilter] = useState("All");
  const visibleThemes = categoryFilter === "All" ? CAPTION_THEMES : CAPTION_THEMES.filter((t) => t.category === categoryFilter);
  const currentName = displayNameFor(themeId, style);

  function set(key, value) {
    onChange({ ...style, [key]: value });
  }

  function pickTheme(theme) {
    onChange({ ...theme.style });
    onThemeIdChange(theme.id);
  }

  const previewText = {
    none: "Your captions look like this",
    uppercase: "YOUR CAPTIONS LOOK LIKE THIS",
    lowercase: "your captions look like this",
    capitalize: "Your Captions Look Like This",
  }[style.text_transform];

  const cascadePreview = {
    none: { prev: "there are a", current: "couple of reasons" },
    uppercase: { prev: "THERE ARE A", current: "COUPLE OF REASONS" },
    lowercase: { prev: "there are a", current: "couple of reasons" },
    capitalize: { prev: "There Are A", current: "Couple Of Reasons" },
  }[style.text_transform];

  const isCascade = style.style_mode === "cascade";

  return (
    <div className="style-editor">
      <div className="theme-category-filter">
        <button
          type="button"
          className={`theme-category-pill${categoryFilter === "All" ? " active" : ""}`}
          onClick={() => setCategoryFilter("All")}
        >
          All
        </button>
        {THEME_CATEGORIES.map((category) => (
          <button
            key={category}
            type="button"
            className={`theme-category-pill${categoryFilter === category ? " active" : ""}`}
            onClick={() => setCategoryFilter(category)}
          >
            {category}
          </button>
        ))}
      </div>

      <div className="theme-picker">
        {visibleThemes.map((theme) => (
          <button key={theme.id} type="button" className="theme-card" onClick={() => pickTheme(theme)} title={theme.description}>
            <span className="theme-card-swatch">
              <ThemeCardPreview themeStyle={theme.style} />
            </span>
            <span className="theme-card-name">{theme.name}</span>
            <span className="theme-card-description">{theme.description}</span>
          </button>
        ))}
      </div>

      <div className="current-theme-row">
        <span className="current-theme-name">Current: {currentName}</span>
        <button type="button" className="link-button" onClick={onResetToFactoryDefault}>
          Reset to factory default
        </button>
      </div>

      <div className="style-controls">
        <label>
          Style mode
          <select value={style.style_mode ?? "classic"} onChange={(e) => set("style_mode", e.target.value)}>
            {STYLE_MODE_OPTIONS.map((m) => (
              <option key={m.value} value={m.value}>
                {m.label}
              </option>
            ))}
          </select>
        </label>

        <label>
          Font
          <select value={style.font_family} onChange={(e) => set("font_family", e.target.value)}>
            {FONT_OPTIONS.map((f) => (
              <option key={f} value={f}>
                {f}
              </option>
            ))}
          </select>
        </label>

        <label>
          Size
          <input
            type="number"
            min={16}
            max={160}
            value={style.font_size}
            onChange={(e) => set("font_size", Number(e.target.value))}
          />
        </label>

        <label>
          Text color
          <input type="color" value={style.text_color} onChange={(e) => set("text_color", e.target.value)} />
        </label>

        <label>
          Outline color
          <input
            type="color"
            value={style.outline_color}
            onChange={(e) => set("outline_color", e.target.value)}
          />
        </label>

        {isCascade && (
          <label>
            Accent color (current word)
            <input type="color" value={style.accent_color} onChange={(e) => set("accent_color", e.target.value)} />
          </label>
        )}

        <label>
          Position
          <select value={style.position} onChange={(e) => set("position", e.target.value)}>
            {POSITION_OPTIONS.map((p) => (
              <option key={p.value} value={p.value}>
                {p.label}
              </option>
            ))}
          </select>
        </label>

        {!isCascade && (
          <label>
            Animation
            <select value={style.animation} onChange={(e) => set("animation", e.target.value)}>
              {ANIMATION_OPTIONS.map((a) => (
                <option key={a.value} value={a.value}>
                  {a.label}
                </option>
              ))}
            </select>
          </label>
        )}

        <label>
          Max words per phrase
          <input
            type="number"
            min={1}
            max={10}
            value={style.words_per_line}
            onChange={(e) => set("words_per_line", Number(e.target.value))}
          />
        </label>

        <label>
          Text case
          <select value={style.text_transform} onChange={(e) => set("text_transform", e.target.value)}>
            {TEXT_TRANSFORM_OPTIONS.map((t) => (
              <option key={t.value} value={t.value}>
                {t.label}
              </option>
            ))}
          </select>
        </label>

        <label>
          Letter spacing
          <input
            type="number"
            min={-5}
            max={30}
            value={style.letter_spacing}
            onChange={(e) => set("letter_spacing", Number(e.target.value))}
          />
        </label>

        <label>
          Drop shadow
          <input
            type="number"
            min={0}
            max={20}
            value={style.shadow_size}
            onChange={(e) => set("shadow_size", Number(e.target.value))}
          />
        </label>

        <label>
          Background
          <select value={style.background} onChange={(e) => set("background", e.target.value)}>
            {BACKGROUND_OPTIONS.map((b) => (
              <option key={b.value} value={b.value}>
                {b.label}
              </option>
            ))}
          </select>
        </label>

        {style.background === "box" && (
          <>
            <label>
              Background color
              <input
                type="color"
                value={style.background_color}
                onChange={(e) => set("background_color", e.target.value)}
              />
            </label>
            <label>
              Background opacity
              <input
                type="number"
                min={0}
                max={100}
                value={style.background_opacity}
                onChange={(e) => set("background_opacity", Number(e.target.value))}
              />
            </label>
          </>
        )}

        <label className="style-controls-checkbox">
          <input type="checkbox" checked={style.bold} onChange={(e) => set("bold", e.target.checked)} />
          Bold
        </label>

        <label className="style-controls-checkbox">
          <input type="checkbox" checked={style.italic} onChange={(e) => set("italic", e.target.checked)} />
          Italic
        </label>
      </div>

      <div
        className="style-preview"
        style={{ justifyContent: style.position === "top" ? "flex-start" : style.position === "middle" ? "center" : "flex-end" }}
      >
        {isCascade ? (
          <CascadeSpotlightPreview
            style={style}
            prevWords={cascadePreview.prev.split(" ")}
            currentWords={cascadePreview.current.split(" ")}
          />
        ) : (
          <span
            key={style.animation}
            className={`style-preview-text ${classicPreviewAnimationClass(style.animation, style.position)}`}
            style={{
              fontFamily: style.font_family,
              fontSize: `${Math.min(style.font_size, 48)}px`,
              fontWeight: style.bold ? "bold" : "normal",
              fontStyle: style.italic ? "italic" : "normal",
              letterSpacing: `${style.letter_spacing}px`,
              color: style.text_color,
              WebkitTextStroke: `1px ${style.outline_color}`,
              textShadow: style.shadow_size > 0 ? `${style.shadow_size}px ${style.shadow_size}px 4px rgba(0,0,0,0.8)` : "none",
              backgroundColor: style.background === "box" ? `${style.background_color}${Math.round((style.background_opacity / 100) * 255).toString(16).padStart(2, "0")}` : "transparent",
              padding: style.background === "box" ? "4px 10px" : 0,
            }}
          >
            {previewText}
          </span>
        )}
      </div>
    </div>
  );
}

export default CaptionStyleEditor;
