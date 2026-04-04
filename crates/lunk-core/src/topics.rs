//! Auto-topic clustering using FTS5 vocabulary analysis.
//!
//! Extracts distinctive terms from the FTS5 index via fts5vocab, then
//! groups entries by shared terms using agglomerative clustering. No ML
//! dependencies — pure Rust with SQLite.

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::Result;
use crate::models::Entry;
use crate::repo;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Topic {
    pub label: String,
    pub entry_ids: Vec<String>,
    pub term_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicSummary {
    pub label: String,
    pub entry_count: usize,
    pub sample_titles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveStats {
    pub total_entries: i64,
    pub pdf_count: i64,
    pub article_count: i64,
    pub domain_count: i64,
    pub recent_count: i64,
}

/// Ensure the fts5vocab virtual table exists.
pub fn ensure_vocab_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS entries_vocab
         USING fts5vocab('entries_fts', 'row');",
    )?;
    Ok(())
}

/// Compute topics by clustering entries based on shared distinctive terms.
pub fn compute_topics(conn: &Connection) -> Result<Vec<Topic>> {
    ensure_vocab_table(conn)?;

    let total_entries: i64 =
        conn.query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0))?;

    if total_entries < 4 {
        return Ok(Vec::new()); // Not enough entries to cluster
    }

    let max_doc_count = (total_entries as f64 * 0.3).max(3.0) as i64;

    // Step 1: Get distinctive terms from the vocabulary
    let mut stmt = conn.prepare(
        "SELECT term, doc, cnt FROM entries_vocab
         WHERE doc > 1 AND doc < ?1 AND length(term) >= 3
         ORDER BY doc ASC
         LIMIT 500",
    )?;

    let terms: Vec<(String, i64)> = stmt
        .query_map([max_doc_count], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .filter_map(|r| r.ok())
        .filter(|(term, _)| !is_stop_word(term) && !is_junk_term(term) && is_real_word(term))
        .collect();

    if terms.is_empty() {
        return Ok(Vec::new());
    }

    // Step 2: Build rowid → entry_id mapping
    let rowid_to_id: HashMap<i64, String> = {
        let mut stmt = conn.prepare(
            "SELECT rowid, id FROM entries",
        )?;
        stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .collect()
    };

    // Step 3: Build inverted index: term → entry_ids, and entry → terms
    let mut term_to_entries: HashMap<String, HashSet<String>> = HashMap::new();
    let mut entry_to_terms: HashMap<String, HashSet<String>> = HashMap::new();

    for (term, _doc_count) in &terms {
        // Query FTS5 for entries containing this term
        let quoted = format!("\"{}\"", term.replace('"', "\"\""));
        let mut fts_stmt = conn.prepare(
            "SELECT rowid FROM entries_fts WHERE entries_fts MATCH ?1",
        )?;

        let rowids: Vec<i64> = fts_stmt
            .query_map([&quoted], |row| row.get::<_, i64>(0))?
            .filter_map(|r| r.ok())
            .collect();

        for rowid in rowids {
            if let Some(entry_id) = rowid_to_id.get(&rowid) {
                term_to_entries
                    .entry(term.clone())
                    .or_default()
                    .insert(entry_id.clone());
                entry_to_terms
                    .entry(entry_id.clone())
                    .or_default()
                    .insert(term.clone());
            }
        }
    }

    // Step 4: Agglomerative clustering
    let clusters = cluster_entries(&entry_to_terms, &term_to_entries, total_entries);

    Ok(clusters)
}

