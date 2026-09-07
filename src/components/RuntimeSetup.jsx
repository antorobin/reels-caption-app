import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import ProgressBar from "./ProgressBar.jsx";

// First-run gate for the slim installer. `runtime_fetch.rs` ships the app
// without the ML runtime; the `tier: "core"` components (a minimal
// ffmpeg + the `stt` Python env, ~170 MB) are downloaded here, once,
// before the editor opens — transcription is the point of the app, so it
// shouldn't start half-working. Everything `tier: "on-demand"` (voiceover,
// cloning, the LLM) is left to its own in-context prompt later.
//
// The full installer bundles all of this, so `missing_core_components`
// comes back empty there and this screen never renders.

function sizeHint(bytes) {
  return bytes ? ` (~${Math.round(bytes / 1_000_000)} MB)` : "";
}

const LABELS = { ffmpeg: "Media engine", "python-stt": "Speech recognition" };

function RuntimeSetup({ components, onReady }) {
  const [doneIds, setDoneIds] = useState([]);
  const [activeId, setActiveId] = useState(null);
  const [progress, setProgress] = useState(null);
  const [error, setError] = useState("");
  const activeIdRef = useRef(null);
  const startedRef = useRef(false);

  async function runAll() {
    setError("");
    const unlisten = await listen("runtime-component-progress", (event) => {
      if (event.payload?.project_id === activeIdRef.current) setProgress(event.payload);
    });
    try {
      for (const c of components) {
        activeIdRef.current = c.id;
        setActiveId(c.id);
        setProgress(null);
        await invoke("download_runtime_component", { id: c.id });
        setDoneIds((d) => [...d, c.id]);
      }
      setActiveId(null);
      onReady();
    } catch (err) {
      setError(String(err));
      setActiveId(null);
    } finally {
      unlisten();
    }
  }

  useEffect(() => {
    if (startedRef.current) return;
    startedRef.current = true;
    runAll();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- run exactly once
  }, []);

  const totalMB = Math.round(components.reduce((s, c) => s + (c.size || 0), 0) / 1_000_000);

  return (
    <div className="container auth-screen">
      <h1>Setting up KraftReel.App</h1>
      <p className="subtitle">
        {error
          ? "Something went wrong downloading the runtime."
          : `Downloading the speech and media engine${totalMB ? ` (~${totalMB} MB)` : ""} — one time only.`}
      </p>

      <div className="card">
        <ul className="runtime-setup-list">
          {components.map((c) => {
            const state = doneIds.includes(c.id) ? "done" : c.id === activeId ? "active" : "pending";
            return (
              <li key={c.id} className={`runtime-setup-item is-${state}`}>
                <span className="runtime-setup-mark">{state === "done" ? "✓" : state === "active" ? "↓" : "•"}</span>
                <span className="runtime-setup-name">
                  {LABELS[c.id] || c.id}
                  <span className="runtime-setup-size">{sizeHint(c.size)}</span>
                </span>
              </li>
            );
          })}
        </ul>

        {activeId && <ProgressBar progress={progress} />}

        {error && (
          <>
            <pre className="result">{error}</pre>
            <button
              type="button"
              onClick={() => {
                startedRef.current = false;
                runAll();
              }}
            >
              Retry
            </button>
          </>
        )}
      </div>

      <p className="subtitle" style={{ marginTop: "1rem", fontSize: "0.82rem" }}>
        Needs an internet connection this once. After it finishes, KraftReel.App works offline.
      </p>
    </div>
  );
}

export default RuntimeSetup;
