//! Admin passwords (Argon2id), session cookies and the checks shared by the
//! admin routes.

use std::net::SocketAddr;
use std::time::Duration;

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use axum::extract::ConnectInfo;
use axum::http::header::COOKIE;
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method};
use axum::response::{IntoResponse, Response};
use reclip_shared::ErrorCode;

use super::caller::error;
use super::ip::{Forwarded, forwarded};
use super::keys::{ct_eq, sha256_hex};
use super::limits::{Bucket, Limited, Limiter};
use super::origin::same_origin;
use crate::app::AppState;
use crate::http::{HttpError, api_error, internal};
use crate::store::AdminAccount;

pub const COOKIE_NAME: &str = "reclip_admin";
/// Failed sign-ins tolerated per IP and per email in 15 minutes.
const MAX_FAILURES: u32 = 5;
/// Longest password considered (longer ones cannot match anything).
pub const MAX_PASSWORD_CHARS: usize = 256;
const TOKEN_BYTES: usize = 32;

/// A signed-in admin, resolved from the session cookie.
#[derive(Debug, Clone)]
pub struct AdminSession {
    pub account: AdminAccount,
    pub token_hash: String,
}

/// Argon2id hash of `password`, computed off the async runtime.
pub async fn hash_password(password: &str) -> anyhow::Result<String> {
    let password = password.to_owned();
    tokio::task::spawn_blocking(move || {
        let mut raw = [0u8; 16];
        getrandom::fill(&mut raw).map_err(|e| anyhow::anyhow!("getrandom: {e}"))?;
        let salt = SaltString::encode_b64(&raw)
            .map_err(|e| anyhow::anyhow!("salt encoding failed: {e}"))?;
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| anyhow::anyhow!("password hashing failed: {e}"))
    })
    .await?
}

/// Whether `password` matches the stored `hash` (off the async runtime).
pub async fn verify_password(password: &str, hash: &str) -> bool {
    let (password, hash) = (password.to_owned(), hash.to_owned());
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&hash).is_ok_and(|h| {
            Argon2::default()
                .verify_password(password.as_bytes(), &h)
                .is_ok()
        })
    })
    .await
    .unwrap_or(false)
}

/// Constant-time comparison of two secrets of any length.
pub fn secret_eq(a: &str, b: &str) -> bool {
    ct_eq(sha256_hex(a).as_bytes(), sha256_hex(b).as_bytes())
}

/// A new session token: 32 random bytes as 64 lowercase hex chars.
pub fn new_token() -> Option<String> {
    let mut buf = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut buf).ok()?;
    Some(buf.iter().map(|b| format!("{b:02x}")).collect())
}

/// The well-formed `reclip_admin` cookie value, if any.
fn token_from(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|h| h.split(';'))
        .filter_map(|p| p.trim().strip_prefix("reclip_admin="))
        .find(|v| v.len() == TOKEN_BYTES * 2 && v.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_ascii_lowercase)
}

/// `Set-Cookie` value for a session (`ttl`) or, with `None`, its removal.
pub fn set_cookie(token: &str, secure: bool, ttl: Option<Duration>) -> Option<HeaderValue> {
    let secure = if secure { "; Secure" } else { "" };
    let (value, age) = match ttl {
        Some(t) => (token, t.as_secs()),
        None => ("", 0),
    };
    let c =
        format!("{COOKIE_NAME}={value}; HttpOnly; SameSite=Strict; Path=/; Max-Age={age}{secure}");
    HeaderValue::from_str(&c).ok()
}

/// Client address and scheme the way `caller_layer` computes them.
pub fn forwarded_of(s: &AppState, parts: &Parts) -> Forwarded {
    let peer = parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());
    let mut fwd = forwarded(peer, &parts.headers, &s.cfg.trusted_proxies);
    if fwd.host.is_none() {
        fwd.host = parts.uri.authority().map(|a| a.to_string());
    }
    fwd
}

/// 403 unless a state-changing request names this site in `Origin`.
pub fn check_origin(s: &AppState, parts: &Parts, fwd: &Forwarded) -> Result<(), Box<Response>> {
    let safe = matches!(parts.method, Method::GET | Method::HEAD | Method::OPTIONS);
    if safe || same_origin(&parts.headers, fwd, s.cfg.public_url.as_deref()) {
        return Ok(());
    }
    Err(Box::new(error(
        ErrorCode::Forbidden,
        "Cross-site request refused",
    )))
}

/// The admin behind the session cookie: unexpired, and still allowed in
/// `admin_emails`.
pub async fn session_of(
    s: &AppState,
    headers: &HeaderMap,
) -> Result<Option<AdminSession>, HttpError> {
    let Some(token) = token_from(headers) else {
        return Ok(None);
    };
    let token_hash = sha256_hex(&token);
    let account = s.store.admin_session(&token_hash).await.map_err(internal)?;
    Ok(account
        .filter(|a| s.cfg.admin_emails.contains(&a.email))
        .map(|account| AdminSession {
            account,
            token_hash,
        }))
}

/// The session's own token hash for a request, regardless of validity.
pub fn token_hash_of(headers: &HeaderMap) -> Option<String> {
    token_from(headers).map(|t| sha256_hex(&t))
}

/// 403 `forbidden` while the admin still has to change the password.
pub fn must_change_error() -> Response {
    error(ErrorCode::Forbidden, "Change your password first")
}

pub fn wrong_credentials() -> Response {
    let mut res = HttpError(api_error(
        ErrorCode::Unauthorized,
        "Wrong email or password",
    ))
    .into_response();
    res.headers_mut().remove("www-authenticate");
    res
}

fn email_key(email: &str) -> String {
    let short: String = email.chars().take(254).collect();
    format!("email:{short}")
}

fn ip_key(fwd: &Forwarded) -> String {
    format!("ip:{}", fwd.client_ip)
}

/// 429 while this IP or email already failed too often.
pub fn check_not_blocked(l: &Limiter, fwd: &Forwarded, email: &str) -> Result<(), Limited> {
    l.blocked(Bucket::AdminFail, &ip_key(fwd), MAX_FAILURES)?;
    l.blocked(Bucket::AdminFail, &email_key(email), MAX_FAILURES)
}

/// Count one failed sign-in against the IP and the email.
pub fn record_failure(l: &Limiter, fwd: &Forwarded, email: &str) {
    for key in [ip_key(fwd), email_key(email)] {
        // The refusal itself is reported by `check_not_blocked` next time.
        let _ = l.check(Bucket::AdminFail, &key, MAX_FAILURES);
    }
}
