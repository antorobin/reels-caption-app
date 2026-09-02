import { useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { buildChunkTimeline, buildStyleTimeline, findActiveChunkEntry } from "../lib/captions.js";
import { formatTime } from "../lib/time.js";

// Caption font sizes are authored in ASS's PlayResX=1920 coordinate space
// (see captions.rs). Scale them down to whatever width the video is
// actually rendered at in the DOM so the preview roughly matches the
// eventual burned-in size.
const ASS_PLAY_RES_WIDTH = 1920;

// Mirrors CASCADE_PREV_SCALE / CASCADE_ACTIVE_SCALE in captions.rs.
const CASCADE_PREV_SCALE = 0.6;
const CASCADE_ACTIVE_SCALE = 1.15;

// Mirrors ducking.rs's own constants exactly -- this recomputes the same
// duck curve in JS so the live preview sounds like a real preview of what
// "Add music with ducking" will actually export, not an approximation.
const DUCK_SPEECH_MERGE_GAP_SECONDS = 0.6;
const DUCK_RAMP_SECONDS = 0.3;

// Mirrors ducking.rs's `speech_windows_from_words` -- coalesces consecutive
// words into merged speech spans, so the music doesn't duck and un-duck
// between every single word.
function speechWindowsFromWords(words) {
  const windows = [];
  for (const w of words) {
    const last = windows[windows.length - 1];
    if (last && w.start - last[1] <= DUCK_SPEECH_MERGE_GAP_SECONDS) {
      last[1] = Math.max(last[1], w.end);
      continue;
    }
    windows.push([w.start, w.end]);
  }
  return windows;
}

// Mirrors ducking.rs's `build_ducking_volume_expr` -- 1.0 (full volume)
// outside every speech window, ramping down to `duckLevel` over
// DUCK_RAMP_SECONDS at each edge, held at `duckLevel` for the window's
// duration. Evaluated directly at one point in time (there's no need to
// build an expression string here, unlike the ffmpeg side) each time the
// video's playback position changes.
function duckVolumeAt(t, speechWindows, duckLevel) {
  for (const [start, end] of speechWindows) {
    if (t >= start && t <= end) return duckLevel;
    if (t >= start - DUCK_RAMP_SECONDS && t < start) {
      const frac = (t - (start - DUCK_RAMP_SECONDS)) / DUCK_RAMP_SECONDS;
      return 1 - frac * (1 - duckLevel);
    }
    if (t > end && t <= end + DUCK_RAMP_SECONDS) {
      const frac = (t - end) / DUCK_RAMP_SECONDS;
      return duckLevel + frac * (1 - duckLevel);
    }
  }
  return 1;
}

// Mirrors KARAOKE_SECONDARY_COLOUR in captions.rs — the muted "not yet
// spoken" gray both "karaoke" and "highlight" transition *from*, right up
// until each word's own start time.
const KARAOKE_PRE_WORD_COLOR = "#808080";

// One-shot entrance animation for a whole-phrase animation, mirroring
// classicPreviewAnimationClass in CaptionStyleEditor.jsx (same mapping),
// but the actual video-preview classes are single-play (see styles.css)
// rather than the picker swatch's always-looping ones — a caption chunk
// animates in once, then holds, matching the real ASS burn.
function videoOverlayAnimationClass(animation, position) {
  switch (animation) {
    case "fade":
      return "video-overlay-anim-fade";
    case "pop":
      return "video-overlay-anim-pop";
    case "bounce":
      return "video-overlay-anim-bounce";
    case "zoom":
      return "video-overlay-anim-zoom";
    case "slide":
      return position === "top" ? "video-overlay-anim-slide-down" : "video-overlay-anim-slide-up";
    default:
      return "";
  }
}

// "karaoke" (continuous \k fill) and "highlight" (discrete \c pop) render
// to the *same* visual result in this app — both switch a word from
// KARAOKE_PRE_WORD_COLOR to the style's own text_color right at that
// word's own start time and hold it there (see captions.rs's
// build_animated_text and KARAOKE_SECONDARY_COLOUR's doc comment for the
// real libass behavior this mirrors) — so one function covers both.
function KaraokeWords({ overlay, currentTime }) {
  return overlay.words.map((w, i) => (
    <span key={i} className="video-overlay-word" style={{ color: currentTime >= w.start ? overlay.text_color : KARAOKE_PRE_WORD_COLOR }}>
      {w.word}
      {i < overlay.words.length - 1 ? " " : ""}
    </span>
  ));
}

// Characters reveal one at a time across each word's own [start, end)
// span — mirrors captions.rs's "typewriter" branch exactly (equal-width
// steps per character, not a flat per-word cutoff).
function TypewriterWords({ overlay, currentTime }) {
  return overlay.words.map((w, i) => {
    const chars = Array.from(w.word);
    const span = Math.max(0.01, w.end - w.start);
    const revealedFraction = Math.max(0, Math.min(1, (currentTime - w.start) / span));
    const revealedCount = currentTime < w.start ? 0 : Math.ceil(revealedFraction * chars.length);
    return (
      <span key={i} className="video-overlay-word">
        {chars.slice(0, revealedCount).join("")}
        {i < overlay.words.length - 1 && revealedCount === chars.length ? " " : ""}
      </span>
    );
  });
}

function OverlayText({ overlay, scale, currentTime }) {
  const fontSize = Math.max(8, overlay.font_size * scale);
  const strokeWidth = Math.max(1, scale * 3);
  const shadowSize = (overlay.shadow_size ?? 0) * scale;
  const animation = overlay.animation ?? "none";
  const isPerWord = animation === "karaoke" || animation === "highlight";
  const isTypewriter = animation === "typewriter";
  const animationClass = !isPerWord && !isTypewriter ? videoOverlayAnimationClass(animation, overlay.position) : "";
  // Re-keyed to this chunk's own start time (below, where OverlayText is
  // rendered) so a fresh element mounts per caption phrase — required for
  // the one-shot CSS animation classes above to actually replay instead of
  // only ever running once on first mount.
  return (
    <div className={`video-overlay-anchor pos-${overlay.position}`}>
      <span
        className={`video-overlay-text ${animationClass}`}
        style={{
          fontFamily: overlay.font_family,
          fontSize: `${fontSize}px`,
          fontWeight: overlay.bold ? "bold" : "normal",
          fontStyle: overlay.italic ? "italic" : "normal",
          letterSpacing: `${(overlay.letter_spacing ?? 0) * scale}px`,
          textTransform: overlay.text_transform && overlay.text_transform !== "none" ? overlay.text_transform : "none",
          color: isPerWord ? undefined : overlay.text_color,
          WebkitTextStroke: `${strokeWidth}px ${overlay.outline_color}`,
          textShadow: shadowSize > 0 ? `${shadowSize}px ${shadowSize}px 4px rgba(0,0,0,0.8)` : "none",
          backgroundColor:
            overlay.background === "box" ? hexWithOpacity(overlay.background_color, overlay.background_opacity) : "transparent",
          padding: overlay.background === "box" ? "0.15em 0.4em" : 0,
          borderRadius: overlay.background === "box" ? "4px" : 0,
        }}
      >
        {isPerWord && <KaraokeWords overlay={overlay} currentTime={currentTime} />}
        {isTypewriter && <TypewriterWords overlay={overlay} currentTime={currentTime} />}
        {!isPerWord && !isTypewriter && overlay.text}
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
  captionStyleOverrides = [],
  onTimeUpdate,
  onLoadedMetadata,
  onSeek,
  voiceoverPath,
  voiceoverOffset,
  musicPath,
  duckLevel,
}) {
  const [displayWidth, setDisplayWidth] = useState(0);
  const src = convertFileSrc(videoPath);
  const voiceoverAudioRef = useRef(null);
  const musicAudioRef = useRef(null);
  const videoFrameRef = useRef(null);
  const [isFullscreen, setIsFullscreen] = useState(false);

  // Recomputed only when the transcript actually changes, not on every
  // timeupdate tick -- `duckVolumeAt` below is called far more often than
  // that.
  const speechWindows = useMemo(() => speechWindowsFromWords(words || []), [words]);

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

  // This one-time set above only holds until something *else* changes the
  // video element's mute state afterward -- and `controls` (kept for
  // scrubbing/fullscreen) gives the browser's own native player chrome a
  // volume/mute button that can do exactly that, completely outside React.
  // A user hearing what looks like a silent video reaching for that button
  // to "fix" it would un-mute the *original* audio right back on, playing
  // it alongside the voiceover -- reported directly as "the preview played
  // both audios" after a voiceover was already active. `volumechange` fires
  // for every route to that state (the native button, a dragged volume
  // slider, the OS media-key mute, ...), so re-asserting muted there closes
  // all of them at once rather than special-casing the native control.
  function handleVolumeChange(e) {
    if (voiceoverPath && !e.currentTarget.muted) {
      e.currentTarget.muted = true;
    }
  }

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

  // Keeps the hidden background-music <audio> element in lockstep with the
  // video the same way syncVoiceoverToVideo does, with two differences: no
  // offset (a music bed always starts at the video's own time 0, unlike a
  // voiceover that might need a mouth-movement-correlated delay), and its
  // `.volume` is set every call to `duckVolumeAt(videoTime, ...)` -- this
  // is what actually makes it a *preview* of "Add music with ducking"
  // below, not just background music playing at a flat level alongside the
  // original audio. The video's own audio is deliberately left untouched
  // here (unlike voiceoverPath's muting) -- ducking lowers the music under
  // the *existing* audio, it doesn't replace it.
  function syncMusicToVideo(videoTime) {
    const audio = musicAudioRef.current;
    if (!audio || !musicPath) return;
    if (Math.abs(audio.currentTime - videoTime) > 0.2) {
      audio.currentTime = videoTime;
    }
    audio.volume = duckVolumeAt(videoTime, speechWindows, duckLevel ?? 0.3);
    const videoEl = videoRef.current;
    const videoPlaying = videoEl && !videoEl.paused && !videoEl.ended;
    if (videoPlaying && audio.paused) {
      audio.play().catch(() => {});
    } else if (!videoPlaying && !audio.paused) {
      audio.pause();
    }
  }

  // Applies a dragged duck-amount slider immediately, even while paused --
  // otherwise `syncMusicToVideo` above only re-runs on an actual playback
  // event (timeupdate/play/seeked), so tuning the slider before pressing
  // play wouldn't audibly change anything until the video actually moves.
  useEffect(() => {
    if (videoRef.current) syncMusicToVideo(videoRef.current.currentTime);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only care about a duck-level change here, not every render
  }, [duckLevel]);

  const scale = displayWidth > 0 ? displayWidth / ASS_PLAY_RES_WIDTH : 0;

  // Range-aware: resolves which style (base or an override) applies at
  // `currentTime`, and chunks each style's own portion of the transcript
  // independently with that style's own words_per_line -- mirrors
  // build_style_timeline/build_chunk_timeline in captions.rs exactly, so
  // this preview can never show a different chunk boundary or style than
  // what the actual burn produces (see captionStyleOverrides' own doc
  // comment in projectStore.js for why that guarantee matters here).
  const stylePieces = useMemo(
    () => (words && words.length > 0 ? buildStyleTimeline(captionStyleOverrides, captionStyle) : []),
    [words, captionStyleOverrides, captionStyle]
  );
  const chunkTimeline = useMemo(() => (words && words.length > 0 ? buildChunkTimeline(words, stylePieces) : []), [words, stylePieces]);
  const activeEntry = findActiveChunkEntry(chunkTimeline, currentTime);
  // No chunk active right now (a gap between phrases) -- fall back to the
  // base style so nothing downstream has to null-check `activeStyle`
  // itself, only whether a chunk/caption is actually showing.
  const activeStyle = activeEntry ? activeEntry.style : captionStyle;
  const isCascade = activeStyle.style_mode === "cascade";

  const activeChunk = !isCascade && activeEntry ? activeEntry.chunk : null;
  const activeCaption = activeChunk
    ? {
        text: activeChunk.map((w) => w.word).join(" "),
        words: activeChunk,
        animation: activeStyle.animation,
        position: activeStyle.position,
        font_family: activeStyle.font_family,
        font_size: activeStyle.font_size,
        text_color: activeStyle.text_color,
        outline_color: activeStyle.outline_color,
        bold: activeStyle.bold,
        italic: activeStyle.italic,
        letter_spacing: activeStyle.letter_spacing,
        text_transform: activeStyle.text_transform,
        background: activeStyle.background,
        background_color: activeStyle.background_color,
        background_opacity: activeStyle.background_opacity,
        shadow_size: activeStyle.shadow_size,
      }
    : null;

  const activeCascade = isCascade && activeEntry ? { current: activeEntry.chunk, prev: activeEntry.prev } : null;

  return (
    <div className="video-preview">
      <div className="video-frame" ref={videoFrameRef}>
        <video
          ref={videoRef}
          src={src}
          controls
          muted={!!voiceoverPath}
          onVolumeChange={handleVolumeChange}
          onTimeUpdate={(e) => {
            const t = e.currentTarget.currentTime;
            onTimeUpdate(t);
            syncVoiceoverToVideo(t);
            syncMusicToVideo(t);
          }}
          onPlay={(e) => {
            syncVoiceoverToVideo(e.currentTarget.currentTime);
            syncMusicToVideo(e.currentTarget.currentTime);
          }}
          onPause={() => {
            voiceoverAudioRef.current?.pause();
            musicAudioRef.current?.pause();
          }}
          onSeeked={(e) => {
            syncVoiceoverToVideo(e.currentTarget.currentTime);
            syncMusicToVideo(e.currentTarget.currentTime);
          }}
          onLoadedMetadata={(e) => {
            onLoadedMetadata(e.currentTarget.duration);
            setDisplayWidth(e.currentTarget.clientWidth);
          }}
          className="video-preview-player"
        />
        {voiceoverPath && <audio ref={voiceoverAudioRef} src={convertFileSrc(voiceoverPath)} style={{ display: "none" }} />}
        {/* Deliberately no `loop` here -- a picked file shorter than the video just goes
            silent for the remainder in the real export too (`amix=duration=first` in
            ducking.rs doesn't loop a short bed), and this preview is meant to match that,
            not be more forgiving than what "Add music with ducking" will actually produce.
            A generated bed never hits this anyway -- music_gen.rs already loops it to the
            video's full length before handing back a path. */}
        {musicPath && <audio ref={musicAudioRef} src={convertFileSrc(musicPath)} style={{ display: "none" }} />}
        {scale > 0 && (
          <div className="video-overlay-layer">
            {activeCaption && (
              // Keyed to this chunk's own start time so a fresh element
              // mounts per caption phrase -- required for the one-shot
              // entrance animation classes (videoOverlayAnimationClass) to
              // actually replay each time, instead of only ever playing
              // once on first mount.
              <OverlayText key={activeChunk[0].start} overlay={activeCaption} scale={scale} currentTime={currentTime} />
            )}
            {activeCascade && (
              <CascadeCaption
                style={activeStyle}
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
