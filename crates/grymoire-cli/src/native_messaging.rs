use std::io::{self, Read, Write};

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};

use grymoire_core::config::Config;
use grymoire_core::db::{self, Db};
use grymoire_core::models::*;
use grymoire_core::repo;

#[derive(Deserialize)]
struct NativeMessage {
    action: String,
    data: Option<serde_json::Value>,
    #[serde(rename = "_requestId")]
    request_id: Option<serde_json::Value>,
}

#[derive(Serialize, Default)]
struct NativeResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(rename = "_requestId", skip_serializing_if = "Option::is_none")]
    request_id: Option<serde_json::Value>,
}

pub async fn run() -> grymoire_core::errors::Result<()> {
    let db_path = Config::db_path()?;
    let mut db = db::open_db(&db_path)?;

    loop {
        // Read message length (4 bytes, little-endian)
        let len = match io::stdin().lock().read_u32::<LittleEndian>() {
            Ok(len) => len as usize,
            Err(ref e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                // Chrome closed the connection
                break;
            }
            Err(e) => return Err(grymoire_core::errors::GrymoireError::Io(e)),
        };

        if len == 0 || len > 1024 * 1024 * 100 {
            // Sanity check: messages shouldn't exceed 100MB
            break;
        }

        // Read message body
        let mut buf = vec![0u8; len];
        io::stdin().lock().read_exact(&mut buf)?;

        let msg: NativeMessage = serde_json::from_slice(&buf)?;
        let request_id = msg.request_id.clone();

        // Process message
        let mut response = handle_message(&mut db, msg);
        response.request_id = request_id;

        // Write response
        let response_bytes = serde_json::to_vec(&response)?;
        let mut stdout = io::stdout().lock();
        stdout.write_u32::<LittleEndian>(response_bytes.len() as u32)?;
        stdout.write_all(&response_bytes)?;
        stdout.flush()?;
    }

    Ok(())
}

fn handle_message(db: &mut Db, msg: NativeMessage) -> NativeResponse {
    match msg.action.as_str() {
        "ping" => NativeResponse {
            success: true,
            data: Some(serde_json::json!({ "pong": true })),
            error: None,
            ..Default::default()
        },

        "save_entry" => {
            let Some(data) = msg.data else {
                return NativeResponse {
                    success: false,
                    data: None,
                    error: Some("missing data".to_string()),
                    ..Default::default()
                };
            };

            match handle_save_entry(db, data) {
                Ok(entry) => NativeResponse {
                    success: true,
                    data: Some(serde_json::to_value(entry).unwrap()),
                    ..Default::default()
                },
                Err(e) => NativeResponse {
                    success: false,
                    data: None,
                    error: Some(e.to_string()),
                    ..Default::default()
                },
            }
        }

        "get_status" => {
            let url = msg
                .data
                .as_ref()
                .and_then(|d| d.get("url"))
                .and_then(|v| v.as_str());

            let conn = db.conn();
            match url {
                Some(url) => match repo::entry_exists_by_url(conn, url) {
                    Ok(Some(id)) => {
                        match repo::get_entry(conn, &id) {
                            Ok(entry) => NativeResponse {
                                success: true,
                                data: Some(serde_json::json!({
                                    "saved": true,
                                    "entry": entry,
                                })),
                                ..Default::default()
                            },
                            Err(e) => NativeResponse {
                                success: false,
                                error: Some(e.to_string()),
                                ..Default::default()
                            },
                        }
                    }
                    Ok(None) => NativeResponse {
                        success: true,
                        data: Some(serde_json::json!({ "saved": false })),
                        ..Default::default()
                    },
                    Err(e) => NativeResponse {
                        success: false,
                        error: Some(e.to_string()),
                        ..Default::default()
                    },
                },
                None => NativeResponse {
                    success: false,
                    data: None,
                    error: Some("missing url".to_string()),
                    ..Default::default()
                },
            }
        }

        "update_tags" => {
            let id = msg
                .data
                .as_ref()
                .and_then(|d| d.get("id"))
                .and_then(|v| v.as_str());
            let tags = msg
                .data
                .as_ref()
                .and_then(|d| d.get("tags"))
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect::<Vec<_>>()
                });

            match (id, tags) {
                (Some(id), Some(tags)) => {
                    let uuid = match uuid::Uuid::parse_str(id) {
                        Ok(u) => u,
                        Err(e) => {
                            return NativeResponse {
                                success: false,
                                data: None,
                                error: Some(format!("invalid id: {e}")),
                                ..Default::default()
                            };
                        }
                    };

                    match repo::update_entry_tags(db, &uuid, &tags) {
                        Ok(entry) => NativeResponse {
                            success: true,
                            data: Some(serde_json::to_value(entry).unwrap()),
                            ..Default::default()
                        },
                        Err(e) => NativeResponse {
                            success: false,
                            error: Some(e.to_string()),
                            ..Default::default()
                        },
                    }
                }
                _ => NativeResponse {
                    success: false,
                    data: None,
                    error: Some("missing id or tags".to_string()),
                    ..Default::default()
                },
            }
        }

        "update_snapshot" => {
            use base64::Engine;
            let engine = base64::engine::general_purpose::STANDARD;

            let data = msg.data.as_ref().unwrap();
            let id_str = data.get("id").and_then(|v| v.as_str());
            let snapshot_b64 = data.get("snapshot_html").and_then(|v| v.as_str());

            match (id_str, snapshot_b64) {
                (Some(id_str), Some(snapshot_b64)) => {
                    let uuid = match uuid::Uuid::parse_str(id_str) {
                        Ok(u) => u,
                        Err(e) => {
                            return NativeResponse {
                                success: false,
                                error: Some(format!("invalid id: {e}")),
                                ..Default::default()
                            };
                        }
                    };

                    let snapshot = match engine.decode(snapshot_b64) {
                        Ok(s) => s,
                        Err(e) => {
                            return NativeResponse {
                                success: false,
                                error: Some(format!("invalid base64: {e}")),
                                ..Default::default()
                            };
                        }
                    };

                    let extracted_text = data.get("extracted_text").and_then(|v| v.as_str());
                    let readable_html = data
                        .get("readable_html")
                        .and_then(|v| v.as_str())
                        .and_then(|s| engine.decode(s).ok());

                    match repo::update_entry_content(
                        db,
                        &uuid,
                        extracted_text,
                        Some(&snapshot),
                        readable_html.as_deref(),
                    ) {
                        Ok(()) => NativeResponse {
                            success: true,
                            data: Some(serde_json::json!({ "ok": true })),
                            ..Default::default()
                        },
                        Err(e) => NativeResponse {
                            success: false,
                            error: Some(e.to_string()),
                            ..Default::default()
                        },
                    }
                }
                _ => NativeResponse {
                    success: false,
                    error: Some("missing id or snapshot_html".to_string()),
                    ..Default::default()
                },
            }
        }

        "get_tag_suggestions" => {
            let conn = db.conn();
            let domain = msg
                .data
                .as_ref()
                .and_then(|d| d.get("domain"))
                .and_then(|v| v.as_str());
            let title = msg
                .data
                .as_ref()
                .and_then(|d| d.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            match repo::get_tag_suggestions(conn, domain, title) {
                Ok(suggestions) => NativeResponse {
                    success: true,
                    data: Some(serde_json::to_value(suggestions).unwrap()),
                    ..Default::default()
                },
                Err(e) => NativeResponse {
                    success: false,
                    error: Some(e.to_string()),
                    ..Default::default()
                },
            }
        }

        _ => NativeResponse {
            success: false,
            data: None,
            error: Some(format!("unknown action: {}", msg.action)),
            ..Default::default()
        },
    }
}

