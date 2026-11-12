//! Who is calling the API: an anonymous visitor (cookie), an API key, or a
//! key acting for a Telegram user. [`caller_layer`] resolves it once per
//! request; handlers extract the [`Caller`] and check the scope they need.

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, FromRequestParts, Request, State};
use axum::http::HeaderValue;
use axum::http::header::{AUTHORIZATION, CACHE_CONTROL, SET_COOKIE, WWW_AUTHENTICATE};
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use reclip_shared::{ApiKeyInfo, ErrorCode, Scope};

use super::cookie;
use super::ip::{Forwarded, forwarded};
use super::keys::{KeyError, verify_key};
use crate::app::AppState;
use crate::config::{Config, Limits};
use crate::http::{HttpError, api_error};

const ON_BEHALF_OF: &str = "x-reclip-on-behalf-of";
const MAX_TG_DIGITS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallerKind {
    Visitor,
    Key,
    /// A key with the `delegate` scope acting for a Telegram user.
    Delegated,
}

#[derive(Debug, Clone)]
pub struct Caller {
    /// Stored in `jobs.owner`: `web:<cookie>`, `key:<id>` or `tg:<user id>`.
    pub owner: String,
    /// What quotas count against: `ip:<addr>` (an IPv6 /64), `key:<id>` or
    /// `tg:<user id>`.
    pub quota_key: String,
    pub kind: CallerKind,
    pub scopes: Vec<Scope>,
    /// `None` means unlimited.
    pub limits: Option<Limits>,
    /// The API key used, also for delegated calls.
    pub key_id: Option<String>,
}

impl Caller {
    /// 403 `forbidden` unless the caller holds `scope`.
    pub fn require(&self, scope: Scope) -> Result<(), HttpError> {
        if self.scopes.contains(&scope) {
            return Ok(());
        }
        let name = serde_json::to_value(scope)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        Err(HttpError(api_error(
            ErrorCode::Forbidden,
            &format!("Missing scope: {name}"),
        )))
    }
}

/// Values inserted by [`caller_layer`]; missing means a wiring bug.
fn from_extensions<T: Clone + Send + Sync + 'static>(parts: &Parts) -> Result<T, HttpError> {
    parts.extensions.get::<T>().cloned().ok_or_else(|| {
        tracing::error!("caller middleware missing");
        HttpError(api_error(ErrorCode::Internal, "Internal error"))
    })
}

impl<S: Send + Sync> FromRequestParts<S> for Caller {
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        from_extensions(parts)
    }
}

impl<S: Send + Sync> FromRequestParts<S> for Forwarded {
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        from_extensions(parts)
    }
}

pub(crate) fn error(code: ErrorCode, message: &str) -> Response {
    HttpError(api_error(code, message)).into_response()
}

