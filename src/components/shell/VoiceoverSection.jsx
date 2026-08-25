import { useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { detectTextLanguage } from "../../lib/languages.js";

const AUDIO_FILTERS = [{ name: "Audio", extensions: ["mp3", "wav", "m4a", "aac"] }];

// Replaces the loaded video's audio, one of two ways: upload an existing
// recording, or write a script and generate one locally (Piper for
// English, MMS-TTS for Indian languages). Both paths converge on the same
// handling: sync to mouth movement (when a video is loaded), then
// re-transcribe the voiceover itself so its captions match what it
// actually says -- burning still needs real word-level timestamps for the
// new audio, not the original video's own transcript (that mismatch was a
// real reported bug: Burn & Export kept using the old captions and never
// touched the video's audio at all until this was wired up). Playback
// happens directly alongside the video in VideoPreview.jsx -- no separate
// output file needed just to hear it. Language is auto-detected, not
// picked: the script's text for "write" mode (see detectTextLanguage),
// the audio itself for "upload" mode (transcribe_audio_file auto-detects
// on the Rust side, same as the main pipeline) -- no dropdown here.
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
function VoiceoverSection({ videoPath, onVoiceoverReady }) {
  const [mode, setMode] = useState(null); // null | "upload" | "write"

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

  async function syncAndHandOff(path) {
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
      const result = await invoke("transcribe_audio_file", { audioPath: path });
      setDetectedLanguage(result.detected_language ?? null);
      onVoiceoverReady(path, offsetSeconds, result.words);
    } catch (transcribeErr) {
      setError((prev) => `${prev ? prev + " " : ""}Couldn't transcribe the voiceover for captions: ${transcribeErr}`);
      onVoiceoverReady(path, offsetSeconds, null);
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
    await syncAndHandOff(selected);
  }

  async function generate() {
    setGenerating(true);
    setError("");
    setEmotion(null);
    setDetectedLanguage(null);
    setVoiceMatch(null);
    setVoiceMatchNote(null);
    setOutputPath("");
    try {
      const language = detectTextLanguage(text);
      const result = await invoke("generate_voiceover", { text, language, autoEmotion, videoPath: videoPath || null });
      setEmotion(result.emotion);
      setVoiceMatch(result.voice_match);
      setVoiceMatchNote(result.voice_match_note ?? null);
      setGenerating(false);
      await syncAndHandOff(result.output_path);
    } catch (err) {
      setError(String(err));
      setGenerating(false);
    }
  }

  const busy = generating || syncing || transcribing;
  const busyLabel = generating
    ? "Generating & matching the voice…"
    : syncing
      ? "Syncing to mouth movement…"
      : transcribing
        ? "Transcribing voiceover…"
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
      <p className="section-hint">Replace this clip's audio (and its captions) with a new voice — upload a recording, or write a script to generate one. Language is detected automatically.</p>

      <div className="voiceover-mode-row">
        <button type="button" className={mode === "upload" ? "voiceover-mode-button active" : "voiceover-mode-button"} onClick={() => setMode("upload")}>
          Upload voice-over
        </button>
        <button type="button" className={mode === "write" ? "voiceover-mode-button active" : "voiceover-mode-button"} onClick={() => setMode("write")}>
          Write voiceover text
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

      {error && <pre className="result">{error}</pre>}
      {outputPath && !busy && (
        <>
          {detectedLanguage && <p className="result result-suggestion">Detected language: {detectedLanguage}</p>}
          {voiceMatchLabel && <p className="result result-suggestion">{voiceMatchLabel}</p>}
          {emotion && <p className="result result-suggestion">Detected tone: {emotion}</p>}
          {videoPath ? (
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
