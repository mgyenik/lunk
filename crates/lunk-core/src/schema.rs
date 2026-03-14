use rusqlite::Connection;

use crate::errors::Result;

pub const SCHEMA_VERSION: i32 = 1;

pub fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL
        );",
    )?;

    let current_version: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if current_version < 1 {
        migrate_v1(conn)?;
    }

    Ok(())
}

fn migrate_v1(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS entries (
            id          TEXT PRIMARY KEY,
            url         TEXT,
            title       TEXT NOT NULL,
            content_type TEXT NOT NULL,
            status      TEXT NOT NULL DEFAULT 'unread',
            domain      TEXT,
            word_count  INTEGER,
            page_count  INTEGER,
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL,
            saved_by    TEXT NOT NULL DEFAULT 'extension'
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS entries_fts USING fts5(
            title,
            extracted_text,
            content='',
            content_rowid='rowid',
            tokenize='porter unicode61'
        );

        CREATE TABLE IF NOT EXISTS entry_content (
            entry_id       TEXT PRIMARY KEY REFERENCES entries(id) ON DELETE CASCADE,
            extracted_text TEXT NOT NULL,
            snapshot_html  BLOB,
            readable_html  BLOB,
            pdf_data       BLOB
        );

        CREATE TABLE IF NOT EXISTS pdf_pages (
            id       TEXT PRIMARY KEY,
            entry_id TEXT NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
            page_num INTEGER NOT NULL,
            text     TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS tags (
            id   TEXT PRIMARY KEY,
            name TEXT NOT NULL UNIQUE
        );

        CREATE TABLE IF NOT EXISTS entry_tags (
            entry_id TEXT NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
            tag_id   TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
            PRIMARY KEY (entry_id, tag_id)
        );

        CREATE TABLE IF NOT EXISTS sync_peers (
            id              TEXT PRIMARY KEY,
            name            TEXT,
            last_sync_at    TEXT,
            last_db_version INTEGER NOT NULL DEFAULT 0
        );

        CREATE INDEX IF NOT EXISTS idx_entries_status ON entries(status);
        CREATE INDEX IF NOT EXISTS idx_entries_content_type ON entries(content_type);
        CREATE INDEX IF NOT EXISTS idx_entries_created_at ON entries(created_at);
        CREATE INDEX IF NOT EXISTS idx_entries_domain ON entries(domain);
        CREATE INDEX IF NOT EXISTS idx_entries_url ON entries(url);
        CREATE INDEX IF NOT EXISTS idx_pdf_pages_entry ON pdf_pages(entry_id, page_num);

        -- FTS triggers for entry_content
        CREATE TRIGGER IF NOT EXISTS entries_fts_insert AFTER INSERT ON entry_content
        BEGIN
            INSERT INTO entries_fts(rowid, title, extracted_text)
            SELECT e.rowid, e.title, NEW.extracted_text
            FROM entries e WHERE e.id = NEW.entry_id;
        END;

        CREATE TRIGGER IF NOT EXISTS entries_fts_delete AFTER DELETE ON entry_content
        BEGIN
            INSERT INTO entries_fts(entries_fts, rowid, title, extracted_text)
            SELECT 'delete', e.rowid, e.title, OLD.extracted_text
            FROM entries e WHERE e.id = OLD.entry_id;
        END;

        CREATE TRIGGER IF NOT EXISTS entries_fts_update AFTER UPDATE OF extracted_text ON entry_content
        BEGIN
            INSERT INTO entries_fts(entries_fts, rowid, title, extracted_text)
            SELECT 'delete', e.rowid, e.title, OLD.extracted_text
            FROM entries e WHERE e.id = OLD.entry_id;
            INSERT INTO entries_fts(rowid, title, extracted_text)
            SELECT e.rowid, e.title, NEW.extracted_text
            FROM entries e WHERE e.id = NEW.entry_id;
        END;

        INSERT INTO schema_version (version) VALUES (1);
        ",
    )?;

    Ok(())
}
