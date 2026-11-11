use reclip_core::config::Config;
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
    let cfg = Config::from_env(&|k| std::env::var(k).ok());
    tracing::info!(port = cfg.port, data_dir = %cfg.data_dir.display(), "config loaded");
}
