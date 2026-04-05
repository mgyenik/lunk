//! LLM inference engine wrapping llama-cpp-2.
//!
//! `LlmEngine` is initialized once at app startup (initializes the llama.cpp
//! backend) and supports hot-swapping models at runtime. Clone is cheap (Arc).
//! Each inference call creates a fresh `LlamaContext` on the calling thread.

use std::num::NonZeroU32;
use std::path::Path;
use std::pin::pin;
use std::sync::{Arc, Mutex, Once, RwLock};

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;

use crate::errors::{GrymoireError, Result};

/// Global backend singleton. `LlamaBackend::init()` must only be called once
/// per process. We use Once + Mutex to guarantee single initialization even
/// across parallel test threads.
static BACKEND_INIT: Once = Once::new();
static BACKEND: Mutex<Option<LlamaBackend>> = Mutex::new(None);

fn get_backend() -> Result<&'static LlamaBackend> {
    BACKEND_INIT.call_once(|| {
        if let Ok(backend) = LlamaBackend::init() {
            *BACKEND.lock().unwrap() = Some(backend);
        }
    });
    // Safety: after call_once, BACKEND is initialized and never mutated again.
    // We leak a reference to avoid holding the mutex during inference.
    let guard = BACKEND.lock().map_err(|e| GrymoireError::Llm(format!("lock: {e}")))?;
    if guard.is_some() {
        // This is safe because the value is never removed after init
        drop(guard);
        let ptr = BACKEND.lock().unwrap();
        let backend_ref: &LlamaBackend = ptr.as_ref().unwrap();
        // Extend lifetime — safe because the static Mutex keeps the value alive forever
        let backend_ref: &'static LlamaBackend = unsafe { &*(backend_ref as *const LlamaBackend) };
        Ok(backend_ref)
    } else {
        Err(GrymoireError::Llm("backend init failed".into()))
    }
}

/// Thread-safe LLM engine. Clone is cheap (Arc).
///
/// The engine wraps an optional loaded model that can be swapped at runtime
/// via `load_model`/`unload_model`. The llama.cpp backend is process-global.
#[derive(Clone)]
pub struct LlmEngine {
    inner: Arc<LlmEngineInner>,
}

struct LlmEngineInner {
    model: RwLock<Option<LoadedModel>>,
}

// Safety: The RwLock protects the model. LlamaModel itself is Send+Sync
// in llama-cpp-2 (it wraps a thread-safe C struct).
unsafe impl Send for LlmEngineInner {}
unsafe impl Sync for LlmEngineInner {}

struct LoadedModel {
    model: LlamaModel,
    catalog_id: String,
}

impl LlmEngine {
    /// Initialize the LLM engine. Does NOT load a model — call `load_model` after.
    /// Safe to call multiple times (the llama.cpp backend is initialized once globally).
    pub fn new() -> Result<Self> {
        // Ensure backend is initialized
        get_backend()?;

        Ok(Self {
            inner: Arc::new(LlmEngineInner {
                model: RwLock::new(None),
            }),
        })
    }

    /// Load a GGUF model from disk. Replaces any previously loaded model.
    pub fn load_model(&self, path: &Path, catalog_id: &str) -> Result<()> {
        let params = LlamaModelParams::default();
        let params = pin!(params);

        let backend = get_backend()?;
        let model = LlamaModel::load_from_file(backend, path, &params)
            .map_err(|e| GrymoireError::Llm(format!("model load: {e}")))?;

        let mut guard = self
            .inner
            .model
            .write()
            .map_err(|e| GrymoireError::Llm(format!("lock poisoned: {e}")))?;
        *guard = Some(LoadedModel {
            model,
            catalog_id: catalog_id.to_string(),
        });

        tracing::info!("LLM model loaded: {catalog_id} from {}", path.display());
        Ok(())
    }

    /// Unload the current model, freeing memory.
    pub fn unload_model(&self) -> Result<()> {
        let mut guard = self
            .inner
            .model
            .write()
            .map_err(|e| GrymoireError::Llm(format!("lock poisoned: {e}")))?;
        if let Some(loaded) = guard.take() {
            tracing::info!("LLM model unloaded: {}", loaded.catalog_id);
        }
        Ok(())
    }

    /// Returns the catalog ID of the currently loaded model, if any.
    pub fn active_model_id(&self) -> Option<String> {
        self.inner
            .model
            .read()
            .ok()
            .and_then(|g| g.as_ref().map(|m| m.catalog_id.clone()))
    }

    /// Check if a model is loaded and ready for inference.
    pub fn is_ready(&self) -> bool {
        self.inner
            .model
            .read()
            .map(|g| g.is_some())
            .unwrap_or(false)
    }

    /// Generate a completion for the given prompt (greedy, no streaming).
    /// Convenience wrapper around `stream_complete`.
    pub fn complete(&self, prompt: &str, max_tokens: u32) -> Result<String> {
        self.stream_complete(
            prompt,
            &SamplingParams {
                max_tokens,
                ..SamplingParams::default()
            },
            None,
            |_| {},
        )
    }

