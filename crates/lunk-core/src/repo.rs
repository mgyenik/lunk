use chrono::Utc;
use rusqlite::types::Value;
use rusqlite::{params, Connection};
use url::Url;
use uuid::Uuid;

use crate::db::Db;
use crate::errors::{LunkError, Result};
use crate::models::*;
use crate::search::sanitize_fts_query;

pub fn create_entry(db: &mut Db, req: CreateEntryRequest) -> Result<Entry> {
    let (ts, ver) = db.next_timestamp();
    let conn = db.conn();
    let id = Uuid::now_v7();
    let now = Utc::now();

    let domain = req.url.as_ref().and_then(|u| {
        Url::parse(u)
            .ok()
            .and_then(|parsed| parsed.host_str().map(|h| h.to_string()))
    });

    let word_count = Some(req.extracted_text.split_whitespace().count() as i64);
    let index_status = if req.extracted_text.is_empty() {
        IndexStatus::Failed
    } else {
        IndexStatus::Ok
    };

    conn.execute(
        "INSERT INTO entries (id, url, title, content_type, status, domain, word_count, page_count, index_status, index_version, created_at, updated_at, saved_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            id.to_string(),
            req.url,
            req.title,
            req.content_type.as_str(),
            "read",
            domain,
            word_count,
            Option::<i64>::None,
            index_status.as_str(),
            crate::pdf::INDEX_VERSION,
            now.to_rfc3339(),
            now.to_rfc3339(),
            req.source.as_str(),
        ],
    )?;

    conn.execute(
        "INSERT INTO entry_content (entry_id, extracted_text, snapshot_html, readable_html, pdf_data)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            id.to_string(),
            req.extracted_text,
            req.snapshot_html,
            req.readable_html,
            req.pdf_data,
        ],
    )?;

    // Handle tags
    let tags = req.tags.unwrap_or_default();
    for tag_name in &tags {
        ensure_tag(conn, tag_name)?;
        let tag_id: String = conn.query_row(
            "SELECT id FROM tags WHERE name = ?1",
            params![tag_name],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO entry_tags (entry_id, tag_id) VALUES (?1, ?2)",
            params![id.to_string(), tag_id],
        )?;
    }

    crate::change_tracking::fixup_trigger_rows(conn, &ts, ver)?;

    Ok(Entry {
        id,
        url: req.url,
        title: req.title,
        content_type: req.content_type,
        domain,
        word_count,
        page_count: None,
        index_status,
        index_version: crate::pdf::INDEX_VERSION,
        created_at: now,
        updated_at: now,
        saved_by: req.source.as_str().to_string(),
        tags,
    })
}

pub fn create_pdf_entry(
    db: &mut Db,
    req: CreateEntryRequest,
    pages: Vec<(i32, String)>,
) -> Result<Entry> {
    let (ts, ver) = db.next_timestamp();
    let conn = db.conn();
    let id = Uuid::now_v7();
    let now = Utc::now();

    let domain = req.url.as_ref().and_then(|u| {
        Url::parse(u)
            .ok()
            .and_then(|parsed| parsed.host_str().map(|h| h.to_string()))
    });

    let word_count = Some(req.extracted_text.split_whitespace().count() as i64);
    let page_count = Some(pages.len() as i64);
    let index_status = if pages.is_empty() && req.extracted_text.is_empty() {
        IndexStatus::Failed
    } else {
        IndexStatus::Ok
    };

    conn.execute(
        "INSERT INTO entries (id, url, title, content_type, status, domain, word_count, page_count, index_status, index_version, created_at, updated_at, saved_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            id.to_string(),
            req.url,
            req.title,
            ContentType::Pdf.as_str(),
            "read",
            domain,
            word_count,
            page_count,
            index_status.as_str(),
            crate::pdf::INDEX_VERSION,
            now.to_rfc3339(),
            now.to_rfc3339(),
            req.source.as_str(),
        ],
    )?;

    conn.execute(
        "INSERT INTO entry_content (entry_id, extracted_text, snapshot_html, readable_html, pdf_data)
         VALUES (?1, ?2, NULL, NULL, ?3)",
        params![id.to_string(), req.extracted_text, req.pdf_data,],
    )?;

    // Insert per-page text
    for (page_num, text) in &pages {
        let page_id = Uuid::now_v7();
        conn.execute(
            "INSERT INTO pdf_pages (id, entry_id, page_num, text) VALUES (?1, ?2, ?3, ?4)",
            params![page_id.to_string(), id.to_string(), page_num, text],
        )?;
    }

    // Handle tags
    let tags = req.tags.unwrap_or_default();
    for tag_name in &tags {
        ensure_tag(conn, tag_name)?;
        let tag_id: String = conn.query_row(
            "SELECT id FROM tags WHERE name = ?1",
            params![tag_name],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO entry_tags (entry_id, tag_id) VALUES (?1, ?2)",
            params![id.to_string(), tag_id],
        )?;
    }

    crate::change_tracking::fixup_trigger_rows(conn, &ts, ver)?;

    Ok(Entry {
        id,
        url: req.url,
        title: req.title,
        content_type: ContentType::Pdf,
        domain,
        word_count,
        page_count,
        index_status,
        index_version: crate::pdf::INDEX_VERSION,
        created_at: now,
        updated_at: now,
        saved_by: req.source.as_str().to_string(),
        tags,
    })
}

