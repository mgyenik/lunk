use thiserror::Error;

#[derive(Debug, Error)]
pub enum LunkError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("url parse error: {0}")]
    UrlParse(#[from] url::ParseError),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("sync error: {0}")]
    Sync(String),

    #[error("transport error: {0}")]
    Transport(String),

    #[error("llm error: {0}")]
    Llm(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, LunkError>;
