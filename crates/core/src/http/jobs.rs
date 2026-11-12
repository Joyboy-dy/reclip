use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use reclip_shared::{CreateJobRequest, Job, JobList};

use super::identity::Owner;
use super::{ApiJson, HttpError, validate_url};
use crate::app::AppState;
use crate::jobs::ManagerError;

pub async fn create(
    State(s): State<AppState>,
    Owner(owner): Owner,
    ApiJson(mut req): ApiJson<CreateJobRequest>,
) -> Result<(StatusCode, Json<Job>), HttpError> {
    req.url = validate_url(&req.url)?;
    let job = s.jobs.create(&owner, req).await?;
    Ok((StatusCode::ACCEPTED, Json(job)))
}

pub async fn list(
    State(s): State<AppState>,
    Owner(owner): Owner,
) -> Result<Json<JobList>, HttpError> {
    let jobs = s.jobs.list(&owner).await?;
    Ok(Json(JobList { jobs }))
}

pub async fn get_job(
    State(s): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<Job>, ManagerError> {
    Ok(Json(s.jobs.get(&owner, &id).await?))
}

pub async fn cancel(
    State(s): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<Json<Job>, ManagerError> {
    Ok(Json(s.jobs.cancel(&owner, &id).await?))
}

pub async fn delete_job(
    State(s): State<AppState>,
    Owner(owner): Owner,
    Path(id): Path<String>,
) -> Result<StatusCode, ManagerError> {
    s.jobs.delete(&owner, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}
