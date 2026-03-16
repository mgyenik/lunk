use base64::Engine;
use rusqlite::types::ValueRef;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::db::Db;
use crate::errors::{LunkError, Result};
use crate::hlc::HlcTimestamp;

/// Protocol version for sync messages.
pub const PROTOCOL_VERSION: u32 = 2;

/// A column-level change record for sync transport.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangesetRow {
    pub tbl: String,
    pub row_id: String,
    pub col: String,
    pub val: SqlValue,
    pub hlc_ts: i64,
    pub hlc_counter: i64,
    pub site_id: String,
    pub db_version: i64,
}

/// A tombstone (delete marker) for sync transport.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TombstoneRow {
    pub tbl: String,
    pub row_id: String,
    pub hlc_ts: i64,
    pub hlc_counter: i64,
    pub site_id: String,
    pub db_version: i64,
}

/// Any SQLite value, serializable for transport.
/// Blobs are base64-encoded for JSON compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", content = "v")]
pub enum SqlValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(String), // base64-encoded
}

impl SqlValue {
    pub fn from_ref(val: ValueRef<'_>) -> Self {
        match val {
            ValueRef::Null => SqlValue::Null,
            ValueRef::Integer(i) => SqlValue::Integer(i),
            ValueRef::Real(f) => SqlValue::Real(f),
            ValueRef::Text(s) => {
                SqlValue::Text(String::from_utf8_lossy(s).into_owned())
            }
            ValueRef::Blob(b) => {
                SqlValue::Blob(base64::engine::general_purpose::STANDARD.encode(b))
            }
        }
    }

    pub fn from_value(val: rusqlite::types::Value) -> Self {
        match val {
            rusqlite::types::Value::Null => SqlValue::Null,
            rusqlite::types::Value::Integer(i) => SqlValue::Integer(i),
            rusqlite::types::Value::Real(f) => SqlValue::Real(f),
            rusqlite::types::Value::Text(s) => SqlValue::Text(s),
            rusqlite::types::Value::Blob(b) => {
                SqlValue::Blob(base64::engine::general_purpose::STANDARD.encode(&b))
            }
        }
    }

    pub fn to_rusqlite(&self) -> rusqlite::types::Value {
        match self {
            SqlValue::Null => rusqlite::types::Value::Null,
            SqlValue::Integer(i) => rusqlite::types::Value::Integer(*i),
            SqlValue::Real(f) => rusqlite::types::Value::Real(*f),
            SqlValue::Text(s) => rusqlite::types::Value::Text(s.clone()),
            SqlValue::Blob(b64) => {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .unwrap_or_default();
                rusqlite::types::Value::Blob(bytes)
            }
        }
    }
}

/// Messages exchanged during the sync protocol.
#[derive(Debug, Serialize, Deserialize)]
pub enum SyncMessage {
    /// Initiator → Responder: "I have your changes up to this version"
    Init {
        site_id: String,
        peer_db_version: i64,
        protocol_version: u32,
    },
    /// Responder → Initiator: their changes + version info
    Reply {
        site_id: String,
        peer_db_version: i64,
        changesets: Vec<ChangesetRow>,
        tombstones: Vec<TombstoneRow>,
        db_version: i64,
    },
    /// Initiator → Responder: our changes
    Payload {
        changesets: Vec<ChangesetRow>,
        tombstones: Vec<TombstoneRow>,
        db_version: i64,
    },
}

// --- Change export ---

/// Get the current db_version from sync_meta.
pub fn get_db_version(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT value FROM sync_meta WHERE key = 'db_version'",
        [],
        |row| {
            let s: String = row.get(0)?;
            Ok(s.parse::<i64>().unwrap_or(0))
        },
    )
    .map_err(|e| LunkError::Sync(format!("failed to get db_version: {e}")))
}

/// Get the local site ID from sync_meta.
pub fn get_site_id(conn: &Connection) -> Result<String> {
    conn.query_row(
        "SELECT value FROM sync_meta WHERE key = 'site_id'",
        [],
        |row| row.get(0),
    )
    .map_err(|e| LunkError::Sync(format!("failed to get site_id: {e}")))
}

