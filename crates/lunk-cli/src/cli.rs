use std::path::Path;

use lunk_core::config::{self, Config};
use lunk_core::db;
use lunk_core::errors::{LunkError, Result};
use lunk_core::models::*;
use lunk_core::repo;
use lunk_core::search;
use lunk_core::sync;
use lunk_core::transport::SyncNode;

fn open_db() -> Result<rusqlite::Connection> {
    let db_path = Config::db_path()?;
    db::open_database(&db_path)
}

pub async fn save_url(url: &str, status: &str, tags: &[String]) -> Result<()> {
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

    let entry_status = EntryStatus::from_str(status)
        .ok_or_else(|| LunkError::InvalidInput(format!("invalid status: {status}")))?;

    let conn = open_db()?;

    // Check for duplicates
    if let Some(existing_id) = repo::entry_exists_by_url(&conn, url)? {
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
        status: Some(entry_status),
        tags: if tags.is_empty() { None } else { Some(tags.to_vec()) },
        source: SaveSource::Cli,
    };

    let entry = repo::create_entry(&conn, req)?;
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

    let conn = open_db()?;

    let req = CreateEntryRequest {
        url: None,
        title: title.clone(),
        content_type: ContentType::Pdf,
        extracted_text: full_text,
        snapshot_html: None,
        readable_html: None,
        pdf_data: Some(pdf_data),
        status: Some(EntryStatus::Unread),
        tags: if tags.is_empty() { None } else { Some(tags.to_vec()) },
        source: SaveSource::Cli,
    };

    let entry = repo::create_pdf_entry(&conn, req, pages)?;
    println!("Imported: {} [{}]", entry.title, entry.id);
    if let Some(pc) = entry.page_count {
        println!("  {} pages | {} words", pc, entry.word_count.unwrap_or(0));
    }

    Ok(())
}

pub async fn search(query: &str, limit: i64, _content_type: Option<&str>, json: bool) -> Result<()> {
    let conn = open_db()?;
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
        let status_badge = e.status.as_str();

        print!("  [{}] ", &e.id.to_string()[..8]);
        print!("[{type_badge}] [{status_badge}] ");
        println!("{}", e.title);

        if let Some(url) = &e.url {
            println!("    {url}");
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
        println!();
    }

    Ok(())
}

pub async fn list_entries(
    status: Option<&str>,
    content_type: Option<&str>,
    tag: Option<&str>,
    limit: i64,
    json: bool,
) -> Result<()> {
    let conn = open_db()?;

    let params = ListParams {
        status: status.and_then(EntryStatus::from_str),
        content_type: content_type.and_then(ContentType::from_str),
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
        let status_badge = e.status.as_str();

        print!("  [{}] ", &e.id.to_string()[..8]);
        print!("[{type_badge}] [{status_badge}] ");
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
        println!();
    }

    Ok(())
}

pub async fn set_status(id: &str, status: &str) -> Result<()> {
    let conn = open_db()?;
    let uuid = uuid::Uuid::parse_str(id)
        .map_err(|e| LunkError::InvalidInput(format!("invalid id: {e}")))?;
    let status = EntryStatus::from_str(status)
        .ok_or_else(|| LunkError::InvalidInput(format!("invalid status: {status}")))?;

    repo::update_entry_status(&conn, &uuid, status)?;
    println!("Updated entry {} to {}", &id[..8.min(id.len())], status.as_str());
    Ok(())
}

pub async fn delete_entry(id: &str) -> Result<()> {
    let conn = open_db()?;
    let uuid = uuid::Uuid::parse_str(id)
        .map_err(|e| LunkError::InvalidInput(format!("invalid id: {e}")))?;

    repo::delete_entry(&conn, &uuid)?;
    println!("Deleted entry {}", &id[..8.min(id.len())]);
    Ok(())
}

