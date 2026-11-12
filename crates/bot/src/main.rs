use reclip_bot::config::Config;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    match Config::from_env(&|k| std::env::var(k).ok()) {
        Ok(cfg) => {
            tracing::info!(?cfg, "configuration loaded");
            eprintln!("reclip-bot: the conversation loop is not implemented yet");
            std::process::exit(2);
        }
        Err(e) => {
            eprintln!("reclip-bot: invalid configuration: {e:#}");
            std::process::exit(1);
        }
    }
}
