//! RAG (Retrieval-Augmented Generation) pipeline.
//!
//! Combines semantic chunk search with FTS5 keyword search to retrieve
//! relevant context for LLM-powered Q&A over the user's archive.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::embeddings::{self, EmbeddingModel};
use crate::errors::Result;
use crate::llm_catalog::ChatTemplate;
use crate::search;

/// Number of chunks to retrieve via semantic search.
const SEMANTIC_TOP_K: usize = 10;
/// Number of entries to retrieve via FTS5.
const FTS_TOP_K: usize = 5;
/// Final number of chunks after reranking/dedup.
const FINAL_TOP_K: usize = 5;
/// Maximum total chars of chunk context in the prompt.
const MAX_CONTEXT_CHARS: usize = 3000;

/// A retrieved chunk with source attribution.
#[derive(Debug, Clone, Serialize)]
pub struct RetrievedChunk {
    pub entry_id: Uuid,
    pub entry_title: String,
    pub chunk_index: usize,
    pub chunk_text: String,
    pub score: f32,
    pub source_label: String,
}

/// A message in the conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// The prepared RAG context: prompt ready for the LLM + source metadata.
pub struct RagContext {
    pub prompt: String,
    pub sources: Vec<RetrievedChunk>,
}

/// Full RAG pipeline: retrieve relevant chunks, build prompt with sources.
pub fn prepare_rag_context(
    conn: &Connection,
    model: &EmbeddingModel,
    history: &[ChatMessage],
    user_query: &str,
    chat_template: ChatTemplate,
) -> Result<RagContext> {
    let sources = hybrid_retrieve(conn, model, user_query)?;
    let prompt = build_rag_prompt(&sources, history, user_query, chat_template);
    Ok(RagContext { prompt, sources })
}

/// Hybrid retrieval: semantic chunk search + FTS5, merged and reranked.
pub fn hybrid_retrieve(
    conn: &Connection,
    model: &EmbeddingModel,
    query: &str,
) -> Result<Vec<RetrievedChunk>> {
    // 1. Semantic search over chunk embeddings
    let query_embedding = model.embed_text(query)?;
    let semantic_results = semantic_search_chunks(conn, &query_embedding, SEMANTIC_TOP_K)?;

    // 2. FTS5 keyword search over entries
    let fts_entry_ids = fts_search_entry_ids(conn, query, FTS_TOP_K)?;

    // 3. Merge: for FTS hits not in semantic results, get their best chunk
    let semantic_entry_ids: std::collections::HashSet<String> = semantic_results
        .iter()
        .map(|r| r.entry_id.to_string())
        .collect();

    let mut candidates = semantic_results;

    for (entry_id, entry_title) in &fts_entry_ids {
        if semantic_entry_ids.contains(&entry_id.to_string()) {
            continue;
        }
        // Find the best chunk for this entry by cosine to query
        if let Ok(Some(chunk)) = best_chunk_for_entry(conn, &query_embedding, entry_id) {
            candidates.push(RetrievedChunk {
                entry_id: *entry_id,
                entry_title: entry_title.clone(),
                chunk_index: chunk.0,
                chunk_text: chunk.1,
                score: chunk.2 * 0.8, // slight discount for keyword-only hits
                source_label: String::new(),
            });
        }
    }

    // 4. Deduplicate by entry_id (keep highest-scoring chunk per entry)
    let mut best_per_entry: std::collections::HashMap<String, RetrievedChunk> =
        std::collections::HashMap::new();
    for chunk in candidates {
        let key = chunk.entry_id.to_string();
        if let Some(existing) = best_per_entry.get(&key) {
            if chunk.score > existing.score {
                best_per_entry.insert(key, chunk);
            }
        } else {
            best_per_entry.insert(key, chunk);
        }
    }

    // 5. Sort by score, take top K, assign labels
    let mut results: Vec<RetrievedChunk> = best_per_entry.into_values().collect();
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(FINAL_TOP_K);

    // Truncate total context to budget
    let mut total_chars = 0;
    results.retain(|c| {
        if total_chars >= MAX_CONTEXT_CHARS {
            return false;
        }
        total_chars += c.chunk_text.len();
        true
    });

    // Assign source labels
    for (i, chunk) in results.iter_mut().enumerate() {
        chunk.source_label = format!("[{}]", i + 1);
    }

    Ok(results)
}

