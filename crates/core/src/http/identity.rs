//! Anonymous visitor identity: a random `reclip_client` cookie.
//!
//! The middleware validates (or mints) the cookie and stores an [`Owner`]
//! in the request extensions; handlers extract it. An owner is never empty,
//! so legacy rows with owner `''` stay unreachable.

use axum::extract::{FromRequestParts, Request};
use axum::http::header::{COOKIE, SET_COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use reclip_shared::ErrorCode;

use super::{HttpError, api_error};

const COOKIE_NAME: &str = "reclip_client";
const MAX_AGE: u32 = 31_536_000;

/// The visitor owning a request, as stored in `jobs.owner` (`web:<cookie>`).
#[derive(Debug, Clone)]
pub struct Owner(pub String);

impl<S: Send + Sync> FromRequestParts<S> for Owner {
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        parts.extensions.get::<Owner>().cloned().ok_or_else(|| {
            tracing::error!("identity middleware missing");
            HttpError(api_error(ErrorCode::Internal, "Internal error"))
        })
    }
}

fn valid(v: &str) -> bool {
    v.len() == 32 && v.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn cookie_value(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|h| h.split(';'))
        .filter_map(|p| p.trim().strip_prefix("reclip_client="))
        .find(|v| valid(v))
        .map(str::to_string)
}

fn generate() -> Option<String> {
    let mut buf = [0u8; 16];
    getrandom::fill(&mut buf).ok()?;
    Some(buf.iter().map(|b| format!("{b:02x}")).collect())
}

pub async fn identity_layer(mut req: Request, next: Next) -> Response {
    let (value, minted) = match cookie_value(req.headers()) {
        Some(v) => (v, false),
        None => match generate() {
            Some(v) => (v, true),
            None => {
                return HttpError(api_error(ErrorCode::Internal, "Internal error")).into_response();
            }
        },
    };
    let secure = req
        .headers()
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("https"));
    req.extensions_mut().insert(Owner(format!("web:{value}")));
    let mut res = next.run(req).await;
    if minted {
        let secure = if secure { "; Secure" } else { "" };
        let c = format!(
            "{COOKIE_NAME}={value}; HttpOnly; SameSite=Lax; Path=/; Max-Age={MAX_AGE}{secure}"
        );
        if let Ok(v) = HeaderValue::from_str(&c) {
            res.headers_mut().append(SET_COOKIE, v);
        }
    }
    res
}
