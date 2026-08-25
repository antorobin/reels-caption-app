import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import ProgressBar, { formatElapsed } from "../ProgressBar.jsx";

const VIDEO_FILTERS = [{ name: "Video", extensions: ["mp4", "mov", "mkv", "avi", "webm"] }];
const AUDIO_FILTERS = [{ name: "Audio", extensions: ["mp3", "wav", "m4a", "aac"] }];

function DuckingPanel({ videoPath, words }) {
  const [musicPath, setMusicPath] = useState("");
  const [duckAmount, setDuckAmount] = useState(70); // % reduction during speech
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState(null);
  const [status, setStatus] = useState("");

  async function pickMusic() {
    const selected = await open({ multiple: false, filters: AUDIO_FILTERS });
    if (typeof selected === "string") setMusicPath(selected);
  }

  async function run() {
    if (!videoPath || !musicPath || words.length === 0) return;
    const outputPath = await save({ defaultPath: "with-music.mp4", filters: VIDEO_FILTERS });
    if (!outputPath) return;

    setRunning(true);
    setStatus("");
    setProgress(null);
    const startedAt = Date.now();
    const unlisten = await listen("ducking-progress", (event) => setProgress(event.payload));
    try {
      const duckLevel = 1 - duckAmount / 100;
      const result = await invoke("duck_music", { videoPath, musicPath, words, duckLevel, outputPath });
      const elapsed = formatElapsed((Date.now() - startedAt) / 1000);
      setStatus(`Saved to ${result} in ${elapsed}.`);
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
      <h2>Background music ducking</h2>
      <p className="section-hint">
        Mixes a music bed under the video's existing audio, automatically lowering the music during speech (using the
        transcript timing you already have — no extra audio analysis needed).
      </p>

      <button onClick={pickMusic}>Choose music file…</button>
      {musicPath && <p className="result">{musicPath}</p>}

      <label className="style-controls">
        Duck amount ({duckAmount}%)
        <input type="range" min={0} max={95} value={duckAmount} onChange={(e) => setDuckAmount(Number(e.target.value))} />
      </label>

      <button onClick={run} disabled={!videoPath || !musicPath || words.length === 0 || running}>
        {running ? "Mixing…" : "Add music with ducking"}
      </button>
      <ProgressBar progress={progress} />
      {status && <pre className="result">{status}</pre>}
    </div>
  );
}

export default DuckingPanel;
