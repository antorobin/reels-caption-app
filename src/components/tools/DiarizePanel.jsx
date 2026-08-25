import ProgressBar from "../ProgressBar.jsx";

const SPEAKER_SWATCHES = ["#FFE600", "#00E5FF", "#FF4D8D", "#7CFF6B"];

function DiarizePanel({ videoPath, words, speakers, analyzingSpeakers, diarizeStatus, diarizeProgress, diarizeSpeakers }) {
  const speakerIds = [...new Set(speakers.map((s) => s.speaker_id))].sort();

  return (
    <div className="inspector-panel">
      <h2>Speaker diarization</h2>
      <p className="section-hint">
        A lightweight heuristic — no cloud model, no account setup: silence-gap boundaries (reused from the jump-cut
        detector) clustered by voice-print (MFCC + k-means). Colors the active cascade word by who's speaking.
      </p>
      <button onClick={diarizeSpeakers} disabled={!videoPath || words.length === 0 || analyzingSpeakers}>
        {analyzingSpeakers ? "Analyzing…" : "Detect speakers"}
      </button>
      <ProgressBar progress={diarizeProgress} />
      {diarizeStatus && <pre className="result">{diarizeStatus}</pre>}
      {speakerIds.length > 0 && (
        <ul className="speaker-legend">
          {speakerIds.map((id) => (
            <li key={id} className="speaker-legend-item">
              <span className="speaker-legend-swatch" style={{ background: SPEAKER_SWATCHES[id % SPEAKER_SWATCHES.length] }} />
              Speaker {id + 1}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export default DiarizePanel;
