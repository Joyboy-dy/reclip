//! The probe and job services: scope checks, URL checks, quotas, then the
//! work itself. The HTTP routes and the MCP tools both call these, so a
//! caller meets the same rules whichever door it uses.

use axum::response::{IntoResponse, Response};
use reclip_shared::{
    ApiError, CreateJobRequest, ErrorCode, Job, ProbeRequest, ProbeResponse, Scope,
};

use crate::access::caller::Caller;
use crate::access::limits::{Limited, RateInfo};
use crate::access::quota::{admission, check_create, check_probe};
use crate::access::ssrf::check_target;
use crate::app::AppState;
use crate::http::{HttpError, api_error, validate_url};
use crate::jobs::ManagerError;
use crate::ytdlp::runner;

/// Why a service call was refused.
#[derive(Debug)]
pub enum ServiceError {
    Api(ApiError),
    /// A quota refused the call (429, with `Retry-After`).
    Limited(Limited),
}

impl ServiceError {
    /// The `{code, message}` an API client would get in the response body.
    pub fn api_error(&self) -> ApiError {
        match self {
            ServiceError::Api(e) => e.clone(),
            ServiceError::Limited(l) => api_error(ErrorCode::RateLimited, &l.message),
        }
    }
}

impl From<ApiError> for ServiceError {
    fn from(e: ApiError) -> Self {
        ServiceError::Api(e)
    }
}

impl From<HttpError> for ServiceError {
    fn from(e: HttpError) -> Self {
        ServiceError::Api(e.0)
    }
}

impl From<Limited> for ServiceError {
    fn from(e: Limited) -> Self {
        ServiceError::Limited(e)
    }
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        match self {
            ServiceError::Api(e) => HttpError(e).into_response(),
            ServiceError::Limited(l) => l.into_response(),
        }
    }
}

/// The body an API client gets for a job lookup that failed.
pub fn manager_api_error(e: ManagerError) -> ApiError {
    match e {
        ManagerError::NotFound => api_error(ErrorCode::InvalidRequest, "Job not found"),
        ManagerError::Api(e) => e,
    }
}

/// Media details of `req.url`. Needs the `probe` scope and counts as one
/// probe in the caller's per-minute bucket.
pub async fn probe(
    s: &AppState,
    caller: &Caller,
    req: ProbeRequest,
) -> Result<(ProbeResponse, Option<RateInfo>), ServiceError> {
    let url = validate_url(&req.url)?;
    caller.require(Scope::Probe)?;
    check_target(&url, &s.resolver).await?;
    let rate = check_probe(s, caller).await?;
    let probed = runner::probe(&s.cfg, &url).await?;
    Ok((probed, rate))
}

/// Check the options the request body cannot express by its types alone.
/// A resumed job reuses the stored values, checked here at creation.
fn validate_options(req: &CreateJobRequest) -> Result<(), ApiError> {
    let invalid = |message| Err(api_error(ErrorCode::InvalidRequest, message));
    if let Some(clip) = req.clip {
        if !clip.start.is_finite() || clip.start < 0.0 {
            return invalid("Clip start must be a time of 0 or more");
        }
        if clip
            .end
            .is_some_and(|end| !end.is_finite() || end <= clip.start)
        {
            return invalid("Clip end must come after its start");
        }
    }
    if req.playlist_item == Some(0) {
        return invalid("Playlist item must be 1 or more");
    }
    Ok(())
}

/// Queue a new job. Needs `jobs:create`; counts against the caller's hourly
/// and active-jobs limits.
pub async fn create_job(
    s: &AppState,
    caller: &Caller,
    mut req: CreateJobRequest,
) -> Result<(Job, Option<RateInfo>), ServiceError> {
    req.url = validate_url(&req.url)?;
    validate_options(&req)?;
    caller.require(Scope::JobsCreate)?;
    check_target(&req.url, &s.resolver).await?;
    // The cap check and the insert must not interleave with another create
    // for the same quota key.
    let _admission = admission(s, caller).await;
    let rate = check_create(s, caller).await?;
    let job = s.jobs.create(&caller.owner, &caller.quota_key, req).await?;
    Ok((job, rate))
}

/// The caller's jobs, newest first. Needs `jobs:read`.
pub async fn list_jobs(s: &AppState, caller: &Caller) -> Result<Vec<Job>, ApiError> {
    caller.require(Scope::JobsRead).map_err(|e| e.0)?;
    s.jobs.list(&caller.owner).await
}

/// One of the caller's jobs. Needs `jobs:read`.
pub async fn get_job(s: &AppState, caller: &Caller, id: &str) -> Result<Job, ManagerError> {
    caller.require(Scope::JobsRead)?;
    s.jobs.get(&caller.owner, id).await
}

/// Cancel one of the caller's jobs. Needs `jobs:cancel`.
pub async fn cancel_job(s: &AppState, caller: &Caller, id: &str) -> Result<Job, ManagerError> {
    caller.require(Scope::JobsCancel)?;
    s.jobs.cancel(&caller.owner, id).await
}
