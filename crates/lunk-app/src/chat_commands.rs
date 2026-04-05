use serde::{Deserialize, Serialize};
use tauri::Emitter;

use lunk_core::db::{with_db, DbPool};
use lunk_core::embeddings::EmbeddingModel;
use lunk_core::llm_engine::{LlmEngine, SamplingParams};

/// A source document referenced in a RAG response.
#[derive(Clone, Serialize)]
pub struct ChatSource {
    pub label: String,
    pub entry_id: String,
    pub entry_title: String,
    pub snippet: String,
}

/// Event emitted during RAG chat streaming. The first event includes sources.
#[derive(Clone, Serialize)]
pub struct ChatResponseEvent {
    pub session_id: String,
    pub token: String,
    pub done: bool,
    pub sources: Option<Vec<ChatSource>>,
}

#[derive(Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[tauri::command]
pub async fn send_chat_message(
    app: tauri::AppHandle,
    db: tauri::State<'_, DbPool>,
    engine: tauri::State<'_, LlmEngine>,
    embedding_model: tauri::State<'_, EmbeddingModel>,
    message: String,
    history: Vec<ChatMessage>,
    session_id: String,
) -> Result<(), String> {
    if !engine.is_ready() {
        return Err("No AI model loaded. Go to Settings to download one.".into());
    }

    let engine = engine.inner().clone();
    let emb_model = embedding_model.inner().clone();
    let db = db.inner().clone();
    let app_handle = app.clone();
    let sid = session_id.clone();

    tokio::task::spawn_blocking(move || {
        use lunk_core::rag;

        // Convert history
        let rag_history: Vec<rag::ChatMessage> = history
            .iter()
            .map(|m| rag::ChatMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();

        // Retrieve context
        let template = lunk_core::llm_titles::active_chat_template(&engine);
        let rag_ctx = with_db(&db, |conn| {
            rag::prepare_rag_context(
                conn,
                &emb_model,
                &rag_history,
                &message,
                template.unwrap_or(lunk_core::llm_catalog::ChatTemplate::ChatML),
            )
        })
        .map_err(|e| format!("retrieval failed: {e}"))?;

        // Emit sources as first event
        let sources: Vec<ChatSource> = rag_ctx
            .sources
            .iter()
            .map(|s| ChatSource {
                label: s.source_label.clone(),
                entry_id: s.entry_id.to_string(),
                entry_title: s.entry_title.clone(),
                snippet: s.chunk_text.chars().take(200).collect(),
            })
            .collect();

        let _ = app_handle.emit(
            "chat-response",
            ChatResponseEvent {
                session_id: sid.clone(),
                token: String::new(),
                done: false,
                sources: Some(sources),
            },
        );

        // Stream LLM response
        let params = SamplingParams {
            temperature: 0.3,
            top_p: 0.9,
            max_tokens: 512,
        };

        let sid2 = sid.clone();
        let app2 = app_handle.clone();
        engine
            .stream_complete(&rag_ctx.prompt, &params, Some(4096), move |token| {
                let _ = app2.emit(
                    "chat-response",
                    ChatResponseEvent {
                        session_id: sid2.clone(),
                        token: token.to_string(),
                        done: false,
                        sources: None,
                    },
                );
            })
            .map_err(|e| format!("generation failed: {e}"))?;

        // Final event
        let _ = app_handle.emit(
            "chat-response",
            ChatResponseEvent {
                session_id: sid,
                token: String::new(),
                done: true,
                sources: None,
            },
        );

        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("task failed: {e}"))?
}
