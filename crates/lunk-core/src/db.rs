use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::errors::Result;
use crate::schema;

pub type DbPool = Arc<Mutex<Connection>>;

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

pub fn create_pool(conn: Connection) -> DbPool {
    Arc::new(Mutex::new(conn))
}

pub fn with_db<F, T>(pool: &DbPool, f: F) -> Result<T>
where
    F: FnOnce(&Connection) -> Result<T>,
{
    let conn = pool
        .lock()
        .map_err(|e| crate::errors::LunkError::Other(format!("db lock poisoned: {e}")))?;
    f(&conn)
}

/// Try to load the cr-sqlite extension. Returns true if loaded successfully.
///
/// Searches for the extension in order:
/// 1. Explicit path (if provided)
/// 2. Next to the current executable
/// 3. In the data directory
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

        // Common system paths
        paths.push(std::path::PathBuf::from(format!("/usr/local/lib/{ext_name}")));
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
