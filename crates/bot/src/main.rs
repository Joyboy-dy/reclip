use std::time::Duration;

use reclip_bot::app::{App, Settings};
use reclip_bot::config::Config;
use reclip_bot::core_client::HttpCore;
use reclip_bot::state::StateStore;
use reclip_bot::telegram::HttpTelegram;

/// How long running deliveries get to finish after SIGTERM / Ctrl-C.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(8);

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let cfg = match Config::from_env(&|k| std::env::var(k).ok()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("reclip-bot: invalid configuration: {e:#}");
            std::process::exit(1);
        }
    };
    // Debug hides the token and the key.
    tracing::info!(?cfg, "configuration loaded");
    let tg = HttpTelegram::new(&cfg.api_url, &cfg.token).unwrap_or_else(|e| fail("telegram", &e));
    let core = HttpCore::new(&cfg.core_url, &cfg.key).unwrap_or_else(|e| fail("core", &e));
    let state = match StateStore::load(&cfg.state_path) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %format!("{e:#}"), "unreadable state, starting empty");
            StateStore::empty(&cfg.state_path)
        }
    };
    let tmp_dir = std::env::temp_dir().join("reclip-bot");
    // Leftovers of a crash: tracked jobs are downloaded again.
    let _ = std::fs::remove_dir_all(&tmp_dir);
    let app = App::new(tg, core, Settings::from_config(&cfg, tmp_dir), state);
    let resumed = app.resume();
    tracing::info!(resumed, "bot started");

    let token = app.shutdown.clone();
    tokio::spawn(async move {
        wait_for_signal().await;
        tracing::info!("shutdown requested");
        token.cancel();
    });
    app.clone().run().await;
    app.tasks.close();
    if tokio::time::timeout(SHUTDOWN_GRACE, app.tasks.wait())
        .await
        .is_err()
    {
        tracing::warn!("some tasks were still running at shutdown");
    }
}

fn fail(what: &str, e: &dyn std::fmt::Display) -> ! {
    eprintln!("reclip-bot: cannot create the {what} client: {e}");
    std::process::exit(1);
}

async fn wait_for_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = term.recv() => {}
                    _ = tokio::signal::ctrl_c() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