pub async fn serve(port: u16) -> Result<()> {
    let profile = config::active_profile();
    let db_path = Config::db_path()?;
    eprintln!("profile: {profile}");
    eprintln!("database: {}", db_path.display());

    let conn = db::open_database(&db_path)?;
    let pool = db::create_pool(conn);

    let state = lunk_server::state::AppState { db: pool, sync_node: None };
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

pub fn export(output: Option<&str>, status: Option<&str>, with_content: bool) -> Result<()> {
    let conn = open_db()?;

    let params = ListParams {
        status: status.and_then(EntryStatus::from_str),
        limit: Some(10_000),
        ..Default::default()
    };

    let (entries, total) = repo::list_entries(&conn, &params)?;

    let export_data: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            let mut obj = serde_json::to_value(e).unwrap_or_default();
            if with_content {
                if let Ok(content) = repo::get_entry_content(&conn, &e.id) {
                    obj["extracted_text"] = serde_json::Value::String(content.extracted_text);
                    if let Some(html) = content.readable_html {
                        obj["readable_html_bytes"] = serde_json::Value::Number(html.len().into());
                    }
                    if let Some(pdf) = content.pdf_data {
                        obj["pdf_data_bytes"] = serde_json::Value::Number(pdf.len().into());
                    }
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
    // Use pdf-extract for text extraction
    // For now, extract all text as a single page since pdf-extract's per-page API
    // requires more setup. We'll improve this in Phase 2.
    let text = pdf_extract::extract_text_from_mem(data)
        .map_err(|e| LunkError::Other(format!("PDF extraction failed: {e}")))?;

    if text.trim().is_empty() {
        return Ok(Vec::new());
    }

    // Split by form feed characters (common page delimiter) or treat as single page
    let pages: Vec<(i32, String)> = text
        .split('\u{0C}')
        .enumerate()
        .filter_map(|(i, page_text)| {
            let trimmed = page_text.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some((i as i32 + 1, trimmed))
            }
        })
        .collect();

    if pages.is_empty() {
        // No form feeds; treat entire text as page 1
        Ok(vec![(1, text.trim().to_string())])
    } else {
        Ok(pages)
    }
}

fn dirs_next() -> std::path::PathBuf {
    dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."))
}

// --- Sync commands ---

fn open_db_with_crsqlite() -> Result<rusqlite::Connection> {
    let config = Config::load()?;
    let db_path = Config::db_path()?;
    let conn = db::open_database(&db_path)?;

    let loaded = db::try_load_crsqlite(&conn, config.sync.crsqlite_ext_path.as_deref());
    if loaded {
        db::register_crrs(&conn)?;
    }

    Ok(conn)
}

pub async fn sync_trigger() -> Result<()> {
    let config = Config::load()?;
    let db_path = Config::db_path()?;
    let conn = db::open_database(&db_path)?;

    let loaded = db::try_load_crsqlite(&conn, config.sync.crsqlite_ext_path.as_deref());
    if !loaded {
        return Err(LunkError::Config(
            "cr-sqlite extension not found; sync requires cr-sqlite".into(),
        ));
    }
    db::register_crrs(&conn)?;

    let pool = db::create_pool(conn);
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
    let conn = open_db_with_crsqlite()?;

    let crsqlite_available = db::is_crsqlite_loaded(&conn);
    println!("cr-sqlite: {}", if crsqlite_available { "loaded" } else { "not available" });

    if crsqlite_available {
        let db_version = sync::get_db_version(&conn)?;
        let site_id = sync::get_site_id(&conn)?;
        println!("Site ID: {site_id}");
        println!("DB version: {db_version}");
    }

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
    let conn = open_db()?;
    sync::add_sync_peer(&conn, id, name)?;

    let short = &id[..16.min(id.len())];
    println!("Added peer: {short}...");
    if let Some(n) = name {
        println!("  Name: {n}");
    }
    Ok(())
}

pub fn sync_remove_peer(id: &str) -> Result<()> {
    let conn = open_db()?;
    sync::remove_sync_peer(&conn, id)?;

    let short = &id[..16.min(id.len())];
    println!("Removed peer: {short}...");
    Ok(())
}

pub fn sync_list_peers() -> Result<()> {
    let conn = open_db()?;
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
