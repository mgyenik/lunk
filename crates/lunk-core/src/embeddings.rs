//! Semantic embedding generation and similarity search.
//!
//! Uses fastembed (ONNX Runtime) to generate 384-dimensional embeddings
//! for document text. Embeddings are stored as BLOBs in SQLite and used
//! for similarity search and topic clustering.

use std::sync::Arc;

use chrono::Utc;
use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::errors::{LunkError, Result};
use crate::models::Entry;
use crate::repo;

/// Dimensionality of the embedding vectors.
pub const EMBEDDING_DIM: usize = 384;

/// Model identifier stored with each embedding for versioning.
pub const MODEL_VERSION: &str = "all-MiniLM-L6-v2-q";

/// Maximum text length (in chars) to embed. The model has a 256-token
/// context window; ~1500 chars covers the title + first few paragraphs.
const MAX_TEXT_LEN: usize = 1500;

/// Wrapper around fastembed's TextEmbedding model.
/// Initialized once at app startup, shared across the Tauri app and HTTP server.
/// Clone is cheap (Arc).
#[derive(Clone)]
pub struct EmbeddingModel {
    inner: Arc<fastembed::TextEmbedding>,
}

impl EmbeddingModel {
    /// Initialize the embedding model from a HuggingFace cache directory.
    ///
    /// If `cache_dir` is None, uses fastembed's default cache (downloads on first use).
    /// For production use, prefer `from_dir()` which loads from bundled resource files.
    pub fn new(cache_dir: Option<&std::path::Path>) -> Result<Self> {
        let mut opts =
            fastembed::InitOptions::new(fastembed::EmbeddingModel::AllMiniLML6V2Q);
        if let Some(dir) = cache_dir {
            opts = opts.with_cache_dir(dir.to_path_buf());
        }
        opts = opts.with_show_download_progress(true);

        let inner = fastembed::TextEmbedding::try_new(opts)
            .map_err(|e| LunkError::Other(format!("embedding model init: {e}")))?;

        Ok(Self { inner: Arc::new(inner) })
    }

    /// Initialize the embedding model from bundled resource files.
    ///
    /// `model_dir` should contain: model_quantized.onnx, tokenizer.json,
    /// config.json, special_tokens_map.json, tokenizer_config.json.
    /// These are downloaded at build time and bundled as Tauri resources.
    pub fn from_dir(model_dir: &std::path::Path) -> Result<Self> {
        let read = |name: &str| -> Result<Vec<u8>> {
            std::fs::read(model_dir.join(name)).map_err(|e| {
                LunkError::Other(format!("missing model file {name}: {e}"))
            })
        };

        let user_model = fastembed::UserDefinedEmbeddingModel::new(
            read("model_quantized.onnx")?,
            fastembed::TokenizerFiles {
                tokenizer_file: read("tokenizer.json")?,
                config_file: read("config.json")?,
                special_tokens_map_file: read("special_tokens_map.json")?,
                tokenizer_config_file: read("tokenizer_config.json")?,
            },
        )
        .with_pooling(fastembed::Pooling::Mean)
        .with_quantization(fastembed::QuantizationMode::Dynamic);

        let inner = fastembed::TextEmbedding::try_new_from_user_defined(
            user_model,
            fastembed::InitOptionsUserDefined::default(),
        )
        .map_err(|e| LunkError::Other(format!("embedding model init from dir: {e}")))?;

        Ok(Self { inner: Arc::new(inner) })
    }

    /// Generate an embedding for a text string.
    pub fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
        // Truncate to model's effective context
        let truncated: String = text.chars().take(MAX_TEXT_LEN).collect();
        let results = self
            .inner
            .embed(vec![truncated], None)
            .map_err(|e| LunkError::Other(format!("embedding failed: {e}")))?;

        results
            .into_iter()
            .next()
            .ok_or_else(|| LunkError::Other("no embedding returned".into()))
    }
}

/// Serialize an f32 vector to bytes (little-endian) for SQLite BLOB storage.
pub fn serialize_embedding(vec: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vec.len() * 4);
    for &val in vec {
        bytes.extend_from_slice(&val.to_le_bytes());
    }
    bytes
}

/// Deserialize bytes (little-endian f32) back to a vector.
pub fn deserialize_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

/// Cosine similarity between two vectors. Returns value in [-1, 1].
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

/// Embed a single entry and store the result.
pub fn embed_entry(
    conn: &Connection,
    model: &EmbeddingModel,
    entry_id: &Uuid,
) -> Result<()> {
    // Get the entry's text content
    let text = get_entry_text(conn, entry_id)?;
    if text.is_empty() {
        return Ok(()); // Nothing to embed
    }

    let embedding = model.embed_text(&text)?;
    let blob = serialize_embedding(&embedding);
    let now = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT OR REPLACE INTO entry_embeddings (entry_id, embedding, model_version, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![entry_id.to_string(), blob, MODEL_VERSION, now],
    )?;

    Ok(())
}