fn handle_save_entry(
    db: &mut Db,
    data: serde_json::Value,
) -> grymoire_core::errors::Result<Entry> {
    use base64::Engine;
    let engine = base64::engine::general_purpose::STANDARD;

    let url = data.get("url").and_then(|v| v.as_str()).map(|s| s.to_string());
    let title = data
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Untitled")
        .to_string();
    let extracted_text = data
        .get("extracted_text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let content_type_str = data
        .get("content_type")
        .and_then(|v| v.as_str())
        .unwrap_or("article");
    let content_type = ContentType::parse(content_type_str).unwrap_or(ContentType::Article);

    let snapshot_html = data
        .get("snapshot_html")
        .and_then(|v| v.as_str())
        .and_then(|s| engine.decode(s).ok());
    let readable_html = data
        .get("readable_html")
        .and_then(|v| v.as_str())
        .and_then(|s| engine.decode(s).ok());
    let pdf_data = data
        .get("pdf_base64")
        .and_then(|v| v.as_str())
        .and_then(|s| engine.decode(s).ok());

    let tags = data
        .get("tags")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect::<Vec<_>>()
        });

    // Check for duplicate URL
    if let Some(ref url) = url
        && let Some(existing_id) = repo::entry_exists_by_url(db.conn(), url)?
    {
        return repo::get_entry(db.conn(), &existing_id);
    }

    // For PDFs, try to extract a better title from PDF metadata if the
    // extension-provided title looks generic (e.g. "pdf", "document", the bare domain)
    let title = if content_type == ContentType::Pdf && grymoire_core::pdf::is_generic_title(&title) {
        if let Some(ref data) = pdf_data {
            grymoire_core::pdf::extract_title(data).unwrap_or(title)
        } else {
            title
        }
    } else {
        title
    };

    let mut req = CreateEntryRequest {
        url,
        title,
        content_type,
        extracted_text,
        snapshot_html,
        readable_html,
        pdf_data,
        tags,
        source: SaveSource::Extension,
    };

    // For PDFs: extract text server-side (extension only sends raw bytes)
    if req.content_type == ContentType::Pdf
        && let Some(ref pdf_bytes) = req.pdf_data
    {
            let pages = grymoire_core::pdf::extract_pages(pdf_bytes);
            let full_text: String = pages
                .iter()
                .map(|(_, t)| t.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");

            if pages.is_empty() {
                tracing::warn!(
                    title = %req.title,
                    "PDF text extraction failed — entry will be saved but not searchable"
                );
            }

            if !full_text.is_empty() {
                req.extracted_text = full_text;
            }

            return repo::create_pdf_entry(db, req, pages);
    }

    repo::create_entry(db, req)
}

