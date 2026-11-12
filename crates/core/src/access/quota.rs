//! Per-caller quotas applied by the probe and job-creation routes.

use axum::response::{IntoResponse, Response};

use super::caller::Caller;
use super::limits::{Bucket, Limited, RateInfo};
use crate::app::AppState;
use crate::http::internal;

/// Seconds a client is told to wait when the active-jobs cap refuses.
const ACTIVE_RETRY_AFTER: u64 = 30;

/// Counts one probe against the caller's per-minute limit.
/// Unlimited callers get `Ok(None)`: nothing counted, no headers.
pub async fn check_probe(s: &AppState, c: &Caller) -> Result<Option<RateInfo>, Limited> {
    let Some(limits) = c.limits else {
        return Ok(None);
    };
    s.limiter
        .check(Bucket::Probe, &c.quota_key, limits.probes_per_minute)
}

/// Checks the active-jobs cap, then counts one creation against the hourly
/// limit. A refusal by the cap does not consume an hourly slot.
pub async fn check_create(s: &AppState, c: &Caller) -> Result<Option<RateInfo>, Response> {
    let Some(limits) = c.limits else {
        return Ok(None);
    };
    if limits.max_active > 0 {
        let active = s
            .jobs
            .store()
            .count_active(&c.quota_key)
            .await
            .map_err(|e| internal(e).into_response())?;
        if active >= limits.max_active {
            let info = RateInfo {
                limit: limits.max_active,
                remaining: 0,
                reset_secs: ACTIVE_RETRY_AFTER,
            };
            return Err(Limited {
                info,
                retry_after: ACTIVE_RETRY_AFTER,
                message: format!("Too many downloads in progress (max {})", limits.max_active),
            }
            .into_response());
        }
    }
    s.limiter
        .check(Bucket::Create, &c.quota_key, limits.jobs_per_hour)
        .map_err(IntoResponse::into_response)
}