pub fn get_entry(conn: &Connection, id: &Uuid) -> Result<Entry> {
    let mut stmt = conn.prepare(
        "SELECT e.id, e.url, e.title, e.content_type, e.domain,
                e.word_count, e.page_count, e.index_status, e.index_version,
                e.created_at, e.updated_at, e.saved_by
         FROM entries e WHERE e.id = ?1",
    )?;

    let entry = stmt.query_row(params![id.to_string()], |row| {
        Ok(EntryRow {
            id: row.get(0)?,
            url: row.get(1)?,
            title: row.get(2)?,
            content_type: row.get(3)?,
            domain: row.get(4)?,
            word_count: row.get(5)?,
            page_count: row.get(6)?,
            index_status: row.get::<_, Option<String>>(7)?.unwrap_or_else(|| "ok".to_string()),
            index_version: row.get::<_, Option<i32>>(8)?.unwrap_or(0),
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
            saved_by: row.get(11)?,
        })
    }).map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => LunkError::NotFound(format!("entry {id}")),
        other => LunkError::Database(other),
    })?;

    let tags = get_entry_tags(conn, &entry.id)?;
    row_to_entry(entry, tags)
}

pub fn get_entry_content(conn: &Connection, id: &Uuid) -> Result<EntryContent> {
    let mut stmt = conn.prepare(
        "SELECT entry_id, extracted_text, snapshot_html, readable_html, pdf_data
         FROM entry_content WHERE entry_id = ?1",
    )?;

    stmt.query_row(params![id.to_string()], |row| {
        Ok(EntryContent {
            entry_id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
            extracted_text: row.get(1)?,
            snapshot_html: row.get(2)?,
            readable_html: row.get(3)?,
            pdf_data: row.get(4)?,
        })
    })
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => LunkError::NotFound(format!("content for entry {id}")),
        other => LunkError::Database(other),
    })
}

pub fn list_entries(conn: &Connection, params: &ListParams) -> Result<(Vec<Entry>, i64)> {
    let mut conditions = Vec::new();
    let mut bind_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ct) = &params.content_type {
        bind_values.push(Box::new(ct.as_str().to_string()));
        conditions.push(format!("e.content_type = ?{}", bind_values.len()));
    }
    if let Some(domain) = &params.domain {
        bind_values.push(Box::new(domain.clone()));
        conditions.push(format!("e.domain = ?{}", bind_values.len()));
    }
    if let Some(tag) = &params.tag {
        bind_values.push(Box::new(tag.clone()));
        conditions.push(format!(
            "EXISTS (SELECT 1 FROM entry_tags et JOIN tags t ON et.tag_id = t.id WHERE et.entry_id = e.id AND t.name = ?{})",
            bind_values.len()
        ));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    let sort_col = match params.sort.as_deref() {
        Some("title") => "e.title",
        Some("updated_at") => "e.updated_at",
        _ => "e.created_at",
    };
    let sort_dir = match params.order.as_deref() {
        Some("asc") => "ASC",
        _ => "DESC",
    };

    let limit = params.limit.unwrap_or(50).min(200);
    let offset = params.offset.unwrap_or(0);

    // Count query
    let count_sql = format!("SELECT COUNT(*) FROM entries e {where_clause}");
    let total: i64 = {
        let mut stmt = conn.prepare(&count_sql)?;
        let refs: Vec<&dyn rusqlite::types::ToSql> = bind_values.iter().map(|b| b.as_ref()).collect();
        stmt.query_row(refs.as_slice(), |row| row.get(0))?
    };

    // Main query
    let sql = format!(
        "SELECT e.id, e.url, e.title, e.content_type, e.domain,
                e.word_count, e.page_count, e.index_status, e.index_version,
                e.created_at, e.updated_at, e.saved_by
         FROM entries e {where_clause}
         ORDER BY {sort_col} {sort_dir}
         LIMIT ?{} OFFSET ?{}",
        bind_values.len() + 1,
        bind_values.len() + 2,
    );

    bind_values.push(Box::new(limit));
    bind_values.push(Box::new(offset));

    let mut stmt = conn.prepare(&sql)?;
    let refs: Vec<&dyn rusqlite::types::ToSql> = bind_values.iter().map(|b| b.as_ref()).collect();

    let rows = stmt.query_map(refs.as_slice(), |row| {
        Ok(EntryRow {
            id: row.get(0)?,
            url: row.get(1)?,
            title: row.get(2)?,
            content_type: row.get(3)?,
            domain: row.get(4)?,
            word_count: row.get(5)?,
            page_count: row.get(6)?,
            index_status: row.get::<_, Option<String>>(7)?.unwrap_or_else(|| "ok".to_string()),
            index_version: row.get::<_, Option<i32>>(8)?.unwrap_or(0),
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
            saved_by: row.get(11)?,
        })
    })?;

    let mut entries = Vec::new();
    for row in rows {
        let row = row?;
        let tags = get_entry_tags(conn, &row.id)?;
        entries.push(row_to_entry(row, tags)?);
    }

    Ok((entries, total))
}

