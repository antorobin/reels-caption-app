import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import ProgressBar, { formatElapsed } from "../ProgressBar.jsx";

const VIDEO_FILTERS = [{ name: "Video", extensions: ["mp4", "mov", "mkv", "avi", "webm"] }];
const AUDIO_FILTERS = [{ name: "Audio", extensions: ["mp3", "wav", "m4a", "aac"] }];

// Standalone tool — not wired into the main transcribe/caption pipeline.
// Output is just a synced video file the user can separately load if they
// want captions on it. For a generated voiceover previewed against the
// loaded video, see the Voiceover panel instead — it plays the voiceover
// alongside the original video directly (see VideoPreview.jsx), with no
// separate exported file needed for that.
function VoSyncPanel({ videoPath: initialVideoPath }) {
  const [videoPath, setVideoPath] = useState(initialVideoPath || "");
  const [voiceoverPath, setVoiceoverPath] = useState("");
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState(null);
  const [status, setStatus] = useState("");
  const [result, setResult] = useState(null);

  async function pickVideo() {
    const selected = await open({ multiple: false, filters: VIDEO_FILTERS });
    if (typeof selected === "string") setVideoPath(selected);
  }

  async function pickVoiceover() {
    const selected = await open({ multiple: false, filters: AUDIO_FILTERS });
    if (typeof selected === "string") setVoiceoverPath(selected);
  }

  async function runSync() {
    if (!videoPath || !voiceoverPath) return;
    const outputPath = await save({ defaultPath: "synced-output.mp4", filters: VIDEO_FILTERS });
    if (!outputPath) return;

    setRunning(true);
    setStatus("");
    setProgress(null);
    setResult(null);
    const startedAt = Date.now();
    const unlisten = await listen("vosync-progress", (event) => setProgress(event.payload));
    try {
      const syncResult = await invoke("sync_voice_over", { videoPath, voiceoverPath, outputPath });
      const elapsed = formatElapsed((Date.now() - startedAt) / 1000);
      setResult(syncResult);
      setStatus(`Synced in ${elapsed}.`);
    } catch (err) {
      setStatus(`Error: ${err}`);
    } finally {
      unlisten();
      setRunning(false);
      setProgress(null);
    }
  }

  return (
    <div className="inspector-panel">
      <h2>Voice-over sync</h2>
      <p className="section-hint">
        Tracks mouth movement in the video and cross-correlates it against the voice-over's speech envelope to find
        the best timing offset. This corrects a single constant timing offset — if the voice-over's pacing drifts
        relative to the video over time, only the initial offset is fixed, not variable drift. Not generative
        lip-sync: it can't make lips move to match different words.
      </p>

      <button onClick={pickVideo}>Choose video…</button>
      {videoPath && <p className="result">{videoPath}</p>}

      <button onClick={pickVoiceover}>Choose voice-over audio…</button>
      {voiceoverPath && <p className="result">{voiceoverPath}</p>}

      <button onClick={runSync} disabled={!videoPath || !voiceoverPath || running}>
        {running ? "Syncing…" : "Sync"}
      </button>
      <ProgressBar progress={progress} />
      {status && <pre className="result">{status}</pre>}
      {result && (
        <p className="result result-suggestion">
          Offset applied: {result.offset_seconds.toFixed(2)}s. Saved to {result.output_path}
        </p>
      )}
    </div>
  );
}

export default VoSyncPanel;
