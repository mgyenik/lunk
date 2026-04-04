//! Title generation for archived entries.
//!
//! Extracts the best possible title from an entry's content using a
//! priority chain: readable HTML headings → PDF metadata → scored text heuristics.

/// Generate a title from an article's readable HTML.
/// Extracts the first <h1>, falling back to <h2> or <h3>.
pub fn title_from_readable_html(html: &[u8]) -> Option<String> {
    let html = std::str::from_utf8(html).ok()?;

    for tag in ["h1", "h2", "h3"] {
        if let Some(title) = extract_first_tag(html, tag) {
            let clean = strip_html_tags(&title);
            let trimmed = clean.trim().to_string();
            if trimmed.len() >= 5 && !is_junk_heading(&trimmed) {
                return Some(truncate(&trimmed, 150));
            }
        }
    }

    None
}

/// Generate a title from extracted text using scored heuristics.
/// Used as a fallback when HTML headings and PDF metadata are unavailable.
pub fn title_from_text(text: &str) -> Option<String> {
    let mut best: Option<(i32, String)> = None;

    for (i, line) in text.lines().enumerate().take(30) {
        let trimmed = line.trim();
        let len = trimmed.len();

        // Hard filters
        if !(10..=200).contains(&len) {
            continue;
        }
        if is_junk_line(trimmed) {
            continue;
        }

        // Score this line
        let mut score: i32 = 0;

        // Position bonus — earlier lines more likely to be titles
        score += (25 - i as i32).max(0);

        // Length sweet spot (30-120 chars)
        if (30..=120).contains(&len) {
            score += 10;
        } else if len < 30 {
            score += 3; // Short but not disqualified
        }

        // Doesn't end with period (titles usually don't)
        if !trimmed.ends_with('.') {
            score += 5;
        }

        // Penalty for paragraph-like openings
        if starts_like_body_text(trimmed) {
            score -= 15;
        }

        // Penalty for all-uppercase short lines (section headers like "ABSTRACT")
        if len < 30 && trimmed.chars().all(|c| c.is_uppercase() || !c.is_alphabetic()) {
            score -= 10;
        }

        // Bonus for title-case-like text
        let words: Vec<&str> = trimmed.split_whitespace().collect();
        if words.len() >= 3 {
            let capitalized = words.iter().filter(|w| {
                w.chars().next().is_some_and(|c| c.is_uppercase())
            }).count();
            if capitalized > words.len() / 2 {
                score += 5;
            }
        }

        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((score, truncate(trimmed, 120)));
        }
    }

    best.map(|(_, t)| t)
}

/// Clean up a title: strip site name suffixes, common prefixes, normalize whitespace.
pub fn clean_title(title: &str) -> String {
    let mut t = title.trim().to_string();

    // Collapse whitespace
    t = t.split_whitespace().collect::<Vec<_>>().join(" ");

    // Remove common site name separators at the end (keep the longer part)
    for sep in [" - ", " | ", " :: ", " — ", " – "] {
        if let Some(pos) = t.rfind(sep) {
            let before = &t[..pos];
            let after = &t[pos + sep.len()..];
            // Keep the longer part if it's substantial
            if before.len() > after.len() && before.len() >= 15 {
                t = before.trim().to_string();
            } else if after.len() > before.len() && after.len() >= 15 {
                t = after.trim().to_string();
            }
        }
    }

    // Remove leading prefixes
    for prefix in ["PDF: ", "Title: ", "Re: ", "RE: "] {
        if let Some(rest) = t.strip_prefix(prefix) {
            t = rest.trim().to_string();
        }
    }

    t
}

