import { useEffect, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { findActiveChunk, findActiveChunkIndex, groupWordsIntoChunks } from "../lib/captions.js";
import { formatTime } from "../lib/time.js";

// Custom-overlay and caption font sizes are authored in ASS's
// PlayResX=1920 coordinate space (see captions.rs). Scale them down to
// whatever width the video is actually rendered at in the DOM so the
// preview roughly matches the eventual burned-in size.
const ASS_PLAY_RES_WIDTH = 1920;

// Mirrors CASCADE_PREV_SCALE in captions.rs.
const CASCADE_PREV_SCALE = 0.6;

function isOverlayActive(overlay, currentTime) {
  return currentTime >= overlay.start && currentTime < overlay.end;
}

function hexWithOpacity(hex, opacityPercent) {
  const alpha = Math.round(((opacityPercent ?? 100) / 100) * 255)
    .toString(16)
    .padStart(2, "0");
  return `${hex}${alpha}`;
}

function OverlayText({ overlay, scale, isDraft }) {
  const fontSize = Math.max(8, overlay.font_size * scale);
  const strokeWidth = Math.max(1, scale * 3);
  const shadowSize = (overlay.shadow_size ?? 0) * scale;
  return (
    <div className={`video-overlay-anchor pos-${overlay.position}`}>
      <span
        className={isDraft ? "video-overlay-text video-overlay-text-draft" : "video-overlay-text"}
        style={{
          fontFamily: overlay.font_family,
          fontSize: `${fontSize}px`,
          fontWeight: overlay.bold ? "bold" : "normal",
          fontStyle: overlay.italic ? "italic" : "normal",
          letterSpacing: `${(overlay.letter_spacing ?? 0) * scale}px`,
          textTransform: overlay.text_transform && overlay.text_transform !== "none" ? overlay.text_transform : "none",
          color: overlay.text_color,
          WebkitTextStroke: `${strokeWidth}px ${overlay.outline_color}`,
          textShadow: shadowSize > 0 ? `${shadowSize}px ${shadowSize}px 4px rgba(0,0,0,0.8)` : "none",
          backgroundColor:
            overlay.background === "box" ? hexWithOpacity(overlay.background_color, overlay.background_opacity) : "transparent",
          padding: overlay.background === "box" ? "0.15em 0.4em" : 0,
          borderRadius: overlay.background === "box" ? "4px" : 0,
        }}
      >
        {overlay.text}
      </span>
    </div>
  );
}

// Cascade mode's rolling 2-line window: the chunk just spoken shrinks to
// `text_color` above, the chunk being spoken now is shown large in
// `accent_color` below. Mirrors build_cascade_text in captions.rs.
function CascadeCaption({ style, scale, current, prev }) {
  const currentText = current.map((w) => w.word).join(" ");
  const prevText = prev ? prev.map((w) => w.word).join(" ") : "";
  const fontSize = Math.max(8, style.font_size * scale);
  const prevFontSize = Math.max(8, style.font_size * scale * CASCADE_PREV_SCALE);
  const strokeWidth = Math.max(1, scale * 3);
  const textTransform = style.text_transform && style.text_transform !== "none" ? style.text_transform : "none";

  return (
    <div className={`video-overlay-anchor pos-${style.position}`}>
      <div className="video-cascade-caption">
        {prevText && (
          <span
            className="video-cascade-line"
            style={{
              fontFamily: style.font_family,
              fontSize: `${prevFontSize}px`,
              fontWeight: style.bold ? "bold" : "normal",
              fontStyle: style.italic ? "italic" : "normal",
              textTransform,
              color: style.text_color,
              WebkitTextStroke: `${strokeWidth}px ${style.outline_color}`,
            }}
          >
            {prevText}
          </span>
        )}
        <span
          className="video-cascade-line"
          style={{
            fontFamily: style.font_family,
            fontSize: `${fontSize}px`,
            fontWeight: style.bold ? "bold" : "normal",
            fontStyle: style.italic ? "italic" : "normal",
            textTransform,
            color: style.accent_color,
            WebkitTextStroke: `${strokeWidth}px ${style.outline_color}`,
          }}
        >
          {currentText}
        </span>
      </div>
    </div>
  );
}

function VideoPreview({
  videoRef,
  videoPath,
  currentTime,
  duration,
  words,
  captionStyle,
  overlays,
  draftOverlay,
  onTimeUpdate,
  onLoadedMetadata,
  onSeek,
}) {
  const [displayWidth, setDisplayWidth] = useState(0);
  const src = convertFileSrc(videoPath);

  useEffect(() => {
    function measure() {
      if (videoRef.current) setDisplayWidth(videoRef.current.clientWidth);
    }
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [videoRef, videoPath]);

  const scale = displayWidth > 0 ? displayWidth / ASS_PLAY_RES_WIDTH : 0;
  const activeOverlays = overlays.filter((o) => isOverlayActive(o, currentTime));
  const hasDraftText = Boolean(draftOverlay?.text?.trim());

  const chunks = words && words.length > 0 ? groupWordsIntoChunks(words, captionStyle.words_per_line) : [];
  const isCascade = captionStyle.style_mode === "cascade";

  const activeChunk = !isCascade ? findActiveChunk(chunks, currentTime) : null;
  const activeCaption = activeChunk
    ? {
        text: activeChunk.map((w) => w.word).join(" "),
        position: captionStyle.position,
        font_family: captionStyle.font_family,
        font_size: captionStyle.font_size,
        text_color: captionStyle.text_color,
        outline_color: captionStyle.outline_color,
        bold: captionStyle.bold,
        italic: captionStyle.italic,
        letter_spacing: captionStyle.letter_spacing,
        text_transform: captionStyle.text_transform,
        background: captionStyle.background,
        background_color: captionStyle.background_color,
        background_opacity: captionStyle.background_opacity,
        shadow_size: captionStyle.shadow_size,
      }
    : null;

  const activeChunkIndex = isCascade ? findActiveChunkIndex(chunks, currentTime) : -1;
  const activeCascade =
    activeChunkIndex >= 0
      ? { current: chunks[activeChunkIndex], prev: activeChunkIndex > 0 ? chunks[activeChunkIndex - 1] : null }
      : null;

  return (
    <div className="video-preview">
      <div className="video-frame">
        <video
          ref={videoRef}
          src={src}
          controls
          onTimeUpdate={(e) => onTimeUpdate(e.currentTarget.currentTime)}
          onLoadedMetadata={(e) => {
            onLoadedMetadata(e.currentTarget.duration);
            setDisplayWidth(e.currentTarget.clientWidth);
          }}
          className="video-preview-player"
        />
        {scale > 0 && (
          <div className="video-overlay-layer">
            {/* Transcript caption first (matches ASS layer 0), custom
                overlays after (ASS layer 1, draws on top if they collide). */}
            {activeCaption && <OverlayText overlay={activeCaption} scale={scale} />}
            {activeCascade && <CascadeCaption style={captionStyle} scale={scale} current={activeCascade.current} prev={activeCascade.prev} />}
            {activeOverlays.map((o) => (
              <OverlayText key={o.id} overlay={o} scale={scale} />
            ))}
            {hasDraftText && <OverlayText overlay={draftOverlay} scale={scale} isDraft />}
          </div>
        )}
      </div>

      <div className="video-timeline">
        <input
          type="range"
          className="video-slider"
          min={0}
          max={duration || 0}
          step={0.01}
          value={Math.min(currentTime, duration || currentTime)}
          onChange={(e) => onSeek(Number(e.target.value))}
        />
        <div className="video-timeline-markers">
          {overlays.map((o) => (
            <div
              key={o.id}
              className="video-timeline-marker"
              style={{ left: `${duration > 0 ? (o.start / duration) * 100 : 0}%` }}
              title={`${o.text} — ${formatTime(o.start)}`}
              onClick={() => onSeek(o.start)}
            />
          ))}
        </div>
      </div>

      <p className="video-time-label">
        {formatTime(currentTime)} / {formatTime(duration)}
      </p>
    </div>
  );
}

export default VideoPreview;