/// Agglomerative clustering by shared distinctive terms.
fn cluster_entries(
    entry_to_terms: &HashMap<String, HashSet<String>>,
    term_to_entries: &HashMap<String, HashSet<String>>,
    total_entries: i64,
) -> Vec<Topic> {
    // Each cluster: set of entry IDs + union of their terms
    struct Cluster {
        entries: HashSet<String>,
        terms: HashSet<String>,
    }

    let mut clusters: Vec<Cluster> = entry_to_terms
        .iter()
        .map(|(id, terms)| Cluster {
            entries: HashSet::from([id.clone()]),
            terms: terms.clone(),
        })
        .collect();

    // Merge threshold: minimum shared terms to consider merging
    let merge_threshold = 2usize;

    // Greedy merging passes
    let mut merged = true;
    let max_iterations = 100;
    let mut iteration = 0;

    while merged && iteration < max_iterations {
        merged = false;
        iteration += 1;

        let mut best_score = 0.0f64;
        let mut best_pair = (0usize, 0usize);

        // Find the best pair to merge
        for i in 0..clusters.len() {
            for j in (i + 1)..clusters.len() {
                let shared: usize = clusters[i]
                    .terms
                    .intersection(&clusters[j].terms)
                    .count();

                if shared >= merge_threshold {
                    let union_size =
                        clusters[i].terms.len() + clusters[j].terms.len() - shared;
                    let jaccard = shared as f64 / union_size as f64;

                    if jaccard > best_score {
                        best_score = jaccard;
                        best_pair = (i, j);
                    }
                }
            }
        }

        if best_score > 0.0 {
            // Merge best_pair.1 into best_pair.0
            let (i, j) = best_pair;
            let removed = clusters.remove(j);
            clusters[i].entries.extend(removed.entries);
            clusters[i].terms.extend(removed.terms);
            merged = true;
        }
    }

    // Filter: keep clusters with 2+ entries, sort by size descending
    let mut topics: Vec<Topic> = clusters
        .into_iter()
        .filter(|c| c.entries.len() >= 2)
        .map(|c| {
            let label = pick_label(&c.terms, &c.entries, term_to_entries, total_entries);
            Topic {
                term_count: c.terms.len(),
                entry_ids: c.entries.into_iter().collect(),
                label,
            }
        })
        .collect();

    topics.sort_by(|a, b| b.entry_ids.len().cmp(&a.entry_ids.len()));

    // Limit to top 20 topics
    topics.truncate(20);
    topics
}

/// Pick the best 2-3 terms as a human-readable label for a cluster.
fn pick_label(
    cluster_terms: &HashSet<String>,
    cluster_entries: &HashSet<String>,
    term_to_entries: &HashMap<String, HashSet<String>>,
    total_entries: i64,
) -> String {
    // Score each term by specificity to this cluster:
    // score = (appearances in cluster / cluster size) / (global doc freq / total entries)
    // Higher = more distinctive to this cluster
    let cluster_size = cluster_entries.len() as f64;
    let total = total_entries as f64;

    let mut scored: Vec<(String, f64)> = cluster_terms
        .iter()
        .filter_map(|term| {
            let global_entries = term_to_entries.get(term)?;
            let in_cluster = global_entries
                .intersection(cluster_entries)
                .count() as f64;
            let global_freq = global_entries.len() as f64;

            let tf = in_cluster / cluster_size;
            let idf = (total / global_freq).ln();
            let score = tf * idf;

            Some((term.clone(), score))
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    scored
        .iter()
        .take(3)
        .map(|(term, _)| term.as_str())
        .collect::<Vec<_>>()
        .join(" / ")
}

/// Get topic summaries with sample titles.
pub fn get_topic_summaries(
    conn: &Connection,
    topics: &[Topic],
) -> Result<Vec<TopicSummary>> {
    let mut summaries = Vec::new();

    for topic in topics {
        let sample_ids: Vec<&str> = topic.entry_ids.iter().take(3).map(|s| s.as_str()).collect();
        let placeholders: Vec<String> = (0..sample_ids.len()).map(|i| format!("?{}", i + 1)).collect();
        let sql = format!(
            "SELECT title FROM entries WHERE id IN ({}) ORDER BY created_at DESC LIMIT 3",
            placeholders.join(", ")
        );

        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> =
            sample_ids.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();
        let titles: Vec<String> = stmt
            .query_map(params.as_slice(), |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();

        summaries.push(TopicSummary {
            label: topic.label.clone(),
            entry_count: topic.entry_ids.len(),
            sample_titles: titles,
        });
    }

    Ok(summaries)
}

/// Get entries by a list of IDs.
pub fn get_entries_by_ids(conn: &Connection, ids: &[String]) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for id in ids {
        let uuid: Uuid = id
            .parse()
            .map_err(|e| crate::errors::LunkError::Other(format!("bad uuid: {e}")))?;
        if let Ok(entry) = repo::get_entry(conn, &uuid) {
            entries.push(entry);
        }
    }
    // Sort by created_at descending
    entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(entries)
}

/// Get aggregate archive statistics.
pub fn get_archive_stats(conn: &Connection) -> Result<ArchiveStats> {
    let stats = conn.query_row(
        "SELECT
            (SELECT COUNT(*) FROM entries),
            (SELECT COUNT(*) FROM entries WHERE content_type = 'pdf'),
            (SELECT COUNT(*) FROM entries WHERE content_type = 'article'),
            (SELECT COUNT(DISTINCT domain) FROM entries WHERE domain IS NOT NULL),
            (SELECT COUNT(*) FROM entries WHERE created_at > datetime('now', '-7 days'))",
        [],
        |row| {
            Ok(ArchiveStats {
                total_entries: row.get(0)?,
                pdf_count: row.get(1)?,
                article_count: row.get(2)?,
                domain_count: row.get(3)?,
                recent_count: row.get(4)?,
            })
        },
    )?;
    Ok(stats)
}

/// Check if a term (possibly porter-stemmed) looks like a real English word.
/// Uses the embedded dictionary, checking both the exact stem and common
/// un-stemmed suffixes.
fn is_real_word(term: &str) -> bool {
    use std::collections::HashSet;
    use std::sync::LazyLock;

    static WORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/pdf/words.txt"))
            .lines()
            .filter(|l| !l.is_empty())
            .collect()
    });

    // Direct match
    if WORDS.contains(term) {
        return true;
    }
    // Porter stemmer removes common suffixes; try adding them back
    let suffixes = [
        "e", "s", "es", "ed", "er", "ing", "ion", "tion", "sion",
        "ly", "al", "ial", "ment", "ness", "ity", "ous", "ious",
        "ive", "ative", "able", "ible", "ence", "ance", "ure",
        "ture", "ment", "ize", "ise", "ate", "ated", "y",
        "ic", "ical", "ology", "ular", "ular",
    ];
    for suffix in &suffixes {
        let candidate = format!("{term}{suffix}");
        if WORDS.contains(candidate.as_str()) {
            return true;
        }
    }
    // Porter stemmer converts trailing 'y' to 'i' — try reversing
    if let Some(base) = term.strip_suffix('i') {
        let with_y = format!("{base}y");
        if WORDS.contains(with_y.as_str()) {
            return true;
        }
        for suffix in &["s", "ing", "ed", "er"] {
            let candidate = format!("{with_y}{suffix}");
            if WORDS.contains(candidate.as_str()) {
                return true;
            }
        }
    }

    // Also try doubling the last consonant + suffix (e.g., "control" → "controlling")
    if let Some(last) = term.as_bytes().last()
        && b"bcdfgklmnprst".contains(last)
    {
        for suffix in &["ed", "er", "ing"] {
            let candidate = format!("{term}{}{suffix}", *last as char);
            if WORDS.contains(candidate.as_str()) {
                return true;
            }
        }
    }
    false
}