pub fn update_entry(
    db: &mut Db,
    id: &Uuid,
    title: Option<&str>,
    tags: Option<&[String]>,
) -> Result<Entry> {
    let (ts, ver) = db.next_timestamp();
    let conn = db.conn();
    let now = Utc::now();
    let id_str = id.to_string();
    let now_str = now.to_rfc3339();

    let mut changes = Vec::new();

    if let Some(title) = title {
        conn.execute(
            "UPDATE entries SET title = ?1, updated_at = ?2 WHERE id = ?3",
            params![title, &now_str, &id_str],
        )?;
        changes.push(("title", Value::Text(title.to_string())));
    }

    if let Some(tags) = tags {
        set_entry_tags(conn, id, tags)?;
    }

    // Always bump updated_at
    conn.execute(
        "UPDATE entries SET updated_at = ?1 WHERE id = ?2",
        params![&now_str, &id_str],
    )?;
    changes.push(("updated_at", Value::Text(now_str)));

    crate::change_tracking::fixup_trigger_rows(conn, &ts, ver)?;
    crate::change_tracking::log_column_changes(conn, &ts, ver, "entries", &id_str, &changes)?;

    get_entry(conn, id)
}

/// Replace all tags on an entry.
pub fn update_entry_tags(db: &mut Db, id: &Uuid, tags: &[String]) -> Result<Entry> {
    let (ts, ver) = db.next_timestamp();
    let conn = db.conn();
    let now = Utc::now();
    let id_str = id.to_string();
    let now_str = now.to_rfc3339();

    set_entry_tags(conn, id, tags)?;
    conn.execute(
        "UPDATE entries SET updated_at = ?1 WHERE id = ?2",
        params![&now_str, &id_str],
    )?;

    crate::change_tracking::fixup_trigger_rows(conn, &ts, ver)?;
    crate::change_tracking::log_column_changes(
        conn,
        &ts,
        ver,
        "entries",
        &id_str,
        &[("updated_at", Value::Text(now_str))],
    )?;

    get_entry(conn, id)
}

/// Update the content blobs for an entry (text, snapshot, readable html).
/// Only non-None fields are updated.
pub fn update_entry_content(
    db: &mut Db,
    id: &Uuid,
    extracted_text: Option<&str>,
    snapshot_html: Option<&[u8]>,
    readable_html: Option<&[u8]>,
) -> Result<()> {
    let (ts, ver) = db.next_timestamp();
    let conn = db.conn();
    let now = Utc::now();
    let id_str = id.to_string();
    let now_str = now.to_rfc3339();

    let mut content_changes = Vec::new();
    let mut entry_changes = Vec::new();

    if let Some(text) = extracted_text {
        conn.execute(
            "UPDATE entry_content SET extracted_text = ?1 WHERE entry_id = ?2",
            params![text, &id_str],
        )?;
        content_changes.push(("extracted_text", Value::Text(text.to_string())));

        let wc = text.split_whitespace().count() as i64;
        conn.execute(
            "UPDATE entries SET word_count = ?1 WHERE id = ?2",
            params![wc, &id_str],
        )?;
        entry_changes.push(("word_count", Value::Integer(wc)));
    }
    if let Some(html) = snapshot_html {
        conn.execute(
            "UPDATE entry_content SET snapshot_html = ?1 WHERE entry_id = ?2",
            params![html, &id_str],
        )?;
        content_changes.push(("snapshot_html", Value::Blob(html.to_vec())));
    }
    if let Some(html) = readable_html {
        conn.execute(
            "UPDATE entry_content SET readable_html = ?1 WHERE entry_id = ?2",
            params![html, &id_str],
        )?;
        content_changes.push(("readable_html", Value::Blob(html.to_vec())));
    }

    conn.execute(
        "UPDATE entries SET updated_at = ?1 WHERE id = ?2",
        params![&now_str, &id_str],
    )?;
    entry_changes.push(("updated_at", Value::Text(now_str)));

    crate::change_tracking::log_column_changes(conn, &ts, ver, "entry_content", &id_str, &content_changes)?;
    crate::change_tracking::log_column_changes(conn, &ts, ver, "entries", &id_str, &entry_changes)?;

    crate::search::rebuild_fts_for_entry(conn, id)?;

    Ok(())
}

