use base64::Engine;
use serde::{Deserialize, Serialize};

use lunk_core::db::{with_db, DbPool};
use lunk_core::models::*;
use lunk_core::{repo, search, sync};

use crate::SyncNodeCell;

#[derive(Serialize)]
pub struct SearchResultResponse {
    entries: Vec<SearchHit>,
    total: i64,
}

#[derive(Serialize)]
pub struct ListResultResponse {
    entries: Vec<Entry>,
    total: i64,
}

#[derive(Deserialize)]
pub struct ListParamsInput {
    status: Option<String>,
    #[serde(rename = "contentType")]
    content_type: Option<String>,
    tag: Option<String>,
    domain: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Serialize)]
pub struct EntryContentResponse {
    entry_id: String,
    extracted_text: String,
    snapshot_html: Option<String>,
    readable_html: Option<String>,
    pdf_base64: Option<String>,
}

#[derive(Serialize)]
pub struct SyncStatusResponse {
    sync_available: bool,
    node_id: Option<String>,
    peers: Vec<SyncPeer>,
}

#[derive(Serialize)]
pub struct SyncResultItem {
    peer_id: String,
    success: bool,
    sent: Option<usize>,
    received: Option<usize>,
    error: Option<String>,
}

#[tauri::command]
pub fn search_entries(
    db: tauri::State<'_, DbPool>,
    query: String,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<SearchResultResponse, String> {
    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);

    let result = with_db(&db, |conn| search::search(conn, &query, limit, offset))
        .map_err(|e| e.to_string())?;

    Ok(SearchResultResponse {
        entries: result.entries,
        total: result.total,
    })
}

#[tauri::command]
pub fn list_entries(
    db: tauri::State<'_, DbPool>,
    params: ListParamsInput,
) -> Result<ListResultResponse, String> {
    let list_params = ListParams {
        status: params.status.as_deref().and_then(EntryStatus::parse),
        content_type: params.content_type.as_deref().and_then(ContentType::parse),
        tag: params.tag,
        domain: params.domain,
        limit: params.limit.or(Some(50)),
        offset: params.offset.or(Some(0)),
        ..Default::default()
    };

    let (entries, total) = with_db(&db, |conn| repo::list_entries(conn, &list_params))
        .map_err(|e| e.to_string())?;

    Ok(ListResultResponse { entries, total })
}

#[tauri::command]
pub fn get_queue(
    db: tauri::State<'_, DbPool>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<ListResultResponse, String> {
    let params = ListParams {
        status: Some(EntryStatus::Unread),
        sort: Some("created_at".to_string()),
        order: Some("desc".to_string()),
        limit: limit.or(Some(50)),
        offset: offset.or(Some(0)),
        ..Default::default()
    };

    let (entries, total) = with_db(&db, |conn| repo::list_entries(conn, &params))
        .map_err(|e| e.to_string())?;

    Ok(ListResultResponse { entries, total })
}

#[tauri::command]
pub fn get_entry(
    db: tauri::State<'_, DbPool>,
    id: String,
) -> Result<Entry, String> {
    let uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("invalid id: {e}"))?;
    with_db(&db, |conn| repo::get_entry(conn, &uuid)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_entry_content(
    db: tauri::State<'_, DbPool>,
    id: String,
) -> Result<EntryContentResponse, String> {
    let engine = base64::engine::general_purpose::STANDARD;
    let uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("invalid id: {e}"))?;

    let content = with_db(&db, |conn| repo::get_entry_content(conn, &uuid))
        .map_err(|e| e.to_string())?;

    Ok(EntryContentResponse {
        entry_id: content.entry_id.to_string(),
        extracted_text: content.extracted_text,
        snapshot_html: content.snapshot_html.map(|b| engine.encode(&b)),
        readable_html: content.readable_html.map(|b| engine.encode(&b)),
        pdf_base64: content.pdf_data.map(|b| engine.encode(&b)),
    })
}

