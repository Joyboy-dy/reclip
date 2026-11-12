//! Router assembly and shared state.

use std::future::Future;
use std::sync::Arc;

use axum::routing::{any, get, post};
use axum::{Router, middleware};
use tower_http::services::{ServeDir, ServeFile};

use crate::config::Config;
use crate::http;
use crate::jobs::JobManager;
use crate::store::Store;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub store: Store,
    pub jobs: JobManager,
}

impl AppState {
    pub fn new(cfg: Arc<Config>, store: Store) -> AppState {
        let jobs = JobManager::new(cfg.clone(), store.clone());
        AppState { cfg, store, jobs }
    }
}

pub fn router(state: AppState) -> Router {
    let index = state.cfg.web_dir.join("index.html");
    let web = ServeDir::new(&state.cfg.web_dir).fallback(ServeFile::new(index));
    let api = Router::new()
        .route("/api/v1/config", get(http::config::get_config))
        .route("/api/v1/probe", post(http::probe::probe))
        .route(
            "/api/v1/jobs",
            get(http::jobs::list).post(http::jobs::create),
        )
        .route(
            "/api/v1/jobs/{id}",
            get(http::jobs::get_job).delete(http::jobs::delete_job),
        )
        .route("/api/v1/jobs/{id}/events", get(http::events::events))
        .route("/api/v1/jobs/{id}/cancel", post(http::jobs::cancel))
        .route("/api/v1/jobs/{id}/file", get(http::files::download))
        .route("/api/{*rest}", any(http::api_not_found))
        .layer(middleware::from_fn(http::identity::identity_layer));
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .merge(api)
        .fallback_service(web)
        .with_state(state)
}

/// Serve `app` until `signal` fires, then shut down in the order that lets
/// the server finish: first the job manager (stops scheduling, stops running
/// downloads, records them interrupted, and ends every open event stream
/// through its shutdown token), then axum's graceful shutdown, which waits
/// for in-flight responses. Without the first step an open event stream
/// would keep the server alive forever.
pub async fn serve(
    listener: tokio::net::TcpListener,
    app: Router,
    jobs: JobManager,
    signal: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let stop = jobs.clone();
    let res = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            signal.await;
            stop.shutdown().await;
        })
        .await;
    // Covers a server error before the signal; a no-op after a normal stop.
    jobs.shutdown().await;
    res
}
