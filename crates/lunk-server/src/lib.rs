pub mod auth;
pub mod handlers;
pub mod state;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::state::AppState;

pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api = handlers::api_routes();

    Router::new()
        .nest("/api/v1", api)
        .layer(DefaultBodyLimit::max(50 * 1024 * 1024)) // 50MB — snapshots with inlined images can be large
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