/// Filter terms that are numeric, URLs, hex codes, or other non-semantic junk.
fn is_junk_term(term: &str) -> bool {
    // Pure numbers (including decimals)
    if term.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return true;
    }
    // Very short after stemming
    if term.len() < 3 {
        return true;
    }
    // Too long — likely concatenated junk from bad text extraction
    if term.len() > 20 {
        return true;
    }
    // Contains non-ASCII chars (math symbols, etc.)
    if !term.is_ascii() {
        return true;
    }
    // Contains any digit (part numbers, mixed codes like "and2", "adb33f")
    if term.contains(|c: char| c.is_ascii_digit()) {
        return true;
    }
    // Contains no vowels (likely an abbreviation or code, not a word)
    if term.len() > 3 && !term.contains(['a', 'e', 'i', 'o', 'u']) {
        return true;
    }
    // Likely concatenated words from bad PDF extraction:
    // starts with common short words glued to more text
    // Reject terms > 12 chars that have unusual consonant clusters
    // (real English words rarely have 4+ consonants in a row)
    if term.len() > 10 {
        let mut consonant_run = 0u32;
        for c in term.chars() {
            if !"aeiou".contains(c) {
                consonant_run += 1;
                if consonant_run >= 4 {
                    return true;
                }
            } else {
                consonant_run = 0;
            }
        }
    }
    let concat_prefixes = [
        "and", "the", "for", "that", "with", "from", "this",
        "are", "but", "not", "was", "has", "have", "been",
        "can", "will", "our", "its", "also", "more",
        "an", "of", "in", "on", "at", "to", "is", "it",
        "or", "if", "so", "no", "do", "up",
    ];
    for prefix in &concat_prefixes {
        if term.len() > prefix.len() + 2 && term.starts_with(prefix) {
            return true;
        }
    }
    false
}

