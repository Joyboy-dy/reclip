//! Anonymous visitor identity: a random `reclip_client` cookie.
//!
//! The value is 32 lowercase hex chars. A missing or malformed cookie is
//! replaced by a fresh one; owners derived from it are never empty, so legacy
//! rows with owner `''` stay unreachable.

use axum::http::header::COOKIE;
use axum::http::{HeaderMap, HeaderValue};

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
