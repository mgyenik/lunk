use rusqlite::{params, Connection};

use crate::errors::Result;

/// Current schema version. Bump this when adding a new migration.
pub const SCHEMA_VERSION: i32 = 3;

/// A schema migration: version number, human description, and migration function.
struct Migration {
    version: i32,
    description: &'static str,
    up: fn(&Connection) -> Result<()>,
}

/// Registry of all migrations in order. Each migrate_vN function handles both
/// DDL changes (ALTER TABLE, new indexes) and inline data transforms (backfills).
///
/// To add a new migration:
/// 1. Write `fn migrate_vN(conn: &Connection) -> Result<()>` below
/// 2. Add it to this array
/// 3. Bump SCHEMA_VERSION to match
///
/// Migrations run automatically on startup. Each one runs in a savepoint so
/// a failure rolls back only that migration, leaving earlier ones applied.
/// The exception is FTS5 virtual table operations which cannot run inside
/// transactions — those must be in their own migration or use rebuild_fts().
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "Initial schema: entries, content, FTS5, tags, sync peers",
        up: migrate_v1,
    },
    Migration {
        version: 2,
        description: "Add index_status and index_version columns for extraction tracking",
        up: migrate_v2,
    },
    Migration {
        version: 3,
        description: "Add change tracking tables and triggers for CRDT sync",
        up: migrate_v3,
    },
];

/// Run all pending migrations. Called on every database open.
pub fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;

    // Create version tracking table
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL
        );",
    )?;

    // Upgrade the tracking table itself (add columns that may not exist yet).
    // SQLite has no ALTER TABLE ADD COLUMN IF NOT EXISTS, so we attempt each
    // and ignore the "duplicate column" error.
    for col in &[
        "ALTER TABLE schema_version ADD COLUMN description TEXT",
        "ALTER TABLE schema_version ADD COLUMN applied_at TEXT",
    ] {
        let _ = conn.execute_batch(col);
    }

    let current_version: i32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    for migration in MIGRATIONS {
        if migration.version > current_version {
            tracing::info!(
                "applying migration v{}: {}",
                migration.version,
                migration.description
            );

            // Run the migration in a savepoint so failures are isolated
            let sp_name = format!("migrate_v{}", migration.version);
            conn.execute_batch(&format!("SAVEPOINT \"{sp_name}\""))?;

            match (migration.up)(conn) {
                Ok(()) => {
                    conn.execute(
                        "INSERT INTO schema_version (version, description, applied_at) \
                         VALUES (?1, ?2, datetime('now'))",
                        params![migration.version, migration.description],
                    )?;
                    conn.execute_batch(&format!("RELEASE \"{sp_name}\""))?;
                    tracing::info!("migration v{} complete", migration.version);
                }
                Err(e) => {
                    tracing::error!("migration v{} failed: {e}", migration.version);
                    conn.execute_batch(&format!("ROLLBACK TO \"{sp_name}\""))?;
                    conn.execute_batch(&format!("RELEASE \"{sp_name}\""))?;
                    return Err(e);
                }
            }
        }
    }

    Ok(())
}

/// Get the current schema version from the database.
pub fn current_version(conn: &Connection) -> Result<i32> {
    let v = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    Ok(v)
}

/// Get info about all applied migrations.
pub fn applied_migrations(conn: &Connection) -> Result<Vec<(i32, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT version, COALESCE(description, ''), COALESCE(applied_at, '') \
         FROM schema_version ORDER BY version",
    )?;
    let rows = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Completely rebuild the FTS5 index from scratch.
///
/// This drops and recreates the FTS virtual table, triggers, and repopulates
/// from entry_content + entries. Use this when:
/// - The tokenizer configuration changes
/// - The set of indexed columns changes
/// - FTS index becomes corrupted
/// - After bulk data imports that bypassed triggers
///
/// Returns the number of entries indexed.
pub fn rebuild_fts(conn: &Connection) -> Result<usize> {
    tracing::info!("rebuilding FTS index...");

    // Drop existing FTS table and triggers
    conn.execute_batch(
        "DROP TRIGGER IF EXISTS entries_fts_insert;
         DROP TRIGGER IF EXISTS entries_fts_delete;
         DROP TRIGGER IF EXISTS entries_fts_update;
         DROP TABLE IF EXISTS entries_fts;",
    )?;

    // Recreate FTS table with current tokenizer config
    conn.execute_batch(
        "CREATE VIRTUAL TABLE entries_fts USING fts5(
            title,
            extracted_text,
            content='',
            content_rowid='rowid',
            tokenize='porter unicode61'
        );",
    )?;

    // Repopulate from existing data
    let count: usize = conn.query_row(
        "SELECT COUNT(*) FROM entries e JOIN entry_content ec ON ec.entry_id = e.id",
        [],
        |row| row.get(0),
    )?;

    conn.execute_batch(
        "INSERT INTO entries_fts(rowid, title, extracted_text)
         SELECT e.rowid, e.title, ec.extracted_text
         FROM entries e
         JOIN entry_content ec ON ec.entry_id = e.id;",
    )?;

    // Recreate triggers
    create_fts_triggers(conn)?;

    tracing::info!("FTS rebuild complete: {count} entries indexed");
    Ok(count)
}

