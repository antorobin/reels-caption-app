import { formatTime } from "../lib/time.js";

function isActiveWord(word, currentTime) {
  return currentTime >= word.start && currentTime < word.end;
}

// ASR transcription can silently drop a word or even a whole phrase — the
// surrounding words still get correct timing (forced alignment anchors
// them to real audio positions), so a dropped stretch of speech shows up
// as an abnormally large gap between two consecutive words' timestamps.
// This is a heuristic, not a guarantee (a short dropped word can still
// slip under the threshold) — confirmed directly: re-transcribing the
// same clip at a lower quality tier reproduced a real dropped word right
// where a user reported a missing sentence at a higher tier.
const GAP_WARNING_SECONDS = 1.2;

function TranscriptEditor({ words, currentTime, onWordChange, onWordDelete, onSeek }) {
  if (!words || words.length === 0) return null;

  return (
    <div className="transcript-editor">
      <p className="section-hint">
        Fix any mistranscribed words here — click a word to jump the preview to it. Clear a word's text and click away
        to remove it. A gap warning means the transcript may be missing a word or phrase there — check the video to
        confirm.
      </p>
      <div className="transcript-editor-words">
        {words.map((w, i) => {
          const gap = i > 0 ? w.start - words[i - 1].end : 0;
          return (
            <span key={i} className="transcript-editor-word-group">
              {gap > GAP_WARNING_SECONDS && (
                <button
                  type="button"
                  className="transcript-gap-warning"
                  title="Possible missing speech — click to jump just before this gap"
                  onClick={() => onSeek(words[i - 1].end)}
                >
                  ⚠ {gap.toFixed(1)}s gap
                </button>
              )}
              <input
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
            </span>
          );
        })}
      </div>
    </div>
  );
}

export default TranscriptEditor;