/// Extract all changesets and tombstones since the given db_version.
///
/// For `__row__` entries (INSERTs), expands to per-column changesets by
/// reading current row values. Skips rows that have since been deleted.
pub fn get_changesets_since(
    conn: &Connection,
    since_version: i64,
) -> Result<(Vec<ChangesetRow>, Vec<TombstoneRow>)> {
    let mut changesets = Vec::new();
    let mut tombstones = Vec::new();

    // Read all change_log entries since version
    let mut stmt = conn.prepare(
        "SELECT tbl, row_id, col, val, hlc_ts, hlc_counter, site_id, db_version
         FROM change_log WHERE db_version > ?1 ORDER BY seq",
    )?;

    #[allow(clippy::type_complexity)]
    let rows: Vec<(String, String, String, SqlValue, i64, i64, String, i64)> = stmt
        .query_map(params![since_version], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                SqlValue::from_ref(row.get_ref(3)?),
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    for (tbl, row_id, col, val, hlc_ts, hlc_counter, site_id, db_version) in rows {
        match col.as_str() {
            "__tombstone__" => {
                tombstones.push(TombstoneRow {
                    tbl,
                    row_id,
                    hlc_ts,
                    hlc_counter,
                    site_id,
                    db_version,
                });
            }
            "__row__" => {
                // Expand to per-column changesets from current row values
                let cols =
                    crate::change_tracking::expand_row_insert(conn, &tbl, &row_id)?;
                for (col_name, col_val) in cols {
                    changesets.push(ChangesetRow {
                        tbl: tbl.clone(),
                        row_id: row_id.clone(),
                        col: col_name,
                        val: SqlValue::from_value(col_val),
                        hlc_ts,
                        hlc_counter,
                        site_id: site_id.clone(),
                        db_version,
                    });
                }
            }
            _ => {
                changesets.push(ChangesetRow {
                    tbl,
                    row_id,
                    col,
                    val,
                    hlc_ts,
                    hlc_counter,
                    site_id,
                    db_version,
                });
            }
        }
    }

    // Also include persistent tombstones
    let mut stmt = conn.prepare(
        "SELECT tbl, row_id, hlc_ts, hlc_counter, site_id, db_version
         FROM tombstones WHERE db_version > ?1",
    )?;

    let ts_rows: Vec<TombstoneRow> = stmt
        .query_map(params![since_version], |row| {
            Ok(TombstoneRow {
                tbl: row.get(0)?,
                row_id: row.get(1)?,
                hlc_ts: row.get(2)?,
                hlc_counter: row.get(3)?,
                site_id: row.get(4)?,
                db_version: row.get(5)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    // Deduplicate (change_log __tombstone__ and tombstones table may overlap)
    for ts in ts_rows {
        if !tombstones.iter().any(|t| t.tbl == ts.tbl && t.row_id == ts.row_id) {
            tombstones.push(ts);
        }
    }

    Ok((changesets, tombstones))
}

// --- Change application ---

/// Apply changesets and tombstones received from a remote peer.
///
/// Uses column-level Last-Writer-Wins with HLC comparison.
/// Returns the number of changes applied.
pub fn apply_changesets(
    db: &mut Db,
    changesets: &[ChangesetRow],
    tombstones: &[TombstoneRow],
) -> Result<usize> {
    // Advance local clock past all incoming timestamps
    for cs in changesets {
        db.hlc_mut().observe(&HlcTimestamp {
            wall_ms: cs.hlc_ts,
            counter: cs.hlc_counter,
            site_id: cs.site_id.clone(),
        });
    }
    for ts in tombstones {
        db.hlc_mut().observe(&HlcTimestamp {
            wall_ms: ts.hlc_ts,
            counter: ts.hlc_counter,
            site_id: ts.site_id.clone(),
        });
    }

    let local_ver = db.next_version();
    let conn = db.conn();

    // Record max change_log seq before apply (to clean up trigger sentinels after)
    let max_seq_before: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM change_log",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let mut applied = 0;

    // Group changesets by (tbl, row_id)
    let mut groups: HashMap<(String, String), Vec<&ChangesetRow>> = HashMap::new();
    for cs in changesets {
        groups
            .entry((cs.tbl.clone(), cs.row_id.clone()))
            .or_default()
            .push(cs);
    }

    // Apply column changes
    for ((tbl, row_id), cols) in &groups {
        let exists = row_exists(conn, tbl, row_id)?;

        if !exists {
            // New row — INSERT from received column values
            if insert_row_from_changesets(conn, tbl, cols)? {
                applied += 1;
                // Log all columns for this new row
                for cs in cols {
                    log_remote_change(conn, cs, local_ver)?;
                }
            }
        } else {
            // Existing row — LWW per column
            for cs in cols {
                let incoming = HlcTimestamp {
                    wall_ms: cs.hlc_ts,
                    counter: cs.hlc_counter,
                    site_id: cs.site_id.clone(),
                };
                if should_apply_column(conn, &cs.tbl, &cs.row_id, &cs.col, &incoming)? {
                    update_column(conn, &cs.tbl, &cs.row_id, &cs.col, &cs.val)?;
                    log_remote_change(conn, cs, local_ver)?;
                    applied += 1;
                }
            }
        }
    }

    // Apply tombstones
    for ts in tombstones {
        let incoming = HlcTimestamp {
            wall_ms: ts.hlc_ts,
            counter: ts.hlc_counter,
            site_id: ts.site_id.clone(),
        };
        if should_apply_tombstone(conn, &ts.tbl, &ts.row_id, &incoming)? {
            delete_row(conn, &ts.tbl, &ts.row_id)?;
            // Record tombstone locally (with remote HLC, local version)
            conn.execute(
                "INSERT OR REPLACE INTO tombstones (tbl, row_id, hlc_ts, hlc_counter, site_id, db_version)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![&ts.tbl, &ts.row_id, ts.hlc_ts, ts.hlc_counter, &ts.site_id, local_ver],
            )?;
            applied += 1;
        }
    }

    // Clean up trigger-created sentinel rows (triggers fire on our INSERT/DELETE above)
    conn.execute(
        "DELETE FROM change_log WHERE seq > ?1 AND hlc_ts = 0 AND site_id = ''",
        params![max_seq_before],
    )?;

    Ok(applied)
}

/// After applying remote changesets, rebuild FTS for affected entries.
pub fn rebuild_fts_after_sync(conn: &Connection, changesets: &[ChangesetRow]) -> Result<()> {
    use std::collections::HashSet;

    let mut entry_ids = HashSet::new();
    for row in changesets {
        match row.tbl.as_str() {
            "entries" | "entry_content" => {
                entry_ids.insert(row.row_id.clone());
            }
            _ => {}
        }
    }

    for id_str in &entry_ids {
        if let Ok(uuid) = uuid::Uuid::parse_str(id_str)
            && let Err(e) = crate::search::rebuild_fts_for_entry(conn, &uuid)
        {
            tracing::warn!("failed to rebuild FTS for {id_str}: {e}");
        }
    }

    Ok(())
}

// --- Internal helpers for apply ---

fn row_exists(conn: &Connection, tbl: &str, row_id: &str) -> Result<bool> {
    let result = match tbl {
        "entry_tags" => {
            let parts: Vec<&str> = row_id.splitn(2, '|').collect();
            if parts.len() != 2 {
                return Ok(false);
            }
            conn.query_row(
                "SELECT 1 FROM entry_tags WHERE entry_id = ?1 AND tag_id = ?2",
                params![parts[0], parts[1]],
                |_| Ok(()),
            )
        }
        "entry_content" => conn.query_row(
            "SELECT 1 FROM entry_content WHERE entry_id = ?1",
            params![row_id],
            |_| Ok(()),
        ),
        _ => conn.query_row(
            &format!("SELECT 1 FROM {tbl} WHERE id = ?1"),
            params![row_id],
            |_| Ok(()),
        ),
    };

    match result {
        Ok(()) => Ok(true),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
        Err(e) => Err(e.into()),
    }
}

fn insert_row_from_changesets(
    conn: &Connection,
    tbl: &str,
    cols: &[&ChangesetRow],
) -> Result<bool> {
    if cols.is_empty() {
        return Ok(false);
    }

    let col_names: Vec<&str> = cols.iter().map(|c| c.col.as_str()).collect();
    let placeholders: Vec<String> = (1..=col_names.len()).map(|i| format!("?{i}")).collect();

    let sql = format!(
        "INSERT OR IGNORE INTO {tbl} ({}) VALUES ({})",
        col_names.join(", "),
        placeholders.join(", ")
    );

    let values: Vec<rusqlite::types::Value> = cols.iter().map(|c| c.val.to_rusqlite()).collect();
    let params: Vec<&dyn rusqlite::types::ToSql> =
        values.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();

    let inserted = conn.execute(&sql, params.as_slice())?;
    Ok(inserted > 0)
}

fn should_apply_column(
    conn: &Connection,
    tbl: &str,
    row_id: &str,
    col: &str,
    incoming: &HlcTimestamp,
) -> Result<bool> {
    let result = conn.query_row(
        "SELECT hlc_ts, hlc_counter, site_id FROM change_log
         WHERE tbl = ?1 AND row_id = ?2 AND col = ?3
         ORDER BY hlc_ts DESC, hlc_counter DESC, site_id DESC
         LIMIT 1",
        params![tbl, row_id, col],
        |row| {
            Ok(HlcTimestamp {
                wall_ms: row.get(0)?,
                counter: row.get(1)?,
                site_id: row.get(2)?,
            })
        },
    );

    match result {
        Ok(local) => Ok(incoming > &local),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(true),
        Err(e) => Err(e.into()),
    }
}

fn should_apply_tombstone(
    conn: &Connection,
    tbl: &str,
    row_id: &str,
    incoming: &HlcTimestamp,
) -> Result<bool> {
    // Compare with the latest local change for this row (any column)
    let result = conn.query_row(
        "SELECT hlc_ts, hlc_counter, site_id FROM change_log
         WHERE tbl = ?1 AND row_id = ?2
         ORDER BY hlc_ts DESC, hlc_counter DESC, site_id DESC
         LIMIT 1",
        params![tbl, row_id],
        |row| {
            Ok(HlcTimestamp {
                wall_ms: row.get(0)?,
                counter: row.get(1)?,
                site_id: row.get(2)?,
            })
        },
    );

    match result {
        Ok(local) => Ok(incoming > &local),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(true),
        Err(e) => Err(e.into()),
    }
}

fn update_column(
    conn: &Connection,
    tbl: &str,
    row_id: &str,
    col: &str,
    val: &SqlValue,
) -> Result<()> {
    let rv = val.to_rusqlite();
    match tbl {
        "entry_tags" => {
            // Junction table — no columns to update (only INSERT/DELETE)
            Ok(())
        }
        "entry_content" => {
            conn.execute(
                &format!("UPDATE entry_content SET \"{col}\" = ?1 WHERE entry_id = ?2"),
                params![rv, row_id],
            )?;
            Ok(())
        }
        _ => {
            conn.execute(
                &format!("UPDATE {tbl} SET \"{col}\" = ?1 WHERE id = ?2"),
                params![rv, row_id],
            )?;
            Ok(())
        }
    }
}

fn delete_row(conn: &Connection, tbl: &str, row_id: &str) -> Result<()> {
    match tbl {
        "entry_tags" => {
            let parts: Vec<&str> = row_id.splitn(2, '|').collect();
            if parts.len() == 2 {
                conn.execute(
                    "DELETE FROM entry_tags WHERE entry_id = ?1 AND tag_id = ?2",
                    params![parts[0], parts[1]],
                )?;
            }
        }
        "entry_content" => {
            conn.execute(
                "DELETE FROM entry_content WHERE entry_id = ?1",
                params![row_id],
            )?;
        }
        _ => {
            conn.execute(
                &format!("DELETE FROM {tbl} WHERE id = ?1"),
                params![row_id],
            )?;
        }
    }
    Ok(())
}

fn log_remote_change(conn: &Connection, cs: &ChangesetRow, local_ver: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO change_log (tbl, row_id, col, val, hlc_ts, hlc_counter, site_id, db_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            &cs.tbl,
            &cs.row_id,
            &cs.col,
            cs.val.to_rusqlite(),
            cs.hlc_ts,
            cs.hlc_counter,
            &cs.site_id,
            local_ver
        ],
    )?;
    Ok(())
}

// --- Peer management (unchanged) ---

/// Get the last-seen db_version for a peer.
pub fn get_peer_db_version(conn: &Connection, peer_id: &str) -> Result<i64> {
    let result = conn.query_row(
        "SELECT last_db_version FROM sync_peers WHERE id = ?1",
        params![peer_id],
        |row| row.get(0),
    );

    match result {
        Ok(v) => Ok(v),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(0),
        Err(e) => Err(e.into()),
    }
}

/// Update the last-seen db_version for a peer after successful sync.
pub fn update_peer_version(conn: &Connection, peer_id: &str, db_version: i64) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO sync_peers (id, last_db_version, last_sync_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET last_db_version = ?2, last_sync_at = ?3",
        params![peer_id, db_version, now],
    )?;
    Ok(())
}

/// Get all sync peers.
pub fn get_sync_peers(conn: &Connection) -> Result<Vec<crate::models::SyncPeer>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, last_sync_at, last_db_version FROM sync_peers ORDER BY name, id",
    )?;

    let rows = stmt.query_map([], |row| {
        let last_sync_str: Option<String> = row.get(2)?;
        Ok(crate::models::SyncPeer {
            id: row.get(0)?,
            name: row.get(1)?,
            last_sync_at: last_sync_str.and_then(|s| {
                chrono::DateTime::parse_from_rfc3339(&s)
                    .ok()
                    .map(|dt| dt.with_timezone(&chrono::Utc))
            }),
            last_db_version: row.get(3)?,
        })
    })?;

    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.into())
}

