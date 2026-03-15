use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use lunk_core::db::with_db;
use lunk_core::models::*;
use lunk_core::{repo, search};

use crate::state::AppState;

pub fn api_routes() -> Router<AppState> {
    Router::new()
        .route("/entries", post(create_entry))
        .route("/entries", get(list_entries))
        .route("/entries/{id}", get(get_entry))
        .route("/entries/{id}", put(update_entry))
        .route("/entries/{id}", delete(delete_entry))
        .route("/entries/{id}/content", get(get_entry_content))
        .route("/entries/{id}/tags", put(update_entry_tags))
        .route("/search", get(search_entries))
        .route("/tags", get(get_tags))
        .route("/tags/suggestions", get(get_tag_suggestions))
        .route("/sync/status", get(sync_status))
        .route("/sync/peers", get(list_sync_peers))
        .route("/sync/peers", post(add_sync_peer_handler))
        .route("/sync/peers/{id}", delete(remove_sync_peer_handler))
        .route("/sync/trigger", post(trigger_sync))
        .route("/health", get(health))
}

// --- Request/Response types ---

#[derive(Deserialize)]
struct CreateEntryBody {
    url: Option<String>,
    title: String,
    content_type: String,
    extracted_text: String,
    snapshot_html: Option<String>,   // base64
    readable_html: Option<String>,   // base64
    pdf_base64: Option<String>,
    tags: Option<Vec<String>>,
    source: Option<String>,
}

#[derive(Deserialize)]
struct UpdateEntryBody {
    title: Option<String>,
    tags: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct UpdateTagsBody {
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct ListQuery {
    content_type: Option<String>,
    tag: Option<String>,
    domain: Option<String>,
    sort: Option<String>,
    order: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Deserialize)]
struct TagSuggestionsQuery {
    domain: Option<String>,
    title: Option<String>,
}

#[derive(Serialize)]
struct ListResponse {
    entries: Vec<Entry>,
    total: i64,
    offset: i64,
    limit: i64,
}

#[derive(Serialize)]
struct ContentResponse {
    entry_id: String,
    extracted_text: String,
    snapshot_html: Option<String>,   // base64
    readable_html: Option<String>,   // base64
    pdf_base64: Option<String>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

type ApiResult<T> = std::result::Result<T, ApiError>;

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let body = serde_json::json!({ "error": self.1 });
        (self.0, Json(body)).into_response()
    }
}

