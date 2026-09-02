import { useRef, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { detectTextLanguage } from "../../lib/languages.js";

const AUDIO_FILTERS = [{ name: "Audio", extensions: ["mp3", "wav", "m4a", "aac"] }];

// Replaces the loaded video's audio, one of three ways: upload an existing
// recording, write a script and generate one locally (Piper for English,
// MMS-TTS for Indian languages), or record straight from the mic. All
// three converge on the same handling: sync to mouth movement (when a
// video is loaded), then re-transcribe the voiceover itself so its
// captions match what it actually says -- burning still needs real
// word-level timestamps for the new audio, not the original video's own
// transcript (that mismatch was a real reported bug: Burn & Export kept
// using the old captions and never touched the video's audio at all until
// this was wired up). Playback happens directly alongside the video in
// VideoPreview.jsx -- no separate output file needed just to hear it.
// Language is auto-detected, not picked: the script's text for "write"
// mode (see detectTextLanguage), the audio itself for "upload"/"record"
// modes (transcribe_audio_file auto-detects on the Rust side, same as the
// main pipeline) -- no dropdown here.
//
// "Record from mic" is the one mode that *doesn't* always replace the
// video's audio -- its own checkbox (`mergeAsVoiceover`) decides that,
// defaulting off. Unchecked, a recording only updates the transcript/
// captions (dictating or re-reading corrections aloud instead of typing
// them) while the video's own audio (or an already-active voiceover)
// stays untouched -- `onCaptionsFromRecording` handles that path directly
// rather than going through `syncAndHandOff`/`onVoiceoverReady`, which
// always treats its target as the new voiceover audio. Capture itself
// goes through the standard `getUserMedia`/`MediaRecorder` Web APIs
// (permission-gated by the WebView, no new native audio dependency); the
// recorded blob's raw bytes are handed to `save_recorded_voice`
// (mic_recording.rs), which shells out to the already-bundled ffmpeg to
// convert whatever container the browser produced (usually webm/opus)
// into the same 16kHz mono WAV every other audio path here expects.
//
// "Write voiceover text" generation also tries to make the result actually
// sound like the video's own speaker: generate_voiceover reshapes the
// synthesized clip's timbre to match a reference clip pulled from the
// loaded video (OpenVoice's ToneColorConverter, see voice_clone.rs) when
// possible, falling back to a gender-matched preset voice or the plain
// default when it isn't (no original audio, or cloning itself failed) --
// see tts.rs's `resolve_voice_reference` for the full decision order.
// "Upload voice-over" is the user's own recording, so none of this
// applies there -- only "write" mode passes videoPath through.
// `projectId` is captured at the start of each slow operation below
// (upload+sync+transcribe, generate+sync+transcribe, or record+transcribe)
// and passed back through `onVoiceoverReady`/`onCaptionsFromRecording` --
// App.jsx checks it before applying a result, the same `forProjectId`-
// capture-and-guard pattern already used for transcription/content-
// strategy/prosody/diarize/burn (see App.jsx's `projectIdRef` doc
// comment). This component previously had no project-id awareness at all,
// so a slow generate-voiceover call whose project got switched away from
// mid-call would silently land on whichever *different* project was open
// when it finally resolved.
function VoiceoverSection({ videoPath, projectId, onVoiceoverReady, onCaptionsFromRecording }) {
  const [mode, setMode] = useState(null); // null | "upload" | "write" | "record"

  const [text, setText] = useState("");
  const [autoEmotion, setAutoEmotion] = useState(false);
  const [emotion, setEmotion] = useState(null);
  const [voiceMatch, setVoiceMatch] = useState(null);
  const [voiceMatchNote, setVoiceMatchNote] = useState(null);

  const [generating, setGenerating] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [transcribing, setTranscribing] = useState(false);
  const [outputPath, setOutputPath] = useState("");
  const [detectedLanguage, setDetectedLanguage] = useState(null);
  const [error, setError] = useState("");

  const [recording, setRecording] = useState(false);
  const [mergeAsVoiceover, setMergeAsVoiceover] = useState(false);
  const [captionsOnlyResult, setCaptionsOnlyResult] = useState(false);
  const mediaRecorderRef = useRef(null);
  const recordedChunksRef = useRef([]);

  async function syncAndHandOff(path, forProjectId) {
    setOutputPath(path);
    let offsetSeconds = 0;
    if (videoPath) {
      setSyncing(true);
      try {
        offsetSeconds = await invoke("compute_voiceover_offset", { videoPath, voiceoverPath: path });
      } catch (syncErr) {
        // Mouth-movement sync failed (e.g. no face detected) -- still
        // usable, just without the timing offset correction.
        setError(`Couldn't sync to mouth movement: ${syncErr}`);
      } finally {
        setSyncing(false);
      }
    }

    setTranscribing(true);
    try {
      const result = await invoke("transcribe_audio_file", { audioPath: path, projectId: forProjectId });
      setDetectedLanguage(result.detected_language ?? null);
      onVoiceoverReady(path, offsetSeconds, result.words, forProjectId);
    } catch (transcribeErr) {
      setError((prev) => `${prev ? prev + " " : ""}Couldn't transcribe the voiceover for captions: ${transcribeErr}`);
      onVoiceoverReady(path, offsetSeconds, null, forProjectId);
    } finally {
      setTranscribing(false);
    }
  }

  async function pickAndUpload() {
    const selected = await open({ multiple: false, filters: AUDIO_FILTERS });
    if (typeof selected !== "string") return;
    setError("");
    setEmotion(null);
    setDetectedLanguage(null);
    setVoiceMatch(null);
    setVoiceMatchNote(null);
    setCaptionsOnlyResult(false);
    await syncAndHandOff(selected, projectId);
  }

  async function startRecording() {
    setError("");
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      const recorder = new MediaRecorder(stream);
      recordedChunksRef.current = [];
      recorder.ondataavailable = (e) => {
        if (e.data.size > 0) recordedChunksRef.current.push(e.data);
      };
      recorder.onstop = () => {
        stream.getTracks().forEach((track) => track.stop());
        handleRecordingStopped();
      };
      recorder.start();
      mediaRecorderRef.current = recorder;
      setRecording(true);
    } catch (err) {
      setError(`Couldn't access the microphone: ${err}`);
    }
  }

  function stopRecording() {
    mediaRecorderRef.current?.stop();
    setRecording(false);
  }

  async function handleRecordingStopped() {
    const forProjectId = projectId;
    const blob = new Blob(recordedChunksRef.current, { type: mediaRecorderRef.current?.mimeType || "audio/webm" });
    recordedChunksRef.current = [];
    if (blob.size === 0) {
      setError("No audio was captured — try recording again.");
      return;
    }

    setError("");
    setEmotion(null);
    setDetectedLanguage(null);
    setVoiceMatch(null);
    setVoiceMatchNote(null);
    setOutputPath("");
    setCaptionsOnlyResult(false);
    setTranscribing(true);
    try {
      const bytes = Array.from(new Uint8Array(await blob.arrayBuffer()));
      const wavPath = await invoke("save_recorded_voice", { bytes });
      if (mergeAsVoiceover) {
        setTranscribing(false);
        await syncAndHandOff(wavPath, forProjectId);
      } else {
        // Captions only -- transcribe for the transcript, but leave the
        // video's own audio (and its currently active voiceover, if any)
        // untouched.
        const result = await invoke("transcribe_audio_file", { audioPath: wavPath, projectId: forProjectId });
        setDetectedLanguage(result.detected_language ?? null);
        setOutputPath(wavPath);
        setCaptionsOnlyResult(true);
        onCaptionsFromRecording(result.words, forProjectId);
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setTranscribing(false);
    }
  }

  async function generate() {
    const forProjectId = projectId;
    setGenerating(true);
    setError("");
    setEmotion(null);
    setDetectedLanguage(null);
    setVoiceMatch(null);
    setVoiceMatchNote(null);
    setOutputPath("");
    setCaptionsOnlyResult(false);
    try {
      const language = detectTextLanguage(text);
      const result = await invoke("generate_voiceover", { text, language, autoEmotion, videoPath: videoPath || null });
      setEmotion(result.emotion);
      setVoiceMatch(result.voice_match);
      setVoiceMatchNote(result.voice_match_note ?? null);
      setGenerating(false);
      await syncAndHandOff(result.output_path, forProjectId);
    } catch (err) {
      setError(String(err));
      setGenerating(false);
    }
  }

  const busy = generating || syncing || transcribing || recording;
  const busyLabel = generating
    ? "Generating & matching the voice…"
    : syncing
      ? "Syncing to mouth movement…"
      : transcribing
        ? "Transcribing…"
        : null;

  const voiceMatchLabel =
    voiceMatch === "cloned"
      ? "Voice matched to the original speaker"
      : voiceMatch === "gender_matched"
        ? `Closest voice match${voiceMatchNote ? ` — couldn't clone: ${voiceMatchNote}` : ""}`
        : voiceMatch === "default"
          ? `Default voice${voiceMatchNote ? ` — ${voiceMatchNote}` : ""}`
          : null;

  return (
    <div className="voiceover-section">
      <div className="voiceover-section-header">
        <span className="voiceover-section-title">Voice-over</span>
      </div>
      <p className="section-hint">
        Replace this clip's audio (and its captions) with a new voice — upload a recording, write a script to
        generate one, or record straight from the mic. Language is detected automatically.
      </p>

      <div className="voiceover-mode-row">
        <button type="button" className={mode === "upload" ? "voiceover-mode-button active" : "voiceover-mode-button"} onClick={() => setMode("upload")}>
          Upload voice-over
        </button>
        <button type="button" className={mode === "write" ? "voiceover-mode-button active" : "voiceover-mode-button"} onClick={() => setMode("write")}>
          Write voiceover text
        </button>
        <button type="button" className={mode === "record" ? "voiceover-mode-button active" : "voiceover-mode-button"} onClick={() => setMode("record")}>
          Record from mic
        </button>
      </div>

      {mode === "upload" && (
        <div className="voiceover-mode-body">
          <button onClick={pickAndUpload} disabled={busy}>
            {busyLabel ?? "Choose audio file…"}
          </button>
        </div>
      )}

      {mode === "write" && (
        <div className="voiceover-mode-body">
          <label className="field-row">
            Script
            <textarea rows={4} value={text} onChange={(e) => setText(e.target.value)} placeholder="Type the voiceover script here…" />
          </label>
          <label className="checkbox-row">
            <input type="checkbox" checked={autoEmotion} onChange={(e) => setAutoEmotion(e.target.checked)} />
            Match delivery to the script's tone (adjusts pace and pitch slightly — not real emotional performance, and
            not driven by any visible speaker's movement)
          </label>
          <button onClick={generate} disabled={!text.trim() || busy}>
            {busyLabel ?? "Generate voiceover"}
          </button>
        </div>
      )}

      {mode === "record" && (
        <div className="voiceover-mode-body">
          <label className="checkbox-row">
            <input
              type="checkbox"
              checked={mergeAsVoiceover}
              onChange={(e) => setMergeAsVoiceover(e.target.checked)}
              disabled={recording || transcribing || syncing}
            />
            Also use this recording as the video's voice-over (replaces its audio) — otherwise it's only used to
            update the captions.
          </label>
          <button
            type="button"
            onClick={recording ? stopRecording : startRecording}
            disabled={transcribing || syncing}
            className={recording ? "voiceover-record-button recording" : "voiceover-record-button"}
          >
            {recording ? "⏹ Stop recording" : transcribing ? "Transcribing…" : syncing ? "Syncing to mouth movement…" : "🎤 Start recording"}
          </button>
        </div>
      )}

      {error && <pre className="result">{error}</pre>}
      {outputPath && !busy && (
        <>
          {detectedLanguage && <p className="result result-suggestion">Detected language: {detectedLanguage}</p>}
          {voiceMatchLabel && <p className="result result-suggestion">{voiceMatchLabel}</p>}
          {emotion && <p className="result result-suggestion">Detected tone: {emotion}</p>}
          {captionsOnlyResult ? (
            <>
              <p className="result result-suggestion">
                Captions updated from your recording — the video's own audio is unchanged.
              </p>
              <audio controls src={convertFileSrc(outputPath)} style={{ width: "100%", marginTop: 12 }} />
            </>
          ) : videoPath ? (
            <p className="result result-suggestion">Playing synced with the video above — captions and Burn &amp; Export now use this voiceover.</p>
          ) : (
            <audio controls src={convertFileSrc(outputPath)} style={{ width: "100%", marginTop: 12 }} />
          )}
        </>
      )}
    </div>
  );
}

export default VoiceoverSection;
