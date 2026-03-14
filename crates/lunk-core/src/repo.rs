use chrono::Utc;
use rusqlite::{params, Connection};
use url::Url;
use uuid::Uuid;

use crate::errors::{LunkError, Result};
use crate::models::*;

pub fn create_entry(conn: &Connection, req: CreateEntryRequest) -> Result<Entry> {
    let id = Uuid::now_v7();
    let now = Utc::now();

    let domain = req.url.as_ref().and_then(|u| {
        Url::parse(u)
            .ok()
            .and_then(|parsed| parsed.host_str().map(|h| h.to_string()))
    });

    let word_count = Some(req.extracted_text.split_whitespace().count() as i64);
    let status = req.status.unwrap_or(EntryStatus::Unread);

    conn.execute(
        "INSERT INTO entries (id, url, title, content_type, status, domain, word_count, page_count, created_at, updated_at, saved_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            id.to_string(),
            req.url,
            req.title,
            req.content_type.as_str(),
            status.as_str(),
            domain,
            word_count,
            Option::<i64>::None,
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

    Ok(Entry {
        id,
        url: req.url,
        title: req.title,
        content_type: req.content_type,
        status,
        domain,
        word_count,
        page_count: None,
        created_at: now,
        updated_at: now,
        saved_by: req.source.as_str().to_string(),
        tags,
    })
}

pub fn create_pdf_entry(
    conn: &Connection,
    req: CreateEntryRequest,
    pages: Vec<(i32, String)>,
) -> Result<Entry> {
    let id = Uuid::now_v7();
    let now = Utc::now();

    let domain = req.url.as_ref().and_then(|u| {
        Url::parse(u)
            .ok()
            .and_then(|parsed| parsed.host_str().map(|h| h.to_string()))
    });

    let word_count = Some(req.extracted_text.split_whitespace().count() as i64);
    let page_count = Some(pages.len() as i64);
    let status = req.status.unwrap_or(EntryStatus::Unread);

    conn.execute(
        "INSERT INTO entries (id, url, title, content_type, status, domain, word_count, page_count, created_at, updated_at, saved_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            id.to_string(),
            req.url,
            req.title,
            ContentType::Pdf.as_str(),
            status.as_str(),
            domain,
            word_count,
            page_count,
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

    Ok(Entry {
        id,
        url: req.url,
        title: req.title,
        content_type: ContentType::Pdf,
        status,
        domain,
        word_count,
        page_count,
        created_at: now,
        updated_at: now,
        saved_by: req.source.as_str().to_string(),
        tags,
    })
}

pub fn get_entry(conn: &Connection, id: &Uuid) -> Result<Entry> {
    let mut stmt = conn.prepare(
        "SELECT e.id, e.url, e.title, e.content_type, e.status, e.domain,
                e.word_count, e.page_count, e.created_at, e.updated_at, e.saved_by
         FROM entries e WHERE e.id = ?1",
    )?;

    let entry = stmt.query_row(params![id.to_string()], |row| {
        Ok(EntryRow {
            id: row.get(0)?,
            url: row.get(1)?,
            title: row.get(2)?,
            content_type: row.get(3)?,
            status: row.get(4)?,
            domain: row.get(5)?,
            word_count: row.get(6)?,
            page_count: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
            saved_by: row.get(10)?,
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

    if let Some(status) = &params.status {
        bind_values.push(Box::new(status.as_str().to_string()));
        conditions.push(format!("e.status = ?{}", bind_values.len()));
    }
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
        "SELECT e.id, e.url, e.title, e.content_type, e.status, e.domain,
                e.word_count, e.page_count, e.created_at, e.updated_at, e.saved_by
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
            status: row.get(4)?,
            domain: row.get(5)?,
            word_count: row.get(6)?,
            page_count: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
            saved_by: row.get(10)?,
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

pub fn update_entry_status(conn: &Connection, id: &Uuid, status: EntryStatus) -> Result<()> {
    let now = Utc::now();
    let changed = conn.execute(
        "UPDATE entries SET status = ?1, updated_at = ?2 WHERE id = ?3",
        params![status.as_str(), now.to_rfc3339(), id.to_string()],
    )?;

    if changed == 0 {
        return Err(LunkError::NotFound(format!("entry {id}")));
    }
    Ok(())
}

pub fn update_entry(
    conn: &Connection,
    id: &Uuid,
    title: Option<&str>,
    status: Option<EntryStatus>,
    tags: Option<&[String]>,
) -> Result<Entry> {
    let now = Utc::now();

    if let Some(title) = title {
        conn.execute(
            "UPDATE entries SET title = ?1, updated_at = ?2 WHERE id = ?3",
            params![title, now.to_rfc3339(), id.to_string()],
        )?;
    }

    if let Some(status) = status {
        conn.execute(
            "UPDATE entries SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status.as_str(), now.to_rfc3339(), id.to_string()],
        )?;
    }

    if let Some(tags) = tags {
        // Remove existing tags
        conn.execute(
            "DELETE FROM entry_tags WHERE entry_id = ?1",
            params![id.to_string()],
        )?;
        // Add new tags
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
    }

    // Always bump updated_at
    conn.execute(
        "UPDATE entries SET updated_at = ?1 WHERE id = ?2",
        params![now.to_rfc3339(), id.to_string()],
    )?;

    get_entry(conn, id)
}

pub fn delete_entry(conn: &Connection, id: &Uuid) -> Result<()> {
    // entry_content, pdf_pages, entry_tags cleaned up by ON DELETE CASCADE
    let changed = conn.execute(
        "DELETE FROM entries WHERE id = ?1",
        params![id.to_string()],
    )?;

    if changed == 0 {
        return Err(LunkError::NotFound(format!("entry {id}")));
    }
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

// --- Internal helpers ---

struct EntryRow {
    id: String,
    url: Option<String>,
    title: String,
    content_type: String,
    status: String,
    domain: Option<String>,
    word_count: Option<i64>,
    page_count: Option<i64>,
    created_at: String,
    updated_at: String,
    saved_by: String,
}

fn row_to_entry(row: EntryRow, tags: Vec<String>) -> Result<Entry> {
    let id = Uuid::parse_str(&row.id)
        .map_err(|e| LunkError::Other(format!("invalid uuid: {e}")))?;
    let content_type = ContentType::from_str(&row.content_type)
        .ok_or_else(|| LunkError::Other(format!("invalid content_type: {}", row.content_type)))?;
    let status = EntryStatus::from_str(&row.status)
        .ok_or_else(|| LunkError::Other(format!("invalid status: {}", row.status)))?;
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
        status,
        domain: row.domain,
        word_count: row.word_count,
        page_count: row.page_count,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn test_conn() -> Connection {
        db::open_in_memory().unwrap()
    }

    #[test]
    fn test_create_and_get_entry() {
        let conn = test_conn();
        let req = CreateEntryRequest {
            url: Some("https://example.com/article".to_string()),
            title: "Test Article".to_string(),
            content_type: ContentType::Article,
            extracted_text: "This is the full text of the article about Rust programming.".to_string(),
            snapshot_html: Some(b"<html>full snapshot</html>".to_vec()),
            readable_html: Some(b"<article>clean text</article>".to_vec()),
            pdf_data: None,
            status: Some(EntryStatus::Unread),
            tags: Some(vec!["rust".to_string(), "programming".to_string()]),
            source: SaveSource::Cli,
        };

        let entry = create_entry(&conn, req).unwrap();
        assert_eq!(entry.title, "Test Article");
        assert_eq!(entry.domain, Some("example.com".to_string()));
        assert_eq!(entry.status, EntryStatus::Unread);
        assert_eq!(entry.tags.len(), 2);

        let fetched = get_entry(&conn, &entry.id).unwrap();
        assert_eq!(fetched.title, "Test Article");
        assert_eq!(fetched.tags.len(), 2);
    }

    #[test]
    fn test_update_status() {
        let conn = test_conn();
        let req = CreateEntryRequest {
            url: Some("https://example.com".to_string()),
            title: "Test".to_string(),
            content_type: ContentType::Article,
            extracted_text: "text".to_string(),
            snapshot_html: None,
            readable_html: None,
            pdf_data: None,
            status: None,
            tags: None,
            source: SaveSource::Cli,
        };

        let entry = create_entry(&conn, req).unwrap();
        assert_eq!(entry.status, EntryStatus::Unread);

        update_entry_status(&conn, &entry.id, EntryStatus::Read).unwrap();
        let updated = get_entry(&conn, &entry.id).unwrap();
        assert_eq!(updated.status, EntryStatus::Read);
    }

    #[test]
    fn test_list_entries() {
        let conn = test_conn();

        for i in 0..5 {
            let req = CreateEntryRequest {
                url: Some(format!("https://example.com/{i}")),
                title: format!("Article {i}"),
                content_type: ContentType::Article,
                extracted_text: format!("content {i}"),
                snapshot_html: None,
                readable_html: None,
                pdf_data: None,
                status: if i < 3 { Some(EntryStatus::Unread) } else { Some(EntryStatus::Read) },
                tags: None,
                source: SaveSource::Cli,
            };
            create_entry(&conn, req).unwrap();
        }

        let (all, total) = list_entries(&conn, &ListParams::default()).unwrap();
        assert_eq!(total, 5);
        assert_eq!(all.len(), 5);

        let (unread, total) = list_entries(&conn, &ListParams {
            status: Some(EntryStatus::Unread),
            ..Default::default()
        }).unwrap();
        assert_eq!(total, 3);
        assert_eq!(unread.len(), 3);
    }

    #[test]
    fn test_delete_entry() {
        let conn = test_conn();
        let req = CreateEntryRequest {
            url: Some("https://example.com".to_string()),
            title: "To Delete".to_string(),
            content_type: ContentType::Article,
            extracted_text: "text".to_string(),
            snapshot_html: None,
            readable_html: None,
            pdf_data: None,
            status: None,
            tags: None,
            source: SaveSource::Cli,
        };

        let entry = create_entry(&conn, req).unwrap();
        delete_entry(&conn, &entry.id).unwrap();

        assert!(get_entry(&conn, &entry.id).is_err());
    }

    #[test]
    fn test_entry_exists_by_url() {
        let conn = test_conn();
        assert!(entry_exists_by_url(&conn, "https://example.com").unwrap().is_none());

        let req = CreateEntryRequest {
            url: Some("https://example.com".to_string()),
            title: "Test".to_string(),
            content_type: ContentType::Article,
            extracted_text: "text".to_string(),
            snapshot_html: None,
            readable_html: None,
            pdf_data: None,
            status: None,
            tags: None,
            source: SaveSource::Cli,
        };
        let entry = create_entry(&conn, req).unwrap();

        let found = entry_exists_by_url(&conn, "https://example.com").unwrap();
        assert_eq!(found, Some(entry.id));
    }
}
