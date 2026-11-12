use axum::Json;
use axum::extract::State;
use reclip_shared::{Transcript, TranscriptRequest};

use super::ApiJson;
use crate::access::caller::Caller;
use crate::access::limits::RateInfo;
use crate::app::AppState;
use crate::transcript::{ServiceError, transcript};

pub async fn get_transcript(
    State(s): State<AppState>,
    caller: Caller,
    ApiJson(req): ApiJson<TranscriptRequest>,
) -> Result<(Option<RateInfo>, Json<Transcript>), ServiceError> {
    let (found, rate) = transcript(&s, &caller, req).await?;
    Ok((rate, Json(found)))
}
