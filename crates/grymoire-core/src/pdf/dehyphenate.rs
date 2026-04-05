//! Dehyphenation: merge words split across lines by layout hyphens.
//!
//! Uses a dictionary to distinguish layout hyphens ("distor-\ntion" → "distortion")
//! from real compound hyphens ("fixed-point" stays as "fixed-point").
//!
//! The key heuristic: if the fragment before the hyphen is NOT a standalone word,
//! it's almost certainly a layout hyphen. Real compound hyphens join two real words.

use std::collections::HashSet;
use std::sync::LazyLock;

/// Embedded word list (SCOWL, 277K words, permissive license).
/// Lowercase, alphabetic only, 3+ chars.
static WORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    let raw = include_str!("words.txt");
    raw.lines().filter(|l| !l.is_empty()).collect()
});

fn is_word(s: &str) -> bool {
    WORDS.contains(s)
}

/// Post-process extracted text to merge layout-hyphenated words.
///
/// Detects two patterns:
/// 1. `<fragment>-\n<continuation>` — hyphen at end of line
/// 2. `<fragment>\n-\n<continuation>` — hyphen on its own line
///
/// Merges when the fragment before the hyphen is not a standalone dictionary word.
pub(crate) fn dehyphenate(text: &str) -> String {
    // Work line-by-line for clearer pattern matching
    let lines: Vec<&str> = text.lines().collect();
    let mut result = String::with_capacity(text.len());
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim_end();

        // Pattern 1: line ends with hyphen, next line continues
        if line.ends_with('-') && i + 1 < lines.len() {
            let before = &line[..line.len() - 1]; // strip the hyphen
            let next_line = lines[i + 1].trim_start();
            if let Some(merge_result) = try_merge(before, next_line) {
                result.push_str(&merge_result);
                // The rest of the next line (after the merged word) is already in merge_result
                result.push('\n');
                i += 2;
                continue;
            }
        }

        // Pattern 2: next line is just "-", line after that continues
        if i + 2 < lines.len() && lines[i + 1].trim() == "-" {
            let next_line = lines[i + 2].trim_start();
            if let Some(merge_result) = try_merge(line, next_line) {
                result.push_str(&merge_result);
                result.push('\n');
                i += 3;
                continue;
            }
        }

        result.push_str(line);
        result.push('\n');
        i += 1;
    }

    // Remove the trailing newline we added
    if result.ends_with('\n') && !text.ends_with('\n') {
        result.pop();
    }

    result
}

/// Try to merge a line ending with a word fragment and a continuation line.
/// Returns the merged line if dehyphenation should happen, None otherwise.
fn try_merge(before_line: &str, next_line: &str) -> Option<String> {
    // Extract the word fragment at the end of the before line
    let before_frag = extract_word_at_end(before_line);
    if before_frag.is_empty() {
        return None;
    }

    // Extract the word fragment at the start of the next line
    let (after_frag, rest_of_next) = extract_word_at_start(next_line);
    if after_frag.is_empty() {
        return None;
    }

    // Next fragment must start lowercase (layout hyphens continue lowercase)
    if !after_frag.starts_with(|c: char| c.is_ascii_lowercase()) {
        return None;
    }

    let before_lower = before_frag.to_lowercase();
    let after_lower = after_frag.to_lowercase();
    let merged = format!("{before_lower}{after_lower}");

    let before_is_word = is_word(&before_lower);
    let merged_is_word = is_word(&merged);

    // Merge if:
    // - fragment before hyphen is not a word ("distor" → layout hyphen)
    // - OR continuation is a common suffix ("precise" + "ly" → "precisely")
    // Keep hyphen if fragment IS a word and continuation isn't a suffix
    // ("fixed" + "point" → keep "fixed-point")
    let should_merge =
        !before_is_word || (merged_is_word && is_suffix(&after_lower));

    if should_merge {
        // Rebuild the line with the merged word
        let prefix = &before_line[..before_line.len() - before_frag.len()];
        Some(format!("{prefix}{before_frag}{after_frag}{rest_of_next}"))
    } else {
        None
    }
}