impl From<lunk_core::errors::LunkError> for ApiError {
    fn from(err: lunk_core::errors::LunkError) -> Self {
        match &err {
            lunk_core::errors::LunkError::NotFound(_) => {
                ApiError(StatusCode::NOT_FOUND, err.to_string())
            }
            lunk_core::errors::LunkError::InvalidInput(_) => {
                ApiError(StatusCode::BAD_REQUEST, err.to_string())
            }
            _ => ApiError(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
        }
    }
}

// --- Handlers ---

async fn create_entry(
    State(state): State<AppState>,
    Json(body): Json<CreateEntryBody>,
) -> ApiResult<(StatusCode, Json<Entry>)> {
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;

    let content_type = ContentType::parse(&body.content_type)
        .ok_or_else(|| ApiError(StatusCode::BAD_REQUEST, format!("invalid content_type: {}", body.content_type)))?;

    let source = match body.source.as_deref() {
        Some("extension") => SaveSource::Extension,
        Some("cli") => SaveSource::Cli,
        _ => SaveSource::Api,
    };

    let mut req = CreateEntryRequest {
        url: body.url,
        title: body.title,
        content_type,
        extracted_text: body.extracted_text,
        snapshot_html: body.snapshot_html.map(|s| engine.decode(s)).transpose()
            .map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("invalid snapshot_html base64: {e}")))?,
        readable_html: body.readable_html.map(|s| engine.decode(s)).transpose()
            .map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("invalid readable_html base64: {e}")))?,
        pdf_data: body.pdf_base64.map(|s| engine.decode(s)).transpose()
            .map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("invalid pdf_base64: {e}")))?,
        tags: body.tags,
        source,
    };

    // Check for duplicate URL before creating
    if let Some(ref url) = req.url {
        let existing = with_db(&state.db, |conn| repo::entry_exists_by_url(conn, url))?;
        if let Some(existing_id) = existing {
            let entry = with_db(&state.db, |conn| repo::get_entry(conn, &existing_id))?;
            return Ok((StatusCode::OK, Json(entry)));
        }
    }

    // For PDFs: extract text server-side if not provided
    if content_type == ContentType::Pdf
        && req.pdf_data.is_some()
    {
        let pdf_data = req.pdf_data.as_ref().unwrap();
        let pages = lunk_core::pdf::extract_pages(pdf_data);
        let full_text: String = pages.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n\n");

        if pages.is_empty() {
            tracing::warn!(
                url = ?req.url,
                title = %req.title,
                "PDF text extraction failed — entry will be saved but not searchable"
            );
        }

        // Try to get a good title: PDF metadata > URL filename > provided title
        let better_title = if lunk_core::pdf::is_generic_title(&req.title) {
            lunk_core::pdf::extract_title(pdf_data)
                .or_else(|| {
                    req.url.as_deref()
                        .and_then(|u| url::Url::parse(u).ok())
                        .and_then(|u| u.path_segments()?.next_back().map(|s| s.to_string()))
                        .filter(|f| !f.is_empty() && !lunk_core::pdf::is_generic_title(f))
                })
        } else {
            None
        };

        let mut req = req;
        if let Some(t) = better_title {
            req.title = t;
        }
        if !full_text.is_empty() {
            req.extracted_text = full_text;
        }

        let entry = with_db(&state.db, |conn| repo::create_pdf_entry(conn, req, pages))?;
        return Ok((StatusCode::CREATED, Json(entry)));
    }

    // For articles: use URL page title if title is empty
    if req.title.is_empty() {
        if let Some(ref url) = req.url {
            req.title = url.clone();
        } else {
            req.title = "Untitled".to_string();
        }
    }

    let entry = with_db(&state.db, |conn| repo::create_entry(conn, req))?;
    Ok((StatusCode::CREATED, Json(entry)))
}

async fn list_entries(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> ApiResult<Json<ListResponse>> {
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset.unwrap_or(0);

    let params = ListParams {
        content_type: query.content_type.as_deref().and_then(ContentType::parse),
        tag: query.tag,
        domain: query.domain,
        sort: query.sort,
        order: query.order,
        limit: Some(limit),
        offset: Some(offset),
    };

    let (entries, total) = with_db(&state.db, |conn| repo::list_entries(conn, &params))?;

    Ok(Json(ListResponse {
        entries,
        total,
        offset,
        limit,
    }))
}

async fn get_entry(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<Entry>> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("invalid id: {e}")))?;
    let entry = with_db(&state.db, |conn| repo::get_entry(conn, &uuid))?;
    Ok(Json(entry))
}

async fn update_entry(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateEntryBody>,
) -> ApiResult<Json<Entry>> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("invalid id: {e}")))?;

    let entry = with_db(&state.db, |conn| {
        repo::update_entry(conn, &uuid, body.title.as_deref(), body.tags.as_deref())
    })?;

    Ok(Json(entry))
}

async fn update_entry_tags(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateTagsBody>,
) -> ApiResult<Json<Entry>> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("invalid id: {e}")))?;

    let entry = with_db(&state.db, |conn| {
        repo::update_entry_tags(conn, &uuid, &body.tags)
    })?;

    Ok(Json(entry))
}

