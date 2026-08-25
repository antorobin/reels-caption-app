import { useEffect, useRef } from "react";
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

// Scope note: this is a rich visualization + scrubber for the one loaded
// clip, not an editable multi-track sequence — the Rust backend stays a
// linear single-clip pipeline (extract -> transcribe -> jumpcut -> style
// -> burn). Word/overlay blocks are read-only seek targets.
function Timeline({ videoPath, words, currentTime, duration, onSeek, jumpCuts = [], prosody = [], speakers = [] }) {
  const trackRef = useRef(null);
  const canvasRef = useRef(null);

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

  function seekFromClientX(clientX) {
    const el = trackRef.current;
    if (!el || !duration) return;
    const rect = el.getBoundingClientRect();
    const fraction = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
    onSeek(fraction * duration);
  }

  const pct = (seconds) => `${duration > 0 ? Math.min(100, Math.max(0, (seconds / duration) * 100)) : 0}%`;

  return (
    <div className="timeline-root">
      <div className="timeline-time-label">
        {formatTime(currentTime)} / {formatTime(duration)}
      </div>
      <div className="timeline-track" ref={trackRef} onClick={(e) => seekFromClientX(e.clientX)}>
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

        <div className="timeline-track-row timeline-words">
          {words.map((w, i) => {
            const intensity = prosody.find((p) => Math.abs(p.start - w.start) < 0.05)?.intensity;
            return (
              <div
                key={i}
                className={intensity ? `timeline-word-block intensity-${intensity}` : "timeline-word-block"}
                style={{ left: pct(w.start), width: pct(Math.max(w.end - w.start, 0.05)) }}
                title={w.word}
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
