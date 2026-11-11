use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use reclip_shared::{CreateJobRequest, Job, JobList};

use super::identity::Owner;
use super::{ApiJson, HttpError, internal, not_found, validate_url};
use crate::app::AppState;
use crate::jobs::spawn_job;

pub async fn create(
    State(s): State<AppState>,
    Owner(owner): Owner,
    ApiJson(mut req): ApiJson<CreateJobRequest>,
) -> Result<(StatusCode, Json<Job>), HttpError> {
    req.url = validate_url(&req.url)?;
    let job = spawn_job(s, req, &owner).await?;
    Ok((StatusCode::ACCEPTED, Json(job)))
}

pub async fn list(State(s): State<AppState>, Owner(owner): Owner) -> Response {
    match s.store.list_by_owner(&owner, 100).await {
        Ok(jobs) => Json(JobList { jobs }).into_response(),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn get_job(
    State(s): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Response {
    match s.store.get_owned(&id, &owner).await {
        Ok(Some(job)) => Json(job).into_response(),
        Ok(None) => not_found("Job not found"),
        Err(e) => internal(e).into_response(),
    }
}
