//! Router assembly and shared state.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::routing::{any, get, post};
use axum::{Router, middleware};
use tower_http::services::{ServeDir, ServeFile};

use crate::access::caller::caller_layer;
use crate::access::key_usage::KeyUsage;
use crate::access::limits::Limiter;
use crate::access::secrets::Site;
use crate::access::ssrf::{Resolver, system_resolver};
use crate::config::Config;
use crate::http;
use crate::jobs::JobManager;
use crate::store::Store;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub store: Store,
    pub jobs: JobManager,
    pub limiter: Limiter,
    /// Coalesces the `last_used_at` / `request_count` writes of API keys.
    pub key_usage: KeyUsage,
    /// Admin token, link key and public URL.
    pub site: Site,
    /// DNS resolver used by the SSRF guard; tests swap in a static one.
    pub resolver: Resolver,
}

impl AppState {
    pub fn new(cfg: Arc<Config>, store: Store, site: Site) -> AppState {
        let jobs = JobManager::new(cfg.clone(), store.clone());
        AppState {
            cfg,
            store,
            jobs,
            limiter: Limiter::new(),
            key_usage: KeyUsage::default(),
            site,
            resolver: system_resolver(),
        }
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
        .route("/api/v1/jobs/{id}/link", post(http::links::create))
        .route("/api/{*rest}", any(http::api_not_found))
        .layer(middleware::from_fn_with_state(state.clone(), caller_layer));
    // Admin routes carry their own token check, outside `caller_layer`.
    let app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/d/{token}", get(http::links::redeem))
        .merge(http::admin::routes(state.clone()))
        .merge(api)
        .fallback_service(web)
        .with_state(state);
    http::headers::with_security_headers(app)
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
    let app = app.into_make_service_with_connect_info::<SocketAddr>();
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
