//! Change tracking for CRDT sync.
//!
//! Tracks all data mutations via:
//! - SQLite triggers on INSERT/DELETE (write sentinel rows to `change_log`)
//! - Application-level logging for UPDATE (column-level change records)
//! - Persistent tombstones for delete propagation
//!
//! The trigger approach catches all inserts/deletes regardless of code path,
//! while application-level UPDATE tracking gives us precise column information
//! for Last-Writer-Wins conflict resolution.

use rusqlite::types::Value;
use rusqlite::{params, Connection};

use crate::errors::Result;
use crate::hlc::HlcTimestamp;

/// Check if change tracking tables exist (migration v3+).
fn has_change_tracking(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT count(*) > 0 FROM sqlite_master WHERE type='table' AND name='change_log'",
        [],
        |row| row.get(0),
    )
    .unwrap_or(false)
}

/// Fix up sentinel rows left by INSERT/DELETE triggers.
///
/// Triggers insert with `hlc_ts=0, site_id=''` as sentinels. This function
/// replaces those with the real HLC timestamp and db_version from the current
/// operation. Call once at the end of each logical write operation.
pub fn fixup_trigger_rows(conn: &Connection, ts: &HlcTimestamp, ver: i64) -> Result<()> {
    if !has_change_tracking(conn) {
        return Ok(());
    }

    conn.execute(
        "UPDATE change_log SET hlc_ts = ?1, hlc_counter = ?2, site_id = ?3, db_version = ?4
         WHERE hlc_ts = 0 AND site_id = ''",
        params![ts.wall_ms, ts.counter, &ts.site_id, ver],
    )?;

    Ok(())
}

