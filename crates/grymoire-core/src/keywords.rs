//! Keyword extraction using YAKE (Yet Another Keyword Extractor).
//!
//! Extracts the most significant key phrases from document text.
//! Keywords are stored per-entry and used for topic labels and UI display.

use rusqlite::{params, Connection};
use uuid::Uuid;
use yake_rust::{Config, StopWords, get_n_best};

use crate::errors::{GrymoireError, Result};

/// Default number of keywords to extract per document.
pub const DEFAULT_KEYWORD_COUNT: usize = 10;

/// A stored keyword with its relevance score.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Keyword {
    pub keyword: String,
    pub score: f64,
}

/// Extract keywords from text using YAKE.
/// Returns (keyword, score) pairs sorted by score ascending (lower = more relevant).
pub fn extract_keywords(text: &str, n: usize) -> Vec<Keyword> {
    if text.len() < 50 {
        return Vec::new(); // Too short for meaningful extraction
    }

    let config = Config {
        ngrams: 3,
        ..Config::default()
    };

    let stop_words = match StopWords::predefined("en") {
        Some(sw) => sw,
        None => return Vec::new(),
    };

    let results = get_n_best(n, text, &stop_words, &config);

    results
        .into_iter()
        .map(|item| Keyword {
            keyword: item.keyword,
            score: item.score,
        })
        .collect()
}

/// Store keywords for an entry (replaces any existing keywords).
pub fn store_keywords(conn: &Connection, entry_id: &Uuid, keywords: &[Keyword]) -> Result<()> {
    let id_str = entry_id.to_string();

    // Delete existing keywords
    conn.execute(
        "DELETE FROM entry_keywords WHERE entry_id = ?1",
        params![id_str],
    )?;

    // Insert new keywords
    let mut stmt = conn.prepare(
        "INSERT INTO entry_keywords (entry_id, keyword, score) VALUES (?1, ?2, ?3)",
    )?;

    for kw in keywords {
        stmt.execute(params![id_str, kw.keyword, kw.score])?;
    }

    Ok(())
}

