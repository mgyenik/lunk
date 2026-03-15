use rusqlite::{params, Connection};

use crate::errors::{LunkError, Result};
use crate::models::*;

/// Full-text search across all entries using FTS5.
pub fn search(
    conn: &Connection,
    query: &str,
    limit: i64,
    offset: i64,
) -> Result<SearchResult> {
    if query.trim().is_empty() {
        return Err(LunkError::InvalidInput("search query cannot be empty".to_string()));
    }

    if query.len() > 1000 {
        return Err(LunkError::InvalidInput("search query too long (max 1000 chars)".to_string()));
    }

    let limit = limit.clamp(1, 1000);
    let offset = offset.max(0);

    // Sanitize query for FTS5: wrap each token in quotes to avoid syntax errors
    let fts_query = sanitize_fts_query(query);

    // Count total matches
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entries_fts WHERE entries_fts MATCH ?1",
        params![fts_query],
        |row| row.get(0),
    )?;

    // Main search query with snippets
    let mut stmt = conn.prepare(
        "SELECT e.id, e.url, e.title, e.content_type, e.domain,
                e.word_count, e.page_count, e.index_status, e.index_version,
                e.created_at, e.updated_at, e.saved_by,
                snippet(entries_fts, 1, '<mark>', '</mark>', '...', 40) as snippet
         FROM entries_fts
         JOIN entries e ON e.rowid = entries_fts.rowid
         WHERE entries_fts MATCH ?1
         ORDER BY bm25(entries_fts, 5.0, 1.0)
         LIMIT ?2 OFFSET ?3",
    )?;

    let rows = stmt.query_map(params![fts_query, limit, offset], |row| {
        Ok((
            EntryRowWithSnippet {
                id: row.get(0)?,
                url: row.get(1)?,
                title: row.get(2)?,
                content_type: row.get(3)?,
                domain: row.get(4)?,
                word_count: row.get(5)?,
                page_count: row.get(6)?,
                index_status: row.get::<_, Option<String>>(7)?.unwrap_or_else(|| "ok".to_string()),
                index_version: row.get::<_, Option<i32>>(8)?.unwrap_or(0),
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
                saved_by: row.get(11)?,
                snippet: row.get(12)?,
            },
        ))
    })?;

    let mut entries = Vec::new();
    for row in rows {
        let (row,) = row?;
        let tags = get_entry_tags(conn, &row.id)?;

        let id = uuid::Uuid::parse_str(&row.id)
            .map_err(|e| LunkError::Other(format!("invalid uuid: {e}")))?;
        let content_type = ContentType::parse(&row.content_type)
            .ok_or_else(|| LunkError::Other(format!("invalid content_type: {}", row.content_type)))?;
        let index_status = IndexStatus::parse(&row.index_status)
            .unwrap_or(IndexStatus::Ok);
        let created_at = chrono::DateTime::parse_from_rfc3339(&row.created_at)
            .map_err(|e| LunkError::Other(format!("invalid timestamp: {e}")))?
            .with_timezone(&chrono::Utc);
        let updated_at = chrono::DateTime::parse_from_rfc3339(&row.updated_at)
            .map_err(|e| LunkError::Other(format!("invalid timestamp: {e}")))?
            .with_timezone(&chrono::Utc);

        // For PDFs, check which page matched
        let matched_page = if content_type == ContentType::Pdf {
            find_matching_pdf_page(conn, &row.id, query)?
        } else {
            None
        };

        entries.push(SearchHit {
            entry: Entry {
                id,
                url: row.url,
                title: row.title,
                content_type,
                domain: row.domain,
                word_count: row.word_count,
                page_count: row.page_count,
                index_status,
                index_version: row.index_version,
                created_at,
                updated_at,
                saved_by: row.saved_by,
                tags,
            },
            snippet: row.snippet,
            matched_page,
        });
    }

    Ok(SearchResult { entries, total })
}