    /// Generate a completion with streaming and configurable sampling.
    ///
    /// Calls `on_token` for each generated token piece. Returns the full
    /// generated text. Creates a fresh context per call.
    ///
    /// `n_ctx` overrides the context window size (default: prompt + max_tokens + 64,
    /// capped at 4096). Pass a larger value for RAG prompts.
    pub fn stream_complete<F>(
        &self,
        prompt: &str,
        params: &SamplingParams,
        n_ctx: Option<u32>,
        mut on_token: F,
    ) -> Result<String>
    where
        F: FnMut(&str),
    {
        let guard = self
            .inner
            .model
            .read()
            .map_err(|e| GrymoireError::Llm(format!("lock poisoned: {e}")))?;
        let loaded = guard
            .as_ref()
            .ok_or_else(|| GrymoireError::Llm("no model loaded".into()))?;

        let model = &loaded.model;

        // Tokenize prompt
        let tokens = model
            .str_to_token(prompt, AddBos::Always)
            .map_err(|e| GrymoireError::Llm(format!("tokenize: {e}")))?;

        if tokens.is_empty() {
            return Ok(String::new());
        }

        // Create context
        let default_ctx = (tokens.len() as u32 + params.max_tokens + 64).min(4096);
        let ctx_size = n_ctx.unwrap_or(default_ctx);
        let backend = get_backend()?;
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(ctx_size));

        let mut ctx = model
            .new_context(backend, ctx_params)
            .map_err(|e| GrymoireError::Llm(format!("context: {e}")))?;

        // Fill batch with prompt tokens
        let batch_size = tokens.len().max(512);
        let mut batch = LlamaBatch::new(batch_size, 1);
        let last_idx = (tokens.len() - 1) as i32;
        for (i, &token) in tokens.iter().enumerate() {
            batch
                .add(token, i as i32, &[0], i as i32 == last_idx)
                .map_err(|e| GrymoireError::Llm(format!("batch add: {e}")))?;
        }

        // Decode prompt
        ctx.decode(&mut batch)
            .map_err(|e| GrymoireError::Llm(format!("decode prompt: {e}")))?;

        // Build sampler chain based on temperature
        let mut sampler = if params.temperature > 0.0 {
            LlamaSampler::chain_simple([
                LlamaSampler::top_p(params.top_p, 1),
                LlamaSampler::temp(params.temperature),
                LlamaSampler::dist(42),
            ])
        } else {
            LlamaSampler::chain_simple([LlamaSampler::greedy()])
        };

        sampler.accept_many(&tokens);

        // Generate tokens
        let mut output = String::new();
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut n_cur = tokens.len() as i32;

        for _ in 0..params.max_tokens {
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            sampler.accept(token);

            if model.is_eog_token(token) {
                break;
            }

            // Detokenize
            match model.token_to_piece(token, &mut decoder, false, None) {
                Ok(piece) => {
                    on_token(&piece);
                    output.push_str(&piece);
                }
                Err(e) => {
                    tracing::warn!("token decode error: {e}");
                    break;
                }
            }

            // Prepare next batch
            batch.clear();
            batch
                .add(token, n_cur, &[0], true)
                .map_err(|e| GrymoireError::Llm(format!("batch add: {e}")))?;
            n_cur += 1;

            ctx.decode(&mut batch)
                .map_err(|e| GrymoireError::Llm(format!("decode: {e}")))?;
        }

        Ok(output)
    }
}

/// Sampling parameters for LLM generation.
#[derive(Debug, Clone)]
pub struct SamplingParams {
    /// Temperature (0.0 = greedy/deterministic, 0.3-0.5 for RAG).
    pub temperature: f32,
    /// Nucleus sampling threshold (default 0.9).
    pub top_p: f32,
    /// Maximum tokens to generate.
    pub max_tokens: u32,
}

impl Default for SamplingParams {
    fn default() -> Self {
        Self {
            temperature: 0.0,
            top_p: 0.9,
            max_tokens: 256,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_init() {
        let engine = LlmEngine::new().unwrap();
        assert!(!engine.is_ready());
        assert!(engine.active_model_id().is_none());
    }

    #[test]
    fn test_complete_without_model() {
        let engine = LlmEngine::new().unwrap();
        let result = engine.complete("hello", 10);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("no model loaded"),
            "should fail when no model is loaded"
        );
    }

    #[test]
    fn test_unload_noop() {
        let engine = LlmEngine::new().unwrap();
        // Unloading when nothing is loaded should succeed
        engine.unload_model().unwrap();
        assert!(!engine.is_ready());
    }

    #[test]
    fn test_clone_shares_state() {
        let engine = LlmEngine::new().unwrap();
        let engine2 = engine.clone();
        assert!(!engine.is_ready());
        assert!(!engine2.is_ready());
        // Both point to the same inner state
        assert!(Arc::ptr_eq(&engine.inner, &engine2.inner));
    }
}