/// 401 `unauthorized` with a `WWW-Authenticate: Bearer` challenge.
pub(crate) fn unauthorized(message: &str) -> Response {
    let challenge = [(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"))];
    (
        challenge,
        HttpError(api_error(ErrorCode::Unauthorized, message)),
    )
        .into_response()
}

/// Resolve the caller, run the handler, and add the per-caller headers.
pub async fn caller_layer(State(s): State<AppState>, req: Request, next: Next) -> Response {
    // Only the parts are borrowed across awaits: the body is not `Sync`.
    let (mut parts, body) = req.into_parts();
    let peer = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());
    let mut fwd = forwarded(peer, &parts.headers, &s.cfg.trusted_proxies);
    if fwd.host.is_none() {
        // HTTP/2 carries the host in the `:authority` pseudo-header.
        fwd.host = parts.uri.authority().map(|a| a.to_string());
    }
    let mut res = match resolve(&s, &parts, &fwd).await {
        Err(res) => res,
        Ok((caller, minted)) => {
            // Only accepted requests may teach the public URL.
            s.site.learn(&s.store, &fwd).await;
            let https = fwd.https;
            parts.extensions.insert(fwd);
            parts.extensions.insert(caller);
            let mut res = next.run(Request::from_parts(parts, body)).await;
            if let Some(v) = minted.and_then(|v| cookie::set_cookie(&v, https)) {
                res.headers_mut().append(SET_COOKIE, v);
            }
            res
        }
    };
    // Responses are per caller: never let a shared cache keep them. A
    // handler may still let the caller's own browser cache (`private, ...`).
    let private = res
        .headers()
        .get(CACHE_CONTROL)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("private"));
    if !private {
        res.headers_mut()
            .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    res
}

/// The caller, plus the cookie value to set when one was just minted.
async fn resolve(
    s: &AppState,
    parts: &Parts,
    fwd: &Forwarded,
) -> Result<(Caller, Option<String>), Response> {
    let headers = &parts.headers;
    // Other schemes (Basic credentials a proxy forwards or a browser resends)
    // are not key attempts: they stay visitors.
    if let Some(auth) = headers.get(AUTHORIZATION).filter(|v| is_bearer_scheme(v)) {
        let invalid = || unauthorized("Invalid or expired API key");
        let token = bearer(auth).ok_or_else(invalid)?;
        let info = match verify_key(&s.store, &s.key_usage, token).await {
            Ok(info) => info,
            Err(KeyError::Invalid) => return Err(invalid()),
            // Already logged by `verify_key`.
            Err(KeyError::Internal) => return Err(error(ErrorCode::Internal, "Internal error")),
        };
        let caller = match headers.get(ON_BEHALF_OF) {
            None => key_caller(&s.cfg, info),
            Some(who) => delegated_caller(&s.cfg, info, who).map_err(|e| e.into_response())?,
        };
        return Ok((caller, None));
    }
    cookie::visitor_caller(s, parts, fwd).map_err(|r| *r)
}

/// Whether the credential's scheme is `Bearer` (case-insensitive), whatever
/// follows it: `Bearer` with a missing or bad token is still a key attempt.
fn is_bearer_scheme(v: &HeaderValue) -> bool {
    let raw = v.as_bytes().trim_ascii_start();
    let scheme = raw.split(|b| b.is_ascii_whitespace()).next();
    scheme.is_some_and(|s| s.eq_ignore_ascii_case(b"bearer"))
}

/// The token of `Bearer <token>` (scheme case-insensitive).
pub(crate) fn bearer(v: &HeaderValue) -> Option<&str> {
    let (scheme, token) = v.to_str().ok()?.trim().split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}

/// The key's own limits, each missing field falling back to the `key` tier.
fn key_limits(cfg: &Config, info: &ApiKeyInfo) -> Option<Limits> {
    if info.unlimited {
        return None;
    }
    let (own, tier) = (&info.limits, cfg.limits.key);
    Some(Limits {
        probes_per_minute: own.probes_per_minute.unwrap_or(tier.probes_per_minute),
        jobs_per_hour: own.jobs_per_hour.unwrap_or(tier.jobs_per_hour),
        max_active: own.max_active.unwrap_or(tier.max_active),
    })
}

fn key_caller(cfg: &Config, info: ApiKeyInfo) -> Caller {
    let id = format!("key:{}", info.id);
    Caller {
        owner: id.clone(),
        quota_key: id,
        kind: CallerKind::Key,
        limits: key_limits(cfg, &info),
        scopes: info.scopes,
        key_id: Some(info.id),
    }
}

/// A key acting for `tg:<digits>`: always the `tg` tier, never `delegate`.
fn delegated_caller(
    cfg: &Config,
    info: ApiKeyInfo,
    who: &HeaderValue,
) -> Result<Caller, HttpError> {
    if !info.scopes.contains(&Scope::Delegate) {
        return Err(HttpError(api_error(
            ErrorCode::Forbidden,
            "This API key cannot act on behalf of users",
        )));
    }
    let Some(user) = telegram_user(who) else {
        return Err(HttpError(api_error(
            ErrorCode::InvalidRequest,
            "X-Reclip-On-Behalf-Of must be tg:<user id>",
        )));
    };
    Ok(Caller {
        owner: user.clone(),
        quota_key: user,
        kind: CallerKind::Delegated,
        scopes: info
            .scopes
            .into_iter()
            .filter(|s| *s != Scope::Delegate)
            .collect(),
        limits: Some(cfg.limits.tg),
        key_id: Some(info.id),
    })
}

/// `tg:` followed by 1 to 20 ASCII digits, without leading zeros, so one
/// Telegram user has exactly one owner string.
fn telegram_user(v: &HeaderValue) -> Option<String> {
    let s = v.to_str().ok()?;
    let digits = s.strip_prefix("tg:")?;
    let ok = (1..=MAX_TG_DIGITS).contains(&digits.len())
        && !digits.starts_with('0')
        && digits.bytes().all(|b| b.is_ascii_digit());
    ok.then(|| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use reclip_shared::KeyLimits;

    #[test]
    fn bearer_and_telegram_parsing() {
        let hv = |s: &str| HeaderValue::from_str(s).unwrap();
        assert_eq!(bearer(&hv("BEARER abc")), Some("abc"));
        assert_eq!(bearer(&hv("Basic abc")), None);
        assert_eq!(bearer(&hv("Bearer")), None);
        for yes in ["Bearer", "bearer x", " BEARER\tx", "Bearer  "] {
            assert!(is_bearer_scheme(&hv(yes)), "{yes}");
        }
        for no in ["Basic abc", "Bearerx", "rck_abc_def", "", "Digest a=b"] {
            assert!(!is_bearer_scheme(&hv(no)), "{no}");
        }
        assert_eq!(telegram_user(&hv("tg:42")).as_deref(), Some("tg:42"));
        assert_eq!(telegram_user(&hv(" tg:42")), None);
        for bad in ["tg:042", "tg:0", "tg:00"] {
            assert_eq!(telegram_user(&hv(bad)), None, "{bad}");
        }
    }

    #[test]
    fn key_and_delegated_limits() {
        let cfg = Config::from_env(&|_| None);
        let mut info = ApiKeyInfo {
            id: "abcd1234".into(),
            name: "k".into(),
            scopes: vec![Scope::Probe, Scope::Delegate],
            unlimited: false,
            limits: KeyLimits {
                jobs_per_hour: Some(7),
                ..Default::default()
            },
            created_at: String::new(),
            expires_at: None,
            revoked_at: None,
            last_used_at: None,
            request_count: 0,
        };
        let l = key_limits(&cfg, &info).unwrap();
        assert_eq!(l.jobs_per_hour, 7);
        assert_eq!(l.probes_per_minute, cfg.limits.key.probes_per_minute);
        info.unlimited = true;
        assert_eq!(key_limits(&cfg, &info), None);
        let d = delegated_caller(&cfg, info, &HeaderValue::from_static("tg:9")).unwrap();
        assert_eq!(d.limits, Some(cfg.limits.tg));
        assert_eq!(d.scopes, vec![Scope::Probe]);
        assert_eq!((d.owner.as_str(), d.kind), ("tg:9", CallerKind::Delegated));
    }
}
