use std::path::Path;

use lunk_core::config::{self, Config};
use lunk_core::db;
use lunk_core::errors::{LunkError, Result};
use lunk_core::models::*;
use lunk_core::repo;
use lunk_core::schema;
use lunk_core::search;
use lunk_core::sync;
use lunk_core::transport::SyncNode;

fn open_conn() -> Result<rusqlite::Connection> {
    let db_path = Config::db_path()?;
    db::open_database(&db_path)
}

fn open_db_wrapped() -> Result<db::Db> {
    let db_path = Config::db_path()?;
    db::open_db(&db_path)
}

pub async fn save_url(url: &str, tags: &[String]) -> Result<()> {
    // Validate URL
    let parsed = url::Url::parse(url)?;
    println!("Fetching {}...", parsed.as_str());

    // Fetch the page
    let client = reqwest::Client::builder()
        .user_agent("lunk/0.1")
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let response = client.get(url).send().await?;
    let html = response.text().await?;

    // Extract text using scraper
    let document = scraper::Html::parse_document(&html);

    // Get title
    let title_selector = scraper::Selector::parse("title").unwrap();
    let title = document
        .select(&title_selector)
        .next()
        .map(|el| el.text().collect::<String>())
        .unwrap_or_else(|| "Untitled".to_string())
        .trim()
        .to_string();

    // Extract body text (simplified: get all paragraph/article/section text)
    let text = extract_text(&document);

    if text.is_empty() {
        return Err(LunkError::Other("could not extract any text from the page".to_string()));
    }

    let mut db = open_db_wrapped()?;

    // Check for duplicates
    if let Some(existing_id) = repo::entry_exists_by_url(db.conn(), url)? {
        println!("URL already saved as entry {existing_id}");
        return Ok(());
    }

    // Get readable HTML (simplified: just the body)
    let body_selector = scraper::Selector::parse("body").unwrap();
    let readable_html = document
        .select(&body_selector)
        .next()
        .map(|el| el.inner_html())
        .map(|h| h.into_bytes());

    let req = CreateEntryRequest {
        url: Some(url.to_string()),
        title: title.clone(),
        content_type: ContentType::Article,
        extracted_text: text,
        snapshot_html: None, // CLI doesn't capture visual snapshots
        readable_html,
        pdf_data: None,
        tags: if tags.is_empty() { None } else { Some(tags.to_vec()) },
        source: SaveSource::Cli,
    };

    let entry = repo::create_entry(&mut db, req)?;
    println!("Saved: {} [{}]", entry.title, entry.id);
    if let Some(wc) = entry.word_count {
        println!("  {} words | {}", wc, entry.domain.as_deref().unwrap_or("unknown"));
    }

    Ok(())
}

pub async fn import_pdf(path: &str, title: Option<&str>, tags: &[String]) -> Result<()> {
    let file_path = Path::new(path);
    if !file_path.exists() {
        return Err(LunkError::InvalidInput(format!("file not found: {path}")));
    }

    let pdf_data = std::fs::read(file_path)?;
    println!("Reading PDF ({} bytes)...", pdf_data.len());

    // Extract text per page
    let pages = extract_pdf_pages(&pdf_data)?;

    if pages.is_empty() {
        return Err(LunkError::Other("could not extract any text from PDF".to_string()));
    }

    let full_text: String = pages.iter().map(|(_, text)| text.as_str()).collect::<Vec<_>>().join("\n\n");

    let title = title
        .map(|t| t.to_string())
        .unwrap_or_else(|| {
            file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Untitled PDF")
                .to_string()
        });

    let mut db = open_db_wrapped()?;

    let req = CreateEntryRequest {
        url: None,
        title: title.clone(),
        content_type: ContentType::Pdf,
        extracted_text: full_text,
        snapshot_html: None,
        readable_html: None,
        pdf_data: Some(pdf_data),
        tags: if tags.is_empty() { None } else { Some(tags.to_vec()) },
        source: SaveSource::Cli,
    };

    let entry = repo::create_pdf_entry(&mut db, req, pages)?;
    println!("Imported: {} [{}]", entry.title, entry.id);
    if let Some(pc) = entry.page_count {
        println!("  {} pages | {} words", pc, entry.word_count.unwrap_or(0));
    }

    Ok(())
}

