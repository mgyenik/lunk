use grymoire_core::db::DbPool;
use grymoire_core::embeddings::EmbeddingModel;
use grymoire_core::llm_engine::LlmEngine;
use grymoire_core::transport::SyncNode;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,
    pub embedding_model: EmbeddingModel,
    pub llm_engine: LlmEngine,
    pub sync_node: Option<Arc<SyncNode>>,
}
