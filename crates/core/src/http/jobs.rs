use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use reclip_shared::{CreateJobRequest, Job};

use super::{ApiJson, HttpError, internal, not_found, validate_url};
use crate::app::AppState;
use crate::jobs::spawn_job;

pub async fn create(
    State(s): State<AppState>,
    ApiJson(mut req): ApiJson<CreateJobRequest>,
) -> Result<(StatusCode, Json<Job>), HttpError> {
    req.url = validate_url(&req.url)?;
    let job = spawn_job(s, req).await?;
    Ok((StatusCode::ACCEPTED, Json(job)))
}

pub async fn get_job(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    match s.store.get(&id).await {
        Ok(Some(job)) => Json(job).into_response(),
        Ok(None) => not_found("Job not found"),
        Err(e) => internal(e).into_response(),
    }
}
