import { useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { formatTime } from "../../lib/time.js";

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

// Snaps a raw dragged time to the start of whichever word is closest —
// so the range a style override actually applies to (a word is assigned
// to a piece by its own start time, see captions.js's buildChunkTimeline)
// always matches what was visually drawn on the timeline, never landing
// silently mid-word.
function snapToNearestWordStart(time, words) {
  if (!words || words.length === 0) return time;
  let closest = words[0].start;
  let closestDistance = Math.abs(words[0].start - time);
  for (const w of words) {
    const distance = Math.abs(w.start - time);
    if (distance < closestDistance) {
      closest = w.start;
      closestDistance = distance;
    }
  }
  return closest;
}

// Scope note: this is a rich visualization + scrubber for the one loaded
// clip, not an editable multi-track sequence — the Rust backend stays a
// linear single-clip pipeline (extract -> transcribe -> jumpcut -> style
// -> burn). Word blocks are read-only seek targets; the track itself is
// click-to-seek, or drag-to-select-a-range for a caption style override
// (see onRangeSelected).
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
  onRangeSelected,
}) {
  const trackRef = useRef(null);
  const canvasRef = useRef(null);
  const dragOriginRef = useRef(null); // { clientX, fraction } from mousedown to the next mouseup
  const [isPointerDown, setIsPointerDown] = useState(false);
  const [pendingDrag, setPendingDrag] = useState(null); // { startFraction, endFraction } once past the threshold

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

  return (
    <div className="timeline-root">
      <div className="timeline-time-label">
        {formatTime(currentTime)} / {formatTime(duration)}
      </div>
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
                title={o.themeName || "Custom style"}
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
    </div>
  );
}

export default Timeline;
