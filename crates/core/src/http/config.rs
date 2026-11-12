use axum::Json;
use axum::extract::State;
use reclip_shared::{PublicConfig, PublicLimits};

use crate::app::AppState;

pub async fn get_config(State(s): State<AppState>) -> Json<PublicConfig> {
    let web = s.cfg.limits.web;
    Json(PublicConfig {
        version: env!("CARGO_PKG_VERSION").to_string(),
        telegram_bot_username: s.cfg.telegram_bot_username.clone(),
        playlist_limit: s.cfg.playlist_limit,
        public_web: s.cfg.public_web,
        limits: PublicLimits {
            probes_per_minute: web.probes_per_minute,
            jobs_per_hour: web.jobs_per_hour,
            max_active: web.max_active,
        },
        max_duration: s.cfg.max_duration,
        file_ttl_secs: s.cfg.file_ttl.as_secs(),
        history_days: s.cfg.history_days,
    })
}
