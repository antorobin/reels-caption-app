import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  autoRangeForSuggestion,
  autoTransitionEffectForSuggestion,
  snapToNearestWordStart,
  suggestTransitionPoints,
  TRANSITION_EFFECT_LABELS,
} from "../../lib/captions.js";
import { autoThemeIdForSuggestion } from "../../lib/themes.js";
import { formatTime } from "../../lib/time.js";
import { displayNameFor } from "../CaptionStyleEditor.jsx";

// Downsamples decoded audio into per-pixel-column min/max pairs and draws
// them as vertical bars — the standard client-side waveform technique, no
// library needed.
function drawWaveform(canvas, audioBuffer) {
  const ctx = canvas.getContext("2d");
  const width = canvas.width;
  const height = canvas.height;
  ctx.clearRect(0, 0, width, height);
  const channel = audioBuffer.getChannelData(0);
  const samplesPerPixel = Math.max(1, Math.floor(channel.length / width));
  ctx.fillStyle = "#4a4d55";
  for (let x = 0; x < width; x++) {
    let min = 1.0;
    let max = -1.0;
    const start = x * samplesPerPixel;
    const end = Math.min(channel.length, start + samplesPerPixel);
    for (let i = start; i < end; i++) {
      const v = channel[i];
      if (v < min) min = v;
      if (v > max) max = v;
    }
    const yMin = ((1 - max) / 2) * height;
    const yMax = ((1 - min) / 2) * height;
    ctx.fillRect(x, yMin, 1, Math.max(1, yMax - yMin));
  }
}

// How many pixels of movement before a mousedown-then-move counts as a
// real drag rather than a plain click -- keeps today's click-to-seek
// working unchanged for anyone not trying to select a range at all.
const DRAG_THRESHOLD_PX = 4;
// A drag shorter than this (in seconds) is treated as an accidental jitter,
// not a real range selection -- avoids firing onRangeSelected for a
// barely-moved mousedown/mouseup that wasn't really a drag gesture.
const MIN_RANGE_SECONDS = 0.05;
// Default span for "Add caption style" from a pin with no known
// "reasons" (a plain dropped pin, an existing override/transition band) --
// autoRangeForSuggestion's own default is used instead whenever reasons
// *are* known (a suggestion), since that also caps the end at the next
// suggestion.
const DEFAULT_STYLE_SPAN_SECONDS = 4;
// How close (in seconds) an existing video transition's own time must be
// to a popover's anchor time to count as "already at this pin" -- mirrors
// the tolerance captions.rs/captions.js already use for matching a word
// to prosody data.
const TRANSITION_MATCH_TOLERANCE_SECONDS = 0.05;

// The transition library's own longer descriptions, shown as a tooltip in
// the effect picker -- kept alongside TRANSITION_EFFECT_LABELS's short
// display names (captions.js, reused by MoreOptionsModal.jsx too) since
// only this picker needs the longer text.
const TRANSITION_EFFECT_DESCRIPTIONS = {
  "zoom-punch":
    "A brief zoom-in-then-settle on the footage itself (about 0.4s) -- a quick 'punch in' at this moment, like editors use at a tone or topic shift. Captions stay put; only the video zooms.",
  "flash-cut": "A quick white flash across the footage (about 0.15s) -- like a camera flash punctuating a beat. Captions stay fully readable through it.",
  shake: "A brief 2D jitter on the footage (about 0.4s) -- a jarring jolt, like a camera bump at a sudden or abrupt beat.",
  "color-pulse": "A brief desaturate-to-gray-and-back pulse (about 0.3s) -- a tonal/mood shift, gentler than a flash.",
};

