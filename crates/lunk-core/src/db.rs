use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::errors::{LunkError, Result};
use crate::hlc::HybridClock;
use crate::schema;

/// Database handle with integrated HLC and version counter.
///
/// Wraps a SQLite connection with the state needed for change tracking:
/// a Hybrid Logical Clock for timestamps and a monotonic db_version counter.
pub struct Db {
    conn: Connection,
    hlc: HybridClock,
    db_version: i64,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db")
            .field("db_version", &self.db_version)
            .finish_non_exhaustive()
    }
}

impl Db {
    /// Wrap a connection with default (uninitialized) sync state.
    /// Used for databases that haven't been migrated to v3 yet,
    /// or for in-memory test databases.
    pub fn new(conn: Connection) -> Self {
        Self {
            conn,
            hlc: HybridClock::new(String::new()),
            db_version: 0,
        }
    }

    /// Wrap a connection with restored sync state from sync_meta.
    pub fn with_sync_state(conn: Connection, hlc: HybridClock, db_version: i64) -> Self {
        Self {
            conn,
            hlc,
            db_version,
        }
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn hlc(&self) -> &HybridClock {
        &self.hlc
    }

    pub fn hlc_mut(&mut self) -> &mut HybridClock {
        &mut self.hlc
    }

    pub fn db_version(&self) -> i64 {
        self.db_version
    }

    /// Bump and return the next db_version (monotonic counter for sync).
    pub fn next_version(&mut self) -> i64 {
        self.db_version += 1;
        self.db_version
    }

    /// Generate a new HLC timestamp and bump db_version in one call.
    pub fn next_timestamp(&mut self) -> (crate::hlc::HlcTimestamp, i64) {
        let ts = self.hlc.now();
        let ver = self.next_version();
        (ts, ver)
    }
}

pub type DbPool = Arc<Mutex<Db>>;

/// Tables to register as cr-sqlite CRRs for sync.
const CRR_TABLES: &[&str] = &["entries", "entry_content", "entry_tags", "tags", "pdf_pages"];

pub fn open_database(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let conn = Connection::open(path)?;
    schema::run_migrations(&conn)?;

    Ok(conn)
}

pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    schema::run_migrations(&conn)?;
    Ok(conn)
}

/// Open a database and wrap it in a Db with sync state.
pub fn open_db(path: &Path) -> Result<Db> {
    let conn = open_database(path)?;
    let db = load_sync_state(conn)?;
    Ok(db)
}

/// Open an in-memory database wrapped in Db (for tests).
pub fn open_in_memory_db() -> Result<Db> {
    let conn = open_in_memory()?;
    let db = load_sync_state(conn)?;
    Ok(db)
}

/// Load sync state (site_id, db_version, HLC) from sync_meta if it exists.
fn load_sync_state(conn: Connection) -> Result<Db> {
    // Check if sync_meta table exists (only after migration v3)
    let has_sync_meta: bool = conn
        .query_row(
            "SELECT count(*) > 0 FROM sqlite_master WHERE type='table' AND name='sync_meta'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !has_sync_meta {
        return Ok(Db::new(conn));
    }

    let site_id: String = conn
        .query_row(
            "SELECT value FROM sync_meta WHERE key = 'site_id'",
            [],
            |row| row.get(0),
        )
        .unwrap_or_default();

    let db_version: i64 = conn
        .query_row(
            "SELECT value FROM sync_meta WHERE key = 'db_version'",
            [],
            |row| {
                let s: String = row.get(0)?;
                Ok(s.parse::<i64>().unwrap_or(0))
            },
        )
        .unwrap_or(0);

    let hlc_wall: i64 = conn
        .query_row(
            "SELECT value FROM sync_meta WHERE key = 'hlc_wall_ms'",
            [],
            |row| {
                let s: String = row.get(0)?;
                Ok(s.parse::<i64>().unwrap_or(0))
            },
        )
        .unwrap_or(0);

    let hlc_counter: i64 = conn
        .query_row(
            "SELECT value FROM sync_meta WHERE key = 'hlc_counter'",
            [],
            |row| {
                let s: String = row.get(0)?;
                Ok(s.parse::<i64>().unwrap_or(0))
            },
        )
        .unwrap_or(0);

    let hlc = HybridClock::restore(site_id, hlc_wall, hlc_counter);
    Ok(Db::with_sync_state(conn, hlc, db_version))
}

