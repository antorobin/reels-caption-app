import { useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { findActiveChunk, findActiveChunkIndex, groupWordsIntoChunks } from "../lib/captions.js";
import { formatTime } from "../lib/time.js";

// Caption font sizes are authored in ASS's PlayResX=1920 coordinate space
// (see captions.rs). Scale them down to whatever width the video is
// actually rendered at in the DOM so the preview roughly matches the
// eventual burned-in size.
const ASS_PLAY_RES_WIDTH = 1920;

// Mirrors CASCADE_PREV_SCALE / CASCADE_ACTIVE_SCALE in captions.rs.
const CASCADE_PREV_SCALE = 0.6;
const CASCADE_ACTIVE_SCALE = 1.15;

function OverlayText({ overlay, scale }) {
  const fontSize = Math.max(8, overlay.font_size * scale);
  const strokeWidth = Math.max(1, scale * 3);
  const shadowSize = (overlay.shadow_size ?? 0) * scale;
  return (
    <div className={`video-overlay-anchor pos-${overlay.position}`}>
      <span
        className="video-overlay-text"
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

function hexWithOpacity(hex, opacityPercent) {
  const alpha = Math.round(((opacityPercent ?? 100) / 100) * 255)
    .toString(16)
    .padStart(2, "0");
  return `${hex}${alpha}`;
}

// Cascade mode's rolling 2-line window: the just-finished phrase shrinks
// to `text_color` above; the phrase being spoken now is shown at full
// size in `text_color`, except the exact word being spoken *right now*
// pops to `accent_color` and slightly larger — a moving spotlight, not a
// whole-phrase color swap. Mirrors build_cascade_text in captions.rs.
function CascadeCaption({ style, scale, current, prev, currentTime }) {
  const prevText = prev ? prev.map((w) => w.word).join(" ") : "";
  const baseFontSize = Math.max(8, style.font_size * scale);
  const prevFontSize = Math.max(8, style.font_size * scale * CASCADE_PREV_SCALE);
  const activeFontSize = Math.max(8, style.font_size * scale * CASCADE_ACTIVE_SCALE);
  const strokeWidth = Math.max(1, scale * 3);
  const textTransform = style.text_transform && style.text_transform !== "none" ? style.text_transform : "none";

  return (
    <div className={`video-overlay-anchor pos-${style.position}`}>
      <div className="video-cascade-caption" style={{ textTransform }}>
        {prevText && (
          <span
            className="video-cascade-line"
            style={{
              fontFamily: style.font_family,
              fontSize: `${prevFontSize}px`,
              fontWeight: style.bold ? "bold" : "normal",
              fontStyle: style.italic ? "italic" : "normal",
              color: style.text_color,
              WebkitTextStroke: `${strokeWidth}px ${style.outline_color}`,
            }}
          >
            {prevText}
          </span>
        )}
        <span className="video-cascade-line">
          {current.map((w, i) => {
            const isActive = currentTime >= w.start && currentTime < w.end;
            return (
              <span
                key={i}
                style={{
                  display: "inline-block",
                  fontFamily: style.font_family,
                  fontSize: `${isActive ? activeFontSize : baseFontSize}px`,
                  fontWeight: style.bold ? "bold" : "normal",
                  fontStyle: style.italic ? "italic" : "normal",
                  color: isActive ? style.accent_color : style.text_color,
                  WebkitTextStroke: `${strokeWidth}px ${style.outline_color}`,
                }}
              >
                {w.word}
                {i < current.length - 1 ? " " : ""}
              </span>
            );
          })}
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
  onTimeUpdate,
  onLoadedMetadata,
  onSeek,
  voiceoverPath,
  voiceoverOffset,
}) {
  const [displayWidth, setDisplayWidth] = useState(0);
  const src = convertFileSrc(videoPath);
  const voiceoverAudioRef = useRef(null);
  const videoFrameRef = useRef(null);
  const [isFullscreen, setIsFullscreen] = useState(false);

  // The caption overlay is a sibling <div> of <video>, not a child of it --
  // so the video element's own native fullscreen button (part of its
  // `controls` UI) excludes the overlay entirely; a fullscreened <video>
  // only ever shows the video's own pixels plus its native control bar,
  // never other page DOM. Fullscreening `.video-frame` (the wrapper around
  // both) instead is the only way to keep captions visible in fullscreen --
  // confirmed the hard way: a user reported captions "not reflecting" an
  // edit that was actually right there in the (non-fullscreen) preview the
  // whole time, and it turned out they'd gone fullscreen via the video's
  // own button to read small captions more easily and found none at all.
  useEffect(() => {
    function handleFullscreenChange() {
      setIsFullscreen(document.fullscreenElement === videoFrameRef.current);
    }
    document.addEventListener("fullscreenchange", handleFullscreenChange);
    return () => document.removeEventListener("fullscreenchange", handleFullscreenChange);
  }, []);

  function toggleFullscreen() {
    if (document.fullscreenElement) {
      document.exitFullscreen();
    } else {
      videoFrameRef.current?.requestFullscreen();
    }
  }

  useEffect(() => {
    function measure() {
      if (videoRef.current) setDisplayWidth(videoRef.current.clientWidth);
    }
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [videoRef, videoPath]);

  // React only applies the `muted` JSX prop on a <video>/<audio> element at
  // initial mount, not on later re-renders (a documented React special-case
  // for media elements) -- so toggling `muted={!!voiceoverPath}` alone
  // never actually mutes the video once a voiceover is generated after the
  // video element already exists, which is the normal order of events here
  // (load video first, generate voiceover later). Confirmed directly: the
  // original audio kept playing underneath the voiceover in the live
  // preview even though `muted` read `true` in the JSX. Setting the DOM
  // property imperatively fixes it; the JSX attribute stays too since it
  // still correctly covers the (rarer) case of a voiceover already being
  // active when the video element first mounts.
  useEffect(() => {
    if (videoRef.current) videoRef.current.muted = !!voiceoverPath;
  }, [videoRef, voiceoverPath]);

  // Keeps the hidden voiceover <audio> element in lockstep with the video
  // element's own playback (offset by voiceoverOffset, the timing offset
  // vo_sync.py's mouth-movement correlation found) -- this plays the
  // generated voiceover directly alongside the loaded video, rather than
  // needing a separately-merged output file just to preview it. Called
  // from every timeupdate/play/pause/seeked event below, so it doubles as
  // both the initial sync and ongoing drift correction (browser audio and
  // video elements can drift apart independently over long playback).
  //
  // Sign convention matches vo_sync.py's own doc comment exactly: a
  // *positive* offset means the voice-over should start *later* --
  // mirroring how vo_sync.py's `mux()` applies it via ffmpeg's
  // `-itsoffset <offset>` on the voiceover input, which shifts the
  // voiceover's own timestamp-0 to appear at time `offset` in the merged
  // output. So the voiceover position that belongs at video time t is
  // `t - offset`, not `t + offset` -- confirmed the hard way (a real bug,
  // not just a hypothetical): the `+` version was live for one turn and
  // produced exactly the symptom its inverted math predicts -- a negative
  // offset read as "not started yet" for the first |offset| seconds, so
  // the voiceover appeared to wait until partway through the video before
  // starting.
  //
  // A *negative* offset ("advance" the voice-over) is clamped to 0 here,
  // deliberately not honored in full -- confirmed directly (round-tripped
  // a generated file through this app's own ASR) that the audio itself
  // always starts at 0:00 with the full script intact, but a negative
  // offset makes this function jump audio.currentTime straight to
  // |offset| seconds in at video-start, which *skips real content* the
  // user typed rather than just being imperfectly timed. That's an
  // acceptable tradeoff for vo_sync.py's actual mux (genuinely advancing
  // a real recorded voice-over that runs ahead of the video), but a bad
  // one for this live preview specifically, and especially likely here:
  // an unrelated video's mouth movement correlated against freshly
  // generated narration has no real timing relationship to find, so a
  // spurious negative offset is a realistic result, not an edge case.
  // Losing typed content is worse than imperfect sync, so this preview
  // only ever delays playback, never truncates the beginning.
  function syncVoiceoverToVideo(videoTime) {
    const audio = voiceoverAudioRef.current;
    if (!audio || !voiceoverPath) return;
    const desired = videoTime - Math.max(0, voiceoverOffset || 0);
    const inBounds = desired >= 0 && (!audio.duration || desired <= audio.duration);
    if (!inBounds) {
      if (!audio.paused) audio.pause();
      return;
    }
    if (Math.abs(audio.currentTime - desired) > 0.2) {
      audio.currentTime = desired;
    }
    const videoEl = videoRef.current;
    const videoPlaying = videoEl && !videoEl.paused && !videoEl.ended;
    if (videoPlaying && audio.paused) {
      audio.play().catch(() => {});
    } else if (!videoPlaying && !audio.paused) {
      audio.pause();
    }
  }

  const scale = displayWidth > 0 ? displayWidth / ASS_PLAY_RES_WIDTH : 0;

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
      <div className="video-frame" ref={videoFrameRef}>
        <video
          ref={videoRef}
          src={src}
          controls
          muted={!!voiceoverPath}
          onTimeUpdate={(e) => {
            const t = e.currentTarget.currentTime;
            onTimeUpdate(t);
            syncVoiceoverToVideo(t);
          }}
          onPlay={(e) => syncVoiceoverToVideo(e.currentTarget.currentTime)}
          onPause={() => voiceoverAudioRef.current?.pause()}
          onSeeked={(e) => syncVoiceoverToVideo(e.currentTarget.currentTime)}
          onLoadedMetadata={(e) => {
            onLoadedMetadata(e.currentTarget.duration);
            setDisplayWidth(e.currentTarget.clientWidth);
          }}
          className="video-preview-player"
        />
        {voiceoverPath && <audio ref={voiceoverAudioRef} src={convertFileSrc(voiceoverPath)} style={{ display: "none" }} />}
        {scale > 0 && (
          <div className="video-overlay-layer">
            {activeCaption && <OverlayText overlay={activeCaption} scale={scale} />}
            {activeCascade && (
              <CascadeCaption
                style={captionStyle}
                scale={scale}
                current={activeCascade.current}
                prev={activeCascade.prev}
                currentTime={currentTime}
              />
            )}
          </div>
        )}
        <button
          type="button"
          className="video-fullscreen-button"
          onClick={toggleFullscreen}
          title={isFullscreen ? "Exit fullscreen" : "Fullscreen (includes captions, unlike the player's own button)"}
        >
          {isFullscreen ? "⤡" : "⤢"}
        </button>
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
      </div>

      <p className="video-time-label">
        {formatTime(currentTime)} / {formatTime(duration)}
      </p>
    </div>
  );
}

export default VideoPreview;
