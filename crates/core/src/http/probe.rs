use axum::Json;
use axum::extract::State;
use reclip_shared::{ProbeRequest, ProbeResponse};

use super::ApiJson;
use crate::access::caller::Caller;
use crate::access::limits::RateInfo;
use crate::app::AppState;
use crate::service::{self, ServiceError};

pub async fn probe(
    State(s): State<AppState>,
    caller: Caller,
    ApiJson(req): ApiJson<ProbeRequest>,
) -> Result<(Option<RateInfo>, Json<ProbeResponse>), ServiceError> {
    let (probed, rate) = service::probe(&s, &caller, req).await?;
    Ok((rate, Json(probed)))
}
