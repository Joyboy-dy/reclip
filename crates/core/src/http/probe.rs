use axum::Json;
use axum::extract::State;
use reclip_shared::{ProbeRequest, ProbeResponse};

use super::{ApiJson, HttpError, validate_url};
use crate::app::AppState;
use crate::ytdlp::runner;

pub async fn probe(
    State(s): State<AppState>,
    ApiJson(req): ApiJson<ProbeRequest>,
) -> Result<Json<ProbeResponse>, HttpError> {
    let url = validate_url(&req.url)?;
    Ok(Json(runner::probe(&s.cfg, &url).await?))
}