pub async fn search(query: &str, limit: i64, _content_type: Option<&str>, json: bool) -> Result<()> {
    let conn = open_conn()?;
    let results = search::search(&conn, query, limit, 0)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
        return Ok(());
    }

    println!("{} result(s) for \"{}\"", results.total, query);
    println!();

    for hit in &results.entries {
        let e = &hit.entry;
        let type_badge = match e.content_type {
            ContentType::Article => "article",
            ContentType::Pdf => "pdf",
        };

        print!("  [{}] ", &e.id.to_string()[..8]);
        print!("[{type_badge}] ");
        println!("{}", e.title);

        if let Some(url) = &e.url {
            println!("    {url}");
        }
        if !e.tags.is_empty() {
            println!("    tags: {}", e.tags.join(", "));
        }
        if let Some(snippet) = &hit.snippet {
            // Strip HTML tags from snippet for terminal display
            let clean = snippet
                .replace("<mark>", "\x1b[1;33m")
                .replace("</mark>", "\x1b[0m");
            println!("    {clean}");
        }
        if let Some(page) = hit.matched_page {
            println!("    Matched on page {page}");
        }
        if e.index_status == IndexStatus::Failed {
            println!("    \x1b[33m⚠ text extraction failed\x1b[0m");
        }
        println!();
    }

    Ok(())
}

pub async fn list_entries(
    content_type: Option<&str>,
    tag: Option<&str>,
    limit: i64,
    json: bool,
) -> Result<()> {
    let conn = open_conn()?;

    let params = ListParams {
        content_type: content_type.and_then(ContentType::parse),
        tag: tag.map(|s| s.to_string()),
        limit: Some(limit),
        ..Default::default()
    };

    let (entries, total) = repo::list_entries(&conn, &params)?;

    if json {
        let result = serde_json::json!({
            "entries": entries,
            "total": total,
        });
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    println!("{total} entries");
    println!();

    for e in &entries {
        let type_badge = match e.content_type {
            ContentType::Article => "article",
            ContentType::Pdf => "pdf",
        };

        print!("  [{}] ", &e.id.to_string()[..8]);
        print!("[{type_badge}] ");
        println!("{}", e.title);

        if let Some(url) = &e.url {
            println!("    {url}");
        }
        if let Some(wc) = e.word_count {
            print!("    {wc} words");
        }
        if let Some(domain) = &e.domain {
            print!(" | {domain}");
        }
        if !e.tags.is_empty() {
            print!(" | tags: {}", e.tags.join(", "));
        }
        println!();
        if e.index_status == IndexStatus::Failed {
            println!("    \x1b[33m⚠ text extraction failed — not searchable\x1b[0m");
        } else if e.index_status == IndexStatus::Pending {
            println!("    \x1b[33m⚠ pending reindex\x1b[0m");
        }
        println!();
    }

    Ok(())
}

pub fn tag_entry(id: &str, tags: &[String], remove: bool) -> Result<()> {
    let mut db = open_db_wrapped()?;
    let uuid = uuid::Uuid::parse_str(id)
        .map_err(|e| LunkError::InvalidInput(format!("invalid id: {e}")))?;

    let entry = repo::get_entry(db.conn(), &uuid)?;
    let mut current_tags = entry.tags;

    if remove {
        current_tags.retain(|t| !tags.contains(t));
    } else {
        for tag in tags {
            if !current_tags.contains(tag) {
                current_tags.push(tag.clone());
            }
        }
    }

    let entry = repo::update_entry_tags(&mut db, &uuid, &current_tags)?;
    let short = &id[..8.min(id.len())];
    if entry.tags.is_empty() {
        println!("Entry {short}: no tags");
    } else {
        println!("Entry {short}: {}", entry.tags.join(", "));
    }
    Ok(())
}

pub async fn delete_entry(id: &str) -> Result<()> {
    let mut db = open_db_wrapped()?;
    let uuid = uuid::Uuid::parse_str(id)
        .map_err(|e| LunkError::InvalidInput(format!("invalid id: {e}")))?;

    repo::delete_entry(&mut db, &uuid)?;
    println!("Deleted entry {}", &id[..8.min(id.len())]);
    Ok(())
}

pub async fn serve(port: u16) -> Result<()> {
    let profile = config::active_profile();
    let db_path = Config::db_path()?;
    eprintln!("profile: {profile}");
    eprintln!("database: {}", db_path.display());

    let wrapped_db = db::open_db(&db_path)?;
    let pool = db::create_pool(wrapped_db);

    let embedding_model = lunk_core::embeddings::EmbeddingModel::new(None)?;
    let state = lunk_server::state::AppState { db: pool, embedding_model, sync_node: None };
    let router = lunk_server::build_router(state);

    let addr = format!("127.0.0.1:{port}");
    println!("Lunk API server listening on http://{addr}");

    let listener = tokio::net::TcpListener::bind(&addr).await
        .map_err(|e| LunkError::Other(format!("failed to bind {addr}: {e}")))?;

    axum::serve(listener, router).await
        .map_err(|e| LunkError::Other(format!("server error: {e}")))?;

    Ok(())
}

pub fn install_native_messaging(extension_id: &str, browser: &str) -> Result<()> {
    let host_dir = match browser {
        "chromium" => {
            dirs_next().join(".config/chromium/NativeMessagingHosts")
        }
        _ => {
            dirs_next().join(".config/google-chrome/NativeMessagingHosts")
        }
    };

    std::fs::create_dir_all(&host_dir)?;

    let binary_path = std::env::current_exe()
        .map_err(|e| LunkError::Other(format!("could not determine binary path: {e}")))?;

    let manifest = serde_json::json!({
        "name": "com.lunk.app",
        "description": "Lunk link archiving system",
        "path": binary_path.to_string_lossy(),
        "type": "stdio",
        "allowed_origins": [
            format!("chrome-extension://{extension_id}/")
        ]
    });

    let manifest_path = host_dir.join("com.lunk.app.json");
    std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)?;

    println!("Installed native messaging host manifest to {}", manifest_path.display());
    println!("Binary path: {}", binary_path.display());
    println!("Extension ID: {extension_id}");

    Ok(())
}

