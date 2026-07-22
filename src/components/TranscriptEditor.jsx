import { formatTime } from "../lib/time.js";

function isActiveWord(word, currentTime) {
  return currentTime >= word.start && currentTime < word.end;
}

function TranscriptEditor({ words, currentTime, onWordChange, onWordDelete, onSeek }) {
  if (!words || words.length === 0) return null;

  return (
    <div className="transcript-editor">
      <p className="section-hint">
        Fix any mistranscribed words here — click a word to jump the preview to it. Clear a word's text and click away
        to remove it.
      </p>
      <div className="transcript-editor-words">
        {words.map((w, i) => (
          <input
            key={i}
            className={isActiveWord(w, currentTime) ? "transcript-editor-word active" : "transcript-editor-word"}
            value={w.word}
            title={`${formatTime(w.start)} – ${formatTime(w.end)}`}
            size={Math.max(2, w.word.length)}
            onFocus={() => onSeek(w.start)}
            onChange={(e) => onWordChange(i, e.target.value)}
            onBlur={() => {
              if (!w.word.trim()) onWordDelete(i);
            }}
          />
        ))}
      </div>
    </div>
  );
}

export default TranscriptEditor;
