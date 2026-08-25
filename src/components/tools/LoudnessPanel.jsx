import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import ProgressBar, { formatElapsed } from "../ProgressBar.jsx";

const VIDEO_FILTERS = [{ name: "Video", extensions: ["mp4", "mov", "mkv", "avi", "webm"] }];

function LoudnessPanel({ videoPath }) {
  const [targetLufs, setTargetLufs] = useState(-14);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState(null);
  const [status, setStatus] = useState("");

  async function run() {
    if (!videoPath) return;
    const outputPath = await save({ defaultPath: "normalized-output.mp4", filters: VIDEO_FILTERS });
    if (!outputPath) return;

    setRunning(true);
    setStatus("");
    setProgress(null);
    const startedAt = Date.now();
    const unlisten = await listen("loudness-progress", (event) => setProgress(event.payload));
    try {
      const result = await invoke("normalize_audio", { videoPath, outputPath, targetLufs });
      const elapsed = formatElapsed((Date.now() - startedAt) / 1000);
      setStatus(`Normalized to ${targetLufs} LUFS. Saved to ${result} in ${elapsed}.`);
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
      <h2>Loudness normalization</h2>
      <p className="section-hint">Two-pass ffmpeg loudnorm — no separate dependency. -14 LUFS is the standard target for social/short-form platforms.</p>
      <label className="style-controls">
        Target LUFS
        <input type="number" value={targetLufs} step={0.5} onChange={(e) => setTargetLufs(Number(e.target.value))} />
      </label>
      <button onClick={run} disabled={!videoPath || running}>
        {running ? "Normalizing…" : "Normalize loudness"}
      </button>
      <ProgressBar progress={progress} />
      {status && <pre className="result">{status}</pre>}
    </div>
  );
}

export default LoudnessPanel;
