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

// Snaps a raw time to the start of whichever word is closest — so a range
// (whether dragged by hand on Timeline.jsx or auto-suggested below) always
// lands exactly where a real override boundary takes effect: a word is
// assigned to a style-timeline piece by its own start time (see
// buildChunkTimeline above), never split across two pieces.
export function snapToNearestWordStart(time, words) {
  if (!words || words.length === 0) return time;
  let closest = words[0].start;
  let closestDistance = Math.abs(words[0].start - time);
  for (const w of words) {
    const distance = Math.abs(w.start - time);
    if (distance < closestDistance) {
      closest = w.start;
      closestDistance = distance;
    }
  }
  return closest;
}

// Thresholds for suggestTransitionPoints below. SILENCE_GAP_SECONDS
// reuses the exact value VideoPreview.jsx's own DUCK_SPEECH_MERGE_GAP_SECONDS
// already established as "a meaningful gap" in this codebase, rather than
// inventing a second, potentially-inconsistent number for the same idea.
const SILENCE_GAP_SECONDS = 0.6;
const LONG_SILENCE_GAP_SECONDS = 1.5;
// Candidates from different signals within this many seconds of each
// other are treated as "the same real moment" and merged into one
// suggestion, combining their reasons -- multiple signals agreeing is
// what makes a suggestion higher-confidence, not any one signal alone.
const MERGE_WINDOW_SECONDS = 0.5;
const PACE_WINDOW_SECONDS = 3.0;
const PACE_CHANGE_RATIO = 1.6;
const MAX_SUGGESTIONS = 12;

// A fourth signal alongside speaker changes/silence/prosody (all reused
// from data this app already computes): local speaking-rate shifts,
// comparing words-per-second just before vs. just after each word purely
// from existing timestamps, no new analysis pass needed. A sudden
// speedup/slowdown often marks a real tonal or topical shift (e.g.
// energetic hook -> measured explanation) that the other three signals
// can miss entirely if the speaker, pauses, and vocal emphasis all stay
// flat through it.
function detectPaceChanges(words) {
  const candidates = [];
  for (let i = 1; i < words.length; i++) {
    const t = words[i].start;
    const before = words.filter((w) => w.start >= t - PACE_WINDOW_SECONDS && w.start < t).length / PACE_WINDOW_SECONDS;
    const after = words.filter((w) => w.start >= t && w.start < t + PACE_WINDOW_SECONDS).length / PACE_WINDOW_SECONDS;
    // Too sparse a window (near a clip edge, or inside a long silence
    // already flagged by the gap signal) to trust a rate ratio from.
    if (before < 0.5 || after < 0.5) continue;
    const ratio = after / before;
    if (ratio >= PACE_CHANGE_RATIO) candidates.push({ time: t, reason: "Speeds up" });
    else if (ratio <= 1 / PACE_CHANGE_RATIO) candidates.push({ time: t, reason: "Slows down" });
  }
  return candidates;
}

// Suggests candidate points for a style-override pin by combining four
// signals, all computed purely from data this app already has (no new
// backend analysis): diarized speaker changes, silence/pause gaps,
// prosody-driven vocal-emphasis jumps into "high" intensity, and local
// speaking-pace shifts (above). Nearby candidates from different signals
// are merged into one suggestion whose `confidence` is how many distinct
// signals agreed on roughly that moment -- a speaker change that also
// lands on a long pause is a much stronger transition candidate than
// either alone. Suggestions landing inside an already-styled override are
// dropped (nothing useful to suggest there), and the result is capped so
// a long video's timeline doesn't get cluttered with dozens of markers.
export function suggestTransitionPoints(words, speakers, prosody, existingOverrides = []) {
  const raw = [];

  const sortedSpeakers = [...speakers].sort((a, b) => a.start - b.start);
  for (let i = 1; i < sortedSpeakers.length; i++) {
    if (sortedSpeakers[i].speaker_id !== sortedSpeakers[i - 1].speaker_id) {
      // Carries which speaker is *starting* here -- autoThemeIdForSuggestion
      // (themes.js) uses this to rotate through a small set of themes keyed
      // by speaker, not just note that "a change" happened.
      raw.push({ time: sortedSpeakers[i].start, reason: "Speaker change", speakerId: sortedSpeakers[i].speaker_id });
    }
  }

  for (let i = 1; i < words.length; i++) {
    const gap = words[i].start - words[i - 1].end;
    if (gap >= LONG_SILENCE_GAP_SECONDS) raw.push({ time: words[i].start, reason: "Long pause" });
    else if (gap >= SILENCE_GAP_SECONDS) raw.push({ time: words[i].start, reason: "Pause" });
  }

  const sortedProsody = [...prosody].sort((a, b) => a.start - b.start);
  for (let i = 1; i < sortedProsody.length; i++) {
    if (sortedProsody[i].intensity === "high" && sortedProsody[i - 1].intensity !== "high") {
      raw.push({ time: sortedProsody[i].start, reason: "Emphasis" });
    }
  }

  raw.push(...detectPaceChanges(words));

  if (raw.length === 0) return [];

  raw.sort((a, b) => a.time - b.time);
  const merged = [];
  for (const c of raw) {
    const last = merged[merged.length - 1];
    if (last && c.time - last.time <= MERGE_WINDOW_SECONDS) {
      if (!last.reasons.includes(c.reason)) last.reasons.push(c.reason);
      if (c.speakerId != null) last.speakerId = c.speakerId;
    } else {
      merged.push({ time: c.time, reasons: [c.reason], speakerId: c.speakerId ?? null });
    }
  }

  return merged
    .filter((m) => !existingOverrides.some((o) => m.time >= o.start && m.time < o.end))
    .map((m) => ({
      time: snapToNearestWordStart(m.time, words),
      reasons: m.reasons,
      speakerId: m.speakerId,
      confidence: m.reasons.length,
    }))
    .sort((a, b) => b.confidence - a.confidence || a.time - b.time)
    .slice(0, MAX_SUGGESTIONS)
    .sort((a, b) => a.time - b.time);
}