/// Search chunk embeddings by cosine similarity to the query vector.
fn semantic_search_chunks(
    conn: &Connection,
    query_vec: &[f32],
    top_k: usize,
) -> Result<Vec<RetrievedChunk>> {
    let mut stmt = conn.prepare(
        "SELECT ec.entry_id, ec.chunk_index, ec.chunk_text, ec.embedding, e.title
         FROM entry_chunks ec
         JOIN entries e ON e.id = ec.entry_id",
    )?;

    let mut scored: Vec<RetrievedChunk> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i32>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Vec<u8>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .map(|(entry_id, chunk_idx, chunk_text, blob, title)| {
            let vec = embeddings::deserialize_embedding(&blob);
            let score = embeddings::cosine_similarity(query_vec, &vec);
            RetrievedChunk {
                entry_id: entry_id.parse().unwrap_or_default(),
                entry_title: title,
                chunk_index: chunk_idx as usize,
                chunk_text,
                score,
                source_label: String::new(),
            }
        })
        .collect();

    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(top_k);
    Ok(scored)
}

/// FTS5 search returning entry IDs + titles (lightweight, no content fetch).
fn fts_search_entry_ids(
    conn: &Connection,
    query: &str,
    top_k: usize,
) -> Result<Vec<(Uuid, String)>> {
    let sanitized = search::sanitize_fts_query(query);
    if sanitized.is_empty() {
        return Ok(Vec::new());
    }

    let mut stmt = conn.prepare(
        "SELECT e.id, e.title
         FROM entries_fts
         JOIN entries e ON e.rowid = entries_fts.rowid
         WHERE entries_fts MATCH ?1
         ORDER BY bm25(entries_fts, 5.0, 1.0)
         LIMIT ?2",
    )?;

    let results: Vec<(Uuid, String)> = stmt
        .query_map(params![sanitized, top_k as i64], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .filter_map(|r| r.ok())
        .filter_map(|(id, title)| id.parse::<Uuid>().ok().map(|u| (u, title)))
        .collect();

    Ok(results)
}

/// Find the best-scoring chunk for a given entry.
fn best_chunk_for_entry(
    conn: &Connection,
    query_vec: &[f32],
    entry_id: &Uuid,
) -> Result<Option<(usize, String, f32)>> {
    let mut stmt = conn.prepare(
        "SELECT chunk_index, chunk_text, embedding FROM entry_chunks WHERE entry_id = ?1",
    )?;

    let mut best: Option<(usize, String, f32)> = None;
    let id_str = entry_id.to_string();

    let rows = stmt.query_map(params![id_str], |row| {
        Ok((
            row.get::<_, i32>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Vec<u8>>(2)?,
        ))
    })?;

    for row in rows.flatten() {
        let (idx, text, blob) = row;
        let vec = embeddings::deserialize_embedding(&blob);
        let score = embeddings::cosine_similarity(query_vec, &vec);
        if best.as_ref().is_none_or(|(_, _, s)| score > *s) {
            best = Some((idx as usize, text, score));
        }
    }

    Ok(best)
}

/// Build the RAG prompt with sources, history, and the user query.
pub fn build_rag_prompt(
    sources: &[RetrievedChunk],
    history: &[ChatMessage],
    user_query: &str,
    template: ChatTemplate,
) -> String {
    let system = "You are a helpful research assistant answering questions about the user's \
        personal knowledge archive. Rules:\n\
        - Base your answer ONLY on the provided sources\n\
        - Cite sources inline using [1], [2], etc.\n\
        - If the sources don't contain enough information, say so honestly\n\
        - Be concise but thorough";

    // Build sources block
    let mut sources_block = String::from("Sources:\n");
    for s in sources {
        sources_block.push_str(&format!(
            "{} \"{}\"\n{}\n\n",
            s.source_label, s.entry_title, s.chunk_text
        ));
    }

    let user_content = format!("{sources_block}Question: {user_query}");

    // Include last 2 history turns to support follow-ups
    let recent_history: Vec<&ChatMessage> = history
        .iter()
        .rev()
        .take(4) // last 2 exchanges (user + assistant each)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    match template {
        ChatTemplate::ChatML => {
            let mut prompt = format!("<|im_start|>system\n{system}<|im_end|>\n");
            for msg in &recent_history {
                prompt.push_str(&format!(
                    "<|im_start|>{}\n{}<|im_end|>\n",
                    msg.role, msg.content
                ));
            }
            prompt.push_str(&format!(
                "<|im_start|>user\n{user_content}<|im_end|>\n<|im_start|>assistant\n"
            ));
            prompt
        }
        ChatTemplate::Llama3 => {
            let mut prompt = format!(
                "<|start_header_id|>system<|end_header_id|>\n\n{system}<|eot_id|>"
            );
            for msg in &recent_history {
                prompt.push_str(&format!(
                    "<|start_header_id|>{}<|end_header_id|>\n\n{}<|eot_id|>",
                    msg.role, msg.content
                ));
            }
            prompt.push_str(&format!(
                "<|start_header_id|>user<|end_header_id|>\n\n{user_content}<|eot_id|>\
                 <|start_header_id|>assistant<|end_header_id|>\n\n"
            ));
            prompt
        }
        ChatTemplate::Gemma => {
            let mut prompt = format!("<start_of_turn>user\n{system}\n\n");
            for msg in &recent_history {
                let turn = if msg.role == "user" { "user" } else { "model" };
                prompt.push_str(&format!(
                    "<end_of_turn>\n<start_of_turn>{turn}\n{}",
                    msg.content
                ));
            }
            prompt.push_str(&format!(
                "{user_content}<end_of_turn>\n<start_of_turn>model\n"
            ));
            prompt
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_rag_prompt_chatml() {
        let sources = vec![
            RetrievedChunk {
                entry_id: Uuid::nil(),
                entry_title: "Test Doc".into(),
                chunk_index: 0,
                chunk_text: "Some content here.".into(),
                score: 0.9,
                source_label: "[1]".into(),
            },
        ];
        let prompt = build_rag_prompt(&sources, &[], "What is this about?", ChatTemplate::ChatML);

        assert!(prompt.contains("<|im_start|>system"));
        assert!(prompt.contains("[1] \"Test Doc\""));
        assert!(prompt.contains("Some content here."));
        assert!(prompt.contains("Question: What is this about?"));
        assert!(prompt.ends_with("<|im_start|>assistant\n"));
    }

    #[test]
    fn test_build_rag_prompt_with_history() {
        let sources = vec![RetrievedChunk {
            entry_id: Uuid::nil(),
            entry_title: "Doc".into(),
            chunk_index: 0,
            chunk_text: "Content.".into(),
            score: 0.8,
            source_label: "[1]".into(),
        }];
        let history = vec![
            ChatMessage { role: "user".into(), content: "First question".into() },
            ChatMessage { role: "assistant".into(), content: "First answer".into() },
        ];
        let prompt = build_rag_prompt(&sources, &history, "Follow-up?", ChatTemplate::ChatML);

        assert!(prompt.contains("First question"));
        assert!(prompt.contains("First answer"));
        assert!(prompt.contains("Follow-up?"));
    }

    #[test]
    fn test_build_rag_prompt_llama3() {
        let sources = vec![RetrievedChunk {
            entry_id: Uuid::nil(),
            entry_title: "Doc".into(),
            chunk_index: 0,
            chunk_text: "Content.".into(),
            score: 0.8,
            source_label: "[1]".into(),
        }];
        let prompt = build_rag_prompt(&sources, &[], "Question?", ChatTemplate::Llama3);
        assert!(prompt.contains("<|start_header_id|>system<|end_header_id|>"));
        assert!(prompt.contains("<|start_header_id|>assistant<|end_header_id|>"));
    }
}
