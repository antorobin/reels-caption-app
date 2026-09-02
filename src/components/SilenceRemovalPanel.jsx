import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import ProgressBar, { formatElapsed } from "./ProgressBar.jsx";

// `projectId` is captured at the start of the run and passed back through
// `onApplied` -- App.jsx's `handleJumpCutApplied` checks it before applying
// the result, the same `forProjectId`-capture pattern already used for
// transcription/content-strategy/prosody/diarize/burn (see App.jsx's
// `projectIdRef` doc comment). This component has no background-routing
// awareness of its own; it just hands the id back to whoever does.
function SilenceRemovalPanel({ videoPath, words, projectId, onApplied }) {
  const [minSilenceSeconds, setMinSilenceSeconds] = useState(0.6);
  const [removeFillerWords, setRemoveFillerWords] = useState(true);
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState(null);
  const [status, setStatus] = useState("");

  async function handleRun() {
    if (!videoPath || words.length === 0) return;
    const forProjectId = projectId;
    setRunning(true);
    setStatus("");
    setProgress(null);
    const startedAt = Date.now();
    const unlisten = await listen("jumpcut-progress", (event) => setProgress(event.payload));
    try {
      const result = await invoke("remove_silence_and_fillers", {
        videoPath,
        words,
        options: { min_silence_seconds: minSilenceSeconds, remove_filler_words: removeFillerWords },
        projectId: forProjectId,
      });
      const elapsed = formatElapsed((Date.now() - startedAt) / 1000);
      setStatus(
        `Removed ${result.removed_seconds.toFixed(1)}s across ${result.cut_count} cut${result.cut_count === 1 ? "" : "s"} in ${elapsed}.`
      );
      onApplied(result, forProjectId);
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
