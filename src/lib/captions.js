// Mirrors group_words_into_lines in src-tauri/src/captions.rs — same
// chunking rule, so the live preview shows the same caption groupings
// that'll actually get burned in.
export function groupWordsIntoChunks(words, wordsPerLine) {
  const size = Math.max(1, wordsPerLine);
  const chunks = [];
  for (let i = 0; i < words.length; i += size) {
    chunks.push(words.slice(i, i + size));
  }
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
