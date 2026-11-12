//! In-memory fixed-window rate limiter and its 429 response.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::http::{HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, IntoResponseParts, Response, ResponseParts};
use reclip_shared::ErrorCode;

use crate::http::api_error;

const RATELIMIT_LIMIT: HeaderName = HeaderName::from_static("ratelimit-limit");
const RATELIMIT_REMAINING: HeaderName = HeaderName::from_static("ratelimit-remaining");
const RATELIMIT_RESET: HeaderName = HeaderName::from_static("ratelimit-reset");

/// Every this many checks, expired windows are dropped.
const PRUNE_EVERY: u64 = 1024;

/// Independent counters per kind of operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bucket {
    /// Probing a URL (60 s window).
    Probe,
    /// Creating a job (3600 s window).
    Create,
    /// Failed admin sign-ins (15 min window).
    AdminFail,
}

impl Bucket {
    fn window(self) -> Duration {
        match self {
            Bucket::Probe => Duration::from_secs(60),
            Bucket::Create => Duration::from_secs(3600),
            Bucket::AdminFail => Duration::from_secs(900),
        }
    }
}

/// Rate-limit state exposed to the client on accepted responses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateInfo {
    pub limit: u32,
    pub remaining: u32,
    /// Seconds until the window ends (at least 1).
    pub reset_secs: u64,
}

impl IntoResponseParts for RateInfo {
    type Error = std::convert::Infallible;

    fn into_response_parts(self, mut res: ResponseParts) -> Result<ResponseParts, Self::Error> {
        let h = res.headers_mut();
        h.insert(RATELIMIT_LIMIT, HeaderValue::from(self.limit));
        h.insert(RATELIMIT_REMAINING, HeaderValue::from(self.remaining));
        h.insert(RATELIMIT_RESET, HeaderValue::from(self.reset_secs));
        Ok(res)
    }
}

/// A refused request: renders as 429 with `Retry-After` and `RateLimit-*`.
#[derive(Debug)]
pub struct Limited {
    pub info: RateInfo,
    pub retry_after: u64,
    pub message: String,
}

impl IntoResponse for Limited {
    fn into_response(self) -> Response {
        let body = axum::Json(api_error(ErrorCode::RateLimited, &self.message));
        (
            StatusCode::TOO_MANY_REQUESTS,
            [(header::RETRY_AFTER, HeaderValue::from(self.retry_after))],
            self.info,
            body,
        )
            .into_response()
    }
}

#[derive(Debug)]
struct Window {
    start: Instant,
    count: u32,
}

#[derive(Debug, Default)]
struct Inner {
    windows: HashMap<(Bucket, String), Window>,
    checks: u64,
}

/// Fixed-window limiter keyed by (bucket, key). Cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct Limiter {
    inner: Arc<Mutex<Inner>>,
}

/// Whole seconds left until `end`, rounded up, at least 1.
fn secs_left(end: Instant, now: Instant) -> u64 {
    let left = end.saturating_duration_since(now);
    (left.as_secs() + u64::from(left.subsec_nanos() > 0)).max(1)
}

