use std::path::Path;
use std::sync::Arc;

use iroh::endpoint::Connection;
use iroh::protocol::{AcceptError, ProtocolHandler, Router};
use iroh::{Endpoint, EndpointId, SecretKey};
use serde::Serialize;

use crate::db::{self, DbPool};
use crate::errors::{LunkError, Result};
use crate::sync;

/// ALPN protocol identifier for lunk sync.
pub const SYNC_ALPN: &[u8] = b"lunk/sync/1";

/// The sync node manages P2P connectivity via iroh.
pub struct SyncNode {
    endpoint: Endpoint,
    router: Router,
    db: DbPool,
}

impl SyncNode {
    /// Create a new sync node. Loads or generates a persistent secret key.
    pub async fn new(data_dir: &Path, db: DbPool) -> Result<Self> {
        let secret_key = load_or_create_secret_key(data_dir)?;

        let endpoint = Endpoint::builder()
            .secret_key(secret_key)
            .alpns(vec![SYNC_ALPN.to_vec()])
            .bind()
            .await
            .map_err(|e| LunkError::Transport(format!("failed to bind endpoint: {e}")))?;

        let handler = SyncProtocol { db: db.clone() };

        let router = Router::builder(endpoint.clone())
            .accept(SYNC_ALPN, Arc::new(handler))
            .spawn();

        tracing::info!("sync node started: {}", endpoint.id());

        Ok(SyncNode {
            endpoint,
            router,
            db,
        })
    }

    /// Get this node's public ID (share this with peers).
    pub fn node_id(&self) -> EndpointId {
        self.endpoint.id()
    }

    /// Get this node's ID as a string for display/sharing.
    pub fn node_id_string(&self) -> String {
        self.endpoint.id().to_string()
    }

    /// Initiate sync with a remote peer.
    pub async fn sync_with_peer(&self, peer_id: &str) -> Result<SyncReport> {
        let endpoint_id: EndpointId = peer_id
            .parse()
            .map_err(|e| LunkError::Transport(format!("invalid peer id: {e}")))?;

        tracing::info!("syncing with peer {peer_id}");

        let conn: Connection = self
            .endpoint
            .connect(endpoint_id, SYNC_ALPN)
            .await
            .map_err(|e| LunkError::Transport(format!("connect failed: {e}")))?;

        let (mut send, mut recv) = conn
            .open_bi()
            .await
            .map_err(|e| LunkError::Transport(format!("open stream: {e}")))?;

        // Phase 1: Send our state
        let (my_site_id, peer_last_version) = db::with_db(&self.db, |c| {
            let site_id = sync::get_site_id(c)?;
            let peer_ver = sync::get_peer_db_version(c, peer_id)?;
            Ok((site_id, peer_ver))
        })?;

        let init = sync::SyncMessage::Init {
            site_id: my_site_id,
            peer_db_version: peer_last_version,
            protocol_version: sync::PROTOCOL_VERSION,
        };
        sync::write_message(&mut send, &init).await?;

        // Phase 2: Receive their reply
        let reply = sync::read_message(&mut recv).await?;
        let (their_peer_version, their_changesets, their_tombstones, their_db_version) =
            match reply {
                sync::SyncMessage::Reply {
                    peer_db_version,
                    changesets,
                    tombstones,
                    db_version,
                    ..
                } => (peer_db_version, changesets, tombstones, db_version),
                _ => return Err(LunkError::Sync("expected Reply message".into())),
            };

        let received_count = their_changesets.len() + their_tombstones.len();

        // Phase 3: Send our changes they need
        let (my_changesets, my_tombstones, my_db_version) = db::with_db(&self.db, |c| {
            let (cs, ts) = sync::get_changesets_since(c, their_peer_version)?;
            let ver = sync::get_db_version(c)?;
            Ok((cs, ts, ver))
        })?;

        let sent_count = my_changesets.len() + my_tombstones.len();

        let payload = sync::SyncMessage::Payload {
            changesets: my_changesets,
            tombstones: my_tombstones,
            db_version: my_db_version,
        };
        sync::write_message(&mut send, &payload).await?;
        send.finish()
            .map_err(|e| LunkError::Transport(format!("finish send: {e}")))?;

        // Phase 4: Apply their changesets and update tracking
        if !their_changesets.is_empty() || !their_tombstones.is_empty() {
            db::with_db_mut(&self.db, |db| {
                sync::apply_changesets(db, &their_changesets, &their_tombstones)?;
                sync::rebuild_fts_after_sync(db.conn(), &their_changesets)?;
                Ok(())
            })?;
        }

        db::with_db(&self.db, |c| {
            sync::update_peer_version(c, peer_id, their_db_version)
        })?;

        tracing::info!("sync complete: sent {sent_count}, received {received_count}");

        Ok(SyncReport {
            peer_id: peer_id.to_string(),
            sent: sent_count,
            received: received_count,
        })
    }

