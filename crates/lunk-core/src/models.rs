use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    Article,
    Pdf,
}

impl ContentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContentType::Article => "article",
            ContentType::Pdf => "pdf",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "article" => Some(ContentType::Article),
            "pdf" => Some(ContentType::Pdf),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntryStatus {
    Unread,
    Read,
    Archived,
}

impl EntryStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntryStatus::Unread => "unread",
            EntryStatus::Read => "read",
            EntryStatus::Archived => "archived",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "unread" => Some(EntryStatus::Unread),
            "read" => Some(EntryStatus::Read),
            "archived" => Some(EntryStatus::Archived),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SaveSource {
    Extension,
    Cli,
    Api,
}

impl SaveSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            SaveSource::Extension => "extension",
            SaveSource::Cli => "cli",
            SaveSource::Api => "api",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: Uuid,
    pub url: Option<String>,
    pub title: String,
    pub content_type: ContentType,
    pub status: EntryStatus,
    pub domain: Option<String>,
    pub word_count: Option<i64>,
    pub page_count: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub saved_by: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryContent {
    pub entry_id: Uuid,
    pub extracted_text: String,
    pub snapshot_html: Option<Vec<u8>>,
    pub readable_html: Option<Vec<u8>>,
    pub pdf_data: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PdfPage {
    pub id: Uuid,
    pub entry_id: Uuid,
    pub page_num: i32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub entries: Vec<SearchHit>,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    #[serde(flatten)]
    pub entry: Entry,
    pub snippet: Option<String>,
    pub matched_page: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateEntryRequest {
    pub url: Option<String>,
    pub title: String,
    pub content_type: ContentType,
    pub extracted_text: String,
    pub snapshot_html: Option<Vec<u8>>,
    pub readable_html: Option<Vec<u8>>,
    pub pdf_data: Option<Vec<u8>>,
    pub status: Option<EntryStatus>,
    pub tags: Option<Vec<String>>,
    pub source: SaveSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListParams {
    pub status: Option<EntryStatus>,
    pub content_type: Option<ContentType>,
    pub tag: Option<String>,
    pub domain: Option<String>,
    pub sort: Option<String>,
    pub order: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

impl Default for ListParams {
    fn default() -> Self {
        Self {
            status: None,
            content_type: None,
            tag: None,
            domain: None,
            sort: Some("created_at".to_string()),
            order: Some("desc".to_string()),
            limit: Some(50),
            offset: Some(0),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagWithCount {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPeer {
    pub id: String,
    pub name: Option<String>,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub last_db_version: i64,
}