pub fn delete_entry(db: &mut Db, id: &Uuid) -> Result<()> {
    let (ts, ver) = db.next_timestamp();
    let conn = db.conn();
    let id_str = id.to_string();

    // entry_content, pdf_pages, entry_tags cleaned up by ON DELETE CASCADE
    let changed = conn.execute(
        "DELETE FROM entries WHERE id = ?1",
        params![&id_str],
    )?;

    if changed == 0 {
        return Err(LunkError::NotFound(format!("entry {id}")));
    }

    crate::change_tracking::fixup_trigger_rows(conn, &ts, ver)?;
    crate::change_tracking::record_tombstone(conn, &ts, ver, "entries", &id_str)?;

    Ok(())
}

pub fn get_tags(conn: &Connection) -> Result<Vec<TagWithCount>> {
    let mut stmt = conn.prepare(
        "SELECT t.name, COUNT(et.entry_id) as cnt
         FROM tags t
         LEFT JOIN entry_tags et ON t.id = et.tag_id
         GROUP BY t.id
         ORDER BY cnt DESC, t.name ASC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(TagWithCount {
            name: row.get(0)?,
            count: row.get(1)?,
        })
    })?;

    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.into())
}

pub fn entry_exists_by_url(conn: &Connection, url: &str) -> Result<Option<Uuid>> {
    let mut stmt = conn.prepare("SELECT id FROM entries WHERE url = ?1")?;
    match stmt.query_row(params![url], |row| row.get::<_, String>(0)) {
        Ok(id_str) => Ok(Some(Uuid::parse_str(&id_str).map_err(|e| {
            LunkError::Other(format!("invalid uuid in db: {e}"))
        })?)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Re-extract text from stored PDF blobs for entries that need reprocessing.
/// Targets PDFs with no extracted text, failed index status, or old index version.
/// Updates extracted_text, word_count, page_count, title (if empty), index_status,
/// index_version, and pdf_pages. Returns the number of entries backfilled.
pub fn backfill_pdfs(db: &mut Db) -> Result<usize> {
    let current_version = crate::pdf::INDEX_VERSION;

    // Phase 1: Read candidates (immutable borrow, then release)
    let candidates: Vec<(String, Option<String>, String, Vec<u8>)> = {
        let conn = db.conn();
        let mut stmt = conn.prepare(
            "SELECT e.id, e.url, e.title, ec.pdf_data
             FROM entries e
             JOIN entry_content ec ON ec.entry_id = e.id
             WHERE e.content_type = 'pdf'
               AND ec.pdf_data IS NOT NULL
               AND (
                   ec.extracted_text IS NULL
                   OR ec.extracted_text = ''
                   OR COALESCE(e.index_status, 'ok') IN ('failed', 'pending')
                   OR COALESCE(e.index_version, 0) < ?1
               )",
        )?;

        stmt.query_map(params![current_version], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Vec<u8>>(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?
    };

    if candidates.is_empty() {
        return Ok(0);
    }

    tracing::info!("backfilling {} PDF entries", candidates.len());
    let mut count = 0;

    // Phase 2: Process each candidate (mutable borrow per iteration)
    for (id, url, title, pdf_data) in &candidates {
        let pages = crate::pdf::extract_pages(pdf_data);
        if pages.is_empty() {
            tracing::warn!("PDF {} produced no text", &id[..8]);
            continue;
        }

        let full_text: String = pages
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let word_count = full_text.split_whitespace().count() as i64;
        let page_count = pages.len() as i64;
        let now_str = Utc::now().to_rfc3339();

        let new_title = if title.is_empty() {
            url.as_deref()
                .and_then(|u| url::Url::parse(u).ok())
                .and_then(|u| {
                    u.path_segments()?.next_back().map(|s| s.to_string())
                })
                .and_then(|f| if f.is_empty() { None } else { Some(f) })
                .unwrap_or_else(|| "Untitled PDF".to_string())
        } else {
            title.clone()
        };

        let (ts, ver) = db.next_timestamp();
        let conn = db.conn();

        conn.execute(
            "UPDATE entry_content SET extracted_text = ?1 WHERE entry_id = ?2",
            params![&full_text, id],
        )?;

        conn.execute(
            "UPDATE entries SET word_count = ?1, page_count = ?2, title = ?3, \
             index_status = ?4, index_version = ?5, updated_at = ?6 WHERE id = ?7",
            params![
                word_count,
                page_count,
                &new_title,
                IndexStatus::Ok.as_str(),
                crate::pdf::INDEX_VERSION,
                &now_str,
                id
            ],
        )?;

        conn.execute("DELETE FROM pdf_pages WHERE entry_id = ?1", params![id])?;
        for (page_num, page_text) in &pages {
            let page_id = Uuid::now_v7();
            conn.execute(
                "INSERT INTO pdf_pages (id, entry_id, page_num, text) VALUES (?1, ?2, ?3, ?4)",
                params![page_id.to_string(), id, page_num, page_text],
            )?;
        }

        crate::change_tracking::fixup_trigger_rows(conn, &ts, ver)?;
        crate::change_tracking::log_column_changes(
            conn, &ts, ver, "entry_content", id,
            &[("extracted_text", Value::Text(full_text))],
        )?;
        crate::change_tracking::log_column_changes(
            conn, &ts, ver, "entries", id,
            &[
                ("word_count", Value::Integer(word_count)),
                ("page_count", Value::Integer(page_count)),
                ("title", Value::Text(new_title.clone())),
                ("index_status", Value::Text(IndexStatus::Ok.as_str().to_string())),
                ("index_version", Value::Integer(crate::pdf::INDEX_VERSION as i64)),
                ("updated_at", Value::Text(now_str)),
            ],
        )?;

        tracing::info!(
            "backfilled {}: \"{}\" ({} pages, {} words)",
            &id[..8],
            new_title,
            page_count,
            word_count
        );
        count += 1;
    }

    if count > 0 {
        crate::schema::rebuild_fts(db.conn())?;
    }

    Ok(count)
}

// --- Internal helpers ---

struct EntryRow {
    id: String,
    url: Option<String>,
    title: String,
    content_type: String,
    domain: Option<String>,
    word_count: Option<i64>,
    page_count: Option<i64>,
    index_status: String,
    index_version: i32,
    created_at: String,
    updated_at: String,
    saved_by: String,
}

fn row_to_entry(row: EntryRow, tags: Vec<String>) -> Result<Entry> {
    let id = Uuid::parse_str(&row.id)
        .map_err(|e| LunkError::Other(format!("invalid uuid: {e}")))?;
    let content_type = ContentType::parse(&row.content_type)
        .ok_or_else(|| LunkError::Other(format!("invalid content_type: {}", row.content_type)))?;
    let index_status = IndexStatus::parse(&row.index_status)
        .unwrap_or(IndexStatus::Ok);
    let created_at = chrono::DateTime::parse_from_rfc3339(&row.created_at)
        .map_err(|e| LunkError::Other(format!("invalid created_at: {e}")))?
        .with_timezone(&chrono::Utc);
    let updated_at = chrono::DateTime::parse_from_rfc3339(&row.updated_at)
        .map_err(|e| LunkError::Other(format!("invalid updated_at: {e}")))?
        .with_timezone(&chrono::Utc);

    Ok(Entry {
        id,
        url: row.url,
        title: row.title,
        content_type,
        domain: row.domain,
        word_count: row.word_count,
        page_count: row.page_count,
        index_status,
        index_version: row.index_version,
        created_at,
        updated_at,
        saved_by: row.saved_by,
        tags,
    })
}

fn get_entry_tags(conn: &Connection, entry_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT t.name FROM tags t
         JOIN entry_tags et ON t.id = et.tag_id
         WHERE et.entry_id = ?1
         ORDER BY t.name",
    )?;

    let rows = stmt.query_map(params![entry_id], |row| row.get::<_, String>(0))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| e.into())
}

fn ensure_tag(conn: &Connection, name: &str) -> Result<()> {
    let id = Uuid::now_v7();
    conn.execute(
        "INSERT OR IGNORE INTO tags (id, name) VALUES (?1, ?2)",
        params![id.to_string(), name],
    )?;
    Ok(())
}

fn set_entry_tags(conn: &Connection, id: &Uuid, tags: &[String]) -> Result<()> {
    conn.execute(
        "DELETE FROM entry_tags WHERE entry_id = ?1",
        params![id.to_string()],
    )?;
    for tag_name in tags {
        ensure_tag(conn, tag_name)?;
        let tag_id: String = conn.query_row(
            "SELECT id FROM tags WHERE name = ?1",
            params![tag_name],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO entry_tags (entry_id, tag_id) VALUES (?1, ?2)",
            params![id.to_string(), tag_id],
        )?;
    }
    Ok(())
}

// --- Cross-database transfer ---

/// Transfer entries from a source database into this connection's database.
/// Skips entries that already exist (by URL match or ID match).
/// Returns (transferred, skipped) counts.
pub fn transfer_entries(db: &mut Db, source_db_path: &str) -> Result<(usize, usize)> {
    let (ts, ver) = db.next_timestamp();
    let conn = db.conn();
    conn.execute(
        "ATTACH DATABASE ?1 AS src",
        params![source_db_path],
    )?;

    let result = transfer_entries_inner(conn);

    let _ = conn.execute("DETACH DATABASE src", []);

    let counts = result?;

    crate::change_tracking::fixup_trigger_rows(conn, &ts, ver)?;

    Ok(counts)
}

fn transfer_entries_inner(conn: &Connection) -> Result<(usize, usize)> {
    // Get all entry IDs from source
    let mut stmt = conn.prepare(
        "SELECT id, url FROM src.entries"
    )?;

    let source_entries: Vec<(String, Option<String>)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut transferred = 0;
    let mut skipped = 0;

    for (src_id, src_url) in &source_entries {
        // Skip if ID already exists in destination
        let id_exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM entries WHERE id = ?1)",
            params![src_id],
            |row| row.get(0),
        )?;
        if id_exists {
            skipped += 1;
            continue;
        }

        // Skip if URL already exists in destination
        if let Some(url) = src_url {
            let url_exists: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM entries WHERE url = ?1)",
                params![url],
                |row| row.get(0),
            )?;
            if url_exists {
                skipped += 1;
                continue;
            }
        }

        // Copy the entry row
        conn.execute(
            "INSERT INTO entries (id, url, title, content_type, status, domain, word_count, page_count,
                                  index_status, index_version, created_at, updated_at, saved_by)
             SELECT id, url, title, content_type, status, domain, word_count, page_count,
                    index_status, index_version, created_at, updated_at, saved_by
             FROM src.entries WHERE id = ?1",
            params![src_id],
        )?;

        // Copy entry_content
        conn.execute(
            "INSERT INTO entry_content (entry_id, extracted_text, snapshot_html, readable_html, pdf_data)
             SELECT entry_id, extracted_text, snapshot_html, readable_html, pdf_data
             FROM src.entry_content WHERE entry_id = ?1",
            params![src_id],
        )?;

        // Copy pdf_pages
        conn.execute(
            "INSERT INTO pdf_pages (id, entry_id, page_num, text)
             SELECT id, entry_id, page_num, text
             FROM src.pdf_pages WHERE entry_id = ?1",
            params![src_id],
        )?;

        // Copy tags: ensure each tag exists in destination, then link
        let mut tag_stmt = conn.prepare(
            "SELECT t.name FROM src.tags t
             JOIN src.entry_tags et ON t.id = et.tag_id
             WHERE et.entry_id = ?1"
        )?;
        let tags: Vec<String> = tag_stmt
            .query_map(params![src_id], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        for tag_name in &tags {
            ensure_tag(conn, tag_name)?;
            let tag_id: String = conn.query_row(
                "SELECT id FROM tags WHERE name = ?1",
                params![tag_name],
                |row| row.get(0),
            )?;
            conn.execute(
                "INSERT OR IGNORE INTO entry_tags (entry_id, tag_id) VALUES (?1, ?2)",
                params![src_id, tag_id],
            )?;
        }

        transferred += 1;
    }

    // Rebuild FTS for transferred entries
    if transferred > 0 {
        crate::schema::rebuild_fts(conn)?;
    }

    Ok((transferred, skipped))
}

