import { useState } from "react";

// Generated result is lifted up to App.jsx (not local state here) so
// ScheduleToInstagramButton can reuse the title+hashtags as a default
// Instagram caption without a separate generation step.
//
// `options` holds all 3 returned strategy angles (Educational/how-to,
// Entertainment/relatable, Aspirational/emotional); `result` is whichever
// one is currently selected (defaults to the first, via App.jsx's
// generateContentIdeas). Picking a different card just swaps which one is
// active -- nothing re-generates. `hints` is a free-text theme/idea the
// user can optionally give to steer generation; it's also the *only*
// input a from-scratch (no-video-upload) project has, so a hint alone with
// no transcript is a real, working case here, not just words+hints.
function ContentIdeasPanel({ words, result, options, hints, onHintsChange, onSelectOption, generating, error, onGenerate }) {
  const [copied, setCopied] = useState("");

  function copy(text, label) {
    navigator.clipboard.writeText(text);
    setCopied(label);
    setTimeout(() => setCopied(""), 1500);
  }

  const hasTranscript = words.length > 0;
  const hasHints = (hints || "").trim().length > 0;
  const strategyOptions = options || [];

  return (
    <div className="inspector-panel">
      <h2>Marketing content strategy (AI)</h2>
      <p className="section-hint">
        Generated locally by Qwen2.5-0.5B-Instruct via llama.cpp — first run starts the model service, which can
        take a moment. Give it a hint or theme to steer the angle, or leave it blank to generate straight from the
        transcript.
      </p>
      <label className="field-row">
        Hints / themes (optional)
        <textarea
          rows={2}
          value={hints || ""}
          onChange={(e) => onHintsChange?.(e.target.value)}
          placeholder="e.g. sustainability, a weekend project, a family recipe…"
        />
      </label>
      <button onClick={onGenerate} disabled={(!hasTranscript && !hasHints) || generating}>
        {generating ? "Generating…" : "Generate strategy options"}
      </button>
      {!hasTranscript && !hasHints && <p className="result">Transcribe a video first, or give a hint above.</p>}
      {error && <pre className="result">{error}</pre>}

      {strategyOptions.length > 0 && (
        <div className="music-suggestions">
          {strategyOptions.map((option, i) => (
            <div
              className={result === option ? "music-suggestion-card selected" : "music-suggestion-card"}
              key={`${option.angle}-${i}`}
            >
              <strong>{option.angle}</strong>
              <p>
                {option.emoji.join(" ")} {option.title}
              </p>
              <p className="section-hint">{option.hook}</p>
              <button onClick={() => onSelectOption?.(option)} disabled={result === option}>
                {result === option ? "✓ In use" : "Use this"}
              </button>
            </div>
          ))}
        </div>
      )}

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