async fn delete_entry(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("invalid id: {e}")))?;
    with_db(&state.db, |conn| repo::delete_entry(conn, &uuid))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_entry_content(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<Json<ContentResponse>> {
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;

    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, format!("invalid id: {e}")))?;

    let content = with_db(&state.db, |conn| repo::get_entry_content(conn, &uuid))?;

    Ok(Json(ContentResponse {
        entry_id: content.entry_id.to_string(),
        extracted_text: content.extracted_text,
        snapshot_html: content.snapshot_html.map(|b| engine.encode(b)),
        readable_html: content.readable_html.map(|b| engine.encode(b)),
        pdf_base64: content.pdf_data.map(|b| engine.encode(b)),
    }))
}

async fn search_entries(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> ApiResult<Json<SearchResult>> {
    let limit = query.limit.unwrap_or(20);
    let offset = query.offset.unwrap_or(0);

    let results = with_db(&state.db, |conn| search::search(conn, &query.q, limit, offset))?;
    Ok(Json(results))
}

async fn get_tags(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<TagWithCount>>> {
    let tags = with_db(&state.db, repo::get_tags)?;
    Ok(Json(tags))
}

async fn get_tag_suggestions(
    State(state): State<AppState>,
    Query(query): Query<TagSuggestionsQuery>,
) -> ApiResult<Json<TagSuggestions>> {
    let title = query.title.as_deref().unwrap_or("");
    let suggestions = with_db(&state.db, |conn| {
        repo::get_tag_suggestions(conn, query.domain.as_deref(), title)
    })?;
    Ok(Json(suggestions))
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

// --- Sync handlers ---

#[derive(Serialize)]
struct SyncStatusResponse {
    sync_available: bool,
    node_id: Option<String>,
    peers: Vec<lunk_core::models::SyncPeer>,
}

#[derive(Deserialize)]
struct AddPeerBody {
    id: String,
    name: Option<String>,
}

#[derive(Serialize)]
struct SyncTriggerResult {
    peer_id: String,
    success: bool,
    sent: Option<usize>,
    received: Option<usize>,
    error: Option<String>,
}

async fn sync_status(
    State(state): State<AppState>,
) -> ApiResult<Json<SyncStatusResponse>> {
    let node_id = state.sync_node.as_ref().map(|n| n.node_id_string());
    let peers = with_db(&state.db, lunk_core::sync::get_sync_peers)?;
    Ok(Json(SyncStatusResponse {
        sync_available: state.sync_node.is_some(),
        node_id,
        peers,
    }))
}

async fn list_sync_peers(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<lunk_core::models::SyncPeer>>> {
    let peers = with_db(&state.db, lunk_core::sync::get_sync_peers)?;
    Ok(Json(peers))
}

async fn add_sync_peer_handler(
    State(state): State<AppState>,
    Json(body): Json<AddPeerBody>,
) -> ApiResult<StatusCode> {
    with_db(&state.db, |conn| {
        lunk_core::sync::add_sync_peer(conn, &body.id, body.name.as_deref())
    })?;
    Ok(StatusCode::CREATED)
}

async fn remove_sync_peer_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    with_db(&state.db, |conn| lunk_core::sync::remove_sync_peer(conn, &id))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn trigger_sync(
    State(state): State<AppState>,
) -> ApiResult<Json<Vec<SyncTriggerResult>>> {
    let node = state
        .sync_node
        .as_ref()
        .ok_or(ApiError(StatusCode::SERVICE_UNAVAILABLE, "sync not available".into()))?;

    let results = node.sync_all().await;

    let response: Vec<SyncTriggerResult> = results
        .into_iter()
        .map(|(peer_id, result)| match result {
            Ok(report) => SyncTriggerResult {
                peer_id,
                success: true,
                sent: Some(report.sent),
                received: Some(report.received),
                error: None,
            },
            Err(e) => SyncTriggerResult {
                peer_id,
                success: false,
                sent: None,
                received: None,
                error: Some(e),
            },
        })
        .collect();

    Ok(Json(response))
}