#[tauri::command]
pub fn update_entry_status(
    db: tauri::State<'_, DbPool>,
    id: String,
    status: String,
) -> Result<(), String> {
    let uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("invalid id: {e}"))?;
    let status = EntryStatus::parse(&status)
        .ok_or_else(|| format!("invalid status: {status}"))?;

    with_db(&db, |conn| repo::update_entry_status(conn, &uuid, status))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_entry(
    db: tauri::State<'_, DbPool>,
    id: String,
) -> Result<(), String> {
    let uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("invalid id: {e}"))?;
    with_db(&db, |conn| repo::delete_entry(conn, &uuid)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_tags(
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<TagWithCount>, String> {
    with_db(&db, repo::get_tags).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_pdf(
    db: tauri::State<'_, DbPool>,
    path: String,
) -> Result<Entry, String> {
    let file_path = std::path::Path::new(&path);
    if !file_path.exists() {
        return Err(format!("file not found: {path}"));
    }

    let pdf_data = std::fs::read(file_path).map_err(|e| e.to_string())?;

    let text = pdf_extract::extract_text_from_mem(&pdf_data)
        .map_err(|e| format!("PDF extraction failed: {e}"))?;

    let title = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled PDF")
        .to_string();

    let pages: Vec<(i32, String)> = text
        .split('\u{0C}')
        .enumerate()
        .filter_map(|(i, t): (usize, &str)| {
            let trimmed = t.trim().to_string();
            if trimmed.is_empty() { None } else { Some((i as i32 + 1, trimmed)) }
        })
        .collect();

    let full_text = if pages.is_empty() { text.trim().to_string() } else {
        pages.iter().map(|(_, t): &(i32, String)| t.as_str()).collect::<Vec<_>>().join("\n\n")
    };

    let final_pages = if pages.is_empty() && !full_text.is_empty() {
        vec![(1, full_text.clone())]
    } else {
        pages
    };

    let req = CreateEntryRequest {
        url: None,
        title,
        content_type: ContentType::Pdf,
        extracted_text: full_text,
        snapshot_html: None,
        readable_html: None,
        pdf_data: Some(pdf_data),
        status: Some(EntryStatus::Unread),
        tags: None,
        source: SaveSource::Api,
    };

    with_db(&db, |conn| repo::create_pdf_entry(conn, req, final_pages))
        .map_err(|e| e.to_string())
}

// --- Sync commands ---

#[tauri::command]
pub fn get_sync_status(
    db: tauri::State<'_, DbPool>,
    sync_node: tauri::State<'_, SyncNodeCell>,
) -> Result<SyncStatusResponse, String> {
    let node_id = sync_node.get().map(|n| n.node_id_string());
    let peers = with_db(&db, sync::get_sync_peers).map_err(|e| e.to_string())?;
    Ok(SyncStatusResponse {
        sync_available: sync_node.get().is_some(),
        node_id,
        peers,
    })
}

#[tauri::command]
pub fn get_sync_peers(
    db: tauri::State<'_, DbPool>,
) -> Result<Vec<SyncPeer>, String> {
    with_db(&db, sync::get_sync_peers).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_sync_peer(
    db: tauri::State<'_, DbPool>,
    id: String,
    name: Option<String>,
) -> Result<(), String> {
    with_db(&db, |conn| sync::add_sync_peer(conn, &id, name.as_deref()))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_sync_peer(
    db: tauri::State<'_, DbPool>,
    id: String,
) -> Result<(), String> {
    with_db(&db, |conn| sync::remove_sync_peer(conn, &id))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn trigger_sync(
    sync_node: tauri::State<'_, SyncNodeCell>,
) -> Result<Vec<SyncResultItem>, String> {
    let node = sync_node
        .get()
        .cloned()
        .ok_or_else(|| "sync not available".to_string())?;

    let results = node.sync_all().await;

    Ok(results
        .into_iter()
        .map(|(peer_id, result)| match result {
            Ok(report) => SyncResultItem {
                peer_id,
                success: true,
                sent: Some(report.sent),
                received: Some(report.received),
                error: None,
            },
            Err(e) => SyncResultItem {
                peer_id,
                success: false,
                sent: None,
                received: None,
                error: Some(e),
            },
        })
        .collect())
}
