//! Anonymous visitors: a random `reclip_client` cookie identifies them.
//!
//! The value is 32 lowercase hex chars. A missing or malformed cookie is
//! replaced by a fresh one; owners derived from it are never empty, so legacy
//! rows with owner `''` stay unreachable.

use axum::http::header::COOKIE;
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method};
use axum::response::Response;
use reclip_shared::{ErrorCode, Scope};

use super::caller::{Caller, CallerKind, error, unauthorized};
use super::ip::Forwarded;
use super::origin::same_origin;
use crate::app::AppState;

const COOKIE_NAME: &str = "reclip_client";
const MAX_AGE: u32 = 31_536_000;

fn valid(v: &str) -> bool {
    v.len() == 32 && v.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The first well-formed `reclip_client` value sent by the browser.
pub fn value(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|h| h.split(';'))
        .filter_map(|p| p.trim().strip_prefix("reclip_client="))
        .find(|v| valid(v))
        .map(str::to_string)
}

/// A new random value; `None` if the OS has no randomness to give.
pub fn generate() -> Option<String> {
    let mut buf = [0u8; 16];
    getrandom::fill(&mut buf).ok()?;
    Some(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// The `Set-Cookie` value for a newly minted cookie.
pub fn set_cookie(value: &str, secure: bool) -> Option<HeaderValue> {
    let secure = if secure { "; Secure" } else { "" };
    let c =
        format!("{COOKIE_NAME}={value}; HttpOnly; SameSite=Lax; Path=/; Max-Age={MAX_AGE}{secure}");
    HeaderValue::from_str(&c).ok()
}

/// A request without `Authorization`: an anonymous visitor, plus the cookie
/// value to set when one was just minted. Anything but GET, HEAD and
/// OPTIONS needs an `Origin` naming this site. The refusal is boxed: it is
/// the cold path and `Response` is large.
pub fn visitor_caller(
    s: &AppState,
    parts: &Parts,
    fwd: &Forwarded,
) -> Result<(Caller, Option<String>), Box<Response>> {
    let open_route = parts.method == Method::GET && parts.uri.path() == "/api/v1/config";
    if !s.cfg.public_web && !open_route {
        return Err(Box::new(unauthorized("An API key is required")));
    }
    let safe = matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS);
    // The configured URL only: a learned one is never trusted as an Origin.
    if !safe && !same_origin(&parts.headers, fwd, s.cfg.public_url.as_deref()) {
        return Err(Box::new(error(
            ErrorCode::Forbidden,
            "Cross-site request refused",
        )));
    }
    let (value, minted) = match value(&parts.headers) {
        Some(v) => (v, false),
        None => match generate() {
            Some(v) => (v, true),
            None => {
                tracing::error!("no randomness for a visitor cookie");
                return Err(Box::new(error(ErrorCode::Internal, "Internal error")));
            }
        },
    };
    let caller = Caller {
        owner: format!("web:{value}"),
        quota_key: format!("ip:{}", fwd.client_ip),
        kind: CallerKind::Visitor,
        scopes: Scope::default_set(),
        limits: Some(s.cfg.limits.web),
        key_id: None,
    };
    Ok((caller, minted.then_some(value)))
}
