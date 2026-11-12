//! Per-caller quotas applied by the probe and job-creation routes.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::OwnedMutexGuard;

use super::caller::Caller;
use super::limits::{Bucket, Limited, RateInfo};
use crate::app::AppState;
use crate::http::internal;
use crate::service::ServiceError;

/// Serializes job admission per quota key, so the active-jobs count and the
/// insertion that follows it cannot interleave between concurrent requests.
#[derive(Clone, Default)]
pub struct AdmissionLocks(Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>);

/// Locks kept before idle ones are dropped.
const ADMISSION_PRUNE_AT: usize = 1024;

impl AdmissionLocks {
    /// Waits for this quota key's admission lock; hold the guard until the
    /// job is inserted.
    pub async fn lock(&self, key: &str) -> OwnedMutexGuard<()> {
        let lock = {
            let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
            if map.len() >= ADMISSION_PRUNE_AT {
                map.retain(|_, l| Arc::strong_count(l) > 1);
            }
            map.entry(key.to_string()).or_default().clone()
        };
        lock.lock_owned().await
    }
}

/// The caller's admission guard when an active-jobs cap applies to it: hold
/// it from [`check_create`] until the job is inserted or requeued.
pub async fn admission(s: &AppState, c: &Caller) -> Option<OwnedMutexGuard<()>> {
    match c.limits {
        Some(l) if l.max_active > 0 => Some(s.admission.lock(&c.quota_key).await),
        _ => None,
    }
}

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
/// limit. A refusal by the cap does not consume an hourly slot. Call it while
/// holding the caller's [`AdmissionLocks`] guard and keep the guard until the
/// job is inserted, or concurrent creations can exceed the cap.
pub async fn check_create(s: &AppState, c: &Caller) -> Result<Option<RateInfo>, ServiceError> {
    let Some(limits) = c.limits else {
        return Ok(None);
    };
    if limits.max_active > 0 {
        let active = s
            .jobs
            .store()
            .count_active(&c.quota_key)
            .await
            .map_err(internal)?;
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
            .into());
        }
    }
    s.limiter
        .check(Bucket::Create, &c.quota_key, limits.jobs_per_hour)
        .map_err(Into::into)
}
