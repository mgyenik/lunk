mod chat_commands;
mod commands;
mod llm_commands;

use std::sync::Arc;
use std::time::Duration;

use lunk_core::config::{self, Config};
use lunk_core::db;
use lunk_core::transport::SyncNode;
use lunk_server::state::AppState;
use tauri::Manager;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tokio::sync::OnceCell;

pub(crate) type SyncNodeCell = Arc<OnceCell<Arc<SyncNode>>>;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,lunk=debug".parse().unwrap()),
        )
        .init();

    let profile = config::active_profile();
    let config = Config::load().expect("failed to load config");
    let db_path = Config::db_path().expect("failed to resolve db path");

    tracing::info!("profile: {profile}");
    tracing::info!("database: {}", db_path.display());

    let wrapped_db = db::open_db(&db_path).expect("failed to open database");
    let pool = db::create_pool(wrapped_db);

    let server_pool = pool.clone();
    let sync_pool = pool.clone();
    let sync_enabled = config.sync.enabled;
    let sync_interval = config.sync.interval_secs;
    let server_bind = format!("{}:{}", config.server.bind, config.server.port);

    // Shared cell for the sync node (filled asynchronously after startup)
    let sync_cell: SyncNodeCell = Arc::new(OnceCell::new());

    // Initialize embedding model from bundled resource files.
    // The model is downloaded at build time (build.rs) and bundled via tauri.conf.json.
    // In dev mode: files are at crates/lunk-app/models/all-MiniLM-L6-v2/
    // In production: bundled in the app's resource directory.
    let embedding_model = {
        let exe_dir = std::env::current_exe()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()).ok_or(std::io::Error::other("no parent")))
            .unwrap_or_default();

        // Candidates for the model directory (production bundle + dev mode)
        let candidates = [
            exe_dir.join("models"),                              // Linux/Windows production
            exe_dir.join("../Resources/models"),                 // macOS .app bundle
            exe_dir.join("../lib/lunk-app/models"),              // Linux AppImage/deb
            std::path::PathBuf::from("models/all-MiniLM-L6-v2"),// dev mode (cwd = crates/lunk-app)
        ];

        let model_dir = candidates.iter()
            .find(|p| p.join("model_quantized.onnx").exists())
            .cloned()
            .unwrap_or_else(|| {
                tracing::warn!("bundled model not found, falling back to download");
                std::path::PathBuf::from(".fastembed_cache") // trigger download fallback
            });

        if model_dir.join("model_quantized.onnx").exists() {
            tracing::info!("loading embedding model from {}", model_dir.display());
            lunk_core::embeddings::EmbeddingModel::from_dir(&model_dir)
                .expect("failed to load bundled embedding model")
        } else {
            tracing::info!("downloading embedding model (first run)");
            lunk_core::embeddings::EmbeddingModel::new(None)
                .expect("failed to load embedding model")
        }
    };
    let server_model = embedding_model.clone();
    let backfill_model = embedding_model.clone();
    let backfill_pool = pool.clone();
    tracing::info!("embedding model ready");

    // Initialize LLM engine (llama.cpp backend)
    let llm_engine = lunk_core::llm_engine::LlmEngine::new()
        .expect("failed to initialize LLM backend");

    // Auto-load the configured active model if it exists on disk
    if !config.llm.active_model.is_empty()
        && let Some(entry) = lunk_core::llm_catalog::get_catalog_entry(&config.llm.active_model)
        && let Ok(path) = lunk_core::llm_models::model_path(entry)
        && path.exists()
    {
        match llm_engine.load_model(&path, &config.llm.active_model) {
            Ok(()) => tracing::info!("LLM model loaded: {}", config.llm.active_model),
            Err(e) => tracing::warn!("failed to load LLM model: {e}"),
        }
    }
    let server_llm = llm_engine.clone();

    // Wrap config in Mutex for mutable access from settings commands
    let config_state = std::sync::Mutex::new(config);

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(pool)
        .manage(embedding_model)
        .manage(llm_engine)
        .manage(config_state)
        .manage(sync_cell.clone())
        .invoke_handler(tauri::generate_handler![
            commands::search_entries,
            commands::list_entries,
            commands::get_entry,
            commands::get_entry_content,
            commands::update_entry_tags,
            commands::get_tag_suggestions,
            commands::delete_entry,
            commands::get_tags,
            commands::import_pdf,
            commands::get_sync_status,
            commands::get_sync_peers,
            commands::add_sync_peer,
            commands::remove_sync_peer,
            commands::trigger_sync,
            commands::get_topics,
            commands::get_topic_entries,
            commands::get_archive_stats,
            commands::get_similar_entries,
            commands::get_entry_keywords,
            commands::trigger_backfill,
            llm_commands::get_model_catalog,
            llm_commands::get_llm_status,
            llm_commands::download_model,
            llm_commands::delete_model,
            llm_commands::activate_model,
            llm_commands::set_title_generation,
            chat_commands::send_chat_message,
        ])
        .setup(move |app| {
            // System tray
            let show = MenuItem::with_id(app, "show", "Show Lunk", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Lunk")
                .menu(&menu)
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
                        "show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    use tauri::tray::{TrayIconEvent, MouseButton, MouseButtonState};
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                        && let Some(w) = tray.app_handle().get_webview_window("main")
                    {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                })
                .build(app)?;

            // Start sync node + HTTP server in background
            let cell = sync_cell;
            tauri::async_runtime::spawn(async move {
                // Initialize sync node if cr-sqlite loaded and sync enabled
                let sync_node: Option<Arc<SyncNode>> = if sync_enabled {
                    let data_dir = Config::data_dir().expect("failed to get data dir");
                    match SyncNode::new(&data_dir, sync_pool).await {
                        Ok(node) => {
                            let node = Arc::new(node);
                            tracing::info!("sync node ID: {}", node.node_id_string());
                            let _ = cell.set(node.clone());
                            Some(node)
                        }
                        Err(e) => {
                            tracing::error!("failed to start sync node: {e}");
                            None
                        }
                    }
                } else {
                    None
                };

                // Start HTTP API server
                let state = AppState {
                    db: server_pool,
                    embedding_model: server_model,
                    llm_engine: server_llm,
                    sync_node: sync_node.clone(),
                };
                let router = lunk_server::build_router(state);
                let listener = tokio::net::TcpListener::bind(&server_bind)
                    .await
                    .expect("failed to bind HTTP server");
                tracing::info!("HTTP API listening on {server_bind}");
                tokio::spawn(async move {
                    axum::serve(listener, router)
                        .await
                        .expect("HTTP server error");
                });

                // Background: backfill embeddings + keywords for existing entries
                tokio::spawn(async move {
                    use lunk_core::db::with_db;
                    match with_db(&backfill_pool, |conn| {
                        let emb = lunk_core::embeddings::embed_all_missing(conn, &backfill_model)?;
                        let kw = lunk_core::keywords::extract_all_missing(conn)?;
                        let chunks = lunk_core::chunks::chunk_all_missing(conn, &backfill_model)?;
                        Ok((emb, kw, chunks))
                    }) {
                        Ok((emb, kw, chunks)) => {
                            if emb > 0 || kw > 0 || chunks > 0 {
                                tracing::info!("backfill: {emb} embeddings, {kw} keywords, {chunks} chunks");
                            }
                        }
                        Err(e) => tracing::warn!("backfill failed: {e}"),
                    }
                });

                // Background periodic sync
                if let Some(node) = sync_node {
                    let mut timer = tokio::time::interval(Duration::from_secs(sync_interval));
                    timer.tick().await; // skip immediate first tick
                    loop {
                        timer.tick().await;
                        let results = node.sync_all().await;
                        for (peer, result) in &results {
                            match result {
                                Ok(r) => {
                                    if r.sent > 0 || r.received > 0 {
                                        tracing::info!(
                                            "bg sync {}: sent={} received={}",
                                            &peer[..16.min(peer.len())],
                                            r.sent,
                                            r.received
                                        );
                                    }
                                }
                                Err(e) => tracing::warn!(
                                    "bg sync {} failed: {e}",
                                    &peer[..16.min(peer.len())]
                                ),
                            }
                        }
                    }
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error running tauri application");
}
