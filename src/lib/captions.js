const SENTENCE_END = new Set([".", "?", "!"]);
const CLAUSE_BREAK = new Set([",", ";", ":"]);

// Mirrors merge_punctuation in src-tauri/src/captions.rs — whisper emits
// punctuation as its own word-timestamp entries, so this folds each one
// onto the word before it (extending that word's end time to cover it)
// and records what kind of break follows.
function mergePunctuation(words) {
  const merged = [];
  for (const w of words) {
    const isSentenceEnd = SENTENCE_END.has(w.word);
    const isClauseBreak = CLAUSE_BREAK.has(w.word);
    if (isSentenceEnd || isClauseBreak) {
      const last = merged[merged.length - 1];
      if (last) {
        last.word = { ...last.word, word: last.word.word + w.word, end: w.end };
        last.breakKind = isSentenceEnd ? "sentence" : "clause";
      }
      continue;
    }
    merged.push({ word: w, breakKind: "none" });
  }
  return merged;
}

// Mirrors group_words_into_phrases in src-tauri/src/captions.rs — breaks
// at sentence/clause punctuation instead of a blind word count, falling
// back to `maxWords` as a cap when a run has no punctuation that long.
export function groupWordsIntoChunks(words, maxWords) {
  const size = Math.max(1, maxWords);
  const chunks = [];
  let current = [];
  for (const { word, breakKind } of mergePunctuation(words)) {
    current.push(word);
    if (breakKind !== "none" || current.length >= size) {
      chunks.push(current);
      current = [];
    }
  }
  if (current.length > 0) chunks.push(current);
  return chunks;
}

export function findActiveChunk(chunks, currentTime) {
  return chunks.find((chunk) => {
    const first = chunk[0];
    const last = chunk[chunk.length - 1];
    return first && last && currentTime >= first.start && currentTime < last.end;
  });
}

// Same match as findActiveChunk, but returns the index so cascade-mode
// preview can also look up the chunk just before it (see captions.rs
// build_cascade_text, which the cascade preview mirrors).
export function findActiveChunkIndex(chunks, currentTime) {
  return chunks.findIndex((chunk) => {
    const first = chunk[0];
    const last = chunk[chunk.length - 1];
    return first && last && currentTime >= first.start && currentTime < last.end;
  });
}
