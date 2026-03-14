mod commands;

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

    let conn = db::open_database(&db_path).expect("failed to open database");

    // Load cr-sqlite extension for CRDT sync
    let crsqlite_loaded = db::try_load_crsqlite(&conn, config.sync.crsqlite_ext_path.as_deref());
    if crsqlite_loaded {
        db::register_crrs(&conn).expect("failed to register CRR tables");
    }

    let pool = db::create_pool(conn);

    let server_pool = pool.clone();
    let sync_pool = pool.clone();
    let sync_enabled = crsqlite_loaded && config.sync.enabled;
    let sync_interval = config.sync.interval_secs;
    let server_bind = format!("{}:{}", config.server.bind, config.server.port);

    // Shared cell for the sync node (filled asynchronously after startup)
    let sync_cell: SyncNodeCell = Arc::new(OnceCell::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(pool)
        .manage(sync_cell.clone())
        .invoke_handler(tauri::generate_handler![
            commands::search_entries,
            commands::list_entries,
            commands::get_queue,
            commands::get_entry,
            commands::get_entry_content,
            commands::update_entry_status,
            commands::delete_entry,
            commands::get_tags,
            commands::import_pdf,
            commands::get_sync_status,
            commands::get_sync_peers,
            commands::add_sync_peer,
            commands::remove_sync_peer,
            commands::trigger_sync,
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
                    if !crsqlite_loaded {
                        tracing::info!("cr-sqlite not loaded; P2P sync disabled");
                    }
                    None
                };

                // Start HTTP API server
                let state = AppState {
                    db: server_pool,
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
