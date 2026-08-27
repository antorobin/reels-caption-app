import { useState } from "react";

// Generated result is lifted up to App.jsx (not local state here) so
// ScheduleToInstagramButton can reuse the title+hashtags as a default
// Instagram caption without a separate generation step.
function ContentIdeasPanel({ words, result, generating, error, onGenerate }) {
  const [copied, setCopied] = useState("");

  function copy(text, label) {
    navigator.clipboard.writeText(text);
    setCopied(label);
    setTimeout(() => setCopied(""), 1500);
  }

  return (
    <div className="inspector-panel">
      <h2>Title, description &amp; hashtags (AI)</h2>
      <p className="section-hint">
        Generated locally by Qwen2.5-0.5B-Instruct via llama.cpp — first run starts the model service, which can
        take a moment.
      </p>
      <button onClick={onGenerate} disabled={words.length === 0 || generating}>
        {generating ? "Generating…" : "Generate title, description & hashtags"}
      </button>
      {words.length === 0 && <p className="result">Transcribe a video first.</p>}
      {error && <pre className="result">{error}</pre>}
      {result && (
        <>
          <div className="checkbox-row" style={{ cursor: "pointer" }} onClick={() => copy(result.title, "title")}>
            <strong>{copied === "title" ? "Copied!" : `${result.emoji.join(" ")} ${result.title}`}</strong>
          </div>
          <p className="result" style={{ cursor: "pointer" }} onClick={() => copy(result.description, "description")}>
            {copied === "description" ? "Copied!" : result.description}
          </p>
          <p className="result" style={{ cursor: "pointer" }} onClick={() => copy(result.hook, "hook")}>
            {copied === "hook" ? "Copied!" : `Hook: ${result.hook}`}
          </p>
          <div className="hashtag-chip-row">
            {result.hashtags.map((tag) => (
              <button key={tag} type="button" className="hashtag-chip" onClick={() => copy(tag, tag)}>
                {copied === tag ? "Copied!" : tag}
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

export default ContentIdeasPanel;