    /// Sync with all known peers.
    pub async fn sync_all(&self) -> Vec<(String, std::result::Result<SyncReport, String>)> {
        let peers = db::with_db(&self.db, sync::get_sync_peers).unwrap_or_default();

        let mut results = Vec::new();
        for peer in peers {
            let r = self.sync_with_peer(&peer.id).await.map_err(|e| e.to_string());
            results.push((peer.id, r));
        }
        results
    }

    /// Shut down the sync node.
    pub async fn shutdown(self) -> Result<()> {
        self.router
            .shutdown()
            .await
            .map_err(|e| LunkError::Transport(format!("shutdown: {e}")))?;
        Ok(())
    }
}

/// Protocol handler for incoming sync connections.
#[derive(Debug, Clone)]
struct SyncProtocol {
    db: DbPool,
}

impl ProtocolHandler for SyncProtocol {
    async fn accept(&self, connection: Connection) -> std::result::Result<(), AcceptError> {
        let db = self.db.clone();
        handle_incoming_sync(db, connection)
            .await
            .map_err(|e| AcceptError::from_err(e))
    }
}

async fn handle_incoming_sync(
    db: DbPool,
    connection: Connection,
) -> std::result::Result<(), LunkError> {
    let remote = connection.remote_id().to_string();

    tracing::info!("incoming sync from {remote}");

    let (mut send, mut recv) = connection
        .accept_bi()
        .await
        .map_err(|e| LunkError::Transport(format!("accept stream: {e}")))?;

    // Phase 1: Receive their init
    let init = sync::read_message(&mut recv).await?;
    let their_peer_version = match init {
        sync::SyncMessage::Init {
            peer_db_version, ..
        } => peer_db_version,
        _ => return Err(LunkError::Sync("expected Init message".into())),
    };

    // Phase 2: Send our reply with changes they need
    let (my_site_id, my_peer_version, my_changesets, my_tombstones, my_db_version) =
        db::with_db(&db, |c| {
            let site_id = sync::get_site_id(c)?;
            let peer_ver = sync::get_peer_db_version(c, &remote)?;
            let (cs, ts) = sync::get_changesets_since(c, their_peer_version)?;
            let db_ver = sync::get_db_version(c)?;
            Ok((site_id, peer_ver, cs, ts, db_ver))
        })?;

    let reply = sync::SyncMessage::Reply {
        site_id: my_site_id,
        peer_db_version: my_peer_version,
        changesets: my_changesets,
        tombstones: my_tombstones,
        db_version: my_db_version,
    };
    sync::write_message(&mut send, &reply).await?;
    send.finish()
        .map_err(|e| LunkError::Transport(format!("finish send: {e}")))?;

    // Phase 3: Receive their changes
    let payload = sync::read_message(&mut recv).await?;
    let (their_changesets, their_tombstones, their_db_version) = match payload {
        sync::SyncMessage::Payload {
            changesets,
            tombstones,
            db_version,
        } => (changesets, tombstones, db_version),
        _ => return Err(LunkError::Sync("expected Payload message".into())),
    };

    // Phase 4: Apply and update
    let received = their_changesets.len() + their_tombstones.len();
    if !their_changesets.is_empty() || !their_tombstones.is_empty() {
        db::with_db_mut(&db, |db| {
            sync::apply_changesets(db, &their_changesets, &their_tombstones)?;
            sync::rebuild_fts_after_sync(db.conn(), &their_changesets)?;
            Ok(())
        })?;
    }

    db::with_db(&db, |c| {
        sync::update_peer_version(c, &remote, their_db_version)
    })?;

    tracing::info!("incoming sync from {remote}: received {received} changes");

    Ok(())
}

/// Result of a sync operation.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct SyncReport {
    pub peer_id: String,
    pub sent: usize,
    pub received: usize,
}

/// Load a secret key from disk or generate a new one.
fn load_or_create_secret_key(data_dir: &Path) -> Result<SecretKey> {
    let key_path = data_dir.join("secret_key");

    if key_path.exists() {
        let bytes = std::fs::read(&key_path)?;
        let arr: [u8; 32] = bytes.try_into().map_err(|_| {
            LunkError::Config("invalid secret key file (wrong length)".to_string())
        })?;
        let key = SecretKey::from_bytes(&arr);
        tracing::debug!("loaded secret key from {}", key_path.display());
        Ok(key)
    } else {
        let key = SecretKey::generate(&mut rand::rng());
        std::fs::create_dir_all(data_dir)?;
        std::fs::write(&key_path, key.to_bytes())?;
        tracing::info!("generated new secret key at {}", key_path.display());
        Ok(key)
    }
}
