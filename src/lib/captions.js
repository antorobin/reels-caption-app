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

// Mirrors build_style_timeline in captions.rs — the base style fills
// every gap around/between the (already possibly-unsorted) `overrides`,
// producing a list of {start, end, style, name} pieces that exactly
// partitions [0, +Infinity) with no gaps or overlaps. The open upper
// bound means the last piece always covers "the rest of the video"
// without needing to know its actual duration here.
export function buildStyleTimeline(overrides, baseStyle) {
  const sorted = [...overrides].sort((a, b) => a.start - b.start);
  const pieces = [];
  let cursor = 0;
  sorted.forEach((ov, i) => {
    if (ov.start > cursor) pieces.push({ start: cursor, end: ov.start, style: baseStyle, name: "Default" });
    pieces.push({ start: ov.start, end: ov.end, style: ov.style, name: `Override${i}` });
    cursor = Math.max(cursor, ov.end);
  });
  pieces.push({ start: cursor, end: Infinity, style: baseStyle, name: "Default" });
  return pieces.filter((p) => p.end > p.start);
}

// Mirrors build_chunk_timeline in captions.rs — chunks each style piece's
// own word slice independently with *that piece's own* words_per_line,
// then concatenates in time order. A word belongs to whichever piece its
// own start time falls in (never split across two pieces), matching the
// same half-open-interval convention the Rust side uses. Built on
// groupWordsIntoChunks rather than duplicating its punctuation-aware
// breaking logic.
export function buildChunkTimeline(words, pieces) {
  const out = [];
  pieces.forEach((piece, pieceIndex) => {
    const pieceWords = words.filter((w) => w.start >= piece.start && w.start < piece.end);
    if (pieceWords.length === 0) return;
    groupWordsIntoChunks(pieceWords, piece.style.words_per_line).forEach((chunk) => {
      out.push({ pieceIndex, chunk, style: piece.style, styleName: piece.name });
    });
  });
  return out;
}

// Resolves which chunk (and its style) is active at `currentTime` against
// a chunk timeline built by buildChunkTimeline -- the live-preview
// equivalent of captions.rs's per-Dialogue-line style resolution. `prev`
// only reaches back within the *same* style piece, mirroring
// build_ass_document's own `chunk_pieces[i-1].piece_index == cp.piece_index`
// check -- a cascade preview's "just-finished phrase" line should never
// stitch the tail of one override onto the head of the next.
export function findActiveChunkEntry(chunkTimeline, currentTime) {
  const index = chunkTimeline.findIndex(({ chunk }) => {
    const first = chunk[0];
    const last = chunk[chunk.length - 1];
    return first && last && currentTime >= first.start && currentTime < last.end;
  });
  if (index === -1) return null;
  const entry = chunkTimeline[index];
  const prev = index > 0 && chunkTimeline[index - 1].pieceIndex === entry.pieceIndex ? chunkTimeline[index - 1].chunk : null;
  return { style: entry.style, chunk: entry.chunk, prev };
}

// UI-side non-overlap guard for the drag-select-a-range flow -- overrides
// are never allowed to overlap (enforced here, at creation time, not by
// any runtime tie-breaking logic in the style-timeline resolution above).
export function overrideRangeOverlaps(start, end, existingOverrides, excludeIndex = -1) {
  return existingOverrides.some((o, i) => i !== excludeIndex && start < o.end && end > o.start);
}