// Scope note: this is a rich visualization + scrubber for the one loaded
// clip, not an editable multi-track sequence — the Rust backend stays a
// linear single-clip pipeline (extract -> transcribe -> jumpcut -> style
// -> burn). Word blocks are read-only seek targets; the track itself is
// click-to-seek, or drag-to-select directly on the waveform for a caption
// style range (unrelated to pins -- see the mousedown/mousemove/mouseup
// handlers below).
//
// Every kind of pin (an auto-suggested point, a manually dropped one, an
// existing caption-style-override band, or an existing video-transition
// marker) opens the *same* small menu on click, built fresh each time
// from what's actually at that pin's own time -- see openPopoverAt below.
// This used to be four separate, hand-built menus (one per pin "kind")
// that had grown to 6-8 rows each as the transition library grew; this
// keeps every menu to a handful of rows regardless of how many effects
// the library ever grows to, at the cost of "add a transition" being a
// two-step pick (choose the effect on a second, still-small screen)
// rather than every effect getting its own row on the first screen.
function Timeline({
  videoPath,
  words,
  currentTime,
  duration,
  onSeek,
  jumpCuts = [],
  prosody = [],
  speakers = [],
  captionStyleOverrides = [],
  videoTransitions = [],
  onRangeSelected,
  onAddCaptionStyle,
  onEditOverride,
  onRemoveOverride,
  onAddVideoTransition,
  onUpdateVideoTransition,
  onRemoveVideoTransition,
  onSuggestTransitionPlan,
  suggestingTransitionPlan = false,
}) {
  const trackRef = useRef(null);
  const canvasRef = useRef(null);
  const dragOriginRef = useRef(null); // { clientX, fraction } from mousedown to the next mouseup
  const [isPointerDown, setIsPointerDown] = useState(false);
  const [pendingDrag, setPendingDrag] = useState(null); // { startFraction, endFraction } once past the threshold
  const [pins, setPins] = useState([]); // [{ time }] -- manually dropped, purely a placeholder marker
  // Suggestions the user has explicitly dismissed ("🚫 Remove suggestion")
  // -- keyed by the suggestion's own (already word-snapped) time, so a
  // dismissal survives the underlying suggestion list being recomputed
  // (e.g. after an unrelated edit elsewhere) as long as that same moment
  // still gets flagged. Component-local and not persisted: suggestions
  // are a live heuristic over the transcript, not stored data, so a
  // dismissed one simply may reappear on a fresh app launch -- same
  // lifetime as `pins` above.
  const [dismissedSuggestionTimes, setDismissedSuggestionTimes] = useState(() => new Set());
  // Result/error text from the last "✨ AI-suggest transitions" click --
  // local and transient, cleared on the next click or manually dismissed.
  const [transitionPlanMessage, setTransitionPlanMessage] = useState(null); // { text, isError } | null

  // A small click-triggered menu anchored to whichever pin was just
  // clicked -- `position: fixed` (so it escapes .timeline-track's
  // `overflow: hidden` instead of getting clipped) and positioned in two
  // passes: `anchor*` records where the clicked element actually was on
  // screen; a useLayoutEffect below then measures the popover's *real*
  // rendered size (which varies with content -- the menu view has more
  // rows than the effect-picker view) and clamps it fully inside the
  // viewport before it's ever visible. A first attempt here just guessed
  // "open above, unless the marker looks too close to the top" from a
  // fixed height estimate -- a real user report showed that still wasn't
  // enough (a short window, or a marker near an edge other than the top,
  // could still push part of the menu off-screen with no way to reach
  // it), so this measures for real instead of estimating.
  //
  // `source` records what was actually clicked (needed for "Remove pin",
  // which only makes sense for a suggestion or a manually-dropped pin --
  // an override band or transition marker has no separate "pin" entity of
  // its own to remove, just its own content); `overrideIndex`/
  // `transitionIndex` are resolved once, at open time, from whatever
  // already covers this exact time, so the menu can offer Add vs.
  // Edit/Remove correctly regardless of which kind of marker was clicked.
  const [popover, setPopover] = useState(null);
  // { time, source: {type, index}, overrideIndex, transitionIndex, reasons, speakerId, anchorTop, anchorBottom, anchorLeft }
  const [popoverView, setPopoverView] = useState("menu"); // "menu" | "pick-transition"
  const [popoverPos, setPopoverPos] = useState(null); // { top, left } in px, or null while unmeasured/hidden
  const popoverRef = useRef(null);

  function openPopoverAt(time, source, e) {
    if (e) e.stopPropagation();
    const overrideIndex = captionStyleOverrides.findIndex((o) => time >= o.start && time < o.end);
    const transitionIndex = videoTransitions.findIndex((t) => Math.abs(t.time - time) < TRANSITION_MATCH_TOLERANCE_SECONDS);
    const reasons = source.type === "suggestion" ? suggestions[source.index]?.reasons ?? null : null;
    const speakerId = source.type === "suggestion" ? suggestions[source.index]?.speakerId ?? null : null;

    let anchorTop;
    let anchorBottom;
    let anchorLeft;
    if (e) {
      const rect = e.currentTarget.getBoundingClientRect();
      anchorTop = rect.top;
      anchorBottom = rect.bottom;
      anchorLeft = rect.left + rect.width / 2;
    } else {
      // dropPin()'s case: the pin doesn't exist in the DOM yet, so there's
      // no element to read a rect from -- computed the same way, straight
      // from the track's own bounding rect and the playhead's fraction of
      // the total duration.
      const trackEl = trackRef.current;
      if (!trackEl || !(duration > 0)) return;
      const rect = trackEl.getBoundingClientRect();
      const fraction = Math.min(1, Math.max(0, time / duration));
      anchorTop = rect.top;
      anchorBottom = rect.bottom;
      anchorLeft = rect.left + fraction * rect.width;
    }

    setPopoverPos(null); // hidden until the effect below measures this popover's actual size
    setPopoverView("menu");
    setPopover({
      time,
      source,
      overrideIndex: overrideIndex >= 0 ? overrideIndex : null,
      transitionIndex: transitionIndex >= 0 ? transitionIndex : null,
      reasons,
      speakerId,
      anchorTop,
      anchorBottom,
      anchorLeft,
    });
  }

  const POPOVER_VIEWPORT_MARGIN_PX = 8;
  const POPOVER_GAP_PX = 10;

  useLayoutEffect(() => {
    if (!popover || !popoverRef.current) return;
    const el = popoverRef.current;
    const w = el.offsetWidth;
    const h = el.offsetHeight;
    const margin = POPOVER_VIEWPORT_MARGIN_PX;

    // Prefer opening above the marker (today's default feel); flip below
    // it only when there truly isn't room above.
    let top = popover.anchorTop - h - POPOVER_GAP_PX;
    if (top < margin) top = popover.anchorBottom + POPOVER_GAP_PX;
    // Clamped on *every* edge regardless of which direction was chosen
    // above -- covers a window too short for either "above" or "below"
    // alone to fully fit the menu.
    top = Math.min(Math.max(top, margin), window.innerHeight - h - margin);

    let left = popover.anchorLeft - w / 2;
    left = Math.min(Math.max(left, margin), window.innerWidth - w - margin);

    setPopoverPos({ top, left });
    // popoverView is intentionally a dependency too -- switching between
    // the menu and the effect-picker changes the popover's real rendered
    // size, and needs to be re-measured/re-clamped the same way a brand
    // new popover does.
  }, [popover, popoverView]);

  // Suggests candidate pin positions from speaker changes, silence gaps,
  // prosody-driven emphasis jumps, and pace shifts -- see
  // suggestTransitionPoints's own doc comment in captions.js for the
  // full algorithm. Recomputed only when the underlying transcript/
  // analysis data actually changes, not on every playhead tick.
  const allSuggestions = useMemo(
    () => suggestTransitionPoints(words, speakers, prosody, captionStyleOverrides),
    [words, speakers, prosody, captionStyleOverrides]
  );
  // What's actually shown/actionable -- excludes anything the user
  // dismissed. `autoRangeForSuggestion` below is deliberately given this
  // filtered list too (not `allSuggestions`), so a dismissed suggestion
  // no longer caps a neighboring auto-applied range either.
  const suggestions = useMemo(
    () => allSuggestions.filter((s) => !dismissedSuggestionTimes.has(s.time)),
    [allSuggestions, dismissedSuggestionTimes]
  );

  // Decode the video's audio track client-side via the Web Audio API and
  // draw it to the waveform canvas. Not every codec ffmpeg accepts is
  // guaranteed to be decodable by the browser's decodeAudioData — on
  // failure this just skips the waveform (no crash), leaving the word
  // track and playhead fully functional on their own.
  useEffect(() => {
    if (!videoPath || !canvasRef.current) return;
    let cancelled = false;
    const canvas = canvasRef.current;
    const audioCtx = new (window.AudioContext || window.webkitAudioContext)();
    fetch(convertFileSrc(videoPath))
      .then((res) => res.arrayBuffer())
      .then((buf) => audioCtx.decodeAudioData(buf))
      .then((audioBuffer) => {
        if (!cancelled && canvasRef.current) drawWaveform(canvas, audioBuffer);
      })
      .catch(() => {
        // Decode failed — leave the waveform blank, rest of the timeline still works.
      })
      .finally(() => audioCtx.close().catch(() => {}));
    return () => {
      cancelled = true;
    };
  }, [videoPath]);

  function fractionFromClientX(clientX) {
    const el = trackRef.current;
    if (!el || !duration) return 0;
    const rect = el.getBoundingClientRect();
    return Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
  }

  function seekFromClientX(clientX) {
    if (!duration) return;
    onSeek(fractionFromClientX(clientX) * duration);
  }

  function handleTrackMouseDown(e) {
    setPopover(null);
    dragOriginRef.current = { clientX: e.clientX, fraction: fractionFromClientX(e.clientX) };
    setIsPointerDown(true);
  }

  // Window-level listeners (not just on the track element) so releasing
  // the mouse outside the track's own bounds still ends the drag
  // correctly — the standard pattern for this kind of drag interaction.
  useEffect(() => {
    if (!isPointerDown) return undefined;

    function onMove(e) {
      const origin = dragOriginRef.current;
      if (!origin) return;
      if (!pendingDrag && Math.abs(e.clientX - origin.clientX) < DRAG_THRESHOLD_PX) return;
      const currentFraction = fractionFromClientX(e.clientX);
      setPendingDrag({
        startFraction: Math.min(origin.fraction, currentFraction),
        endFraction: Math.max(origin.fraction, currentFraction),
      });
    }

    function onUp(e) {
      const origin = dragOriginRef.current;
      dragOriginRef.current = null;
      setIsPointerDown(false);
      if (!origin) return;
      if (!pendingDrag) {
        seekFromClientX(e.clientX); // never crossed the drag threshold — today's plain click-to-seek
        return;
      }
      const rawStart = pendingDrag.startFraction * duration;
      const rawEnd = pendingDrag.endFraction * duration;
      setPendingDrag(null);
      if (rawEnd - rawStart <= MIN_RANGE_SECONDS || !onRangeSelected) return;
      const start = snapToNearestWordStart(rawStart, words);
      const end = snapToNearestWordStart(rawEnd, words);
      if (end - start > MIN_RANGE_SECONDS) onRangeSelected(start, end);
    }

    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    return () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- fractionFromClientX/seekFromClientX close over refs/props read fresh each call
  }, [isPointerDown, pendingDrag, duration, onRangeSelected, words]);

  const pct = (seconds) => `${duration > 0 ? Math.min(100, Math.max(0, (seconds / duration) * 100)) : 0}%`;

  // Drops a new marker pin at the playhead and immediately opens its
  // menu -- there's nothing to configure about a bare pin itself, it's
  // purely a placeholder marking a moment the user wants to come back to
  // and act on right away.
  function dropPin() {
    const newIndex = pins.length;
    setPins((prev) => [...prev, { time: currentTime }]);
    openPopoverAt(currentTime, { type: "pin", index: newIndex });
  }

  function removePin(index) {
    setPins((prev) => prev.filter((_, i) => i !== index));
  }

  // Sends the heuristic's own current candidate list (times only -- the
  // backend re-derives everything else, including which ones actually
  // deserve one, from the real transcript text) to transition_planner.rs
  // and applies whatever it kept as real VideoTransitions -- see that
  // module's own doc comment for why it's constrained to these candidates
  // rather than free timestamps, and for the one real, honestly-kept
  // limitation (it can still keep a candidate that doesn't really deserve
  // one on a narratively flat transcript) -- every result lands as a
  // plain, individually-removable transition, never auto-committed to
  // anything harder to undo.
  async function handleSuggestTransitionPlan() {
    setTransitionPlanMessage(null);
    try {
      const result = await onSuggestTransitionPlan?.(suggestions);
      // The LLM's own authored "reason" isn't persisted on the
      // VideoTransition itself (that struct is also burn_captions's own
      // input shape, and the reason has no bearing on the actual ffmpeg
      // filter) -- surfaced here, once, instead, so the "why" isn't lost
      // entirely.
      const reasonsList = (result || []).map((r) => `"${r.reason}"`).join(", ");
      setTransitionPlanMessage({
        text:
          result && result.length > 0
            ? `✨ Added ${result.length} AI-suggested transition${result.length === 1 ? "" : "s"}: ${reasonsList}`
            : "✨ No moments in this transcript seemed to need a transition.",
        isError: false,
      });
    } catch (err) {
      setTransitionPlanMessage({ text: `Couldn't suggest transitions: ${err}`, isError: true });
    }
  }

  return (
    <div className="timeline-root">
      <div className="timeline-time-label">
        <span>
          {formatTime(currentTime)} / {formatTime(duration)}
        </span>
        {duration > 0 && (
          <span className="timeline-time-label-actions">
            <button type="button" className="timeline-pin-button" onClick={dropPin}>
              📍 Pin a style override
            </button>
            {suggestions.length > 0 && (
              <button
                type="button"
                className="timeline-pin-button"
                disabled={suggestingTransitionPlan}
                title="Sends the suggested pin times (not the video) to the local AI, which reads the actual transcript to decide which ones deserve a transition and picks the best effect for each."
                onClick={handleSuggestTransitionPlan}
              >
                {suggestingTransitionPlan ? "✨ Thinking…" : "✨ AI-suggest transitions"}
              </button>
            )}
          </span>
        )}
      </div>
      {transitionPlanMessage && (
        <p className={`timeline-transition-plan-message${transitionPlanMessage.isError ? " error" : ""}`}>
          {transitionPlanMessage.text}
          <button type="button" className="link-button" onClick={() => setTransitionPlanMessage(null)}>
            Dismiss
          </button>
        </p>
      )}
      <div className="timeline-track" ref={trackRef} onMouseDown={handleTrackMouseDown}>
        <canvas ref={canvasRef} className="timeline-waveform" width={1200} height={48} />

        {speakers.length > 0 && (
          <div className="timeline-overlay-track timeline-speaker-track">
            {speakers.map((s, i) => (
              <div
                key={i}
                className="timeline-speaker-band"
                style={{ left: pct(s.start), width: pct(s.end - s.start), background: `var(--shell-speaker-${s.speaker_id % 4})` }}
              />
            ))}
          </div>
        )}

        {jumpCuts.length > 0 && (
          <div className="timeline-overlay-track timeline-jumpcut-track">
            {jumpCuts.map((c, i) => (
              <div key={i} className="timeline-jumpcut-marker" style={{ left: pct(c.start), width: pct(c.end - c.start) }} />
            ))}
          </div>
        )}

        {captionStyleOverrides.length > 0 && (
          <div className="timeline-overlay-track timeline-override-track">
            {captionStyleOverrides.map((o, i) => (
              <div
                key={i}
                className="timeline-override-band"
                style={{ left: pct(o.start), width: pct(o.end - o.start) }}
                title={`${displayNameFor(o.themeId, o.style)} — click for options`}
                onMouseDown={(e) => e.stopPropagation()}
                onClick={(e) => openPopoverAt(o.start, { type: "override", index: i }, e)}
              />
            ))}
          </div>
        )}

        {pendingDrag && (
          <div className="timeline-overlay-track timeline-override-track">
            <div
              className="timeline-override-band timeline-override-band-pending"
              style={{
                left: `${pendingDrag.startFraction * 100}%`,
                width: `${(pendingDrag.endFraction - pendingDrag.startFraction) * 100}%`,
              }}
            />
          </div>
        )}

        {suggestions.map((s, i) => (
          <div
            key={`suggestion-${i}`}
            className="timeline-pin-marker timeline-pin-marker-suggested"
            style={{ left: pct(s.time) }}
            title={`Suggested: ${s.reasons.join(" + ")} — click to view options`}
            onMouseDown={(e) => e.stopPropagation()}
            onClick={(e) => openPopoverAt(s.time, { type: "suggestion", index: i }, e)}
          >
            <span className="timeline-pin-marker-flag">📍</span>
          </div>
        ))}

        {pins.map((pin, i) => (
          <div
            key={`pin-${i}`}
            className="timeline-pin-marker"
            style={{ left: pct(pin.time) }}
            title="Click for options"
            onMouseDown={(e) => e.stopPropagation()}
            onClick={(e) => openPopoverAt(pin.time, { type: "pin", index: i }, e)}
          >
            <span className="timeline-pin-marker-flag">📍</span>
          </div>
        ))}

        {videoTransitions.length > 0 && (
          <div className="timeline-overlay-track timeline-transition-track">
            {videoTransitions.map((t, i) => (
              <div
                key={`transition-${i}`}
                className="timeline-transition-marker"
                style={{ left: pct(t.time) }}
                title={`${(TRANSITION_EFFECT_LABELS[t.effect] || t.effect).replace("🎬 ", "")} — click for options`}
                onMouseDown={(e) => e.stopPropagation()}
                onClick={(e) => openPopoverAt(t.time, { type: "transition", index: i }, e)}
              >
                <span className="timeline-transition-marker-flag">🎬</span>
              </div>
            ))}
          </div>
        )}

        <div className="timeline-track-row timeline-words">
          {words.map((w, i) => {
            const intensity = prosody.find((p) => Math.abs(p.start - w.start) < 0.05)?.intensity;
            return (
              <div
                key={i}
                className={intensity ? `timeline-word-block intensity-${intensity}` : "timeline-word-block"}
                style={{ left: pct(w.start), width: pct(Math.max(w.end - w.start, 0.05)) }}
                title={w.word}
                // Stops a drag from ever starting on top of a word block
                // (they tile across almost the whole track) — a
                // range-selection gesture can only begin from the
                // waveform/background area; word-click-to-seek below is
                // untouched.
                onMouseDown={(e) => e.stopPropagation()}
                onClick={(e) => {
                  e.stopPropagation();
                  onSeek(w.start);
                }}
              >
                {w.word}
              </div>
            );
          })}
        </div>

        <div className="timeline-playhead" style={{ left: pct(currentTime) }} />
      </div>

      {popover &&
        (() => {
          const hasOverride = popover.overrideIndex != null;
          const hasTransition = popover.transitionIndex != null;
          const existingTransition = hasTransition ? videoTransitions[popover.transitionIndex] : null;
          // Only a suggestion or a manually-dropped pin has a separate
          // "pin" entity to remove -- an override band or transition
          // marker's own content is removed via its own dedicated action
          // instead, with nothing left over afterward.
          const canRemovePin = popover.source.type === "suggestion" || popover.source.type === "pin";
          const suggestedEffect = popover.reasons ? autoTransitionEffectForSuggestion(popover.reasons) : null;

          function handleAddCaptionStyle() {
            const range = popover.reasons
              ? autoRangeForSuggestion(popover.time, suggestions, duration)
              : { start: popover.time, end: Math.min(popover.time + DEFAULT_STYLE_SPAN_SECONDS, duration) };
            const seedThemeId = popover.reasons ? autoThemeIdForSuggestion(popover.reasons, popover.speakerId) : null;
            onAddCaptionStyle?.(range.start, range.end, seedThemeId);
            setPopover(null);
          }

          function handleRemovePin() {
            if (popover.source.type === "suggestion") {
              const s = suggestions[popover.source.index];
              if (s) setDismissedSuggestionTimes((prev) => new Set(prev).add(s.time));
            } else if (popover.source.type === "pin") {
              removePin(popover.source.index);
            }
            setPopover(null);
          }

          function handlePickEffect(effect) {
            if (hasTransition) onUpdateVideoTransition?.(popover.transitionIndex, { time: existingTransition.time, effect });
            else onAddVideoTransition?.(popover.time, effect);
            setPopover(null);
          }

          const title = popover.reasons ? popover.reasons.join(" + ") : `Pin at ${formatTime(popover.time)}`;

          return (
            <div
              ref={popoverRef}
              className="timeline-popover"
              style={popoverPos ? { top: popoverPos.top, left: popoverPos.left } : { top: -9999, left: -9999, visibility: "hidden" }}
              onMouseDown={(e) => e.stopPropagation()}
            >
              {popoverView === "menu" && (
                <>
                  <div className="timeline-popover-title">{title}</div>
                  {!hasOverride && <button type="button" onClick={handleAddCaptionStyle}>🎨 Add caption style</button>}
                  {hasOverride && (
                    <button
                      type="button"
                      onClick={() => {
                        onEditOverride?.(popover.overrideIndex);
                        setPopover(null);
                      }}
                    >
                      🎨 Edit caption style
                    </button>
                  )}
                  {hasOverride && (
                    <button
                      type="button"
                      className="timeline-popover-remove"
                      onClick={() => {
                        onRemoveOverride?.(popover.overrideIndex);
                        setPopover(null);
                      }}
                    >
                      ✕ Remove caption style
                    </button>
                  )}
                  {!hasTransition && (
                    <button type="button" onClick={() => setPopoverView("pick-transition")}>
                      🎬 Add transition
                    </button>
                  )}
                  {hasTransition && (
                    <button type="button" onClick={() => setPopoverView("pick-transition")}>
                      🎬 Edit transition ({(TRANSITION_EFFECT_LABELS[existingTransition.effect] || existingTransition.effect).replace(
                        "🎬 ",
                        ""
                      )})
                    </button>
                  )}
                  {hasTransition && (
                    <button
                      type="button"
                      className="timeline-popover-remove"
                      onClick={() => {
                        onRemoveVideoTransition?.(popover.transitionIndex);
                        setPopover(null);
                      }}
                    >
                      ✕ Remove transition
                    </button>
                  )}
                  {canRemovePin && (
                    <button type="button" className="timeline-popover-remove" onClick={handleRemovePin}>
                      ✕ Remove pin
                    </button>
                  )}
                  <button type="button" className="timeline-popover-dismiss" onClick={() => setPopover(null)}>
                    Cancel
                  </button>
                </>
              )}

              {popoverView === "pick-transition" && (
                <>
                  <div className="timeline-popover-title">Choose an effect</div>
                  {Object.keys(TRANSITION_EFFECT_LABELS).map((effect) => (
                    <button key={effect} type="button" title={TRANSITION_EFFECT_DESCRIPTIONS[effect]} onClick={() => handlePickEffect(effect)}>
                      {TRANSITION_EFFECT_LABELS[effect]}
                      {suggestedEffect === effect ? " (suggested)" : ""}
                      {hasTransition && existingTransition.effect === effect ? " (current)" : ""}
                    </button>
                  ))}
                  <button type="button" className="timeline-popover-dismiss" onClick={() => setPopoverView("menu")}>
                    Back
                  </button>
                </>
              )}
            </div>
          );
        })()}
    </div>
  );
}

export default Timeline;
