use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use reclip_shared::{CreateJobRequest, Job, JobList, Scope};

use super::{ApiJson, HttpError, validate_url};
use crate::access::caller::Caller;
use crate::access::limits::RateInfo;
use crate::access::quota::check_create;
use crate::access::ssrf::check_target;
use crate::app::AppState;
use crate::jobs::{ManagerError, not_resumable};

pub async fn create(
    State(s): State<AppState>,
    caller: Caller,
    ApiJson(mut req): ApiJson<CreateJobRequest>,
) -> Result<(StatusCode, Option<RateInfo>, Json<Job>), Response> {
    req.url = validate_url(&req.url).map_err(|e| HttpError(e).into_response())?;
    caller
        .require(Scope::JobsCreate)
        .map_err(IntoResponse::into_response)?;
    check_target(&req.url, &s.resolver)
        .await
        .map_err(|e| HttpError(e).into_response())?;
    let rate = check_create(&s, &caller).await?;
    let job = s
        .jobs
        .create(&caller.owner, &caller.quota_key, req)
        .await
        .map_err(|e| HttpError(e).into_response())?;
    Ok((StatusCode::ACCEPTED, rate, Json(job)))
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

/// Requeue a cancelled or interrupted job (202), under the same checks and
/// quotas as a creation; 409 if it cannot be resumed.
pub async fn resume(
    State(s): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<(StatusCode, Option<RateInfo>, Json<Job>), Response> {
    caller
        .require(Scope::JobsCreate)
        .map_err(IntoResponse::into_response)?;
    let conflict = || (StatusCode::CONFLICT, Json(not_resumable())).into_response();
    let job = s
        .jobs
        .get(&caller.owner, &id)
        .await
        .map_err(IntoResponse::into_response)?;
    if !job.resumable {
        return Err(conflict());
    }
    check_target(&job.url, &s.resolver)
        .await
        .map_err(|e| HttpError(e).into_response())?;
    let rate = check_create(&s, &caller).await?;
    match s.jobs.resume(&caller.owner, &id).await {
        Ok(Some(job)) => Ok((StatusCode::ACCEPTED, rate, Json(job))),
        Ok(None) => Err(conflict()),
        Err(e) => Err(e.into_response()),
    }
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
