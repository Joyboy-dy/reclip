use axum::Json;
use axum::extract::State;
use reclip_shared::PublicConfig;

use crate::app::AppState;

pub async fn get_config(State(s): State<AppState>) -> Json<PublicConfig> {
    Json(PublicConfig {
        version: env!("CARGO_PKG_VERSION").to_string(),
        telegram_bot_username: s.cfg.telegram_bot_username.clone(),
        playlist_limit: s.cfg.playlist_limit,
    })
}