// --- Tag suggestion queries ---

/// Suggest tags commonly used with entries from the same domain.
pub fn suggest_tags_by_domain(conn: &Connection, domain: &str, limit: usize) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT t.name, COUNT(*) as cnt
         FROM tags t
         JOIN entry_tags et ON t.id = et.tag_id
         JOIN entries e ON et.entry_id = e.id
         WHERE e.domain = ?1
         GROUP BY t.name
         ORDER BY cnt DESC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![domain, limit as i64], |row| row.get::<_, String>(0))?;
    rows.collect::<std::result::Result<Vec<_>, _>>().map_err(|e| e.into())
}

/// Suggest tags from entries with similar titles (via FTS5).
pub fn suggest_tags_by_similarity(conn: &Connection, title: &str, limit: usize) -> Result<Vec<String>> {
    let fts_query = sanitize_fts_query(title);
    if fts_query.is_empty() {
        return Ok(Vec::new());
    }

    let mut stmt = conn.prepare(
        "SELECT t.name, COUNT(*) as cnt
         FROM entries_fts
         JOIN entries e ON e.rowid = entries_fts.rowid
         JOIN entry_tags et ON et.entry_id = e.id
         JOIN tags t ON t.id = et.tag_id
         WHERE entries_fts MATCH ?1
         GROUP BY t.name
         ORDER BY cnt DESC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![fts_query, limit as i64], |row| row.get::<_, String>(0))?;
    rows.collect::<std::result::Result<Vec<_>, _>>().map_err(|e| e.into())
}