/// Add a sync peer.
pub fn add_sync_peer(conn: &Connection, id: &str, name: Option<&str>) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO sync_peers (id, name, last_db_version) VALUES (?1, ?2, 0)",
        params![id, name],
    )?;
    Ok(())
}

/// Remove a sync peer.
pub fn remove_sync_peer(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM sync_peers WHERE id = ?1", params![id])?;
    Ok(())
}

// --- Transport helpers ---

/// Write a length-prefixed JSON message to an async writer.
pub async fn write_message(
    send: &mut iroh::endpoint::SendStream,
    msg: &SyncMessage,
) -> Result<()> {
    let json = serde_json::to_vec(msg)?;
    let len = (json.len() as u32).to_be_bytes();
    send.write_all(&len)
        .await
        .map_err(|e| LunkError::Transport(format!("write len: {e}")))?;
    send.write_all(&json)
        .await
        .map_err(|e| LunkError::Transport(format!("write body: {e}")))?;
    Ok(())
}

/// Read a length-prefixed JSON message from an async reader.
pub async fn read_message(recv: &mut iroh::endpoint::RecvStream) -> Result<SyncMessage> {
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf)
        .await
        .map_err(|e| LunkError::Transport(format!("read len: {e}")))?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > 256 * 1024 * 1024 {
        return Err(LunkError::Transport(format!(
            "message too large: {len} bytes"
        )));
    }

    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf)
        .await
        .map_err(|e| LunkError::Transport(format!("read body: {e}")))?;

    serde_json::from_slice(&buf).map_err(|e| LunkError::Sync(format!("invalid message: {e}")))
}
