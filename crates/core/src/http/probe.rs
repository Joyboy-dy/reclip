use axum::Json;
use axum::extract::State;
use reclip_shared::{ProbeRequest, ProbeResponse, Scope};

use super::{ApiJson, HttpError, validate_url};
use crate::access::caller::Caller;
use crate::app::AppState;
use crate::ytdlp::runner;

pub async fn probe(
    State(s): State<AppState>,
    caller: Caller,
    ApiJson(req): ApiJson<ProbeRequest>,
) -> Result<Json<ProbeResponse>, HttpError> {
    caller.require(Scope::Probe)?;
    let url = validate_url(&req.url)?;
    Ok(Json(runner::probe(&s.cfg, &url).await?))
}
