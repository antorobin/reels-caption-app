// Tamil-English (Tanglish) slang normalization: an optional post-processing
// step that collapses common spelling variants of the same colloquial word
// to one canonical spelling (e.g. "semma"/"sema"/"semmaya" -> "semma"),
// without changing word timing.
//
// Pure local string matching against a small hand-built dictionary — no
// model, no network call, nothing that needs the same kind of
// account/token setup this project has deliberately avoided elsewhere
// (see stt.rs's alignment-model note). Applied only when the creator
// opts in (`normalize_slang` on `run_pipeline`) — the transcript otherwise
// keeps whatever spelling the STT engine produced, since some creators want
// their own spelling preserved verbatim.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::pipeline::WordTimestamp;

/// (canonical spelling, [known variants including the canonical form
/// itself]) — grouped by meaning, not by language, since Tanglish mixes
/// Tamil transliteration and English freely within one word family.
const VARIANT_GROUPS: &[(&str, &[&str])] = &[
    ("semma", &["semma", "sema", "semmaya", "semmma", "simma"]),
    ("machi", &["machi", "macha", "machaa", "machan", "maccha"]),
    ("da", &["da", "daa", "dhaa"]),
    ("super", &["super", "supera", "soopar", "supr"]),
    ("vera level", &["veralevel", "veraleval", "veralevl"]),
];

static VARIANT_TO_CANONICAL: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    for (canonical, variants) in VARIANT_GROUPS {
        for variant in *variants {
            map.insert(*variant, *canonical);
        }
    }
    map
});

/// Normalizes one token's slang spelling, preserving any leading/trailing
/// punctuation and the original's capitalization. Returns `None` if the
/// token isn't a known slang variant, or is already spelled canonically —
/// callers should leave the original untouched in that case.
fn normalize_token(token: &str) -> Option<String> {
    let is_not_alnum = |c: char| !c.is_alphanumeric();
    let trimmed = token.trim_matches(is_not_alnum);
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_lowercase();
    let canonical = *VARIANT_TO_CANONICAL.get(lower.as_str())?;
    if canonical == lower {
        return None;
    }

    let prefix = &token[..token.len() - token.trim_start_matches(is_not_alnum).len()];
    let suffix = &token[token.len() - token.trim_end_matches(is_not_alnum).len()..];

    // Match the original's casing style: ALL CAPS stays all caps, a
    // capitalized first letter stays capitalized (rest lowercase, since
    // every canonical spelling in VARIANT_GROUPS is already lowercase),
    // otherwise lowercase.
    let is_all_upper = trimmed.chars().any(char::is_alphabetic) && trimmed.chars().all(|c| !c.is_alphabetic() || c.is_uppercase());
    let starts_upper = trimmed.chars().next().is_some_and(char::is_uppercase);
    let body = if is_all_upper {
        canonical.to_uppercase()
    } else if starts_upper {
        let mut chars = canonical.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => canonical.to_string(),
        }
    } else {
        canonical.to_string()
    };

    Some(format!("{prefix}{body}{suffix}"))
}

/// Normalizes slang spellings across a transcript in place. Word timing is
/// never touched — only the `word` field of entries with a known variant
/// spelling changes.
pub fn normalize_words(words: &mut [WordTimestamp]) {
    for word in words.iter_mut() {
        if let Some(normalized) = normalize_token(&word.word) {
            word.word = normalized;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_known_variant() {
        assert_eq!(normalize_token("sema").as_deref(), Some("semma"));
    }

    #[test]
    fn leaves_already_canonical_spelling_untouched() {
        assert_eq!(normalize_token("semma"), None);
    }

    #[test]
    fn leaves_unknown_word_untouched() {
        assert_eq!(normalize_token("hello"), None);
    }

    #[test]
    fn preserves_surrounding_punctuation() {
        assert_eq!(normalize_token("sema!").as_deref(), Some("semma!"));
        assert_eq!(normalize_token("\"macha,").as_deref(), Some("\"machi,"));
    }

    #[test]
    fn preserves_leading_capitalization() {
        assert_eq!(normalize_token("Sema").as_deref(), Some("Semma"));
        assert_eq!(normalize_token("MACHA").as_deref(), Some("MACHI"));
    }

    #[test]
    fn is_case_insensitive_on_lookup() {
        assert_eq!(normalize_token("SEMA").as_deref(), Some("SEMMA"));
    }

    #[test]
    fn normalize_words_updates_matching_entries_only() {
        let mut words = vec![
            WordTimestamp { word: "sema".to_string(), start: 0.0, end: 0.5 },
            WordTimestamp { word: "hello".to_string(), start: 0.5, end: 1.0 },
            WordTimestamp { word: "macha".to_string(), start: 1.0, end: 1.5 },
        ];
        normalize_words(&mut words);
        assert_eq!(words[0].word, "semma");
        assert_eq!(words[1].word, "hello");
        assert_eq!(words[2].word, "machi");
        assert_eq!(words[0].start, 0.0);
        assert_eq!(words[2].end, 1.5);
    }
}