// Default duration for an auto-applied override (no explicit end point
// given, unlike a hand-dragged range) -- long enough to read as a real
// stylistic beat, short enough not to swallow whatever comes next by
// default.
const AUTO_RANGE_DEFAULT_SPAN_SECONDS = 4;

// Determines the [start, end) range for one-click "auto-apply" on a
// suggestion -- extends from the suggestion's own time for
// AUTO_RANGE_DEFAULT_SPAN_SECONDS, but never past whatever the *next*
// suggestion in the same list is (so auto-applying one transition
// doesn't silently swallow the next one before the user gets to react to
// it), and never past the video's own end.
export function autoRangeForSuggestion(time, allSuggestions, duration) {
  const next = allSuggestions.find((s) => s.time > time + 1.0);
  const uncappedEnd = time + AUTO_RANGE_DEFAULT_SPAN_SECONDS;
  const cappedEnd = next && next.time < uncappedEnd ? next.time : uncappedEnd;
  return { start: time, end: Math.min(cappedEnd, duration) };
}

// Display name for each entry in video_transitions.rs's effect library --
// shared by every place that renders one (Timeline.jsx's popovers and
// marker tooltips, MoreOptionsModal.jsx's list), so a new effect only
// needs a label added here, not a ternary updated in three files.
export const TRANSITION_EFFECT_LABELS = {
  "zoom-punch": "🎬 Zoom punch",
  "flash-cut": "🎬 Flash cut",
  shake: "🎬 Shake",
  "color-pulse": "🎬 Color pulse",
};

// Same rule-based approach as themes.js's autoThemeIdForSuggestion, not a
// model -- picking one of a small, fixed set of categorical options from
// signals suggestTransitionPoints already computed (speaker change, pace,
// pause, emphasis) is a plain priority-ordered mapping, not an open-ended
// generation task; there's no real training data for "which transition
// effect is best" and a rule encodes the same editorial logic a model
// would have to learn anyway. Kept here (not themes.js) since it's about
// video_transitions.rs's effect library, not caption styling.
//
// The editorial logic, now spread across all four effects: a flash cut is
// the traditional hard-punctuation mark between two different speakers or
// during a quiet beat -- brief, low-energy. A zoom punch rides *rising*
// energy -- vocal emphasis, the clearest "lean in" moment. A shake is a
// jarring, kinetic jolt -- a better match for a sudden pace speedup than
// zoom's smoother push-in. A color pulse (a brief desaturate) reads as a
// tonal/mood shift -- a gentler fit for slowing down than a flash's
// abruptness.
export function autoTransitionEffectForSuggestion(reasons) {
  if (reasons.includes("Emphasis")) return "zoom-punch";
  if (reasons.includes("Speaker change")) return "flash-cut";
  if (reasons.includes("Speeds up")) return "shake";
  if (reasons.includes("Slows down")) return "color-pulse";
  if (reasons.includes("Long pause") || reasons.includes("Pause")) return "flash-cut";
  return "zoom-punch";
}
