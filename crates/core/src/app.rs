//! Router assembly and shared state.

use std::sync::Arc;

use axum::Router;
use axum::routing::{any, get, post};
use tower_http::services::{ServeDir, ServeFile};

use crate::config::Config;
use crate::http;
use crate::store::Store;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub store: Store,
}

pub fn router(state: AppState) -> Router {
    let index = state.cfg.web_dir.join("index.html");
    let web = ServeDir::new(&state.cfg.web_dir).fallback(ServeFile::new(index));
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/api/v1/config", get(http::config::get_config))
        .route("/api/v1/probe", post(http::probe::probe))
        .route("/api/v1/jobs", post(http::jobs::create))
        .route("/api/v1/jobs/{id}", get(http::jobs::get_job))
        .route("/api/v1/jobs/{id}/file", get(http::files::download))
        .route("/api/{*rest}", any(http::api_not_found))
        .fallback_service(web)
        .with_state(state)
}
