import TranscriptEditor from "../TranscriptEditor.jsx";

// Always shows the transcript (auto-populated once the pipeline finishes)
// -- replaces the old Inspector's per-section switching. Everything else
// that used to live behind sidebar navigation now lives contextually in
// MainPanel (see MainPanel.jsx, ToolCard.jsx, MoreOptionsModal.jsx).
function TranscriptPanel({ words, currentTime, onWordChange, onWordDelete, onSeek }) {
  return (
    <aside className="shell-inspector-col">
      <div className="shell-inspector">
        <div className="inspector-panel">
          <h2>Transcript</h2>
          {words.length === 0 ? (
            <p className="section-hint">Fills in automatically once a video is transcribed.</p>
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
