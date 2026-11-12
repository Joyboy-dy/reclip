//! Per-request helpers shared by `caller_layer` and the `/mcp` key check:
//! the request's origin, the API key of a `Bearer` credential, and the
//! no-store rule for per-caller responses.

use std::net::SocketAddr;

use axum::extract::ConnectInfo;
use axum::http::HeaderValue;
use axum::http::header::CACHE_CONTROL;
use axum::http::request::Parts;
use axum::response::Response;
use reclip_shared::{ApiKeyInfo, ErrorCode};

use super::caller::{bearer, error, unauthorized};
use super::ip::{Forwarded, forwarded};
use super::keys::{KeyError, verify_key};
use crate::app::AppState;

/// Responses are per caller: never let a shared cache keep them. A handler
/// may still let the caller's own browser cache (`private, ...`).
pub(crate) fn no_store(res: &mut Response) {
    let private = res
        .headers()
        .get(CACHE_CONTROL)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("private"));
    if !private {
        res.headers_mut()
            .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
}

/// Where the request came from, once trusted proxies are accounted for.
pub(crate) fn request_origin(s: &AppState, parts: &Parts) -> Forwarded {
    let peer = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());
    let mut fwd = forwarded(peer, &parts.headers, &s.cfg.trusted_proxies);
    if fwd.host.is_none() {
        // HTTP/2 carries the host in the `:authority` pseudo-header.
        fwd.host = parts.uri.authority().map(|a| a.to_string());
    }
    fwd
}

/// The key of a `Bearer` credential, or the 401 (500 on a store failure).
pub(crate) async fn bearer_key(s: &AppState, auth: &HeaderValue) -> Result<ApiKeyInfo, Response> {
    let invalid = || unauthorized("Invalid or expired API key");
    let token = bearer(auth).ok_or_else(invalid)?;
    match verify_key(&s.store, &s.key_usage, token).await {
        Ok(info) => Ok(info),
        Err(KeyError::Invalid) => Err(invalid()),
        // Already logged by `verify_key`.
        Err(KeyError::Internal) => Err(error(ErrorCode::Internal, "Internal error")),
    }
}
