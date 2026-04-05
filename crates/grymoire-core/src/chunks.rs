//! Document chunking and chunk-level embeddings for RAG retrieval.
//!
//! Splits entry text into overlapping windows, embeds each chunk,
//! and stores them in the `entry_chunks` table. Chunks provide
//! fine-grained semantic search for RAG — the existing whole-doc
//! embeddings (`entry_embeddings`) continue to serve similarity
//! and clustering use cases.

use chrono::Utc;
use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::embeddings::{self, EmbeddingModel};
use crate::errors::{GrymoireError, Result};

/// Target chunk size in characters (~500 tokens at ~4 chars/token).
pub const CHUNK_SIZE: usize = 2000;
/// Overlap between consecutive chunks in characters (~100 tokens).
pub const CHUNK_OVERLAP: usize = 400;
/// Minimum text length worth chunking/embedding.
pub const MIN_CHUNK_LEN: usize = 100;
/// Max chars for chunk embedding input (title prefix + chunk text).
const CHUNK_EMBED_MAX: usize = 2500;

/// A text chunk with its position in the source document.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub index: usize,
    pub text: String,
}

/// Split text into overlapping chunks with sentence-boundary snapping.
pub fn split_into_chunks(text: &str) -> Vec<Chunk> {
    let text = text.trim();
    if text.len() < MIN_CHUNK_LEN {
        return Vec::new();
    }

    // Short text: single chunk
    if text.len() <= CHUNK_SIZE {
        return vec![Chunk {
            index: 0,
            text: text.to_string(),
        }];
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();

    while start < total {
        let raw_end = (start + CHUNK_SIZE).min(total);

        // Snap to sentence boundary within the overlap region
        let end = if raw_end < total {
            snap_to_sentence(&chars, start + CHUNK_SIZE - CHUNK_OVERLAP, raw_end)
        } else {
            raw_end
        };

        let chunk_text: String = chars[start..end].iter().collect();
        let trimmed = chunk_text.trim();
        if trimmed.len() >= MIN_CHUNK_LEN {
            chunks.push(Chunk {
                index: chunks.len(),
                text: trimmed.to_string(),
            });
        }

        // Advance by (end - overlap), ensuring progress
        let advance = if end > start + CHUNK_OVERLAP {
            end - start - CHUNK_OVERLAP
        } else {
            CHUNK_SIZE - CHUNK_OVERLAP
        };
        start += advance.max(1);
    }

    chunks
}

/// Find the best sentence boundary (`.` `!` `?` followed by space) in the range [from..to].
/// Returns the position after the sentence-ending punctuation, or `to` if none found.
fn snap_to_sentence(chars: &[char], from: usize, to: usize) -> usize {
    let from = from.max(0);
    // Scan backward from `to` looking for sentence-ending punctuation
    for i in (from..to).rev() {
        if matches!(chars[i], '.' | '!' | '?')
            && i + 1 < chars.len()
            && chars[i + 1].is_whitespace()
        {
            return i + 1; // include the punctuation, split before the space
        }
    }
    to
}

/// Chunk and embed a single entry. Replaces any existing chunks.
/// Returns the number of chunks created.
pub fn chunk_and_embed_entry(
    conn: &Connection,
    model: &EmbeddingModel,
    entry_id: &Uuid,
) -> Result<usize> {
    let id_str = entry_id.to_string();

    // Get title and extracted text
    let (title, text): (String, String) = conn
        .query_row(
            "SELECT e.title, COALESCE(ec.extracted_text, '')
             FROM entries e
             JOIN entry_content ec ON ec.entry_id = e.id
             WHERE e.id = ?1",
            params![id_str],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| GrymoireError::Other(format!("entry not found: {e}")))?;

    if text.len() < MIN_CHUNK_LEN {
        return Ok(0);
    }

    let chunks = split_into_chunks(&text);
    if chunks.is_empty() {
        return Ok(0);
    }

    // Delete existing chunks for this entry
    conn.execute("DELETE FROM entry_chunks WHERE entry_id = ?1", params![id_str])?;

    let now = Utc::now().to_rfc3339();
    let mut stmt = conn.prepare(
        "INSERT INTO entry_chunks (entry_id, chunk_index, chunk_text, embedding, model_version, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;

    for chunk in &chunks {
        // Prefix with title to anchor chunk semantics
        let embed_input = format!("{title}: {}", chunk.text);
        let truncated: String = embed_input.chars().take(CHUNK_EMBED_MAX).collect();
        let embedding = model.embed_text(&truncated)?;
        let blob = embeddings::serialize_embedding(&embedding);

        stmt.execute(params![
            id_str,
            chunk.index as i32,
            chunk.text,
            blob,
            embeddings::MODEL_VERSION,
            now,
        ])?;
    }

    Ok(chunks.len())
}

/// Backfill: chunk and embed all entries that have no chunks yet.
/// Returns the total number of chunks created.
pub fn chunk_all_missing(conn: &Connection, model: &EmbeddingModel) -> Result<usize> {
    let mut stmt = conn.prepare(
        "SELECT e.id FROM entries e
         WHERE NOT EXISTS (
             SELECT 1 FROM entry_chunks ec WHERE ec.entry_id = e.id
         )",
    )?;

    let ids: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();

    let mut total_chunks = 0;
    for id_str in &ids {
        let uuid: Uuid = id_str
            .parse()
            .map_err(|e| GrymoireError::Other(format!("bad uuid: {e}")))?;
        match chunk_and_embed_entry(conn, model, &uuid) {
            Ok(n) => total_chunks += n,
            Err(e) => tracing::warn!(entry_id = %id_str, "chunk+embed failed: {e}"),
        }
    }

    if total_chunks > 0 {
        tracing::info!("chunked {total_chunks} chunks from {} entries", ids.len());
    }
    Ok(total_chunks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_empty() {
        assert!(split_into_chunks("").is_empty());
        assert!(split_into_chunks("short").is_empty());
    }

    #[test]
    fn test_split_single_chunk() {
        let text = "A".repeat(500);
        let chunks = split_into_chunks(&text);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].index, 0);
    }

    #[test]
    fn test_split_multiple_chunks() {
        // 6000 chars with CHUNK_SIZE=2000, OVERLAP=400 → expect ~3-4 chunks
        let text = "Hello world. ".repeat(500); // ~6500 chars
        let chunks = split_into_chunks(&text);
        assert!(chunks.len() >= 3, "got {} chunks", chunks.len());
        // Verify all chunks are reasonable size
        for c in &chunks {
            assert!(c.text.len() >= MIN_CHUNK_LEN, "chunk {} too short: {}", c.index, c.text.len());
        }
    }

    #[test]
    fn test_split_preserves_all_content() {
        // Every sentence from the original should appear in at least one chunk
        let sentences: Vec<String> = (0..20)
            .map(|i| format!("Sentence number {i} with some padding text to fill space. "))
            .collect();
        let text = sentences.join("");
        let chunks = split_into_chunks(&text);
        for sentence in &sentences {
            let trimmed = sentence.trim();
            assert!(
                chunks.iter().any(|c| c.text.contains(trimmed)),
                "sentence not found in any chunk: {trimmed}"
            );
        }
    }

    #[test]
    fn test_chunks_overlap() {
        let text = "Word. ".repeat(1000); // ~6000 chars
        let chunks = split_into_chunks(&text);
        if chunks.len() >= 2 {
            // Last part of chunk 0 should appear in chunk 1
            let tail_0 = &chunks[0].text[chunks[0].text.len().saturating_sub(100)..];
            // At least some overlap should exist
            assert!(
                chunks[1].text.contains(tail_0) || tail_0.contains(&chunks[1].text[..100.min(chunks[1].text.len())]),
                "expected overlap between consecutive chunks"
            );
        }
    }

    #[test]
    fn test_chunk_indices_sequential() {
        let text = "Some text here. ".repeat(400);
        let chunks = split_into_chunks(&text);
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.index, i);
        }
    }
}