/// Return the most frequently used tags.
pub fn suggest_tags_popular(conn: &Connection, limit: usize) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT t.name
         FROM tags t
         JOIN entry_tags et ON t.id = et.tag_id
         GROUP BY t.id
         ORDER BY COUNT(et.entry_id) DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], |row| row.get::<_, String>(0))?;
    rows.collect::<std::result::Result<Vec<_>, _>>().map_err(|e| e.into())
}

/// Combined tag suggestions from domain, content similarity, and popularity.
/// Results are deduplicated: domain > similar > popular priority.
pub fn get_tag_suggestions(conn: &Connection, domain: Option<&str>, title: &str) -> Result<TagSuggestions> {
    let domain_tags = if let Some(d) = domain {
        suggest_tags_by_domain(conn, d, 5)?
    } else {
        Vec::new()
    };

    let all_similar = suggest_tags_by_similarity(conn, title, 10)?;
    let similar_tags: Vec<String> = all_similar
        .into_iter()
        .filter(|t| !domain_tags.contains(t))
        .take(5)
        .collect();

    let all_popular = suggest_tags_popular(conn, 15)?;
    let popular_tags: Vec<String> = all_popular
        .into_iter()
        .filter(|t| !domain_tags.contains(t) && !similar_tags.contains(t))
        .take(5)
        .collect();

    Ok(TagSuggestions {
        domain_tags,
        similar_tags,
        popular_tags,
    })
}

