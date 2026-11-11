use std::sync::Arc;

use anyhow::Context;
use reclip_core::app::{AppState, router};
use reclip_core::config::Config;
use reclip_core::healthcheck;
use reclip_core::store::Store;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if is_healthcheck(&args) {
        // Used by the Docker HEALTHCHECK: the image has no curl.
        let cfg = Config::from_env(&|k| std::env::var(k).ok());
        let ok = healthcheck::probe(cfg.port, healthcheck::TIMEOUT).await;
        std::process::exit(if ok { 0 } else { 1 });
    }
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
    let state = AppState::new(Arc::new(cfg), store);
    let jobs = state.jobs.clone();
    let app = router(state).layer(
        TraceLayer::new_for_http()
            .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
            .on_response(DefaultOnResponse::new().level(Level::INFO)),
    );
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    tracing::info!(%addr, "listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    // Stop running downloads through their tokens (never by dropping them).
    jobs.shutdown().await;
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

fn is_healthcheck(args: &[String]) -> bool {
    args.first().is_some_and(|a| a == "healthcheck")
}

#[cfg(test)]
mod tests {
    use super::is_healthcheck;

    #[test]
    fn dispatches_healthcheck_subcommand() {
        assert!(is_healthcheck(&["healthcheck".to_string()]));
        assert!(!is_healthcheck(&[]));
        assert!(!is_healthcheck(&["serve".to_string()]));
    }
}
