use base64::Engine;
use rusqlite::{params, types::ValueRef, Connection};
use serde::{Deserialize, Serialize};

use crate::errors::{LunkError, Result};

/// A single row from crsql_changes — represents one column change in a CRR table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangesetRow {
    pub table: String,
    pub pk: String,
    pub cid: String,
    pub val: SqlValue,
    pub col_version: i64,
    pub db_version: i64,
    pub site_id: Option<String>, // hex-encoded BLOB
    pub cl: i64,
    pub seq: i64,
}

/// Any SQLite value, serializable for transport.
/// Blobs are base64-encoded to avoid JSON bloat.
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
    fn from_ref(val: ValueRef<'_>) -> Self {
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

    fn to_rusqlite(&self) -> rusqlite::types::Value {
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
    },
    /// Responder → Initiator: their changes + "I have your changes up to this version"
    Reply {
        site_id: String,
        peer_db_version: i64,
        changesets: Vec<ChangesetRow>,
        db_version: i64,
    },
    /// Initiator → Responder: my changes for you
    Payload {
        changesets: Vec<ChangesetRow>,
        db_version: i64,
    },
}

/// Get the current cr-sqlite database version (Lamport clock).
pub fn get_db_version(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT crsql_db_version()", [], |row| row.get(0))
        .map_err(|e| LunkError::Sync(format!("failed to get db_version: {e}")))
}

/// Get the local site ID as a hex string.
pub fn get_site_id(conn: &Connection) -> Result<String> {
    let site_id: Vec<u8> = conn
        .query_row("SELECT crsql_site_id()", [], |row| row.get(0))
        .map_err(|e| LunkError::Sync(format!("failed to get site_id: {e}")))?;
    Ok(hex::encode(site_id))
}

/// Extract all changesets since the given db_version.
pub fn get_changesets_since(conn: &Connection, since_version: i64) -> Result<Vec<ChangesetRow>> {
    let mut stmt = conn.prepare(
        "SELECT [table], pk, cid, val, col_version, db_version,
                site_id, cl, seq
         FROM crsql_changes
         WHERE db_version > ?1",
    )?;

    let rows = stmt.query_map(params![since_version], |row| {
        let site_id_blob: Option<Vec<u8>> = row.get(6)?;
        Ok(ChangesetRow {
            table: row.get(0)?,
            pk: row.get(1)?,
            cid: row.get(2)?,
            val: SqlValue::from_ref(row.get_ref(3)?),
            col_version: row.get(4)?,
            db_version: row.get(5)?,
            site_id: site_id_blob.map(|b| hex::encode(&b)),
            cl: row.get(7)?,
            seq: row.get(8)?,
        })
    })?;

    let mut changesets = Vec::new();
    for row in rows {
        changesets.push(row.map_err(LunkError::Database)?);
    }

    Ok(changesets)
}

/// Apply changesets received from a remote peer.
/// MUST use bind parameters — never interpolate values directly.
pub fn apply_changesets(conn: &Connection, changesets: &[ChangesetRow]) -> Result<()> {
    let mut stmt = conn.prepare(
        "INSERT INTO crsql_changes
            ([table], pk, cid, val, col_version, db_version, site_id, cl, seq)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;

    for row in changesets {
        let site_id_blob: Option<Vec<u8>> = row.site_id.as_ref().map(|h| {
            hex::decode(h).unwrap_or_default()
        });

        stmt.execute(rusqlite::params![
            row.table,
            row.pk,
            row.cid,
            row.val.to_rusqlite(),
            row.col_version,
            row.db_version,
            site_id_blob,
            row.cl,
            row.seq,
        ])?;
    }

    Ok(())
}

/// After applying remote changesets, rebuild FTS for all affected entries.
/// cr-sqlite changes bypass triggers, so FTS must be updated manually.
pub fn rebuild_fts_after_sync(conn: &Connection, changesets: &[ChangesetRow]) -> Result<()> {
    use std::collections::HashSet;

    // Collect unique entry IDs that were affected
    let mut entry_ids = HashSet::new();

    for row in changesets {
        match row.table.as_str() {
            "entries" => {
                // The pk is the entry's own ID
                entry_ids.insert(row.pk.clone());
            }
            "entry_content" => {
                // The pk is the entry_id
                entry_ids.insert(row.pk.clone());
            }
            _ => {}
        }
    }

    for entry_id_pk in &entry_ids {
        // cr-sqlite encodes PKs; for single-column TEXT PKs it's just the value
        // Strip any surrounding quotes that cr-sqlite may add
        let id_str = entry_id_pk.trim_matches('\'');

        if let Ok(uuid) = uuid::Uuid::parse_str(id_str)
            && let Err(e) = crate::search::rebuild_fts_for_entry(conn, &uuid)
        {
            tracing::warn!("failed to rebuild FTS for {id_str}: {e}");
        }
    }

    Ok(())
}

/// Get or create a sync peer record.
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
        return Err(LunkError::Transport(format!("message too large: {len} bytes")));
    }

    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf)
        .await
        .map_err(|e| LunkError::Transport(format!("read body: {e}")))?;

    serde_json::from_slice(&buf).map_err(|e| LunkError::Sync(format!("invalid message: {e}")))
}