// --- Retitling ---

/// Re-generate titles for all entries using the current title extraction logic.
/// Articles: readable HTML headings → text heuristics.
/// PDFs: font-size extraction → metadata title → text heuristics.
/// Returns (total, updated) counts.
#[allow(clippy::type_complexity)]
pub fn retitle_all(conn: &Connection) -> Result<(usize, usize)> {
    use crate::titles;

    let mut stmt = conn.prepare(
        "SELECT e.id, e.title, e.content_type, ec.extracted_text, ec.readable_html, ec.pdf_data
         FROM entries e
         JOIN entry_content ec ON ec.entry_id = e.id",
    )?;

    let rows: Vec<(String, String, String, String, Option<Vec<u8>>, Option<Vec<u8>>)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<Vec<u8>>>(4)?,
                row.get::<_, Option<Vec<u8>>>(5)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut total = 0;
    let mut updated = 0;

    for (id, old_title, content_type, extracted_text, readable_html, pdf_data) in &rows {
        total += 1;

        let new_title = if content_type == "article" {
            readable_html
                .as_ref()
                .and_then(|html| titles::title_from_readable_html(html))
                .or_else(|| titles::title_from_text(extracted_text))
        } else {
            pdf_data
                .as_ref()
                .and_then(|data| crate::pdf::extract_title(data))
                .or_else(|| titles::title_from_text(extracted_text))
        };

        let new_title = new_title.map(|t| titles::clean_title(&t));

        if let Some(ref title) = new_title
            && title != old_title
        {
            conn.execute(
                "UPDATE entries SET title = ?1, updated_at = ?2 WHERE id = ?3",
                params![title, Utc::now().to_rfc3339(), id],
            )?;
            tracing::info!(id, old = old_title, new = title, "retitled");
            updated += 1;
        }
    }

    Ok((total, updated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, Db};

    fn test_db() -> Db {
        db::open_in_memory_db().unwrap()
    }

    fn test_req(url: &str, title: &str, text: &str) -> CreateEntryRequest {
        CreateEntryRequest {
            url: Some(url.to_string()),
            title: title.to_string(),
            content_type: ContentType::Article,
            extracted_text: text.to_string(),
            snapshot_html: None,
            readable_html: None,
            pdf_data: None,
            tags: None,
            source: SaveSource::Cli,
        }
    }

    #[test]
    fn test_create_and_get_entry() {
        let mut db = test_db();
        let mut req = test_req(
            "https://example.com/article",
            "Test Article",
            "This is the full text of the article about Rust programming.",
        );
        req.snapshot_html = Some(b"<html>full snapshot</html>".to_vec());
        req.readable_html = Some(b"<article>clean text</article>".to_vec());
        req.tags = Some(vec!["rust".to_string(), "programming".to_string()]);

        let entry = create_entry(&mut db, req).unwrap();
        assert_eq!(entry.title, "Test Article");
        assert_eq!(entry.domain, Some("example.com".to_string()));
        assert_eq!(entry.tags.len(), 2);

        let fetched = get_entry(db.conn(), &entry.id).unwrap();
        assert_eq!(fetched.title, "Test Article");
        assert_eq!(fetched.tags.len(), 2);
    }

    #[test]
    fn test_update_tags() {
        let mut db = test_db();
        let entry = create_entry(&mut db, test_req("https://example.com", "Test", "text")).unwrap();
        assert!(entry.tags.is_empty());

        let updated = update_entry_tags(&mut db, &entry.id, &["rust".to_string(), "web".to_string()]).unwrap();
        assert_eq!(updated.tags.len(), 2);

        let updated = update_entry_tags(&mut db, &entry.id, &["rust".to_string()]).unwrap();
        assert_eq!(updated.tags, vec!["rust"]);
    }

    #[test]
    fn test_list_entries() {
        let mut db = test_db();

        for i in 0..5 {
            let mut req = test_req(
                &format!("https://example.com/{i}"),
                &format!("Article {i}"),
                &format!("content {i}"),
            );
            if i < 3 {
                req.tags = Some(vec!["batch-a".to_string()]);
            }
            create_entry(&mut db, req).unwrap();
        }

        let (all, total) = list_entries(db.conn(), &ListParams::default()).unwrap();
        assert_eq!(total, 5);
        assert_eq!(all.len(), 5);

        let (tagged, total) = list_entries(db.conn(), &ListParams {
            tag: Some("batch-a".to_string()),
            ..Default::default()
        }).unwrap();
        assert_eq!(total, 3);
        assert_eq!(tagged.len(), 3);
    }

    #[test]
    fn test_delete_entry() {
        let mut db = test_db();
        let entry = create_entry(&mut db, test_req("https://example.com", "To Delete", "text")).unwrap();
        delete_entry(&mut db, &entry.id).unwrap();
        assert!(get_entry(db.conn(), &entry.id).is_err());
    }

    #[test]
    fn test_entry_exists_by_url() {
        let mut db = test_db();
        assert!(entry_exists_by_url(db.conn(), "https://example.com").unwrap().is_none());

        let entry = create_entry(&mut db, test_req("https://example.com", "Test", "text")).unwrap();
        let found = entry_exists_by_url(db.conn(), "https://example.com").unwrap();
        assert_eq!(found, Some(entry.id));
    }

    #[test]
    fn test_tag_suggestions_by_domain() {
        let mut db = test_db();

        for i in 0..3 {
            let mut req = test_req(
                &format!("https://arxiv.org/paper/{i}"),
                &format!("Paper {i}"),
                &format!("research content {i}"),
            );
            req.tags = Some(vec!["research".to_string()]);
            create_entry(&mut db, req).unwrap();
        }

        let suggestions = suggest_tags_by_domain(db.conn(), "arxiv.org", 5).unwrap();
        assert_eq!(suggestions, vec!["research"]);

        let suggestions = suggest_tags_by_domain(db.conn(), "example.com", 5).unwrap();
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_tag_suggestions_popular() {
        let mut db = test_db();

        for i in 0..5 {
            let mut req = test_req(
                &format!("https://example.com/{i}"),
                &format!("Article {i}"),
                &format!("content {i}"),
            );
            req.tags = Some(vec!["common".to_string()]);
            create_entry(&mut db, req).unwrap();
        }
        let mut req = test_req("https://example.com/rare", "Rare", "rare content");
        req.tags = Some(vec!["rare".to_string()]);
        create_entry(&mut db, req).unwrap();

        let popular = suggest_tags_popular(db.conn(), 5).unwrap();
        assert_eq!(popular[0], "common");
    }
}
