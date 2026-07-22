export const FONT_OPTIONS = ["Arial", "Verdana", "Georgia", "Impact", "Courier New"];
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
  { value: "karaoke", label: "Karaoke fill" },
  { value: "highlight", label: "Highlight (word pop-in)" },
  { value: "typewriter", label: "Typewriter" },
];
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
  return {
    font_family: "Arial",
    font_size: 64,
    text_color: "#FFFFFF",
    outline_color: "#000000",
    position: "bottom",
    animation: "karaoke",
    words_per_line: 4,
    bold: true,
    italic: false,
    letter_spacing: 0,
    text_transform: "none",
    background: "none",
    background_color: "#000000",
    background_opacity: 70,
    shadow_size: 0,
    style_mode: "classic",
    accent_color: "#FFE600",
  };
}

// karaoke/highlight animate the line's own color, which fights the
// cascade mode's accent-color override on the current line — keep those
// off the menu there rather than let the two silently clash.
const CASCADE_SAFE_ANIMATIONS = ["none", "fade", "pop", "bounce", "typewriter"];

function CaptionStyleEditor({ style, onChange }) {
  function set(key, value) {
    onChange({ ...style, [key]: value });
  }

  function setStyleMode(mode) {
    const patch = { style_mode: mode };
    if (mode === "cascade" && !CASCADE_SAFE_ANIMATIONS.includes(style.animation)) {
      patch.animation = "none";
    }
    onChange({ ...style, ...patch });
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
      <div className="style-controls">
        <label>
          Style mode
          <select value={style.style_mode ?? "classic"} onChange={(e) => setStyleMode(e.target.value)}>
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

        <label>
          Animation
          <select value={style.animation} onChange={(e) => set("animation", e.target.value)}>
            {(isCascade ? ANIMATION_OPTIONS.filter((a) => CASCADE_SAFE_ANIMATIONS.includes(a.value)) : ANIMATION_OPTIONS).map((a) => (
              <option key={a.value} value={a.value}>
                {a.label}
              </option>
            ))}
          </select>
        </label>

        <label>
          {isCascade ? "Words per phrase" : "Words per line"}
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
          <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 4 }}>
            <span
              className="style-preview-text"
              style={{
                fontFamily: style.font_family,
                fontSize: `${Math.min(style.font_size, 48) * 0.6}px`,
                fontWeight: style.bold ? "bold" : "normal",
                fontStyle: style.italic ? "italic" : "normal",
                letterSpacing: `${style.letter_spacing}px`,
                color: style.text_color,
                WebkitTextStroke: `1px ${style.outline_color}`,
              }}
            >
              {cascadePreview.prev}
            </span>
            <span
              className="style-preview-text"
              style={{
                fontFamily: style.font_family,
                fontSize: `${Math.min(style.font_size, 48)}px`,
                fontWeight: style.bold ? "bold" : "normal",
                fontStyle: style.italic ? "italic" : "normal",
                letterSpacing: `${style.letter_spacing}px`,
                color: style.accent_color,
                WebkitTextStroke: `1px ${style.outline_color}`,
              }}
            >
              {cascadePreview.current}
            </span>
          </div>
        ) : (
          <span
            className="style-preview-text"
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
