// RAKE-lite (Rapid Automatic Keyword Extraction) — pure JS, no backend
// roundtrip, no model. Standard technique: split the transcript into
// candidate phrases at stopword/punctuation boundaries, score each word
// by degree/frequency (how often it co-occurs with other words in a
// phrase, divided by how often it appears alone), sum word scores per
// phrase, then take the top-scoring phrases.

const DEFAULT_STOPWORDS = new Set([
  "a", "about", "above", "after", "again", "against", "all", "am", "an", "and", "any", "are", "aren't", "as", "at",
  "be", "because", "been", "before", "being", "below", "between", "both", "but", "by",
  "can", "can't", "cannot", "could", "couldn't",
  "did", "didn't", "do", "does", "doesn't", "doing", "don't", "down", "during",
  "each", "few", "for", "from", "further",
  "had", "hadn't", "has", "hasn't", "have", "haven't", "having", "he", "he'd", "he'll", "he's", "her", "here",
  "here's", "hers", "herself", "him", "himself", "his", "how", "how's",
  "i", "i'd", "i'll", "i'm", "i've", "if", "in", "into", "is", "isn't", "it", "it's", "its", "itself",
  "let's", "me", "more", "most", "mustn't", "my", "myself",
  "no", "nor", "not", "of", "off", "on", "once", "only", "or", "other", "ought", "our", "ours", "ourselves", "out",
  "over", "own",
  "same", "shan't", "she", "she'd", "she'll", "she's", "should", "shouldn't", "so", "some", "such",
  "than", "that", "that's", "the", "their", "theirs", "them", "themselves", "then", "there", "there's", "these",
  "they", "they'd", "they'll", "they're", "they've", "this", "those", "through", "to", "too",
  "under", "until", "up",
  "very",
  "was", "wasn't", "we", "we'd", "we'll", "we're", "we've", "were", "weren't", "what", "what's", "when", "when's",
  "where", "where's", "which", "while", "who", "who's", "whom", "why", "why's", "with", "won't", "would", "wouldn't",
  "you", "you'd", "you'll", "you're", "you've", "your", "yours", "yourself", "yourselves",
  "um", "uh", "like", "just", "so", "really", "gonna", "wanna", "kinda", "okay", "ok", "yeah",
]);

function cleanWord(raw) {
  return raw.toLowerCase().replace(/[^a-z0-9']/g, "");
}

/** Splits `words` (transcript word objects with a `.word` string) into candidate phrases at stopword/punctuation boundaries. */
function extractCandidatePhrases(words, stopwords) {
  const phrases = [];
  let current = [];
  for (const w of words) {
    const cleaned = cleanWord(w.word ?? w);
    const isStopwordOrEmpty = cleaned.length === 0 || stopwords.has(cleaned);
    if (isStopwordOrEmpty) {
      if (current.length > 0) phrases.push(current);
      current = [];
    } else {
      current.push(cleaned);
    }
  }
  if (current.length > 0) phrases.push(current);
  return phrases;
}

/** RAKE word score: degree (co-occurrence count within phrases) / frequency. */
function scoreWords(phrases) {
  const freq = new Map();
  const degree = new Map();
  for (const phrase of phrases) {
    const phraseDegree = phrase.length - 1;
    for (const word of phrase) {
      freq.set(word, (freq.get(word) ?? 0) + 1);
      degree.set(word, (degree.get(word) ?? 0) + phraseDegree);
    }
  }
  const scores = new Map();
  for (const [word, f] of freq) {
    const d = degree.get(word) ?? 0;
    scores.set(word, (d + f) / f);
  }
  return scores;
}

function toHashtag(phraseWords) {
  return "#" + phraseWords.map((w) => w.charAt(0).toUpperCase() + w.slice(1)).join("");
}

/**
 * Suggests hashtags from a transcript's word list.
 * @param {Array<{word: string}>} words
 * @param {{topN?: number, stopwords?: Set<string>}} [options]
 * @returns {string[]}
 */
export function suggestHashtags(words, { topN = 8, stopwords = DEFAULT_STOPWORDS } = {}) {
  if (!words || words.length === 0) return [];

  const phrases = extractCandidatePhrases(words, stopwords);
  if (phrases.length === 0) return [];

  const wordScores = scoreWords(phrases);

  const phraseScores = phrases.map((phrase) => ({
    phrase,
    score: phrase.reduce((sum, w) => sum + (wordScores.get(w) ?? 0), 0),
  }));

  phraseScores.sort((a, b) => b.score - a.score);

  const seen = new Set();
  const hashtags = [];
  for (const { phrase } of phraseScores) {
    const key = phrase.join(" ");
    if (seen.has(key)) continue;
    seen.add(key);
    hashtags.push(toHashtag(phrase));
    if (hashtags.length >= topN) break;
  }
  return hashtags;
}