/// Log column-level changes for an UPDATE operation.
///
/// Each `(column_name, new_value)` pair becomes a row in `change_log`.
/// The sync protocol uses these for per-column Last-Writer-Wins resolution.
pub fn log_column_changes(
    conn: &Connection,
    ts: &HlcTimestamp,
    ver: i64,
    table: &str,
    row_id: &str,
    changes: &[(&str, Value)],
) -> Result<()> {
    if !has_change_tracking(conn) || changes.is_empty() {
        return Ok(());
    }

    let mut stmt = conn.prepare_cached(
        "INSERT INTO change_log (tbl, row_id, col, val, hlc_ts, hlc_counter, site_id, db_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;

    for (col, val) in changes {
        stmt.execute(params![
            table,
            row_id,
            *col,
            val,
            ts.wall_ms,
            ts.counter,
            &ts.site_id,
            ver
        ])?;
    }

    Ok(())
}

/// Record a persistent tombstone for a deleted row.
///
/// Tombstones survive change_log compaction to ensure deletes propagate
/// to peers that haven't synced recently.
pub fn record_tombstone(
    conn: &Connection,
    ts: &HlcTimestamp,
    ver: i64,
    table: &str,
    row_id: &str,
) -> Result<()> {
    if !has_change_tracking(conn) {
        return Ok(());
    }

    conn.execute(
        "INSERT OR REPLACE INTO tombstones (tbl, row_id, hlc_ts, hlc_counter, site_id, db_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![table, row_id, ts.wall_ms, ts.counter, &ts.site_id, ver],
    )?;

    Ok(())
}

/// Expand a `__row__` change_log entry into per-column key-value pairs.
///
/// Used during sync export: when a row was inserted, we only recorded
/// `__row__` in the change_log. At export time, we read the current
/// column values so the remote peer can apply the full insert.
///
/// Returns empty vec if the row no longer exists (was deleted since).
pub fn expand_row_insert(
    conn: &Connection,
    table: &str,
    row_id: &str,
) -> Result<Vec<(String, Value)>> {
    let cols = get_column_names(conn, table)?;
    if cols.is_empty() {
        return Ok(vec![]);
    }

    let col_list = cols.join(", ");

    let (sql, is_composite) = match table {
        "entry_content" => (
            format!("SELECT {col_list} FROM entry_content WHERE entry_id = ?1"),
            false,
        ),
        "entry_tags" => (
            format!("SELECT {col_list} FROM entry_tags WHERE entry_id = ?1 AND tag_id = ?2"),
            true,
        ),
        _ => (
            format!("SELECT {col_list} FROM {table} WHERE id = ?1"),
            false,
        ),
    };

    let mut stmt = conn.prepare(&sql)?;

    let row_fn = |row: &rusqlite::Row| {
        let mut pairs = Vec::with_capacity(cols.len());
        for (i, col) in cols.iter().enumerate() {
            pairs.push((col.clone(), row.get::<_, Value>(i)?));
        }
        Ok(pairs)
    };

    let result = if is_composite {
        let parts: Vec<&str> = row_id.splitn(2, '|').collect();
        if parts.len() != 2 {
            return Ok(vec![]);
        }
        stmt.query_row(params![parts[0], parts[1]], row_fn)
    } else {
        stmt.query_row(params![row_id], row_fn)
    };

    match result {
        Ok(pairs) => Ok(pairs),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(vec![]),
        Err(e) => Err(e.into()),
    }
}

/// Get column names for a table via PRAGMA table_info.
fn get_column_names(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let cols = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(cols)
}

/// Install INSERT/DELETE change tracking triggers on all tracked tables.
///
/// These triggers write sentinel rows (hlc_ts=0, site_id='') that must be
/// fixed up by calling `fixup_trigger_rows` after each write operation.
pub fn install_triggers(conn: &Connection) -> Result<()> {
    // entries (PK: id)
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS entries_change_insert AFTER INSERT ON entries
         BEGIN
             INSERT INTO change_log (tbl, row_id, col, val, hlc_ts, hlc_counter, site_id, db_version)
             VALUES ('entries', NEW.id, '__row__', NULL, 0, 0, '', 0);
         END;

         CREATE TRIGGER IF NOT EXISTS entries_change_delete AFTER DELETE ON entries
         BEGIN
             INSERT INTO change_log (tbl, row_id, col, val, hlc_ts, hlc_counter, site_id, db_version)
             VALUES ('entries', OLD.id, '__tombstone__', NULL, 0, 0, '', 0);
         END;",
    )?;

    // entry_content (PK: entry_id)
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS entry_content_change_insert AFTER INSERT ON entry_content
         BEGIN
             INSERT INTO change_log (tbl, row_id, col, val, hlc_ts, hlc_counter, site_id, db_version)
             VALUES ('entry_content', NEW.entry_id, '__row__', NULL, 0, 0, '', 0);
         END;

         CREATE TRIGGER IF NOT EXISTS entry_content_change_delete AFTER DELETE ON entry_content
         BEGIN
             INSERT INTO change_log (tbl, row_id, col, val, hlc_ts, hlc_counter, site_id, db_version)
             VALUES ('entry_content', OLD.entry_id, '__tombstone__', NULL, 0, 0, '', 0);
         END;",
    )?;

    // tags (PK: id)
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS tags_change_insert AFTER INSERT ON tags
         BEGIN
             INSERT INTO change_log (tbl, row_id, col, val, hlc_ts, hlc_counter, site_id, db_version)
             VALUES ('tags', NEW.id, '__row__', NULL, 0, 0, '', 0);
         END;

         CREATE TRIGGER IF NOT EXISTS tags_change_delete AFTER DELETE ON tags
         BEGIN
             INSERT INTO change_log (tbl, row_id, col, val, hlc_ts, hlc_counter, site_id, db_version)
             VALUES ('tags', OLD.id, '__tombstone__', NULL, 0, 0, '', 0);
         END;",
    )?;

    // entry_tags (composite PK: entry_id || '|' || tag_id)
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS entry_tags_change_insert AFTER INSERT ON entry_tags
         BEGIN
             INSERT INTO change_log (tbl, row_id, col, val, hlc_ts, hlc_counter, site_id, db_version)
             VALUES ('entry_tags', NEW.entry_id || '|' || NEW.tag_id, '__row__', NULL, 0, 0, '', 0);
         END;

         CREATE TRIGGER IF NOT EXISTS entry_tags_change_delete AFTER DELETE ON entry_tags
         BEGIN
             INSERT INTO change_log (tbl, row_id, col, val, hlc_ts, hlc_counter, site_id, db_version)
             VALUES ('entry_tags', OLD.entry_id || '|' || OLD.tag_id, '__tombstone__', NULL, 0, 0, '', 0);
         END;",
    )?;

    // pdf_pages (PK: id)
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS pdf_pages_change_insert AFTER INSERT ON pdf_pages
         BEGIN
             INSERT INTO change_log (tbl, row_id, col, val, hlc_ts, hlc_counter, site_id, db_version)
             VALUES ('pdf_pages', NEW.id, '__row__', NULL, 0, 0, '', 0);
         END;

         CREATE TRIGGER IF NOT EXISTS pdf_pages_change_delete AFTER DELETE ON pdf_pages
         BEGIN
             INSERT INTO change_log (tbl, row_id, col, val, hlc_ts, hlc_counter, site_id, db_version)
             VALUES ('pdf_pages', OLD.id, '__tombstone__', NULL, 0, 0, '', 0);
         END;",
    )?;

    Ok(())
}