/// Create the FTS sync triggers on entry_content.
fn create_fts_triggers(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS entries_fts_insert AFTER INSERT ON entry_content
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
        END;",
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Migration v1: Initial schema
// ---------------------------------------------------------------------------

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
        ",
    )?;

    // FTS and triggers are created outside the savepoint (FTS5 virtual tables
    // have limitations with transactions in some SQLite versions).
    // Using IF NOT EXISTS makes this safe to re-run.
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS entries_fts USING fts5(
            title,
            extracted_text,
            content='',
            content_rowid='rowid',
            tokenize='porter unicode61'
        );",
    )?;

    create_fts_triggers(conn)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Migration v2: Index status + version tracking
// ---------------------------------------------------------------------------

fn migrate_v2(conn: &Connection) -> Result<()> {
    // Add columns for tracking extraction/indexing state
    conn.execute_batch(
        "ALTER TABLE entries ADD COLUMN index_status TEXT NOT NULL DEFAULT 'ok';
         ALTER TABLE entries ADD COLUMN index_version INTEGER NOT NULL DEFAULT 0;",
    )?;

    // Backfill: entries with non-empty extracted text get version 1 (ok).
    // PDFs with no extracted text get status 'failed', version 0.
    conn.execute_batch(
        "UPDATE entries SET index_version = 1
         WHERE id IN (
             SELECT e.id FROM entries e
             JOIN entry_content ec ON ec.entry_id = e.id
             WHERE ec.extracted_text IS NOT NULL AND ec.extracted_text != ''
         );

         UPDATE entries SET index_status = 'failed'
         WHERE content_type = 'pdf'
           AND id IN (
               SELECT e.id FROM entries e
               JOIN entry_content ec ON ec.entry_id = e.id
               WHERE ec.extracted_text IS NULL OR ec.extracted_text = ''
           );",
    )?;

    let backfilled: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entries WHERE index_version = 1",
        [],
        |row| row.get(0),
    )?;
    let failed: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entries WHERE index_status = 'failed'",
        [],
        |row| row.get(0),
    )?;
    tracing::info!(
        "backfilled index tracking: {backfilled} ok, {failed} failed"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Migration v3: Change tracking tables + triggers for CRDT sync
// ---------------------------------------------------------------------------

fn migrate_v3(conn: &Connection) -> Result<()> {
    let site_id = uuid::Uuid::now_v7().to_string();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_millis() as i64;

    // sync_meta: key-value store for site identity and clock state
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS sync_meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )?;

    conn.execute(
        "INSERT OR IGNORE INTO sync_meta (key, value) VALUES ('site_id', ?1)",
        params![&site_id],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO sync_meta (key, value) VALUES ('db_version', '1')",
        [],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO sync_meta (key, value) VALUES ('hlc_wall_ms', ?1)",
        params![now_ms.to_string()],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO sync_meta (key, value) VALUES ('hlc_counter', '0')",
        [],
    )?;

    // change_log: per-column change records for sync
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS change_log (
            seq         INTEGER PRIMARY KEY AUTOINCREMENT,
            tbl         TEXT    NOT NULL,
            row_id      TEXT    NOT NULL,
            col         TEXT    NOT NULL,
            val         BLOB,
            hlc_ts      INTEGER NOT NULL,
            hlc_counter INTEGER NOT NULL,
            site_id     TEXT    NOT NULL,
            db_version  INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_change_log_version ON change_log(db_version);",
    )?;

    // tombstones: persistent delete markers that survive change_log compaction
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS tombstones (
            tbl         TEXT NOT NULL,
            row_id      TEXT NOT NULL,
            hlc_ts      INTEGER NOT NULL,
            hlc_counter INTEGER NOT NULL,
            site_id     TEXT NOT NULL,
            db_version  INTEGER NOT NULL,
            PRIMARY KEY (tbl, row_id)
        );
        CREATE INDEX IF NOT EXISTS idx_tombstones_version ON tombstones(db_version);",
    )?;

    // Install change tracking triggers
    crate::change_tracking::install_triggers(conn)?;

    // Seed change_log for all existing data so it gets synced on first connect
    for (tbl, pk_expr) in &[
        ("entries", "id"),
        ("entry_content", "entry_id"),
        ("tags", "id"),
        ("pdf_pages", "id"),
    ] {
        conn.execute(
            &format!(
                "INSERT INTO change_log (tbl, row_id, col, val, hlc_ts, hlc_counter, site_id, db_version)
                 SELECT ?1, {pk_expr}, '__row__', NULL, ?2, 0, ?3, 1
                 FROM {tbl}"
            ),
            params![tbl, now_ms, &site_id],
        )?;
    }

    // entry_tags: composite PK
    conn.execute(
        "INSERT INTO change_log (tbl, row_id, col, val, hlc_ts, hlc_counter, site_id, db_version)
         SELECT 'entry_tags', entry_id || '|' || tag_id, '__row__', NULL, ?1, 0, ?2, 1
         FROM entry_tags",
        params![now_ms, &site_id],
    )?;

    let seeded: i64 = conn.query_row(
        "SELECT COUNT(*) FROM change_log",
        [],
        |row| row.get(0),
    )?;
    tracing::info!(
        "change tracking initialized: site_id={}, seeded {seeded} entries",
        &site_id[..8]
    );

    Ok(())
}