/// Check if a word fragment is a common English suffix that wouldn't stand
/// alone in a compound hyphenation (e.g., "-ly", "-tion", "-ing").
fn is_suffix(s: &str) -> bool {
    matches!(
        s,
        "ly" | "tion" | "sion" | "ment" | "ness" | "ing" | "ed" | "er" | "est"
            | "ous" | "ive" | "ful" | "less" | "able" | "ible" | "al" | "ial"
            | "ity" | "ies" | "ence" | "ance" | "ure" | "ture"
            | "ling" | "dings" | "tions" | "sions" | "ments" | "ously"
            | "ively" | "fully" | "lessly" | "ably" | "ibly"
            | "ating" | "ting" | "ning" | "ring"
            | "ated" | "ized" | "ised"
            | "ization" | "isation" | "iously"
            | "ety" | "ary" | "ery" | "ory"
    )
}

/// Extract the word fragment at the end of a line.
fn extract_word_at_end(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] == b' ' {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && bytes[start - 1].is_ascii_alphabetic() {
        start -= 1;
    }
    &line[start..end]
}

/// Extract the word fragment at the start of a line, returning (fragment, rest).
fn extract_word_at_start(line: &str) -> (&str, &str) {
    let end = line
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(line.len());
    (&line[..end], &line[end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_layout_hyphen() {
        let input = "inharmonic distor-\ntion it becomes";
        let result = dehyphenate(input);
        assert!(
            result.contains("distortion"),
            "should merge 'distor-\\ntion' into 'distortion', got: {result}"
        );
    }

    #[test]
    fn test_merge_hyphen_on_own_line() {
        // Pattern 2: hyphen on its own line
        let input = "inharmonic distor\n-\ntion it becomes";
        let result = dehyphenate(input);
        assert!(
            result.contains("distortion"),
            "should merge 'distor\\n-\\ntion' into 'distortion', got: {result}"
        );
    }

    #[test]
    fn test_keep_real_hyphen() {
        let input = "the fixed-\npoint implementation";
        let result = dehyphenate(input);
        assert!(
            result.contains("fixed-"),
            "should keep hyphen in 'fixed-point', got: {result}"
        );
    }

    #[test]
    fn test_keep_inline_hyphen() {
        let input = "self-contained system";
        let result = dehyphenate(input);
        assert!(result.contains("self-contained"), "got: {result}");
    }

    #[test]
    fn test_merge_implementation() {
        let input = "the imple-\nmentation of";
        let result = dehyphenate(input);
        assert!(
            result.contains("implementation"),
            "should merge, got: {result}"
        );
    }

    #[test]
    fn test_keep_uppercase_continuation() {
        let input = "the Anglo-\nSaxon era";
        let result = dehyphenate(input);
        assert!(
            result.contains("Anglo-"),
            "should keep hyphen before uppercase, got: {result}"
        );
    }

    #[test]
    fn test_no_change_without_hyphen() {
        let input = "hello world\nfoo bar";
        assert_eq!(dehyphenate(input), input);
    }

    #[test]
    fn test_merge_suffix() {
        // "precise" is a word, but "-ly" is a suffix → merge to "precisely"
        let input = "can be precise-\nly represented";
        let result = dehyphenate(input);
        assert!(result.contains("precisely"), "got: {result}");
    }

    #[test]
    fn test_merge_suffix_own_line() {
        let input = "can be precise\n-\nly represented";
        let result = dehyphenate(input);
        assert!(result.contains("precisely"), "got: {result}");
    }

    #[test]
    fn test_word_lookup() {
        assert!(is_word("distortion"));
        assert!(is_word("fixed"));
        assert!(is_word("point"));
        assert!(!is_word("distor"));
        assert!(!is_word("imple"));
    }
}
