import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

// The floating HUD a global hotkey (Ctrl+Shift+D, see dictation.rs) pops
// up from anywhere in the OS. Uses the same live, code-switching-aware
// dictation backend as LiveDictationPanel.jsx's in-app "Live dictation"
// panel (streaming_stt.rs) -- text grows here too, not just after
// recording stops, and Tamil+English switching works the same way.
// Starts listening immediately on mount, ends on Escape (cancel), Enter,
// or the hotkey being pressed again (relayed from Rust as
// "dictation-toggle-stop", since a second hotkey press while this window
// already exists means "finish", not "open another one"). Finishing
// broadcasts a "dictation-result" event the main window listens for
// (App.jsx) -- this window has no idea whether a video is even loaded;
// that's the main window's job to check.
//
// `stop_live_dictation` itself returns immediately (it only signals the
// backend to stop) -- the actual final transcript arrives later via a
// "live-dictation-final" event, since a chunk transcription can genuinely
// take a while (or, rarely, get stuck) and this window must never be left
// with no way to close just because that's slow. The close button stays
// available through every phase for exactly that reason.
function formatElapsed(seconds) {
  const whole = Math.floor(seconds);
  const m = Math.floor(whole / 60);
  const s = whole % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

function DictationHud() {
  const [phase, setPhase] = useState("starting"); // starting | listening | processing | error
  const [elapsed, setElapsed] = useState(0);
  const [liveText, setLiveText] = useState("");
  const [error, setError] = useState("");
  // Separate from `phase`: the very first live-dictation session in an app
  // run pays a real, one-time model-load cost (Parakeet + Whisper lang-ID,
  // ~20s+ on ordinary hardware) before any chunk can transcribe -- without
  // some signal for it, that whole window looks identical to "this is
  // broken and never shows any text," which is exactly the confusion a
  // fresh test hit. Every session after the first sees this stay false the
  // whole time, since the worker's already warm by then.
  const [loadingModels, setLoadingModels] = useState(false);

  const startedAtRef = useRef(null);
  const stoppingRef = useRef(false);

  function requestFinish() {
    if (stoppingRef.current) return;
    stoppingRef.current = true;
    setPhase("processing");
    invoke("stop_live_dictation").catch((err) => {
      setError(String(err));
      setPhase("error");
    });
    // The actual close happens in the "live-dictation-final" listener
    // below, once the backend actually has a result.
  }

  function requestCancel() {
    if (stoppingRef.current) {
      // Already finishing -- treat a second request as "give up waiting
      // and just close", since the result (if it ever arrives) would have
      // nowhere useful to go once this window is gone anyway.
      getCurrentWindow().close();
      return;
    }
    stoppingRef.current = true;
    invoke("stop_live_dictation").catch(() => {});
    getCurrentWindow().close();
  }

  useEffect(() => {
    let cancelled = false;
    let timer = null;
    let startedHere = false;

    async function start() {
      try {
        await invoke("start_live_dictation");
        if (cancelled) {
          // React 18 StrictMode (dev only) mounts, cleans up, then mounts
          // again -- this effect can win the race and start a real backend
          // session just before its own cleanup runs. Stop it immediately
          // rather than leaving an orphaned session that would make the
          // *next* mount's start_live_dictation fail with "already
          // running" -- exactly the bug that used to surface on the very
          // first hotkey press in dev mode.
          invoke("stop_live_dictation").catch(() => {});
          return;
        }
        startedHere = true;
        startedAtRef.current = Date.now();
        setPhase("listening");
        timer = setInterval(() => setElapsed((Date.now() - startedAtRef.current) / 1000), 200);
      } catch (err) {
        if (cancelled) return;
        setError(String(err));
        setPhase("error");
      }
    }

    start();

    function onKeyDown(e) {
      if (e.key === "Escape") requestCancel();
      else if (e.key === "Enter") requestFinish();
    }
    window.addEventListener("keydown", onKeyDown);

    const partialUnlisten = listen("live-dictation-partial", (event) => setLiveText(event.payload?.text ?? ""));
    const workerStatusUnlisten = listen("live-dictation-worker-status", (event) =>
      setLoadingModels(Boolean(event.payload?.loading))
    );
    const toggleStopUnlisten = listen("dictation-toggle-stop", () => requestFinish());
    const finalUnlisten = listen("live-dictation-final", async (event) => {
      const { words, error: finalError } = event.payload ?? {};
      if (finalError) {
        setError(finalError);
        setPhase("error");
        return;
      }
      await emit("dictation-result", { words });
      await getCurrentWindow().close();
    });

    return () => {
      cancelled = true;
      if (timer) clearInterval(timer);
      window.removeEventListener("keydown", onKeyDown);
      partialUnlisten.then((unlisten) => unlisten());
      workerStatusUnlisten.then((unlisten) => unlisten());
      toggleStopUnlisten.then((unlisten) => unlisten());
      finalUnlisten.then((unlisten) => unlisten());
      // Same StrictMode double-mount case as above, just caught at the
      // other end of it: if this effect instance's own start_live_dictation
      // already resolved and nothing has explicitly finished/cancelled it
      // yet, this unmount is the only place that will ever stop it.
      if (startedHere && !stoppingRef.current) {
        invoke("stop_live_dictation").catch(() => {});
      }
    };
  }, []);

  return (
    <div className={`hud phase-${phase}`}>
      <button type="button" className="hud-close" onClick={requestCancel} title="Close">
        ✕
      </button>
      {phase === "error" ? (
        <p className="hud-error">{error}</p>
      ) : (
        <>
          <div className="hud-mic">
            <span className={phase === "listening" ? "hud-mic-dot pulsing" : "hud-mic-dot"} />
            🎤
          </div>
          <div className="hud-status">
            {phase === "starting" && "Starting…"}
            {phase === "listening" && loadingModels && "Loading speech models (first time only)…"}
            {phase === "listening" && !loadingModels && `Listening… ${formatElapsed(elapsed)}`}
            {phase === "processing" && "Finishing… (click ✕ to give up waiting)"}
          </div>
          {phase === "listening" && (
            <>
              <div className="hud-live-text">{liveText}</div>
              <div className="hud-hint">
                {loadingModels
                  ? "Keep talking -- text starts appearing once loading finishes."
                  : "Enter or the hotkey to finish · Esc to cancel"}
              </div>
            </>
          )}
        </>
      )}
    </div>
  );
}

export default DictationHud;
