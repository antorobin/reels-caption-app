import { useMemo, useState } from "react";
import { suggestHashtags } from "../../lib/keywords.js";

function KeywordsPanel({ words }) {
  const [copiedAll, setCopiedAll] = useState(false);
  const hashtags = useMemo(() => suggestHashtags(words), [words]);

  function copyOne(tag) {
    navigator.clipboard.writeText(tag);
  }

  function copyAll() {
    navigator.clipboard.writeText(hashtags.join(" "));
    setCopiedAll(true);
    setTimeout(() => setCopiedAll(false), 1500);
  }

  return (
    <div className="inspector-panel">
      <h2>Keyword &amp; hashtag suggestions</h2>
      <p className="section-hint">
        Extracted straight from the transcript (RAKE — no model, no backend call) — click a tag to copy it.
      </p>
      {hashtags.length === 0 ? (
        <p className="result">Transcribe a video first to get suggestions.</p>
      ) : (
        <>
          <div className="hashtag-chip-row">
            {hashtags.map((tag) => (
              <button key={tag} type="button" className="hashtag-chip" onClick={() => copyOne(tag)}>
                {tag}
              </button>
            ))}
          </div>
          <button onClick={copyAll}>{copiedAll ? "Copied!" : "Copy all"}</button>
        </>
      )}
    </div>
  );
}

export default KeywordsPanel;