/// Check if a line is junk that should never be a title.
fn is_junk_line(line: &str) -> bool {
    let l = line.trim().to_lowercase();

    // URLs
    if l.starts_with("http") || l.starts_with("www.") || l.starts_with("mailto:") {
        return true;
    }

    // Navigation / boilerplate
    let boilerplate = [
        "skip to main content", "skip to content", "skip navigation",
        "table of contents", "home", "menu", "search", "subscribe",
        "share", "print", "close", "sign in", "log in", "sign up",
        "cookie", "privacy policy", "terms of service", "all rights reserved",
        "copyright", "advertisement", "sponsored",
    ];
    if boilerplate.iter().any(|b| l == *b || l.starts_with(b)) {
        return true;
    }

    // Date-like patterns at the start
    if l.starts_with("posted on")
        || l.starts_with("published")
        || l.starts_with("updated")
        || l.starts_with("date:")
    {
        return true;
    }

    // Page numbers, metadata
    if l.starts_with("page ") && l.len() < 15 {
        return true;
    }

    // Author affiliations / email
    if l.starts_with("author") || l.contains('@') && l.contains('.') {
        return true;
    }

    // "Subscriber access provided by..." (common in academic PDFs)
    if l.starts_with("subscriber access") {
        return true;
    }

    // Pure numbers
    if l.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-' || c == ' ') {
        return true;
    }

    false
}

/// Check if a heading tag content is junk (section labels, not real titles).
fn is_junk_heading(heading: &str) -> bool {
    let l = heading.trim().to_lowercase();
    matches!(
        l.as_str(),
        "abstract" | "introduction" | "references" | "conclusion"
            | "acknowledgments" | "acknowledgements" | "appendix"
            | "contents" | "table of contents" | "bibliography"
    ) || l.starts_with("1.") && l.len() < 20 // "1. Introduction"
}

/// Check if a line starts like body text (not a title).
fn starts_like_body_text(line: &str) -> bool {
    let starters = [
        "the ", "this ", "these ", "those ", "that ",
        "in this ", "we ", "i ", "it ", "there ",
        "a ", "an ", "as ", "for ", "to ",
        "however", "although", "because", "since",
        "figure ", "table ", "note:",
    ];
    let lower = line.to_lowercase();
    starters.iter().any(|s| lower.starts_with(s))
}

/// Extract the text content of the first occurrence of an HTML tag.
fn extract_first_tag(html: &str, tag: &str) -> Option<String> {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);

    let start_pos = html.find(&open)?;
    let after_open = &html[start_pos..];
    let gt_pos = after_open.find('>')?;
    let content_start = start_pos + gt_pos + 1;
    let content_end = html[content_start..].find(&close)? + content_start;

    Some(html[content_start..content_end].to_string())
}

/// Strip HTML tags from a string.
fn strip_html_tags(s: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            result.push(c);
        }
    }
    result
}