pub fn export(output: Option<&str>, with_content: bool) -> Result<()> {
    let conn = open_conn()?;

    let params = ListParams {
        limit: Some(10_000),
        ..Default::default()
    };

    let (entries, total) = repo::list_entries(&conn, &params)?;

    let export_data: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            let mut obj = serde_json::to_value(e).unwrap_or_default();
            if with_content
                && let Ok(content) = repo::get_entry_content(&conn, &e.id)
            {
                obj["extracted_text"] = serde_json::Value::String(content.extracted_text);
                if let Some(html) = content.readable_html {
                    obj["readable_html_bytes"] = serde_json::Value::Number(html.len().into());
                }
                if let Some(pdf) = content.pdf_data {
                    obj["pdf_data_bytes"] = serde_json::Value::Number(pdf.len().into());
                }
            }
            obj
        })
        .collect();

    let output_json = serde_json::json!({
        "version": "1",
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "total": total,
        "entries": export_data,
    });

    let json_str = serde_json::to_string_pretty(&output_json)?;

    if let Some(path) = output {
        std::fs::write(path, &json_str)?;
        eprintln!("Exported {total} entries to {path}");
    } else {
        println!("{json_str}");
    }

    Ok(())
}

pub fn backfill_pdfs() -> Result<()> {
    let profile = config::active_profile();
    let db_path = Config::db_path()?;
    eprintln!("profile: {profile}");
    eprintln!("database: {}", db_path.display());

    let mut db = open_db_wrapped()?;
    let count = repo::backfill_pdfs(&mut db)?;
    if count == 0 {
        println!("No PDFs need backfilling (all already have extracted text)");
    } else {
        println!("Backfilled {count} PDF(s)");
    }
    Ok(())
}

pub fn retitle() -> Result<()> {
    let db_path = Config::db_path()?;
    eprintln!("database: {}", db_path.display());

    let conn = open_conn()?;
    let (total, updated) = repo::retitle_all(&conn)?;
    println!("Retitled {updated} of {total} entries");
    Ok(())
}

pub fn rebuild_fts() -> Result<()> {
    let profile = config::active_profile();
    let db_path = Config::db_path()?;
    eprintln!("profile: {profile}");
    eprintln!("database: {}", db_path.display());

    let conn = open_conn()?;
    let count = schema::rebuild_fts(&conn)?;
    println!("Rebuilt FTS index: {count} entries indexed");
    Ok(())
}

pub fn transfer(from: &str) -> Result<()> {
    let source_path = resolve_db_path(from)?;

    if !source_path.exists() {
        return Err(LunkError::InvalidInput(format!(
            "source database not found: {}",
            source_path.display()
        )));
    }

    let dest_profile = config::active_profile();
    let dest_path = Config::db_path()?;

    if source_path == dest_path {
        return Err(LunkError::InvalidInput(
            "source and destination are the same database".to_string(),
        ));
    }

    eprintln!("source:      {} ({})", from, source_path.display());
    eprintln!("destination: {} ({})", dest_profile, dest_path.display());
    eprintln!();

    let mut db = open_db_wrapped()?;
    let (transferred, skipped) = repo::transfer_entries(&mut db, source_path.to_str().unwrap())?;

    println!("Transferred {transferred} entries, skipped {skipped} duplicates");
    Ok(())
}

/// Resolve a profile name or file path to a database path.
fn resolve_db_path(from: &str) -> Result<std::path::PathBuf> {
    // If it looks like a file path (contains / or ends with .db), use it directly
    if from.contains('/') || from.ends_with(".db") {
        return Ok(std::path::PathBuf::from(from));
    }

    // Otherwise treat as a profile name
    Config::db_path_for_profile(from)
}