/// Persist sync state back to sync_meta. Call before closing.
pub fn save_sync_state(db: &Db) -> Result<()> {
    let has_sync_meta: bool = db
        .conn()
        .query_row(
            "SELECT count(*) > 0 FROM sqlite_master WHERE type='table' AND name='sync_meta'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !has_sync_meta {
        return Ok(());
    }

    db.conn().execute(
        "UPDATE sync_meta SET value = ?1 WHERE key = 'db_version'",
        [db.db_version().to_string()],
    )?;
    db.conn().execute(
        "UPDATE sync_meta SET value = ?1 WHERE key = 'hlc_wall_ms'",
        [db.hlc().wall_ms().to_string()],
    )?;
    db.conn().execute(
        "UPDATE sync_meta SET value = ?1 WHERE key = 'hlc_counter'",
        [db.hlc().counter().to_string()],
    )?;

    Ok(())
}

pub fn create_pool(db: Db) -> DbPool {
    Arc::new(Mutex::new(db))
}

/// Read-only access to the database connection.
pub fn with_db<F, T>(pool: &DbPool, f: F) -> Result<T>
where
    F: FnOnce(&Connection) -> Result<T>,
{
    let db = pool
        .lock()
        .map_err(|e| LunkError::Other(format!("db lock poisoned: {e}")))?;
    f(db.conn())
}

/// Mutable access to the Db (connection + HLC + version counter).
/// Use this for all write operations.
pub fn with_db_mut<F, T>(pool: &DbPool, f: F) -> Result<T>
where
    F: FnOnce(&mut Db) -> Result<T>,
{
    let mut db = pool
        .lock()
        .map_err(|e| LunkError::Other(format!("db lock poisoned: {e}")))?;
    f(&mut db)
}

// --- cr-sqlite (legacy, to be removed) ---

/// Try to load the cr-sqlite extension. Returns true if loaded successfully.
pub fn try_load_crsqlite(conn: &Connection, ext_path: Option<&str>) -> bool {
    let candidates: Vec<std::path::PathBuf> = {
        let ext_name = if cfg!(target_os = "macos") {
            "crsqlite.dylib"
        } else if cfg!(target_os = "windows") {
            "crsqlite.dll"
        } else {
            "crsqlite.so"
        };

        let mut paths = Vec::new();

        if let Some(p) = ext_path {
            paths.push(std::path::PathBuf::from(p));
        }

        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            paths.push(dir.join(ext_name));
        }

        if let Ok(data_dir) = crate::config::Config::data_dir() {
            paths.push(data_dir.join(ext_name));
        }

        paths.push(std::path::PathBuf::from(format!(
            "/usr/local/lib/{ext_name}"
        )));
        paths.push(std::path::PathBuf::from(format!("/usr/lib/{ext_name}")));

        paths
    };

    for path in &candidates {
        if !path.exists() {
            continue;
        }

        let result = unsafe {
            if conn.load_extension_enable().is_err() {
                continue;
            }
            conn.load_extension(path, Some("sqlite3_crsqlite_init"))
        };

        match result {
            Ok(_) => {
                tracing::info!("loaded cr-sqlite extension from {}", path.display());
                return true;
            }
            Err(e) => {
                tracing::warn!("failed to load cr-sqlite from {}: {e}", path.display());
            }
        }
    }

    tracing::info!("cr-sqlite extension not found; sync disabled");
    false
}

/// Register tables as CRRs. Call after loading cr-sqlite extension.
pub fn register_crrs(conn: &Connection) -> Result<()> {
    for table in CRR_TABLES {
        conn.execute_batch(&format!("SELECT crsql_as_crr('{table}');"))?;
    }
    tracing::debug!("registered {} tables as CRRs", CRR_TABLES.len());
    Ok(())
}

/// Check if cr-sqlite is loaded by testing if crsql_db_version() works.
pub fn is_crsqlite_loaded(conn: &Connection) -> bool {
    conn.query_row("SELECT crsql_db_version()", [], |row| row.get::<_, i64>(0))
        .is_ok()
}

/// Must be called before closing a connection that has cr-sqlite loaded.
pub fn finalize_crsqlite(conn: &Connection) {
    let _ = conn.execute_batch("SELECT crsql_finalize();");
}
