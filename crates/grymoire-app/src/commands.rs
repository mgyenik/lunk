use base64::Engine;
use serde::{Deserialize, Serialize};

use grymoire_core::db::{with_db, with_db_mut, DbPool};
use grymoire_core::models::*;
use grymoire_core::{chunks, embeddings, keywords, repo, search, sync, topics};

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
pub fn update_entry_tags(
    db: tauri::State<'_, DbPool>,
    id: String,
    tags: Vec<String>,
) -> Result<Entry, String> {
    let uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("invalid id: {e}"))?;
    with_db_mut(&db, |db| repo::update_entry_tags(db, &uuid, &tags))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_tag_suggestions(
    db: tauri::State<'_, DbPool>,
    domain: Option<String>,
    title: Option<String>,
) -> Result<TagSuggestions, String> {
    let title = title.as_deref().unwrap_or("");
    with_db(&db, |conn| repo::get_tag_suggestions(conn, domain.as_deref(), title))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_entry(
    db: tauri::State<'_, DbPool>,
    id: String,
) -> Result<(), String> {
    let uuid = uuid::Uuid::parse_str(&id).map_err(|e| format!("invalid id: {e}"))?;
    with_db_mut(&db, |db| repo::delete_entry(db, &uuid)).map_err(|e| e.to_string())
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
    model: tauri::State<'_, embeddings::EmbeddingModel>,
    path: String,
) -> Result<Entry, String> {
    let file_path = std::path::Path::new(&path);
    if !file_path.exists() {
        return Err(format!("file not found: {path}"));
    }

    let pdf_data = std::fs::read(file_path).map_err(|e| e.to_string())?;

    let pages = grymoire_core::pdf::extract_pages(&pdf_data);

    let title = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled PDF")
        .to_string();

    let full_text: String = pages.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n\n");

    let req = CreateEntryRequest {
        url: None,
        title,
        content_type: ContentType::Pdf,
        extracted_text: full_text,
        snapshot_html: None,
        readable_html: None,
        pdf_data: Some(pdf_data),
        tags: None,
        source: SaveSource::Api,
    };

    let entry = with_db_mut(&db, |db| repo::create_pdf_entry(db, req, pages))
        .map_err(|e| e.to_string())?;

    // Generate embedding + keywords + chunks for the new entry
    let entry_id = entry.id;
    let model = model.inner();
    if let Err(e) = with_db(&db, |conn| {
        embeddings::embed_entry(conn, model, &entry_id)?;
        keywords::extract_and_store(conn, &entry_id)?;
        chunks::chunk_and_embed_entry(conn, model, &entry_id)?;
        Ok(())
    }) {
        tracing::warn!(%entry_id, "post-save semantic processing failed: {e}");
    }

    Ok(entry)
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

// --- Topic commands ---

#[tauri::command]
pub fn get_topics(db: tauri::State<'_, DbPool>) -> Result<Vec<topics::TopicSummary>, String> {
    with_db(&db, |conn| {
        let t = topics::compute_topics(conn)?;
        topics::get_topic_summaries(conn, &t)
    })
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_topic_entries(
    db: tauri::State<'_, DbPool>,
    label: String,
) -> Result<ListResultResponse, String> {
    with_db(&db, |conn| {
        let all_topics = topics::compute_topics(conn)?;
        let topic = all_topics.iter().find(|t| t.label == label);
        match topic {
            Some(t) => {
                let entries = topics::get_entries_by_ids(conn, &t.entry_ids)?;
                let total = entries.len() as i64;
                Ok(ListResultResponse { entries, total })
            }
            None => Ok(ListResultResponse {
                entries: vec![],
                total: 0,
            }),
        }
    })
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_archive_stats(
    db: tauri::State<'_, DbPool>,
) -> Result<topics::ArchiveStats, String> {
    with_db(&db, topics::get_archive_stats).map_err(|e| e.to_string())
}

// --- Semantic commands ---

#[derive(Serialize)]
pub struct SimilarEntryResponse {
    #[serde(flatten)]
    pub entry: Entry,
    pub similarity: f32,
}

#[tauri::command]
pub fn get_similar_entries(
    db: tauri::State<'_, DbPool>,
    id: String,
    limit: Option<usize>,
) -> Result<Vec<SimilarEntryResponse>, String> {
    let uuid: uuid::Uuid = id.parse().map_err(|e| format!("bad id: {e}"))?;
    let limit = limit.unwrap_or(5);

    with_db(&db, |conn| {
        let results = embeddings::find_similar(conn, &uuid, limit)?;
        Ok(results
            .into_iter()
            .map(|(entry, similarity)| SimilarEntryResponse { entry, similarity })
            .collect())
    })
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_entry_keywords(
    db: tauri::State<'_, DbPool>,
    id: String,
) -> Result<Vec<keywords::Keyword>, String> {
    let uuid: uuid::Uuid = id.parse().map_err(|e| format!("bad id: {e}"))?;
    with_db(&db, |conn| keywords::get_entry_keywords(conn, &uuid)).map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct BackfillResult {
    pub embeddings_created: usize,
    pub keywords_extracted: usize,
    pub chunks_created: usize,
}

#[tauri::command]
pub fn trigger_backfill(
    db: tauri::State<'_, DbPool>,
    model: tauri::State<'_, embeddings::EmbeddingModel>,
) -> Result<BackfillResult, String> {
    with_db(&db, |conn| {
        let embeddings_created = embeddings::embed_all_missing(conn, &model)?;
        let keywords_extracted = keywords::extract_all_missing(conn)?;
        let chunks_created = chunks::chunk_all_missing(conn, &model)?;
        Ok(BackfillResult {
            embeddings_created,
            keywords_extracted,
            chunks_created,
        })
    })
    .map_err(|e| e.to_string())
}