/// Truncate a string to a maximum length at a word boundary.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    // Find the last space before max
    match s[..max].rfind(' ') {
        Some(pos) if pos > max / 2 => s[..pos].to_string(),
        _ => s.chars().take(max).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_title_from_readable_html() {
        let html = b"<div><h1>My Great Article</h1><p>Some text</p></div>";
        assert_eq!(title_from_readable_html(html), Some("My Great Article".into()));
    }

    #[test]
    fn test_title_from_html_h2_fallback() {
        let html = b"<div><h2>Fallback Heading</h2></div>";
        assert_eq!(title_from_readable_html(html), Some("Fallback Heading".into()));
    }

    #[test]
    fn test_title_from_html_skips_junk_headings() {
        let html = b"<div><h1>Abstract</h1><h2>Real Title Here</h2></div>";
        assert_eq!(title_from_readable_html(html), Some("Real Title Here".into()));
    }

    #[test]
    fn test_title_from_text_basic() {
        let text = "AN133 - A Closed-Loop, Wideband, 100A Active Load\nSome body text follows here.";
        let title = title_from_text(text).unwrap();
        assert!(title.contains("AN133"), "got: {title}");
    }

    #[test]
    fn test_title_from_text_skips_junk() {
        let text = "Skip to main content\nCopyright 2024\nThe Real Title of This Document\nSome body text.";
        let title = title_from_text(text).unwrap();
        assert!(title.contains("Real Title"), "got: {title}");
    }

    #[test]
    fn test_title_from_text_skips_body_openers() {
        let text = "The quick brown fox jumped over the lazy dog and this is clearly a paragraph not a title.\nMP6002 Monolithic Flyback Converter";
        let title = title_from_text(text).unwrap();
        assert!(title.contains("MP6002"), "got: {title}");
    }

    #[test]
    fn test_clean_title_strips_suffix() {
        assert_eq!(
            clean_title("A Very Long Article Title - Reddit"),
            "A Very Long Article Title"
        );
        assert_eq!(clean_title("Title | Site Name"), "Title | Site Name"); // too short to split
        assert_eq!(
            clean_title("A Very Long Title Indeed - Some Website"),
            "A Very Long Title Indeed"
        );
    }

    #[test]
    fn test_is_junk_line() {
        assert!(is_junk_line("Skip to main content"));
        assert!(is_junk_line("https://example.com/page"));
        assert!(is_junk_line("Posted on March 18, 2017"));
        assert!(is_junk_line("Author affiliations"));
        assert!(is_junk_line("Subscriber access provided by UNIV OF SOUTHAMPTON"));
        assert!(!is_junk_line("A Review of Impedance Measurements"));
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("short", 100), "short");
        assert_eq!(truncate("hello world this is long", 15), "hello world");
    }

    #[test]
    fn test_title_from_html_nested_tags() {
        let html = b"<div><h1><a href='#'>Linked Title</a></h1></div>";
        assert_eq!(title_from_readable_html(html), Some("Linked Title".into()));
    }

    #[test]
    fn test_title_from_html_empty() {
        let html = b"<div><p>No headings here</p></div>";
        assert_eq!(title_from_readable_html(html), None);
    }

    #[test]
    fn test_title_from_html_short_heading() {
        let html = b"<h1>Hi</h1><h2>Real Title Here</h2>";
        // "Hi" is too short (<5 chars), should fall back to h2
        assert_eq!(title_from_readable_html(html), Some("Real Title Here".into()));
    }

    #[test]
    fn test_title_from_text_all_junk() {
        let text = "Skip to content\nhttps://example.com\nCopyright 2024\nPage 1\n";
        assert_eq!(title_from_text(text), None);
    }

    #[test]
    fn test_title_from_text_prefers_earlier_lines() {
        let text = "First Good Title Line Here\nSecond Also Good Title Line\nBody text follows.";
        let title = title_from_text(text).unwrap();
        assert!(title.contains("First"), "should prefer first line, got: {title}");
    }

    #[test]
    fn test_clean_title_collapses_whitespace() {
        assert_eq!(clean_title("  Title   with   spaces  "), "Title with spaces");
    }

    #[test]
    fn test_clean_title_strips_prefix() {
        assert_eq!(clean_title("PDF: Some Document"), "Some Document");
        assert_eq!(clean_title("Re: Discussion Thread Title Here"), "Discussion Thread Title Here");
    }

    #[test]
    fn test_clean_title_multiple_separators() {
        let result = clean_title("Article Title - Subtitle - Site Name");
        // Should strip the last separator (Site Name) since it's shortest
        assert!(result.contains("Article Title"), "got: {result}");
    }

    #[test]
    fn test_is_junk_line_comprehensive() {
        // URLs
        assert!(is_junk_line("https://example.com/page"));
        assert!(is_junk_line("www.example.com"));
        assert!(is_junk_line("mailto:user@example.com"));
        // Navigation
        assert!(is_junk_line("Skip to main content"));
        assert!(is_junk_line("Subscribe to newsletter"));
        // Dates
        assert!(is_junk_line("Posted on January 5, 2024"));
        assert!(is_junk_line("Published: March 2023"));
        // Metadata
        assert!(is_junk_line("Page 12"));
        assert!(is_junk_line("Author affiliations"));
        assert!(is_junk_line("Subscriber access provided by MIT"));
        // Numbers
        assert!(is_junk_line("12345"));
        assert!(is_junk_line("3.14 - 2.71"));
        // Good titles should NOT be junk
        assert!(!is_junk_line("Digital Filter Design for Audio"));
        assert!(!is_junk_line("A Review of Impedance Spectroscopy Methods"));
    }

    #[test]
    fn test_is_junk_heading() {
        assert!(is_junk_heading("Abstract"));
        assert!(is_junk_heading("INTRODUCTION"));
        assert!(is_junk_heading("1. Introduction"));
        assert!(!is_junk_heading("Digital Filter Design"));
    }

    #[test]
    fn test_starts_like_body_text() {
        assert!(starts_like_body_text("The quick brown fox"));
        assert!(starts_like_body_text("In this paper we present"));
        assert!(starts_like_body_text("We propose a novel approach"));
        assert!(!starts_like_body_text("Digital Filter Design"));
        assert!(!starts_like_body_text("MP6002 Monolithic Converter"));
    }
}
