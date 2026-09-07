import { useState } from "react";
import CaptionStyleEditor, { defaultCaptionStyle } from "../CaptionStyleEditor.jsx";
import { overrideRangeOverlaps } from "../../lib/captions.js";
import { defaultTheme } from "../../lib/themes.js";
import { formatTime } from "../../lib/time.js";

// Appears (as MainPanel.jsx local state, `pendingRange`) right after a
// drag-select completes on the Timeline, or after "+ Add a style override
// for this portion" — a compact inline panel, not a full modal. Reuses
// CaptionStyleEditor wholesale (its theme grid, category filter, and
// every fine-tuning control) against a fresh draft style/themeId, seeded
// from the project's *current* base style rather than the hardcoded
// factory default, so tuning an override starts from what the video
// already looks like everywhere else.
//
// `range` is only the *initial* start/end -- both button and drag entry
// points can only ever produce an approximate range (the button defaults
// to "3s from the playhead"; a drag is precise but still worth double-
// checking), so start/end are real editable number inputs here, not a
// read-only label, clamped to `[0, duration]`.
//
// Doubles as the *edit* panel for an existing override: pass its own
// style/themeId as `baseStyle`/`baseThemeId` (the draft seeds from
// whatever it should start looking like -- the project base style when
// creating, the override's own current style when editing) and its own
// index as `excludeIndex` so the overlap check doesn't flag the range
// against itself. `onRemove` is only provided in edit mode, rendering an
// extra destructive action alongside Cancel/Apply.
function CaptionOverridePanel({
  range,
  duration,
  baseStyle,
  baseThemeId,
  existingOverrides,
  excludeIndex,
  onApply,
  onCancel,
  onRemove,
}) {
  const [draftStyle, setDraftStyle] = useState(() => ({ ...baseStyle }));
  const [draftThemeId, setDraftThemeId] = useState(baseThemeId || defaultTheme().id);
  const [start, setStart] = useState(range.start);
  const [end, setEnd] = useState(range.end);
  const overlaps = overrideRangeOverlaps(start, end, existingOverrides, excludeIndex);
  const validRange = end > start;

  function clamp(value) {
    return Math.min(Math.max(0, value), duration || value);
  }

  return (
    <div className="caption-override-panel">
      <div className="caption-override-panel-header">
        <span>Style for</span>
        <span className="caption-override-panel-range">
          <input
            type="number"
            min={0}
            max={duration || undefined}
            step={0.1}
            value={Math.round(start * 10) / 10}
            onChange={(e) => setStart(clamp(Number(e.target.value)))}
          />
          <span>–</span>
          <input
            type="number"
            min={0}
            max={duration || undefined}
            step={0.1}
            value={Math.round(end * 10) / 10}
            onChange={(e) => setEnd(clamp(Number(e.target.value)))}
          />
          <span className="section-hint">seconds ({formatTime(start)} – {formatTime(end)})</span>
        </span>
        <button type="button" className="shell-modal-close" onClick={onCancel}>
          ✕
        </button>
      </div>

      {!validRange && <p className="caption-override-panel-error">End must be after start.</p>}
      {validRange && overlaps && (
        <p className="caption-override-panel-error">This range overlaps an existing override — adjust it above.</p>
      )}

      <CaptionStyleEditor
        style={draftStyle}
        onChange={setDraftStyle}
        themeId={draftThemeId}
        onThemeIdChange={setDraftThemeId}
        onResetToFactoryDefault={() => {
          setDraftStyle(defaultCaptionStyle());
          setDraftThemeId(defaultTheme().id);
        }}
      />

      <div className="caption-override-panel-actions">
        {onRemove && (
          <button type="button" className="caption-override-panel-remove" onClick={onRemove}>
            Remove override
          </button>
        )}
        <button type="button" onClick={onCancel}>
          Cancel
        </button>
        <button
          type="button"
          disabled={overlaps || !validRange}
          onClick={() => onApply({ start, end, style: draftStyle, themeId: draftThemeId })}
        >
          Apply
        </button>
      </div>
    </div>
  );
}

export default CaptionOverridePanel;
