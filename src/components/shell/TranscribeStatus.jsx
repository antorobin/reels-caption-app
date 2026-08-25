// Replaces the old manual "Transcribe" section: transcription now starts
// automatically the moment a video loads (see App.jsx's pickVideo), so
// this is purely a status display -- a processing banner while it runs,
// then a compact one-line summary once done. Language is auto-detected
// from the audio (see stt::detect_spoken_language) -- no picker here
// anymore; the detected language is shown in the done-summary instead.
function TranscribeStatus({ pipelineRunning, pipelineProgress, pipelineStatus, detectedLanguage, wordsCount }) {
  return (
    <div className="transcribe-status">
      {pipelineRunning ? (
        <div className="processing-banner">
          <div className="processing-banner-header">
            <svg
              className="processing-spin"
              width="15"
              height="15"
              viewBox="0 0 24 24"
              fill="none"
              stroke="var(--shell-accent)"
              strokeWidth="2.2"
              strokeLinecap="round"
            >
              <circle cx="12" cy="12" r="9" strokeOpacity="0.25" />
              <path d="M21 12a9 9 0 0 0-9-9" />
            </svg>
            <span>Getting your captions ready…</span>
          </div>
          {pipelineProgress?.stage && <div className="processing-banner-stage">{pipelineProgress.stage}</div>}
          <div className="processing-banner-track">
            <div
              className="processing-banner-fill"
              style={{ width: `${pipelineProgress?.percent ?? 0}%` }}
            />
          </div>
        </div>
      ) : wordsCount > 0 ? (
        <div className="transcribe-status-done">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#34c759" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
            <path d="M20 6L9 17l-5-5" />
          </svg>
          <span>
            Transcribed automatically{detectedLanguage ? ` — ${detectedLanguage}` : ""} · {wordsCount} words
          </span>
        </div>
      ) : (
        // Empty transcript with no error (see pipeline.rs's has_audio_stream
        // check) — a silent video, not a failure. Worth a distinct,
        // non-alarming message rather than showing nothing at all.
        pipelineStatus &&
        !pipelineStatus.startsWith("Error") && (
          <p className="result result-suggestion">{pipelineStatus}</p>
        )
      )}
      {!pipelineRunning && pipelineStatus && pipelineStatus.startsWith("Error") && <pre className="result">{pipelineStatus}</pre>}
    </div>
  );
}

export default TranscribeStatus;