impl Limiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Counts one request. `limit == 0` means unlimited (`Ok(None)`, not counted).
    pub fn check(
        &self,
        bucket: Bucket,
        key: &str,
        limit: u32,
    ) -> Result<Option<RateInfo>, Limited> {
        self.check_at(bucket, key, limit, Instant::now())
    }

    /// Give back one slot taken by `check` (an attempt that succeeded).
    pub fn release(&self, bucket: Bucket, key: &str) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(w) = inner.windows.get_mut(&(bucket, key.to_string())) {
            w.count = w.count.saturating_sub(1);
        }
    }

    pub fn check_at(
        &self,
        bucket: Bucket,
        key: &str,
        limit: u32,
        now: Instant,
    ) -> Result<Option<RateInfo>, Limited> {
        if limit == 0 {
            return Ok(None);
        }
        let span = bucket.window();
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        inner.checks += 1;
        if inner.checks.is_multiple_of(PRUNE_EVERY) {
            inner.windows.retain(|(b, _), w| now < w.start + b.window());
        }
        let w = inner
            .windows
            .entry((bucket, key.to_string()))
            .or_insert(Window {
                start: now,
                count: 0,
            });
        if now >= w.start + span {
            *w = Window {
                start: now,
                count: 0,
            };
        }
        let reset_secs = secs_left(w.start + span, now);
        if w.count >= limit {
            return Err(Limited {
                info: RateInfo {
                    limit,
                    remaining: 0,
                    reset_secs,
                },
                retry_after: reset_secs,
                message: format!("Too many requests, try again in {reset_secs} s"),
            });
        }
        w.count += 1;
        Ok(Some(RateInfo {
            limit,
            remaining: limit - w.count,
            reset_secs,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::response::IntoResponse;

    #[test]
    fn allows_up_to_limit_then_429() {
        let l = Limiter::new();
        for expected in [2, 1, 0] {
            let info = l.check(Bucket::Probe, "ip", 3).unwrap().unwrap();
            assert_eq!(info.limit, 3);
            assert_eq!(info.remaining, expected);
        }
        let err = l.check(Bucket::Probe, "ip", 3).unwrap_err();
        assert!(err.retry_after > 0);
        assert_eq!(err.info.remaining, 0);
    }

    #[test]
    fn window_resets() {
        let l = Limiter::new();
        let t0 = Instant::now();
        assert!(l.check_at(Bucket::Probe, "ip", 1, t0).is_ok());
        assert!(l.check_at(Bucket::Probe, "ip", 1, t0).is_err());
        let later = t0 + Duration::from_secs(61);
        assert!(l.check_at(Bucket::Probe, "ip", 1, later).is_ok());
    }

    #[test]
    fn keys_are_independent() {
        let l = Limiter::new();
        assert!(l.check(Bucket::Probe, "a", 1).is_ok());
        assert!(l.check(Bucket::Probe, "a", 1).is_err());
        assert!(l.check(Bucket::Probe, "b", 1).is_ok());
        assert!(l.check(Bucket::Create, "a", 1).is_ok());
    }

    #[test]
    fn zero_is_unlimited() {
        let l = Limiter::new();
        for _ in 0..10 {
            assert_eq!(l.check(Bucket::Create, "ip", 0).unwrap(), None);
        }
    }

    #[test]
    fn reset_is_rounded_up_and_bucket_windows_differ() {
        let l = Limiter::new();
        let t0 = Instant::now();
        let info = l.check_at(Bucket::Create, "k", 5, t0).unwrap().unwrap();
        assert_eq!(info.reset_secs, 3600);
        let info = l
            .check_at(Bucket::Create, "k", 5, t0 + Duration::from_millis(500))
            .unwrap()
            .unwrap();
        assert_eq!(info.reset_secs, 3600);
        let err = l
            .check_at(Bucket::Probe, "k", 1, t0)
            .and_then(|_| l.check_at(Bucket::Probe, "k", 1, t0 + Duration::from_millis(59_500)))
            .unwrap_err();
        assert_eq!(err.retry_after, 1);
        assert_eq!(err.message, "Too many requests, try again in 1 s");
    }

    #[test]
    fn expired_windows_are_pruned() {
        let l = Limiter::new();
        let t0 = Instant::now();
        l.check_at(Bucket::Probe, "old", 5, t0).unwrap();
        let later = t0 + Duration::from_secs(120);
        for _ in 0..PRUNE_EVERY {
            l.check_at(Bucket::Probe, "new", 0, later).unwrap();
            l.check_at(Bucket::Probe, "x", 1_000_000, later).unwrap();
        }
        assert!(
            !l.inner
                .lock()
                .unwrap()
                .windows
                .contains_key(&(Bucket::Probe, "old".to_string()))
        );
    }

    #[tokio::test]
    async fn limited_response_has_headers() {
        let l = Limiter::new();
        l.check(Bucket::Probe, "ip", 1).unwrap();
        let resp = l.check(Bucket::Probe, "ip", 1).unwrap_err().into_response();
        assert_eq!(resp.status(), 429);
        let h = resp.headers().clone();
        assert!(h["retry-after"].to_str().unwrap().parse::<u64>().unwrap() > 0);
        assert_eq!(h["ratelimit-limit"], "1");
        assert_eq!(h["ratelimit-remaining"], "0");
        assert!(h.contains_key("ratelimit-reset"));
        let body = to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["code"], "rate_limited");
        assert!(
            v["message"]
                .as_str()
                .unwrap()
                .starts_with("Too many requests")
        );
    }

    #[test]
    fn rate_info_sets_headers_on_parts() {
        let info = RateInfo {
            limit: 3,
            remaining: 2,
            reset_secs: 30,
        };
        let resp = (info, "ok").into_response();
        assert_eq!(resp.headers()["ratelimit-limit"], "3");
        assert_eq!(resp.headers()["ratelimit-remaining"], "2");
        assert_eq!(resp.headers()["ratelimit-reset"], "30");
    }
}
