import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import ProgressBar, { formatElapsed } from "./ProgressBar.jsx";

function SilenceRemovalPanel({ videoPath, words, onApplied }) {
  const [minSilenceSeconds, setMinSilenceSeconds] = useState(0.6);
  const [removeFillerWords, setRemoveFillerWords] = useState(true);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState(null);
  const [status, setStatus] = useState("");

  async function handleRun() {
    if (!videoPath || words.length === 0) return;
    setRunning(true);
    setStatus("");
    setProgress(null);
    const startedAt = Date.now();
    const unlisten = await listen("jumpcut-progress", (event) => setProgress(event.payload));
    try {
      const result = await invoke("remove_silence_and_fillers", {
        videoPath,
        words,
        options: { minSilenceSeconds, removeFillerWords },
      });
      const elapsed = formatElapsed((Date.now() - startedAt) / 1000);
      setStatus(
        `Removed ${result.removed_seconds.toFixed(1)}s across ${result.cut_count} cut${result.cut_count === 1 ? "" : "s"} in ${elapsed}.`
      );
      onApplied(result);
    } catch (err) {
      setStatus(`Error: ${err}`);
    } finally {
      unlisten();
      setRunning(false);
      setProgress(null);
    }
  }

  return (
    <div className="silence-panel">
      <div className="style-controls">
        <label>
          Minimum silence to cut (s)
          <input
            type="number"
            min={0.2}
            step={0.1}
            value={minSilenceSeconds}
            onChange={(e) => setMinSilenceSeconds(Number(e.target.value))}
          />
        </label>

        <label className="silence-panel-checkbox">
          <input
            type="checkbox"
            checked={removeFillerWords}
            onChange={(e) => setRemoveFillerWords(e.target.checked)}
          />
          Remove filler words (um, uh, …)
        </label>
      </div>

      <button onClick={handleRun} disabled={words.length === 0 || running}>
        {running ? "Trimming…" : "Remove silence & filler words"}
      </button>
      <ProgressBar progress={progress} />
      {status && <pre className="result">{status}</pre>}
    </div>
  );
}

export default SilenceRemovalPanel;
