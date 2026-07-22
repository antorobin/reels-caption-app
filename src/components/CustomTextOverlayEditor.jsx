import { ANIMATION_OPTIONS, BACKGROUND_OPTIONS, FONT_OPTIONS, POSITION_OPTIONS, TEXT_TRANSFORM_OPTIONS } from "./CaptionStyleEditor.jsx";
import { formatTime } from "../lib/time.js";

function CustomTextOverlayEditor({
  currentTime,
  draft,
  onDraftTextChange,
  onDraftFieldChange,
  onDraftDurationChange,
  onDraftEndTimeChange,
  overlays,
  onAdd,
  onRemove,
  onSeek,
}) {
  const endTime = currentTime + draft.duration_secs;
  const sortedOverlays = [...overlays].sort((a, b) => a.start - b.start);

  return (
    <div className="overlay-editor">
      <div className="overlay-form">
        <input
          type="text"
          className="overlay-text-input"
          placeholder="Custom text…"
          value={draft.text}
          onChange={(e) => onDraftTextChange(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && onAdd()}
        />

        <div className="style-controls">
          <label>
            Duration (s)
            <input
              type="number"
              min={0.1}
              step={0.1}
              value={draft.duration_secs}
              onChange={(e) => onDraftDurationChange(Number(e.target.value))}
            />
          </label>

          <label>
            Ends at (s)
            <input
              type="number"
              min={Math.round((currentTime + 0.1) * 10) / 10}
              step={0.1}
              value={Math.round(endTime * 10) / 10}
              onChange={(e) => onDraftEndTimeChange(Number(e.target.value))}
            />
          </label>

          <label>
            Position
            <select value={draft.position} onChange={(e) => onDraftFieldChange("position", e.target.value)}>
              {POSITION_OPTIONS.map((p) => (
                <option key={p.value} value={p.value}>
                  {p.label}
                </option>
              ))}
            </select>
          </label>

          <label>
            Font
            <select value={draft.font_family} onChange={(e) => onDraftFieldChange("font_family", e.target.value)}>
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
              max={200}
              value={draft.font_size}
              onChange={(e) => onDraftFieldChange("font_size", Number(e.target.value))}
            />
          </label>

          <label>
            Text color
            <input
              type="color"
              value={draft.text_color}
              onChange={(e) => onDraftFieldChange("text_color", e.target.value)}
            />
          </label>

          <label>
            Outline color
            <input
              type="color"
              value={draft.outline_color}
              onChange={(e) => onDraftFieldChange("outline_color", e.target.value)}
            />
          </label>

          <label>
            Animation
            <select value={draft.animation} onChange={(e) => onDraftFieldChange("animation", e.target.value)}>
              {ANIMATION_OPTIONS.map((a) => (
                <option key={a.value} value={a.value}>
                  {a.label}
                </option>
              ))}
            </select>
          </label>

          <label>
            Text case
            <select value={draft.text_transform} onChange={(e) => onDraftFieldChange("text_transform", e.target.value)}>
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
              value={draft.letter_spacing}
              onChange={(e) => onDraftFieldChange("letter_spacing", Number(e.target.value))}
            />
          </label>

          <label>
            Drop shadow
            <input
              type="number"
              min={0}
              max={20}
              value={draft.shadow_size}
              onChange={(e) => onDraftFieldChange("shadow_size", Number(e.target.value))}
            />
          </label>

          <label>
            Background
            <select value={draft.background} onChange={(e) => onDraftFieldChange("background", e.target.value)}>
              {BACKGROUND_OPTIONS.map((b) => (
                <option key={b.value} value={b.value}>
                  {b.label}
                </option>
              ))}
            </select>
          </label>

          {draft.background === "box" && (
            <>
              <label>
                Background color
                <input
                  type="color"
                  value={draft.background_color}
                  onChange={(e) => onDraftFieldChange("background_color", e.target.value)}
                />
              </label>
              <label>
                Background opacity
                <input
                  type="number"
                  min={0}
                  max={100}
                  value={draft.background_opacity}
                  onChange={(e) => onDraftFieldChange("background_opacity", Number(e.target.value))}
                />
              </label>
            </>
          )}

          <label className="style-controls-checkbox">
            <input type="checkbox" checked={draft.bold} onChange={(e) => onDraftFieldChange("bold", e.target.checked)} />
            Bold
          </label>

          <label className="style-controls-checkbox">
            <input
              type="checkbox"
              checked={draft.italic}
              onChange={(e) => onDraftFieldChange("italic", e.target.checked)}
            />
            Italic
          </label>
        </div>

        <p className="overlay-timing-hint">
          Shown {formatTime(currentTime)} – {formatTime(endTime)} ({draft.duration_secs.toFixed(1)}s
          {draft.duration_touched ? "" : ", auto-suggested for comfortable reading"})
        </p>

        <button onClick={onAdd} disabled={!draft.text.trim()}>
          Add text at {formatTime(currentTime)}
        </button>
      </div>

      {sortedOverlays.length > 0 && (
        <ul className="overlay-list">
          {sortedOverlays.map((o) => (
            <li key={o.id} className="overlay-list-item">
              <button className="overlay-seek" onClick={() => onSeek(o.start)}>
                {formatTime(o.start)}–{formatTime(o.end)}
              </button>
              <span className="overlay-text">{o.text}</span>
              <button className="overlay-remove" onClick={() => onRemove(o.id)} aria-label={`Remove "${o.text}"`}>
                ✕
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export default CustomTextOverlayEditor;