/// Get stored keywords for an entry.
pub fn get_entry_keywords(conn: &Connection, entry_id: &Uuid) -> Result<Vec<Keyword>> {
    let mut stmt = conn.prepare(
        "SELECT keyword, score FROM entry_keywords
         WHERE entry_id = ?1 ORDER BY score ASC",
    )?;

    let keywords = stmt
        .query_map(params![entry_id.to_string()], |row| {
            Ok(Keyword {
                keyword: row.get(0)?,
                score: row.get(1)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(keywords)
}

/// Extract and store keywords for a single entry.
pub fn extract_and_store(conn: &Connection, entry_id: &Uuid) -> Result<()> {
    let text = get_entry_text(conn, entry_id)?;
    if text.is_empty() {
        return Ok(());
    }

    let keywords = extract_keywords(&text, DEFAULT_KEYWORD_COUNT);
    if !keywords.is_empty() {
        store_keywords(conn, entry_id, &keywords)?;
    }
    Ok(())
}

/// Extract and store keywords for all entries that don't have them yet.
pub fn extract_all_missing(conn: &Connection) -> Result<usize> {
    let mut stmt = conn.prepare(
        "SELECT e.id FROM entries e
         WHERE NOT EXISTS (
             SELECT 1 FROM entry_keywords ek WHERE ek.entry_id = e.id
         )",
    )?;

    let ids: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();

    let mut count = 0;
    for id_str in &ids {
        let uuid: Uuid = id_str
            .parse()
            .map_err(|e| GrymoireError::Other(format!("bad uuid: {e}")))?;
        if let Err(e) = extract_and_store(conn, &uuid) {
            tracing::warn!(entry_id = %id_str, "keyword extraction failed: {e}");
        } else {
            count += 1;
        }
    }

    Ok(count)
}

/// Get the most common keywords across a set of entries.
/// Used for labeling topic clusters.
pub fn top_keywords_for_entries(
    conn: &Connection,
    entry_ids: &[String],
    limit: usize,
) -> Result<Vec<String>> {
    if entry_ids.is_empty() {
        return Ok(Vec::new());
    }

    // Build placeholders for the IN clause
    let placeholders: Vec<String> = (1..=entry_ids.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "SELECT keyword, COUNT(*) as cnt, MIN(score) as best_score
         FROM entry_keywords
         WHERE entry_id IN ({})
         GROUP BY keyword
         ORDER BY cnt DESC, best_score ASC
         LIMIT ?{}",
        placeholders.join(", "),
        entry_ids.len() + 1,
    );

    let mut stmt = conn.prepare(&sql)?;

    let mut bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = entry_ids
        .iter()
        .map(|id| Box::new(id.clone()) as Box<dyn rusqlite::types::ToSql>)
        .collect();
    bind_values.push(Box::new(limit as i64));

    let refs: Vec<&dyn rusqlite::types::ToSql> =
        bind_values.iter().map(|b| b.as_ref()).collect();

    let keywords: Vec<String> = stmt
        .query_map(refs.as_slice(), |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(keywords)
}

/// Get the text for keyword extraction: title + extracted text.
fn get_entry_text(conn: &Connection, entry_id: &Uuid) -> Result<String> {
    let id_str = entry_id.to_string();

    let title: String = conn
        .query_row(
            "SELECT title FROM entries WHERE id = ?1",
            params![id_str],
            |row| row.get(0),
        )
        .unwrap_or_default();

    let text: String = conn
        .query_row(
            "SELECT extracted_text FROM entry_content WHERE entry_id = ?1",
            params![id_str],
            |row| row.get(0),
        )
        .unwrap_or_default();

    Ok(format!("{title}. {text}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_keywords() {
        let text = "Digital filters are capable of excellent performance when done right. \
            Second-order recursive filters provide the building blocks for audio processing. \
            Fixed-point arithmetic introduces quantization noise that must be managed carefully. \
            The frequency response of these filters depends on coefficient precision.";

        let keywords = extract_keywords(text, 5);
        assert!(!keywords.is_empty());

        // Keywords should include something about filters
        let all_kw: String = keywords.iter().map(|k| k.keyword.as_str()).collect::<Vec<_>>().join(" ");
        assert!(
            all_kw.contains("filter") || all_kw.contains("digital") || all_kw.contains("frequency"),
            "keywords should relate to the text, got: {all_kw}"
        );
    }

    #[test]
    fn test_extract_empty_text() {
        let keywords = extract_keywords("hi", 5);
        assert!(keywords.is_empty()); // Too short
    }

    #[test]
    fn test_keyword_scores_ascending() {
        let text = "Rust programming language enables safe concurrent systems programming. \
            The ownership model prevents data races at compile time. Memory safety without \
            garbage collection is the key innovation. Async/await provides efficient I/O.";

        let keywords = extract_keywords(text, 5);
        // Scores should be in ascending order (lower = more relevant)
        for pair in keywords.windows(2) {
            assert!(pair[0].score <= pair[1].score);
        }
    }

    // --- Database integration tests ---

    use crate::db;
    use crate::models::{ContentType, CreateEntryRequest, SaveSource};
    use crate::repo;

    #[test]
    fn test_store_and_retrieve_keywords() {
        let mut db = db::open_in_memory_db().unwrap();
        let req = CreateEntryRequest {
            url: None,
            title: "Test".to_string(),
            content_type: ContentType::Article,
            extracted_text: "Some text".to_string(),
            snapshot_html: None,
            readable_html: None,
            pdf_data: None,
            tags: None,
            source: SaveSource::Cli,
        };
        let entry = repo::create_entry(&mut db, req).unwrap();

        let keywords = vec![
            Keyword { keyword: "digital filters".into(), score: 0.05 },
            Keyword { keyword: "audio processing".into(), score: 0.12 },
        ];
        store_keywords(db.conn(), &entry.id, &keywords).unwrap();

        let retrieved = get_entry_keywords(db.conn(), &entry.id).unwrap();
        assert_eq!(retrieved.len(), 2);
        assert_eq!(retrieved[0].keyword, "digital filters"); // lowest score first
        assert_eq!(retrieved[1].keyword, "audio processing");
    }

    #[test]
    fn test_store_keywords_replaces() {
        let mut db = db::open_in_memory_db().unwrap();
        let req = CreateEntryRequest {
            url: None,
            title: "Test".to_string(),
            content_type: ContentType::Article,
            extracted_text: "Some text".to_string(),
            snapshot_html: None,
            readable_html: None,
            pdf_data: None,
            tags: None,
            source: SaveSource::Cli,
        };
        let entry = repo::create_entry(&mut db, req).unwrap();

        let kw1 = vec![Keyword { keyword: "old".into(), score: 0.1 }];
        store_keywords(db.conn(), &entry.id, &kw1).unwrap();

        let kw2 = vec![Keyword { keyword: "new".into(), score: 0.2 }];
        store_keywords(db.conn(), &entry.id, &kw2).unwrap();

        let retrieved = get_entry_keywords(db.conn(), &entry.id).unwrap();
        assert_eq!(retrieved.len(), 1);
        assert_eq!(retrieved[0].keyword, "new");
    }

    #[test]
    fn test_top_keywords_for_entries() {
        let mut db = db::open_in_memory_db().unwrap();

        let mut ids = Vec::new();
        for i in 0..3 {
            let req = CreateEntryRequest {
                url: None,
                title: format!("Entry {i}"),
                content_type: ContentType::Article,
                extracted_text: "text".to_string(),
                snapshot_html: None,
                readable_html: None,
                pdf_data: None,
                tags: None,
                source: SaveSource::Cli,
            };
            ids.push(repo::create_entry(&mut db, req).unwrap().id);
        }

        // All 3 share "common", only 1 has "rare"
        for id in &ids {
            store_keywords(db.conn(), id, &[
                Keyword { keyword: "common".into(), score: 0.1 },
            ]).unwrap();
        }
        store_keywords(db.conn(), &ids[0], &[
            Keyword { keyword: "common".into(), score: 0.1 },
            Keyword { keyword: "rare".into(), score: 0.2 },
        ]).unwrap();

        let id_strs: Vec<String> = ids.iter().map(|id| id.to_string()).collect();
        let top = top_keywords_for_entries(db.conn(), &id_strs, 5).unwrap();
        assert!(!top.is_empty());
        assert_eq!(top[0], "common", "most common keyword should be first");
    }
}
