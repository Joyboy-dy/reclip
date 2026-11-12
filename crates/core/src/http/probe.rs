use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use reclip_shared::{ProbeRequest, ProbeResponse, Scope};

use super::{ApiJson, validate_url};
use crate::access::caller::Caller;
use crate::access::limits::RateInfo;
use crate::access::quota::check_probe;
use crate::access::ssrf::check_target;
use crate::app::AppState;
use crate::ytdlp::runner;

pub async fn probe(
    State(s): State<AppState>,
    caller: Caller,
    ApiJson(req): ApiJson<ProbeRequest>,
) -> Result<(Option<RateInfo>, Json<ProbeResponse>), Response> {
    let url = validate_url(&req.url).map_err(|e| super::HttpError(e).into_response())?;
    caller
        .require(Scope::Probe)
        .map_err(IntoResponse::into_response)?;
    check_target(&url, &s.resolver)
        .await
        .map_err(|e| super::HttpError(e).into_response())?;
    let rate = check_probe(&s, &caller)
        .await
        .map_err(IntoResponse::into_response)?;
    let probed = runner::probe(&s.cfg, &url)
        .await
        .map_err(|e| super::HttpError(e).into_response())?;
    Ok((rate, Json(probed)))
}
