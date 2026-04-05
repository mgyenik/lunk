use std::sync::Mutex;

use serde::Serialize;
use tauri::Emitter;

use lunk_core::config::Config;
use lunk_core::llm_catalog;
use lunk_core::llm_engine::LlmEngine;
use lunk_core::llm_models;

pub type ConfigState = Mutex<Config>;

// --- Response types ---

#[derive(Serialize)]
pub struct CatalogModelResponse {
    pub id: String,
    pub name: String,
    pub description: String,
    pub param_label: String,
    pub quant_label: String,
    pub size_bytes: u64,
    pub size_display: String,
    pub context_size: u32,
    pub recommended: bool,
    pub min_ram_mb: u32,
    pub downloaded: bool,
}

#[derive(Serialize)]
pub struct LlmStatusResponse {
    pub active_model: Option<String>,
    pub model_loaded: bool,
    pub title_generation_enabled: bool,
}

#[derive(Clone, Serialize)]
pub struct DownloadProgressEvent {
    pub model_id: String,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub phase: String,
    pub error: Option<String>,
}

// --- Commands ---

#[tauri::command]
pub fn get_model_catalog() -> Result<Vec<CatalogModelResponse>, String> {
    llm_catalog::CATALOG
        .iter()
        .map(|entry| {
            let downloaded = llm_models::is_model_downloaded(entry).unwrap_or(false);
            Ok(CatalogModelResponse {
                id: entry.id.to_string(),
                name: entry.name.to_string(),
                description: entry.description.to_string(),
                param_label: entry.param_label.to_string(),
                quant_label: entry.quant_label.to_string(),
                size_bytes: entry.size_bytes,
                size_display: llm_catalog::format_size(entry.size_bytes),
                context_size: entry.context_size,
                recommended: entry.recommended,
                min_ram_mb: entry.min_ram_mb,
                downloaded,
            })
        })
        .collect()
}

#[tauri::command]
pub fn get_llm_status(
    engine: tauri::State<'_, LlmEngine>,
    config: tauri::State<'_, ConfigState>,
) -> Result<LlmStatusResponse, String> {
    let cfg = config.lock().map_err(|e| format!("config lock: {e}"))?;
    Ok(LlmStatusResponse {
        active_model: engine.active_model_id(),
        model_loaded: engine.is_ready(),
        title_generation_enabled: cfg.llm.title_generation,
    })
}

#[tauri::command]
pub async fn download_model(
    app: tauri::AppHandle,
    model_id: String,
) -> Result<(), String> {
    let entry = llm_catalog::get_catalog_entry(&model_id)
        .ok_or_else(|| format!("unknown model: {model_id}"))?;

    let client = reqwest::Client::new();
    let app_clone = app.clone();
    let mid = model_id.clone();

    let emit_progress = move |downloaded: u64, total: u64| {
        let _ = app_clone.emit(
            "llm-download-progress",
            DownloadProgressEvent {
                model_id: mid.clone(),
                bytes_downloaded: downloaded,
                total_bytes: total,
                phase: "downloading".to_string(),
                error: None,
            },
        );
    };

    llm_models::download_model(&client, entry, emit_progress)
        .await
        .map_err(|e| e.to_string())?;

    let _ = app.emit(
        "llm-download-progress",
        DownloadProgressEvent {
            model_id,
            bytes_downloaded: entry.size_bytes,
            total_bytes: entry.size_bytes,
            phase: "complete".to_string(),
            error: None,
        },
    );

    Ok(())
}

#[tauri::command]
pub fn delete_model(
    model_id: String,
    engine: tauri::State<'_, LlmEngine>,
) -> Result<(), String> {
    let entry = llm_catalog::get_catalog_entry(&model_id)
        .ok_or_else(|| format!("unknown model: {model_id}"))?;

    // Unload if this is the active model
    if engine.active_model_id().as_deref() == Some(&model_id) {
        engine.unload_model().map_err(|e| e.to_string())?;
    }

    llm_models::delete_model(entry).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn activate_model(
    model_id: String,
    engine: tauri::State<'_, LlmEngine>,
    config: tauri::State<'_, ConfigState>,
) -> Result<(), String> {
    let entry = llm_catalog::get_catalog_entry(&model_id)
        .ok_or_else(|| format!("unknown model: {model_id}"))?;

    let path = llm_models::model_path(entry).map_err(|e| e.to_string())?;
    if !path.exists() {
        return Err(format!("model not downloaded: {model_id}"));
    }

    engine
        .load_model(&path, &model_id)
        .map_err(|e| e.to_string())?;

    // Persist to config
    let mut cfg = config.lock().map_err(|e| format!("config lock: {e}"))?;
    cfg.llm.active_model = model_id;
    cfg.save().map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub fn set_title_generation(
    enabled: bool,
    config: tauri::State<'_, ConfigState>,
) -> Result<(), String> {
    let mut cfg = config.lock().map_err(|e| format!("config lock: {e}"))?;
    cfg.llm.title_generation = enabled;
    cfg.save().map_err(|e| e.to_string())?;
    Ok(())
}
