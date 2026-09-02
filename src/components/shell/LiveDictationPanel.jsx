import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// A deliberately separate, lightweight workflow from VoiceoverSection's
// "Record from mic" -- that one records the whole clip, then transcribes
// it once (same as every other STT path in this app). This one shows text
// growing every ~2.5s while you speak, by periodically re-transcribing
// everything captured so far through the SAME code-switching-aware batch
// pipeline (mixed_language.rs) a whole video already goes through --
// chosen over a true incremental streaming recognizer specifically because
// no streaming-capable model exists for Tamil (or Tamil+English
// code-switching) today. See streaming_stt.rs's doc comment for the full
// story. Applies its result the same way -- captions only, via
// `onCaptionsFromRecording` -- there's no voiceover-merge option here;
// that's a more deliberate action better suited to the other workflow.
//
// `stop_live_dictation` returns immediately (it only signals the backend
// to stop) -- the actual final transcript arrives later via a
// "live-dictation-final" event, since a chunk transcription can genuinely
// take a while. The "Finishing…" state has its own cancel button for
// exactly that reason, rather than leaving this stuck if it's slow.
function LiveDictationPanel({ onCaptionsFromRecording }) {
  const [listening, setListening] = useState(false);
  const [liveText, setLiveText] = useState("");
  const [finishing, setFinishing] = useState(false);
  const [error, setError] = useState("");
  // See DictationHud.jsx's own note on this: the first live-dictation
  // session in an app run pays a real, one-time model-load cost (~20s+)
  // before any chunk can transcribe. Without this, that delay is
  // indistinguishable from "broken."
  const [loadingModels, setLoadingModels] = useState(false);

  useEffect(() => {
    if (!listening && !finishing) return undefined;
    const partialUnlisten = listen("live-dictation-partial", (event) => setLiveText(event.payload?.text ?? ""));
    const workerStatusUnlisten = listen("live-dictation-worker-status", (event) =>
      setLoadingModels(Boolean(event.payload?.loading))
    );
    const finalUnlisten = listen("live-dictation-final", (event) => {
      const { words, error: finalError } = event.payload ?? {};
      setListening(false);
      setFinishing(false);
      if (finalError) {
        setError(finalError);
        return;
      }
      onCaptionsFromRecording(words);
    });
    return () => {
      partialUnlisten.then((unlisten) => unlisten());
      workerStatusUnlisten.then((unlisten) => unlisten());
      finalUnlisten.then((unlisten) => unlisten());
    };
  }, [listening, finishing, onCaptionsFromRecording]);

  async function start() {
    setError("");
    setLiveText("");
    try {
      await invoke("start_live_dictation");
      setListening(true);
    } catch (err) {
      setError(String(err));
    }
  }

  async function stop() {
    setFinishing(true);
    try {
      await invoke("stop_live_dictation");
    } catch (err) {
      setError(String(err));
      setFinishing(false);
      setListening(false);
    }
  }

  function cancel() {
    invoke("stop_live_dictation").catch(() => {});
    setFinishing(false);
    setListening(false);
  }

  return (
    <div className="voiceover-section">
      <div className="voiceover-section-header">
        <span className="voiceover-section-title">Live dictation (beta)</span>
      </div>
      <p className="section-hint">
        Text grows every couple seconds as you speak -- Tamil and English are auto-detected, even mid-sentence,
        same as the main transcription pipeline. Updates the captions only; the video's audio is never touched here.
      </p>

      {!listening && !finishing ? (
        <button type="button" onClick={start}>
          🎤 Start live dictation
        </button>
      ) : (
        <>
          <div className="live-dictation-text">
            {finishing
              ? "Finishing…"
              : loadingModels
                ? "Loading speech models (first time only)… keep talking, text appears once this finishes."
                : liveText || "Listening…"}
          </div>
          <div className="burn-export-buttons-row">
            {listening && !finishing && (
              <button type="button" onClick={stop}>
                ⏹ Stop & use as captions
              </button>
            )}
            <button type="button" className="link-button" onClick={cancel}>
              Cancel
            </button>
          </div>
        </>
      )}

      {error && <pre className="result">{error}</pre>}
    </div>
  );
}

export default LiveDictationPanel;
