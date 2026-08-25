import ProgressBar from "../ProgressBar.jsx";

function ProsodyPanel({ videoPath, words, prosody, analyzingProsody, prosodyStatus, prosodyProgress, analyzeProsody }) {
  return (
    <div className="inspector-panel">
      <h2>Vocal emphasis</h2>
      <p className="section-hint">
        Reads vocal loudness per word (RMS energy, relative to this clip's own range) and pops cascade captions
        bigger on emphasized words, subtler on quiet ones — an audio-only, no-vision-model replacement for the old
        emotion-based emphasis.
      </p>
      <button onClick={analyzeProsody} disabled={!videoPath || words.length === 0 || analyzingProsody}>
        {analyzingProsody ? "Analyzing…" : "Analyze vocal emphasis"}
      </button>
      <ProgressBar progress={prosodyProgress} />
      {prosodyStatus && <pre className="result">{prosodyStatus}</pre>}
      {prosody.length > 0 && (
        <p className="result result-suggestion">
          ✨ Cascade word emphasis will follow {prosody.length} analyzed words — bigger pops on emphasized words,
          subtler ones on quiet stretches.
        </p>
      )}
    </div>
  );
}

export default ProsodyPanel;