/// Embed all entries that don't have embeddings yet. Returns count of newly embedded.
pub fn embed_all_missing(conn: &Connection, model: &EmbeddingModel) -> Result<usize> {
    let mut stmt = conn.prepare(
        "SELECT e.id FROM entries e
         WHERE NOT EXISTS (
             SELECT 1 FROM entry_embeddings ee WHERE ee.entry_id = e.id
         )",
    )?;

    let ids: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();

    let mut count = 0;
    for id_str in &ids {
        let uuid: Uuid = id_str
            .parse()
            .map_err(|e| LunkError::Other(format!("bad uuid: {e}")))?;
        if let Err(e) = embed_entry(conn, model, &uuid) {
            tracing::warn!(entry_id = %id_str, "failed to embed: {e}");
        } else {
            count += 1;
        }
    }

    Ok(count)
}

/// Find the N most similar entries to a given entry.
/// Returns (entry, similarity_score) pairs sorted by similarity descending.
pub fn find_similar(
    conn: &Connection,
    entry_id: &Uuid,
    limit: usize,
) -> Result<Vec<(Entry, f32)>> {
    // Get the query entry's embedding
    let query_blob: Vec<u8> = conn
        .query_row(
            "SELECT embedding FROM entry_embeddings WHERE entry_id = ?1",
            params![entry_id.to_string()],
            |row| row.get(0),
        )
        .map_err(|_| LunkError::Other("entry has no embedding".into()))?;

    let query_vec = deserialize_embedding(&query_blob);

    // Load all other embeddings
    let mut stmt = conn.prepare(
        "SELECT entry_id, embedding FROM entry_embeddings WHERE entry_id != ?1",
    )?;

    let mut scored: Vec<(String, f32)> = stmt
        .query_map(params![entry_id.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?
        .filter_map(|r| r.ok())
        .map(|(id, blob)| {
            let vec = deserialize_embedding(&blob);
            let sim = cosine_similarity(&query_vec, &vec);
            (id, sim)
        })
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);

    // Fetch full entry data
    let mut results = Vec::new();
    for (id_str, score) in scored {
        let uuid: Uuid = id_str.parse().map_err(|e| LunkError::Other(format!("{e}")))?;
        if let Ok(entry) = repo::get_entry(conn, &uuid) {
            results.push((entry, score));
        }
    }

    Ok(results)
}

/// Load all embeddings as (entry_id_string, Vec<f32>) pairs.
/// Used by the clustering module.
pub fn load_all_embeddings(conn: &Connection) -> Result<Vec<(String, Vec<f32>)>> {
    let mut stmt = conn.prepare("SELECT entry_id, embedding FROM entry_embeddings")?;
    let results: Vec<(String, Vec<f32>)> = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?
        .filter_map(|r| r.ok())
        .map(|(id, blob)| (id, deserialize_embedding(&blob)))
        .collect();
    Ok(results)
}

/// Get the text to embed for an entry: title + extracted text (truncated).
fn get_entry_text(conn: &Connection, entry_id: &Uuid) -> Result<String> {
    let id_str = entry_id.to_string();

    let title: String = conn
        .query_row(
            "SELECT title FROM entries WHERE id = ?1",
            params![id_str],
            |row| row.get(0),
        )
        .unwrap_or_default();

    let text: String = conn
        .query_row(
            "SELECT extracted_text FROM entry_content WHERE entry_id = ?1",
            params![id_str],
            |row| row.get(0),
        )
        .unwrap_or_default();

    // Combine title + text for richer context
    Ok(format!("{title}. {text}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialize_roundtrip() {
        let vec = vec![1.0f32, -2.5, 0.0, 7.77];
        let bytes = serialize_embedding(&vec);
        assert_eq!(bytes.len(), 16);
        let back = deserialize_embedding(&bytes);
        assert_eq!(vec, back);
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&a, &a);
        assert!((sim - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let sim = cosine_similarity(&a, &b);
        assert!(sim.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 2.0, 3.0];
        let b: Vec<f32> = a.iter().map(|x| -x).collect();
        let sim = cosine_similarity(&a, &b);
        assert!((sim + 1.0).abs() < 1e-6);
    }

    #[test]
    #[ignore] // Downloads model on first run — not hermetic. Run with --ignored.
    fn test_model_loads() {
        let model = EmbeddingModel::new(None);
        if model.is_err() {
            eprintln!("Skipping: model not available");
            return;
        }
        let model = model.unwrap();
        let embedding = model.embed_text("hello world").unwrap();
        assert_eq!(embedding.len(), EMBEDDING_DIM);
    }
}
