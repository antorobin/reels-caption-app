import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import AuthScreen from "./components/auth/AuthScreen.jsx";
import { defaultCaptionStyle } from "./components/CaptionStyleEditor.jsx";
import { formatElapsed } from "./components/ProgressBar.jsx";
import AppShell from "./components/shell/AppShell.jsx";
import { useAuth } from "./context/AuthContext.jsx";

const VIDEO_FILTERS = [{ name: "Video", extensions: ["mp4", "mov", "mkv", "avi", "webm"] }];

function App() {
  const [videoPath, setVideoPath] = useState("");
  const videoRef = useRef(null);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);

  const [words, setWords] = useState([]);
  const [pipelineStatus, setPipelineStatus] = useState("");
  const [pipelineRunning, setPipelineRunning] = useState(false);
  const [pipelineProgress, setPipelineProgress] = useState(null);
  const [detectedLanguage, setDetectedLanguage] = useState(null);

  const [captionStyle, setCaptionStyle] = useState(defaultCaptionStyle());

  const [burnStatus, setBurnStatus] = useState("");
  const [burning, setBurning] = useState(false);
  const [burnProgress, setBurnProgress] = useState(null);

  const [prosody, setProsody] = useState([]);
  const [analyzingProsody, setAnalyzingProsody] = useState(false);
  const [prosodyStatus, setProsodyStatus] = useState("");
  const [prosodyProgress, setProsodyProgress] = useState(null);

  const [speakers, setSpeakers] = useState([]);
  const [analyzingSpeakers, setAnalyzingSpeakers] = useState(false);
  const [diarizeStatus, setDiarizeStatus] = useState("");
  const [diarizeProgress, setDiarizeProgress] = useState(null);

  // The active generated voiceover, if any -- lifted up here (rather than
  // kept local to VoiceoverSection) because VideoPreview needs it too, to
  // play it alongside the loaded video directly (see VideoPreview.jsx),
  // without writing a separately-merged file just to preview it.
  const [voiceoverPath, setVoiceoverPath] = useState("");
  const [voiceoverOffset, setVoiceoverOffset] = useState(0);

  const { user, loading: authLoading } = useAuth();

  // Splashscreen shows natively the instant the process starts (see
  // tauri.conf.json) so there's no blank window while the webview spins
  // up. A short minimum delay here keeps it from flashing away instantly
  // on fast machines — this app mounts in well under that.
  useEffect(() => {
    const timer = setTimeout(() => {
      invoke("close_splashscreen").catch(() => {});
    }, 400);
    return () => clearTimeout(timer);
  }, []);

  async function pickVideo() {
    const selected = await open({ multiple: false, filters: VIDEO_FILTERS });
    if (typeof selected === "string") {
      setVideoPath(selected);
      setWords([]);
      setPipelineStatus("");
      setBurnStatus("");
      setProsody([]);
      setProsodyStatus("");
      setSpeakers([]);
      setDiarizeStatus("");
      setVoiceoverPath("");
      setVoiceoverOffset(0);
      setCurrentTime(0);
      setDuration(0);
      // Auto-starts the moment a video is chosen -- no separate "Run
      // pipeline" step. Passed the path directly rather than relying on
      // the videoPath state var, since setVideoPath above hasn't
      // committed yet in this same render cycle.
      runPipelineFor(selected);
    }
  }

  function handleVoiceoverReady(path, offsetSeconds, voiceoverWords) {
    const offset = offsetSeconds ?? 0;
    setVoiceoverPath(path);
    setVoiceoverOffset(offset);

    // The voiceover replaces the video's own audio, so its transcript
    // (real word-level timestamps from re-transcribing it -- see
    // VoiceoverSection.jsx) becomes the working transcript too: what
    // Burn & Export burns, and what the Transcript panel/Timeline show.
    // Shifted by `offset` so timestamps land where the voiceover audio
    // actually plays against the video, same as the live-preview sync.
    // If re-transcription failed, the *audio* swap still applies in
    // Burn & Export (passed via voiceoverPath below) -- just leave the
    // existing transcript in place rather than losing captions entirely.
    if (voiceoverWords) {
      setWords(voiceoverWords.map((w) => ({ ...w, start: w.start + offset, end: w.end + offset })));
      // Prosody/diarization were computed against the old transcript's
      // word count and timing -- stale now, would silently mismatch if
      // burned alongside the new captions.
      setProsody([]);
      setProsodyStatus("");
      setSpeakers([]);
      setDiarizeStatus("");
    }
  }

  function handleSeek(t) {
    if (videoRef.current) {
      videoRef.current.currentTime = t;
    }
    setCurrentTime(t);
  }

  function handleWordChange(index, text) {
    setWords((prev) => prev.map((w, i) => (i === index ? { ...w, word: text } : w)));
  }

  function handleWordDelete(index) {
    setWords((prev) => prev.filter((_, i) => i !== index));
  }

  async function runPipelineFor(path) {
    setPipelineRunning(true);
    setPipelineStatus("");
    setPipelineProgress(null);
    setDetectedLanguage(null);
    const startedAt = Date.now();
    const unlisten = await listen("pipeline-progress", (event) => {
      setPipelineProgress(event.payload);
    });
    try {
      // Tanglish slang normalization is available in the backend
      // (`slang::normalize_words`) but not exposed in this UI right now --
      // deliberately, to keep the default flow to as few decisions as
      // possible. Wire a toggle back in (e.g. in MoreOptionsModal) if it
      // turns out to be missed.
      const result = await invoke("run_pipeline", { videoPath: path, normalizeSlang: false });
      const elapsed = formatElapsed((Date.now() - startedAt) / 1000);
      setWords(result.words);
      setDetectedLanguage(result.detected_language ?? null);
      // A silent video (see pipeline.rs's has_audio_stream check) comes
      // back with an empty transcript, not an error -- worth a distinct
      // message rather than the slightly odd "Transcribed 0 words."
      setPipelineStatus(
        result.words.length > 0
          ? `Transcribed ${result.words.length} words in ${elapsed}.`
          : "No audio track found on this video — add a voice-over below to add captions."
      );
    } catch (err) {
      setPipelineStatus(`Error: ${err}`);
    } finally {
      unlisten();
      setPipelineRunning(false);
      setPipelineProgress(null);
    }
  }

  async function analyzeProsody() {
    if (!videoPath || words.length === 0) return;
    setAnalyzingProsody(true);
    setProsodyStatus("");
    setProsodyProgress(null);
    const startedAt = Date.now();
    const unlisten = await listen("prosody-progress", (event) => setProsodyProgress(event.payload));
    try {
      const result = await invoke("analyze_prosody", { videoPath, words });
      const elapsed = formatElapsed((Date.now() - startedAt) / 1000);
      setProsody(result);
      setProsodyStatus(`Analyzed ${result.length} words in ${elapsed}.`);
    } catch (err) {
      setProsodyStatus(`Error: ${err}`);
    } finally {
      unlisten();
      setAnalyzingProsody(false);
      setProsodyProgress(null);
    }
  }

  async function diarizeSpeakers() {
    if (!videoPath || words.length === 0) return;
    setAnalyzingSpeakers(true);
    setDiarizeStatus("");
    setDiarizeProgress(null);
    const startedAt = Date.now();
    const unlisten = await listen("diarize-progress", (event) => setDiarizeProgress(event.payload));
    try {
      const result = await invoke("diarize_speakers", { videoPath, words });
      const elapsed = formatElapsed((Date.now() - startedAt) / 1000);
      setSpeakers(result);
      const speakerCount = new Set(result.map((s) => s.speaker_id)).size;
      setDiarizeStatus(`Found ${speakerCount} speaker${speakerCount === 1 ? "" : "s"} across ${result.length} segments in ${elapsed}.`);
    } catch (err) {
      setDiarizeStatus(`Error: ${err}`);
    } finally {
      unlisten();
      setAnalyzingSpeakers(false);
      setDiarizeProgress(null);
    }
  }

  function handleJumpCutApplied(result) {
    // Video preview picks up the new file automatically via the videoPath prop.
    setVideoPath(result.output_path);
    setWords(result.words);
    setCurrentTime(0);
    setDuration(0);
    // Any active voiceover's sync offset was computed against the
    // *pre-cut* video's timing — re-cutting invalidates it, the same
    // staleness class handleVoiceoverReady guards prosody/speakers
    // against.
    setVoiceoverPath("");
    setVoiceoverOffset(0);
  }

  async function burnCaptions() {
    if (!videoPath || words.length === 0) return;

    // No client-side freshness check here — burn_captions itself verifies
    // (via a disk-persisted record, not React state) that videoPath hasn't
    // changed since it was transcribed, and rejects with a clear error if
    // it has. That holds even across separate app sessions/processes,
    // which an in-memory check here would not.

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
        outputPath,
        prosody,
        speakers,
        voiceoverPath: voiceoverPath || null,
        voiceoverOffsetSeconds: voiceoverPath ? voiceoverOffset : null,
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

  if (authLoading) {
    return (
      <div className="container auth-screen">
        <p className="subtitle">Loading…</p>
      </div>
    );
  }

  if (!user) {
    return <AuthScreen />;
  }

  return (
    <AppShell
      videoPath={videoPath}
      videoRef={videoRef}
      currentTime={currentTime}
      duration={duration}
      setCurrentTime={setCurrentTime}
      setDuration={setDuration}
      words={words}
      pipelineStatus={pipelineStatus}
      pipelineRunning={pipelineRunning}
      pipelineProgress={pipelineProgress}
      detectedLanguage={detectedLanguage}
      captionStyle={captionStyle}
      setCaptionStyle={setCaptionStyle}
      burnStatus={burnStatus}
      burning={burning}
      burnProgress={burnProgress}
      prosody={prosody}
      analyzingProsody={analyzingProsody}
      prosodyStatus={prosodyStatus}
      prosodyProgress={prosodyProgress}
      analyzeProsody={analyzeProsody}
      speakers={speakers}
      analyzingSpeakers={analyzingSpeakers}
      diarizeStatus={diarizeStatus}
      diarizeProgress={diarizeProgress}
      diarizeSpeakers={diarizeSpeakers}
      voiceoverPath={voiceoverPath}
      voiceoverOffset={voiceoverOffset}
      onVoiceoverReady={handleVoiceoverReady}
      pickVideo={pickVideo}
      handleSeek={handleSeek}
      handleWordChange={handleWordChange}
      handleWordDelete={handleWordDelete}
      handleJumpCutApplied={handleJumpCutApplied}
      burnCaptions={burnCaptions}
    />
  );
}

export default App;