/// Common English stop words to exclude from clustering.
fn is_stop_word(term: &str) -> bool {
    matches!(
        term,
        "the" | "and" | "for" | "are" | "but" | "not" | "you" | "all"
            | "can" | "had" | "her" | "was" | "one" | "our" | "out"
            | "has" | "have" | "been" | "from" | "that" | "they"
            | "this" | "with" | "will" | "each" | "make" | "like"
            | "into" | "them" | "than" | "its" | "also" | "more"
            | "some" | "when" | "what" | "which" | "their" | "these"
            | "then" | "would" | "other" | "about" | "there" | "were"
            | "after" | "should" | "where" | "being" | "could"
            | "does" | "such" | "just" | "only" | "very" | "even"
            | "most" | "much" | "those" | "both" | "well" | "over"
            | "may" | "use" | "used" | "using" | "how" | "any"
            | "new" | "way" | "see" | "two" | "first" | "between"
            | "time" | "get" | "need" | "set" | "number" | "work"
            | "page" | "www" | "com" | "http" | "https" | "html"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        schema::run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn test_archive_stats_empty() {
        let conn = setup_db();
        let stats = get_archive_stats(&conn).unwrap();
        assert_eq!(stats.total_entries, 0);
        assert_eq!(stats.pdf_count, 0);
    }

    #[test]
    fn test_compute_topics_empty() {
        let conn = setup_db();
        let topics = compute_topics(&conn).unwrap();
        assert!(topics.is_empty());
    }

    #[test]
    fn test_ensure_vocab_table() {
        let conn = setup_db();
        ensure_vocab_table(&conn).unwrap();
        // Should be idempotent
        ensure_vocab_table(&conn).unwrap();
    }

    #[test]
    fn test_is_stop_word() {
        assert!(is_stop_word("the"));
        assert!(is_stop_word("from"));
        assert!(!is_stop_word("resistor"));
        assert!(!is_stop_word("quantum"));
    }

    #[test]
    #[ignore]
    fn test_inspect_vocab() {
        let db_path = crate::config::Config::db_path();
        let db_path = match db_path {
            Ok(p) if p.exists() => p,
            _ => return,
        };
        let conn = Connection::open(&db_path).unwrap();
        ensure_vocab_table(&conn).unwrap();

        let mut stmt = conn.prepare(
            "SELECT term, doc, cnt FROM entries_vocab
             WHERE doc > 1 AND doc < 6 AND length(term) >= 4 AND length(term) <= 15
             ORDER BY doc DESC LIMIT 50"
        ).unwrap();
        let terms: Vec<(String, i64, i64)> = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
        }).unwrap().filter_map(|r| r.ok()).collect();

        eprintln!("Top vocabulary terms (doc_count 2-14, length 4-15):");
        for (term, doc, cnt) in &terms {
            eprintln!("  {term:20} doc={doc:3} cnt={cnt:4}");
        }
    }

    #[test]
    #[ignore]
    fn test_vocab_filter() {
        let db_path = crate::config::Config::db_path();
        let db_path = match db_path { Ok(p) if p.exists() => p, _ => return };
        let conn = Connection::open(&db_path).unwrap();
        ensure_vocab_table(&conn).unwrap();
        let total: i64 = conn.query_row("SELECT COUNT(*) FROM entries", [], |r| r.get(0)).unwrap();
        let max_doc = (total as f64 * 0.3).max(3.0) as i64;

        let mut stmt = conn.prepare(
            "SELECT term, doc FROM entries_vocab WHERE doc > 1 AND doc < ?1 AND length(term) >= 3 ORDER BY doc DESC LIMIT 500"
        ).unwrap();
        let all_terms: Vec<(String, i64)> = stmt.query_map([max_doc], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        }).unwrap().filter_map(|r| r.ok()).collect();

        let passed: Vec<_> = all_terms.iter()
            .filter(|(t, _)| !is_stop_word(t) && !is_junk_term(t) && is_real_word(t))
            .take(30)
            .collect();
        let failed_dict: Vec<_> = all_terms.iter()
            .filter(|(t, _)| !is_stop_word(t) && !is_junk_term(t) && !is_real_word(t))
            .take(20)
            .collect();

        eprintln!("Passed all filters ({}):", passed.len());
        for (t, d) in &passed { eprintln!("  {t:20} doc={d}"); }
        eprintln!("Failed dict check ({}):", failed_dict.len());
        for (t, d) in &failed_dict { eprintln!("  {t:20} doc={d}"); }
    }

    #[test]
    fn test_compute_topics_with_real_data() {
        // Test with the actual database if available
        let db_path = crate::config::Config::db_path();
        let db_path = match db_path {
            Ok(p) if p.exists() => p,
            _ => return,
        };

        let conn = Connection::open(&db_path).unwrap();
        let stats = get_archive_stats(&conn).unwrap();
        if stats.total_entries < 4 {
            return;
        }

        let topics = compute_topics(&conn).unwrap();
        let summaries = get_topic_summaries(&conn, &topics).unwrap();
        eprintln!("Found {} topics from {} entries:", summaries.len(), stats.total_entries);
        for s in &summaries {
            eprintln!("  {} ({} entries)", s.label, s.entry_count);
            for t in &s.sample_titles {
                eprintln!("    - {}", t);
            }
        }
        // Just verify it doesn't crash and produces reasonable results
        assert!(topics.len() <= 20);
    }
}
