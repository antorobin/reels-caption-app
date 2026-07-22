// Suggests how long a piece of on-screen text should stay visible for a
// viewer to comfortably read it — the same idea subtitle-timing guidelines
// use (e.g. Netflix's ~17 characters/second reading-speed target), tuned
// a little more conservative since this is a standalone caption competing
// with the rest of the video for attention, not a dedicated subtitle line.
const CHARS_PER_SECOND = 15;
const MIN_SECONDS = 1.2;
const MAX_SECONDS = 8;

export function estimateReadingDurationSeconds(text) {
  const trimmed = (text ?? "").trim();
  if (!trimmed) return MIN_SECONDS;
  const estimated = trimmed.length / CHARS_PER_SECOND;
  return Math.round(Math.min(MAX_SECONDS, Math.max(MIN_SECONDS, estimated)) * 10) / 10;
}
