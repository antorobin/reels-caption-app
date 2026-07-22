import { useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import CaptionStyleEditor, { defaultCaptionStyle } from "./components/CaptionStyleEditor.jsx";
import CustomTextOverlayEditor from "./components/CustomTextOverlayEditor.jsx";
import ProgressBar, { formatElapsed } from "./components/ProgressBar.jsx";
import SilenceRemovalPanel from "./components/SilenceRemovalPanel.jsx";
import TranscriptEditor from "./components/TranscriptEditor.jsx";
import VideoPreview from "./components/VideoPreview.jsx";
import { estimateReadingDurationSeconds } from "./lib/reading-time.js";

const VIDEO_FILTERS = [{ name: "Video", extensions: ["mp4", "mov", "mkv", "avi", "webm"] }];

function newOverlayId() {
  return typeof crypto !== "undefined" && crypto.randomUUID ? crypto.randomUUID() : `overlay-${Date.now()}-${Math.random()}`;
}

function defaultDraftOverlay() {
  return {
    text: "",
    duration_secs: estimateReadingDurationSeconds(""),
    duration_touched: false,
    position: "top",
    font_family: "Impact",
    font_size: 72,
    text_color: "#FFFF00",
    outline_color: "#000000",
    animation: "pop",
    bold: true,
    italic: false,
    letter_spacing: 0,
    text_transform: "none",
    background: "none",
    background_color: "#000000",
    background_opacity: 70,
    shadow_size: 0,
  };
}

function App() {
  const [greeting, setGreeting] = useState("");
  const [ffmpegVersion, setFfmpegVersion] = useState("");
  const [whisperStatus, setWhisperStatus] = useState("");
  const [checking, setChecking] = useState(false);

  const [videoPath, setVideoPath] = useState("");
  const videoRef = useRef(null);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);

  const [customOverlays, setCustomOverlays] = useState([]);
  const [draftOverlay, setDraftOverlay] = useState(defaultDraftOverlay());

  const [words, setWords] = useState([]);
  const [pipelineStatus, setPipelineStatus] = useState("");
  const [pipelineRunning, setPipelineRunning] = useState(false);
  const [pipelineProgress, setPipelineProgress] = useState(null);

  const [captionStyle, setCaptionStyle] = useState(defaultCaptionStyle());

  const [burnStatus, setBurnStatus] = useState("");
  const [burning, setBurning] = useState(false);
  const [burnProgress, setBurnProgress] = useState(null);

  async function sayHello() {
    const result = await invoke("greet", { name: "Reels Creator" });
    setGreeting(result);
  }

  async function checkFfmpeg() {
    setChecking(true);
    try {
      setFfmpegVersion(await invoke("check_ffmpeg"));
    } catch (err) {
      setFfmpegVersion(`Error: ${err}`);
    } finally {
      setChecking(false);
    }
  }

  async function checkWhisper() {
    setChecking(true);
    try {
      setWhisperStatus(await invoke("check_whisper"));
    } catch (err) {
      setWhisperStatus(`Error: ${err}`);
    } finally {
      setChecking(false);
    }
  }

  async function pickVideo() {
    const selected = await open({ multiple: false, filters: VIDEO_FILTERS });
    if (typeof selected === "string") {
      setVideoPath(selected);
      setWords([]);
      setPipelineStatus("");
      setBurnStatus("");
      setCustomOverlays([]);
      setDraftOverlay(defaultDraftOverlay());
      setCurrentTime(0);
      setDuration(0);
    }
  }

  function handleSeek(t) {
    if (videoRef.current) {
      videoRef.current.currentTime = t;
    }
    setCurrentTime(t);
  }

  // Draft text/duration/end-time are kept in sync: typing text auto-suggests
  // a reading-comfortable duration (until the user overrides duration or
  // end-time directly, at which point their choice sticks even as the text
  // keeps changing).
  function handleDraftTextChange(text) {
    setDraftOverlay((prev) => ({
      ...prev,
      text,
      duration_secs: prev.duration_touched ? prev.duration_secs : estimateReadingDurationSeconds(text),
    }));
  }

  function handleDraftFieldChange(key, value) {
    setDraftOverlay((prev) => ({ ...prev, [key]: value }));
  }

  function handleDraftDurationChange(durationSecs) {
    setDraftOverlay((prev) => ({ ...prev, duration_secs: Math.max(0.1, durationSecs), duration_touched: true }));
  }

  function handleDraftEndTimeChange(endTime) {
    setDraftOverlay((prev) => ({
      ...prev,
      duration_secs: Math.max(0.1, endTime - currentTime),
      duration_touched: true,
    }));
  }

  function handleAddOverlay() {
    const trimmed = draftOverlay.text.trim();
    if (!trimmed) return;
    const start = currentTime;
    const end = duration > 0 ? Math.min(start + draftOverlay.duration_secs, duration) : start + draftOverlay.duration_secs;
    // Draft carries a couple of UI-only fields (duration_secs, duration_touched)
    // that aren't part of the burned overlay's shape — drop those, keep
    // everything else (style/animation fields) as-is.
    const { duration_secs, duration_touched, text, ...styleFields } = draftOverlay;
    setCustomOverlays((prev) => [...prev, { id: newOverlayId(), text: trimmed, start, end, ...styleFields }]);
    // Clear the text and re-suggest a fresh duration, but keep the style
    // choices — adding several similarly-styled overlays in a row is common.
    setDraftOverlay((prev) => ({ ...prev, text: "", duration_secs: estimateReadingDurationSeconds(""), duration_touched: false }));
  }

  function handleRemoveOverlay(id) {
    setCustomOverlays((prev) => prev.filter((o) => o.id !== id));
  }

  function handleWordChange(index, text) {
    setWords((prev) => prev.map((w, i) => (i === index ? { ...w, word: text } : w)));
  }

  function handleWordDelete(index) {
    setWords((prev) => prev.filter((_, i) => i !== index));
  }

  async function runPipeline() {
    if (!videoPath) return;
    setPipelineRunning(true);
    setPipelineStatus("");
    setPipelineProgress(null);
    const startedAt = Date.now();
    const unlisten = await listen("pipeline-progress", (event) => setPipelineProgress(event.payload));
    try {
      const result = await invoke("run_pipeline", { videoPath });
      const elapsed = formatElapsed((Date.now() - startedAt) / 1000);
      setWords(result.words);
      setPipelineStatus(`Transcribed ${result.words.length} words in ${elapsed}.`);
    } catch (err) {
      setPipelineStatus(`Error: ${err}`);
    } finally {
      unlisten();
      setPipelineRunning(false);
      setPipelineProgress(null);
    }
  }

  function handleJumpCutApplied(result) {
    // The timeline changed, so anything timed against the old one
    // (custom overlays) can no longer be trusted — clear them rather
    // than silently leave them pointing at the wrong moments. Video
    // preview picks up the new file automatically via the videoPath prop.
    setVideoPath(result.output_path);
    setWords(result.words);
    setCustomOverlays([]);
    setDraftOverlay(defaultDraftOverlay());
    setCurrentTime(0);
    setDuration(0);
  }

  async function burnCaptions() {
    if (!videoPath || (words.length === 0 && customOverlays.length === 0)) return;
    const outputPath = await save({ defaultPath: "captioned-output.mp4", filters: VIDEO_FILTERS });
    if (!outputPath) return;

    setBurning(true);
    setBurnStatus("");
    setBurnProgress(null);
    const startedAt = Date.now();
    const unlisten = await listen("burn-progress", (event) => setBurnProgress(event.payload));
    try {
      const result = await invoke("burn_captions", {
        videoPath,
        words,
        style: captionStyle,
        customOverlays,
        outputPath,
      });
      const elapsed = formatElapsed((Date.now() - startedAt) / 1000);
      setBurnStatus(`Saved captioned video to ${result} in ${elapsed}.`);
    } catch (err) {
      setBurnStatus(`Error: ${err}`);
    } finally {
      unlisten();
      setBurning(false);
      setBurnProgress(null);
    }
  }

  return (
    <div className="container">
      <h1>🎬 Reels Caption App</h1>
      <p className="subtitle">Local-first: Tauri + Rust + React + ffmpeg + whisper.cpp</p>

      <section className="card">
        <h2>1. Rust ↔ React bridge</h2>
        <button onClick={sayHello}>Say hello from Rust</button>
        {greeting && <p className="result">{greeting}</p>}
      </section>

      <section className="card">
        <h2>2. Check ffmpeg (video engine)</h2>
        <button onClick={checkFfmpeg} disabled={checking}>
          Check ffmpeg version
        </button>
        {ffmpegVersion && <pre className="result">{ffmpegVersion}</pre>}
      </section>

      <section className="card">
        <h2>3. Check whisper.cpp (transcription engine)</h2>
        <button onClick={checkWhisper} disabled={checking}>
          Check whisper.cpp binary
        </button>
        {whisperStatus && <pre className="result">{whisperStatus}</pre>}
      </section>

      <section className="card">
        <h2>4. Select a video</h2>
        <button onClick={pickVideo}>Choose video file…</button>
        {videoPath && <p className="result">{videoPath}</p>}
      </section>

      <section className="card">
        <h2>5. Transcribe (extract audio → whisper.cpp)</h2>
        <button onClick={runPipeline} disabled={!videoPath || pipelineRunning}>
          {pipelineRunning ? "Transcribing…" : "Run transcription pipeline"}
        </button>
        <ProgressBar progress={pipelineProgress} />
        {pipelineStatus && <pre className="result">{pipelineStatus}</pre>}
      </section>

      {words.length > 0 && (
        <section className="card">
          <h2>6. Remove silence &amp; filler words (optional)</h2>
          <p className="section-hint">
            Cuts dead air and "um"/"uh" automatically, using the transcript timing you already have — no re-transcription
            needed. Runs once; re-adding custom text after this keeps it in sync with the trimmed video.
          </p>
          <SilenceRemovalPanel videoPath={videoPath} words={words} onApplied={handleJumpCutApplied} />
        </section>
      )}

      {videoPath && (
        <section className="card">
          <h2>7. Preview, edit transcript &amp; add custom text</h2>
          <VideoPreview
            videoRef={videoRef}
            videoPath={videoPath}
            currentTime={currentTime}
            duration={duration}
            words={words}
            captionStyle={captionStyle}
            overlays={customOverlays}
            draftOverlay={draftOverlay}
            onTimeUpdate={setCurrentTime}
            onLoadedMetadata={setDuration}
            onSeek={handleSeek}
          />
          <TranscriptEditor
            words={words}
            currentTime={currentTime}
            onWordChange={handleWordChange}
            onWordDelete={handleWordDelete}
            onSeek={handleSeek}
          />
          <CustomTextOverlayEditor
            currentTime={currentTime}
            draft={draftOverlay}
            onDraftTextChange={handleDraftTextChange}
            onDraftFieldChange={handleDraftFieldChange}
            onDraftDurationChange={handleDraftDurationChange}
            onDraftEndTimeChange={handleDraftEndTimeChange}
            overlays={customOverlays}
            onAdd={handleAddOverlay}
            onRemove={handleRemoveOverlay}
            onSeek={handleSeek}
          />
        </section>
      )}

      <section className="card">
        <h2>8. Style your transcript captions</h2>
        <CaptionStyleEditor style={captionStyle} onChange={setCaptionStyle} />
      </section>

      <section className="card">
        <h2>9. Burn captions &amp; custom text onto the video (ffmpeg)</h2>
        <button onClick={burnCaptions} disabled={(words.length === 0 && customOverlays.length === 0) || burning}>
          {burning ? "Burning…" : "Burn & save video"}
        </button>
        <ProgressBar progress={burnProgress} />
        {burnStatus && <pre className="result">{burnStatus}</pre>}
      </section>

      <footer>
        <p>Runs fully offline. No cloud calls, no per-user inference cost.</p>
      </footer>
    </div>
  );
}

export default App;
