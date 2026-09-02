import TranscriptEditor from "../TranscriptEditor.jsx";

// Always shows the transcript (auto-populated once the pipeline finishes)
// -- replaces the old Inspector's per-section switching. Everything else
// that used to live behind sidebar navigation now lives contextually in
// MainPanel (see MainPanel.jsx, ToolCard.jsx, MoreOptionsModal.jsx).
//
// Also owns "Delete project" -- requested directly to live in this right
// pane (the open project's own view), not as a per-row action in the
// Sidebar's list. The actual confirmation + `delete_project` call +
// resetting the whole editing view back to blank lives in App.jsx's
// `deleteCurrentProject` (see its own doc comment); this is just where the
// button is anchored.
//
// The "Transcribe" button covers a real gap: there's no cross-restart
// resumability for a job that was still running when the app closed (see
// projectStore.js's own doc comment -- this is a deliberate scope cut, not
// an oversight) -- a video whose transcription never finished (app closed
// mid-pipeline, a transient failure, or a genuinely silent video someone
// later added a voice-over to) would otherwise have no way back to a
// transcript short of re-importing the whole video as a brand-new
// project. `onTranscribe` (App.jsx) just re-invokes the same
// `runPipelineFor` a fresh import already uses, against the video already
// on disk -- nothing new on the backend, this is purely exposing an
// existing capability as a manual retry.
function TranscriptPanel({
  words,
  currentTime,
  onWordChange,
  onWordDelete,
  onSeek,
  currentProjectId,
  onDeleteProject,
  videoPath,
  pipelineRunning,
  pipelineStatus,
  onTranscribe,
}) {
  return (
    <aside className="shell-inspector-col">
      <div className="shell-inspector">
        <div className="inspector-panel">
          <div className="inspector-panel-header">
            <h2>Transcript</h2>
            {currentProjectId && (
              <button type="button" className="project-delete-button" onClick={onDeleteProject}>
                Delete project
              </button>
            )}
          </div>
          {words.length === 0 ? (
            <>
              <p className="section-hint">
                {videoPath
                  ? "Fills in automatically once a video is transcribed."
                  : "Choose a video first -- there's nothing to transcribe yet."}
              </p>
              {videoPath && (
                <button type="button" onClick={onTranscribe} disabled={pipelineRunning}>
                  {pipelineRunning ? "Transcribing…" : "Transcribe"}
                </button>
              )}
              {pipelineStatus && <p className="section-hint">{pipelineStatus}</p>}
            </>
          ) : (
            <>
              <p className="section-hint">Click any word to edit or jump to it in the preview.</p>
              <TranscriptEditor words={words} currentTime={currentTime} onWordChange={onWordChange} onWordDelete={onWordDelete} onSeek={onSeek} />
            </>
          )}
        </div>
      </div>
    </aside>
  );
}

export default TranscriptPanel;
