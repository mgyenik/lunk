use lunk_core::db::DbPool;
use lunk_core::embeddings::EmbeddingModel;
use lunk_core::llm_engine::LlmEngine;
use lunk_core::transport::SyncNode;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,
    pub embedding_model: EmbeddingModel,
    pub llm_engine: LlmEngine,
    pub sync_node: Option<Arc<SyncNode>>,
}
