use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use reclip_shared::{CreateJobRequest, Job, JobList, Scope};

use super::{ApiJson, HttpError, validate_url};
use crate::access::caller::Caller;
use crate::app::AppState;
use crate::jobs::ManagerError;

pub async fn create(
    State(s): State<AppState>,
    caller: Caller,
    ApiJson(mut req): ApiJson<CreateJobRequest>,
) -> Result<(StatusCode, Json<Job>), HttpError> {
    caller.require(Scope::JobsCreate)?;
    req.url = validate_url(&req.url)?;
    let job = s.jobs.create(&caller.owner, req).await?;
    Ok((StatusCode::ACCEPTED, Json(job)))
}

pub async fn list(State(s): State<AppState>, caller: Caller) -> Result<Json<JobList>, HttpError> {
    caller.require(Scope::JobsRead)?;
    let jobs = s.jobs.list(&caller.owner).await?;
    Ok(Json(JobList { jobs }))
}

pub async fn get_job(
    State(s): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<Json<Job>, ManagerError> {
    caller.require(Scope::JobsRead)?;
    Ok(Json(s.jobs.get(&caller.owner, &id).await?))
}

pub async fn cancel(
    State(s): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<Json<Job>, ManagerError> {
    caller.require(Scope::JobsCancel)?;
    Ok(Json(s.jobs.cancel(&caller.owner, &id).await?))
}

pub async fn delete_job(
    State(s): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<StatusCode, ManagerError> {
    caller.require(Scope::JobsCancel)?;
    s.jobs.delete(&caller.owner, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}