pub fn migrate_status() -> Result<()> {
    let profile = config::active_profile();
    let db_path = Config::db_path()?;
    println!("profile:  {profile}");
    println!("database: {}", db_path.display());
    println!();

    let conn = open_conn()?;
    let current = schema::current_version(&conn)?;
    let target = schema::SCHEMA_VERSION;

    println!("schema version: {current} (target: {target})");

    if current < target {
        println!("  {} pending migration(s)", target - current);
    } else {
        println!("  up to date");
    }

    println!();
    println!("applied migrations:");

    let migrations = schema::applied_migrations(&conn)?;
    if migrations.is_empty() {
        println!("  (none)");
    } else {
        for (version, desc, applied_at) in &migrations {
            let at = if applied_at.is_empty() {
                "(unknown)".to_string()
            } else {
                applied_at.clone()
            };
            println!("  v{version}: {desc}");
            println!("    applied: {at}");
        }
    }

    Ok(())
}

// --- Helpers ---

fn extract_text(document: &scraper::Html) -> String {
    // Try to get text from common content selectors, falling back to body
    let selectors = [
        "article",
        "main",
        "[role=main]",
        ".post-content",
        ".entry-content",
        ".article-content",
        "#content",
        "body",
    ];

    for sel_str in &selectors {
        if let Ok(selector) = scraper::Selector::parse(sel_str) {
            let text: String = document
                .select(&selector)
                .flat_map(|el| el.text())
                .collect::<Vec<_>>()
                .join(" ");

            let cleaned = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if cleaned.len() > 100 {
                return cleaned;
            }
        }
    }

    String::new()
}

fn extract_pdf_pages(data: &[u8]) -> Result<Vec<(i32, String)>> {
    let pages = lunk_core::pdf::extract_pages(data);
    if pages.is_empty() {
        return Err(LunkError::Other("could not extract any text from PDF".to_string()));
    }
    Ok(pages)
}

fn dirs_next() -> std::path::PathBuf {
    dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."))
}

// --- Sync commands ---

pub async fn sync_trigger() -> Result<()> {
    let db_path = Config::db_path()?;
    let wrapped_db = db::open_db(&db_path)?;
    let pool = db::create_pool(wrapped_db);
    let data_dir = Config::data_dir()?;

    let node = SyncNode::new(&data_dir, pool).await?;
    println!("Node ID: {}", node.node_id_string());
    println!("Syncing with all peers...");
    println!();

    let results = node.sync_all().await;

    if results.is_empty() {
        println!("No peers configured. Use `lunk sync add <PEER_ID>` to add peers.");
    }

    for (peer, result) in &results {
        let short = &peer[..16.min(peer.len())];
        match result {
            Ok(report) => {
                println!("  {short}...: sent={} received={}", report.sent, report.received);
            }
            Err(e) => {
                println!("  {short}...: error: {e}");
            }
        }
    }

    node.shutdown().await?;
    Ok(())
}

pub fn sync_status() -> Result<()> {
    let conn = open_conn()?;

    let db_version = sync::get_db_version(&conn)?;
    let site_id = sync::get_site_id(&conn)?;
    println!("Site ID: {site_id}");
    println!("DB version: {db_version}");
    println!();

    let peers = sync::get_sync_peers(&conn)?;
    if peers.is_empty() {
        println!("No sync peers configured.");
    } else {
        println!("Peers ({}):", peers.len());
        for p in &peers {
            let short = &p.id[..16.min(p.id.len())];
            let name = p.name.as_deref().unwrap_or("");
            println!("  {short}... {name}");
            if let Some(last) = &p.last_sync_at {
                println!("    Last sync: {last}");
            }
            println!("    Peer DB version: {}", p.last_db_version);
        }
    }

    Ok(())
}

pub fn sync_add_peer(id: &str, name: Option<&str>) -> Result<()> {
    let conn = open_conn()?;
    sync::add_sync_peer(&conn, id, name)?;

    let short = &id[..16.min(id.len())];
    println!("Added peer: {short}...");
    if let Some(n) = name {
        println!("  Name: {n}");
    }
    Ok(())
}

pub fn sync_remove_peer(id: &str) -> Result<()> {
    let conn = open_conn()?;
    sync::remove_sync_peer(&conn, id)?;

    let short = &id[..16.min(id.len())];
    println!("Removed peer: {short}...");
    Ok(())
}

pub fn sync_list_peers() -> Result<()> {
    let conn = open_conn()?;
    let peers = sync::get_sync_peers(&conn)?;

    if peers.is_empty() {
        println!("No sync peers configured.");
        return Ok(());
    }

    println!("{} peer(s):", peers.len());
    println!();

    for p in &peers {
        let short = &p.id[..16.min(p.id.len())];
        let name = p.name.as_deref().unwrap_or("(unnamed)");
        println!("  {short}...  {name}");
        if let Some(last) = &p.last_sync_at {
            println!("    Last sync: {last}");
        }
        println!("    DB version: {}", p.last_db_version);
        println!();
    }

    Ok(())
}