/// Remove change_log entries older than the given db_version.
/// Call after successful sync with all peers, using the minimum
/// of all peers' last_db_version.
pub fn compact_change_log(conn: &Connection, min_version: i64) -> Result<usize> {
    let deleted = conn.execute(
        "DELETE FROM change_log WHERE db_version < ?1",
        params![min_version],
    )?;
    Ok(deleted)
}

/// Remove tombstones older than the given HLC wall timestamp.
/// Default horizon: 90 days.
pub fn prune_tombstones(conn: &Connection, before_wall_ms: i64) -> Result<usize> {
    let deleted = conn.execute(
        "DELETE FROM tombstones WHERE hlc_ts < ?1",
        params![before_wall_ms],
    )?;
    Ok(deleted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::models::*;
    use crate::repo;

    fn test_db() -> db::Db {
        db::open_in_memory_db().unwrap()
    }

    #[test]
    fn test_insert_trigger_fires() {
        let mut db = test_db();

        repo::create_entry(
            &mut db,
            CreateEntryRequest {
                url: Some("https://example.com".to_string()),
                title: "Test".to_string(),
                content_type: ContentType::Article,
                extracted_text: "hello world".to_string(),
                snapshot_html: None,
                readable_html: None,
                pdf_data: None,
                tags: None,
                source: SaveSource::Cli,
            },
        )
        .unwrap();

        // Should have __row__ entries for entries and entry_content
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM change_log WHERE col = '__row__'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(count >= 2, "expected at least 2 __row__ entries, got {count}");

        // All sentinel values should be fixed up (no hlc_ts=0 remaining)
        let unfixed: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM change_log WHERE hlc_ts = 0 AND site_id = ''",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unfixed, 0, "all sentinel rows should be fixed up");
    }

    #[test]
    fn test_delete_trigger_fires() {
        let mut db = test_db();

        let entry = repo::create_entry(
            &mut db,
            CreateEntryRequest {
                url: Some("https://example.com".to_string()),
                title: "Test".to_string(),
                content_type: ContentType::Article,
                extracted_text: "hello world".to_string(),
                snapshot_html: None,
                readable_html: None,
                pdf_data: None,
                tags: None,
                source: SaveSource::Cli,
            },
        )
        .unwrap();

        repo::delete_entry(&mut db, &entry.id).unwrap();

        // Should have __tombstone__ entries from DELETE triggers
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM change_log WHERE col = '__tombstone__'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            count >= 1,
            "expected at least 1 __tombstone__ entry, got {count}"
        );

        // Should have a persistent tombstone for the entries table
        let tombstone_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM tombstones WHERE tbl = 'entries'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tombstone_count, 1);
    }

    #[test]
    fn test_cascade_delete_triggers() {
        let mut db = test_db();

        let entry = repo::create_entry(
            &mut db,
            CreateEntryRequest {
                url: Some("https://example.com".to_string()),
                title: "Test".to_string(),
                content_type: ContentType::Article,
                extracted_text: "hello".to_string(),
                snapshot_html: None,
                readable_html: None,
                pdf_data: None,
                tags: Some(vec!["rust".to_string()]),
                source: SaveSource::Cli,
            },
        )
        .unwrap();

        // Clear change_log to isolate delete effects
        db.conn()
            .execute("DELETE FROM change_log", [])
            .unwrap();

        repo::delete_entry(&mut db, &entry.id).unwrap();

        // CASCADE deletes should trigger tombstone entries for children
        let tables: Vec<String> = {
            let mut stmt = db
                .conn()
                .prepare("SELECT DISTINCT tbl FROM change_log WHERE col = '__tombstone__'")
                .unwrap();
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap()
        };

        assert!(
            tables.contains(&"entries".to_string()),
            "entries tombstone missing"
        );
        assert!(
            tables.contains(&"entry_content".to_string()),
            "entry_content tombstone missing"
        );
        assert!(
            tables.contains(&"entry_tags".to_string()),
            "entry_tags tombstone missing"
        );
    }

    #[test]
    fn test_fixup_trigger_rows() {
        let mut db = test_db();

        // Manually insert a sentinel row
        db.conn()
            .execute(
                "INSERT INTO change_log (tbl, row_id, col, val, hlc_ts, hlc_counter, site_id, db_version)
                 VALUES ('test', 'row1', '__row__', NULL, 0, 0, '', 0)",
                [],
            )
            .unwrap();

        let (ts, ver) = db.next_timestamp();
        fixup_trigger_rows(db.conn(), &ts, ver).unwrap();

        // Verify sentinel was replaced
        let (hlc_ts, site_id, db_ver): (i64, String, i64) = db
            .conn()
            .query_row(
                "SELECT hlc_ts, site_id, db_version FROM change_log WHERE row_id = 'row1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();

        assert!(hlc_ts > 0, "hlc_ts should be nonzero after fixup");
        assert!(!site_id.is_empty(), "site_id should be non-empty after fixup");
        assert_eq!(db_ver, ver);
    }

    #[test]
    fn test_log_column_changes() {
        let mut db = test_db();
        let (ts, ver) = db.next_timestamp();

        log_column_changes(
            db.conn(),
            &ts,
            ver,
            "entries",
            "test-id",
            &[
                ("title", Value::Text("New Title".to_string())),
                ("word_count", Value::Integer(42)),
            ],
        )
        .unwrap();

        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM change_log WHERE tbl = 'entries' AND row_id = 'test-id'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);

        // Check specific column value
        let val: String = db
            .conn()
            .query_row(
                "SELECT val FROM change_log WHERE row_id = 'test-id' AND col = 'title'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(val, "New Title");
    }

    #[test]
    fn test_db_version_increments() {
        let mut db = test_db();
        let initial = db.db_version();
        // After migration v3, db_version starts at 1
        assert!(initial >= 1, "db_version should be >= 1 after migration v3");

        repo::create_entry(
            &mut db,
            CreateEntryRequest {
                url: Some("https://a.com".to_string()),
                title: "A".to_string(),
                content_type: ContentType::Article,
                extracted_text: "aaa".to_string(),
                snapshot_html: None,
                readable_html: None,
                pdf_data: None,
                tags: None,
                source: SaveSource::Cli,
            },
        )
        .unwrap();

        let v1 = db.db_version();
        assert!(v1 > initial, "db_version should increase after write");

        repo::create_entry(
            &mut db,
            CreateEntryRequest {
                url: Some("https://b.com".to_string()),
                title: "B".to_string(),
                content_type: ContentType::Article,
                extracted_text: "bbb".to_string(),
                snapshot_html: None,
                readable_html: None,
                pdf_data: None,
                tags: None,
                source: SaveSource::Cli,
            },
        )
        .unwrap();

        let v2 = db.db_version();
        assert!(v2 > v1, "db_version should increase monotonically");
    }

    #[test]
    fn test_expand_row_insert() {
        let mut db = test_db();

        let entry = repo::create_entry(
            &mut db,
            CreateEntryRequest {
                url: Some("https://example.com".to_string()),
                title: "Expand Test".to_string(),
                content_type: ContentType::Article,
                extracted_text: "some text".to_string(),
                snapshot_html: None,
                readable_html: None,
                pdf_data: None,
                tags: None,
                source: SaveSource::Cli,
            },
        )
        .unwrap();

        let cols = expand_row_insert(db.conn(), "entries", &entry.id.to_string()).unwrap();
        assert!(!cols.is_empty(), "should return column values");

        // Find title column
        let title = cols.iter().find(|(name, _)| name == "title");
        assert!(title.is_some(), "should include title column");
        if let Some((_, Value::Text(t))) = title {
            assert_eq!(t, "Expand Test");
        } else {
            panic!("title should be Text value");
        }
    }

    #[test]
    fn test_expand_deleted_row_returns_empty() {
        let db = test_db();
        let cols = expand_row_insert(db.conn(), "entries", "nonexistent-id").unwrap();
        assert!(cols.is_empty());
    }
}