/// Rebuild FTS index for a specific entry (used after sync).
pub fn rebuild_fts_for_entry(conn: &Connection, entry_id: &uuid::Uuid) -> Result<()> {
    let id_str = entry_id.to_string();

    // Get the entry's rowid
    let rowid: i64 = conn.query_row(
        "SELECT rowid FROM entries WHERE id = ?1",
        params![id_str],
        |row| row.get(0),
    )?;

    // Get entry title and content
    let (title, text): (String, String) = conn.query_row(
        "SELECT e.title, ec.extracted_text
         FROM entries e
         JOIN entry_content ec ON ec.entry_id = e.id
         WHERE e.id = ?1",
        params![id_str],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    // Delete old FTS entry (using the special delete command)
    // We need to get the old values first, but with contentless FTS5 this is tricky.
    // Safest approach: delete by rowid using a trick, then re-insert.
    let _ = conn.execute(
        "INSERT INTO entries_fts(entries_fts, rowid, title, extracted_text) VALUES('delete', ?1, ?2, ?3)",
        params![rowid, title, text],
    );

    // Re-insert
    conn.execute(
        "INSERT INTO entries_fts(rowid, title, extracted_text) VALUES(?1, ?2, ?3)",
        params![rowid, title, text],
    )?;

    Ok(())
}

fn find_matching_pdf_page(conn: &Connection, entry_id: &str, query: &str) -> Result<Option<i32>> {
    let terms: Vec<&str> = query.split_whitespace().collect();
    if terms.is_empty() {
        return Ok(None);
    }

    // Simple approach: find first page containing any query term (case-insensitive)
    let pattern = format!("%{}%", terms[0].to_lowercase());
    let result = conn.query_row(
        "SELECT page_num FROM pdf_pages WHERE entry_id = ?1 AND LOWER(text) LIKE ?2 ORDER BY page_num LIMIT 1",
        params![entry_id, pattern],
        |row| row.get::<_, i32>(0),
    );

    match result {
        Ok(page) => Ok(Some(page)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn sanitize_fts_query(query: &str) -> String {
    // Wrap each word in quotes and join with spaces (implicit AND in FTS5).
    // Quoting prevents FTS5 operator injection (AND, OR, NOT, NEAR, etc.)
    // and ensures special characters like parentheses and colons are treated as literals.
    // The last term automatically gets a prefix wildcard for search-as-you-type.
    let words: Vec<&str> = query.split_whitespace().collect();
    let last_idx = words.len().saturating_sub(1);
    words
        .iter()
        .enumerate()
        .map(|(i, word)| {
            // Strip any existing quotes and backslashes
            let clean: String = word.chars().filter(|c| *c != '"' && *c != '\\').collect();
            if clean.is_empty() {
                return String::new();
            }
            // Allow explicit * suffix for prefix matching, or auto-prefix the last term
            if clean.ends_with('*') {
                let stem = &clean[..clean.len() - 1];
                if stem.is_empty() {
                    return String::new();
                }
                format!("\"{stem}\"*")
            } else if i == last_idx {
                format!("\"{clean}\"*")
            } else {
                format!("\"{clean}\"")
            }
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn get_entry_tags(conn: &Connection, entry_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT t.name FROM tags t
         JOIN entry_tags et ON t.id = et.tag_id
         WHERE et.entry_id = ?1
         ORDER BY t.name",
    )?;

    let rows = stmt.query_map(params![entry_id], |row| row.get::<_, String>(0))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.into())
}

struct EntryRowWithSnippet {
    id: String,
    url: Option<String>,
    title: String,
    content_type: String,
    domain: Option<String>,
    word_count: Option<i64>,
    page_count: Option<i64>,
    index_status: String,
    index_version: i32,
    created_at: String,
    updated_at: String,
    saved_by: String,
    snippet: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::repo;

    fn test_conn() -> Connection {
        db::open_in_memory().unwrap()
    }

    #[test]
    fn test_search_articles() {
        let conn = test_conn();

        repo::create_entry(&conn, CreateEntryRequest {
            url: Some("https://example.com/rust".to_string()),
            title: "Learning Rust".to_string(),
            content_type: ContentType::Article,
            extracted_text: "Rust is a systems programming language focused on safety and performance.".to_string(),
            snapshot_html: None,
            readable_html: None,
            pdf_data: None,
            tags: None,
            source: SaveSource::Cli,
        }).unwrap();

        repo::create_entry(&conn, CreateEntryRequest {
            url: Some("https://example.com/go".to_string()),
            title: "Learning Go".to_string(),
            content_type: ContentType::Article,
            extracted_text: "Go is a simple and efficient programming language by Google.".to_string(),
            snapshot_html: None,
            readable_html: None,
            pdf_data: None,
            tags: None,
            source: SaveSource::Cli,
        }).unwrap();

        let results = search(&conn, "rust", 10, 0).unwrap();
        assert_eq!(results.total, 1);
        assert_eq!(results.entries[0].entry.title, "Learning Rust");

        let results = search(&conn, "programming", 10, 0).unwrap();
        assert_eq!(results.total, 2);
    }

    #[test]
    fn test_search_empty_query() {
        let conn = test_conn();
        assert!(search(&conn, "", 10, 0).is_err());
        assert!(search(&conn, "   ", 10, 0).is_err());
    }

    #[test]
    fn test_sanitize_fts_query() {
        // Last term gets auto-prefix
        assert_eq!(sanitize_fts_query("hello world"), "\"hello\" \"world\"*");
        // Explicit * preserved
        assert_eq!(sanitize_fts_query("rust*"), "\"rust\"*");
        // Single term gets auto-prefix
        assert_eq!(sanitize_fts_query("  spaces  "), "\"spaces\"*");
        // Multi-word: only last gets prefix
        assert_eq!(sanitize_fts_query("foo bar baz"), "\"foo\" \"bar\" \"baz\"*");
    }
}
