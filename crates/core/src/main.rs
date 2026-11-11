use std::sync::Arc;

use anyhow::Context;
use reclip_core::app::{AppState, router};
use reclip_core::config::Config;
use reclip_core::store::Store;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let cfg = Config::from_env(&|k| std::env::var(k).ok());
    std::fs::create_dir_all(&cfg.data_dir).context("create data dir")?;
    std::fs::create_dir_all(&cfg.downloads_dir).context("create downloads dir")?;
    let store = Store::open(&cfg.data_dir.join("reclip.db")).await?;
    let addr = format!("{}:{}", cfg.host, cfg.port);
    let state = AppState {
        cfg: Arc::new(cfg),
        store,
    };
    let app = router(state).layer(TraceLayer::new_for_http());
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    tracing::info!(%addr, "listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!("shut down");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
}
