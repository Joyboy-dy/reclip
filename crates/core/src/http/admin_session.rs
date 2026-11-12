//! `/api/v1/admin/session`, `/me`, `/password`: admin sign-in with email and
//! password. Outside `caller_layer`, like `/api/v1/admin/keys`.

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::header::{CACHE_CONTROL, SET_COOKIE};
use axum::http::request::Parts;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use reclip_shared::{AdminLogin, AdminMe, AdminPasswordChange, ErrorCode};
use tower_http::set_header::SetResponseHeaderLayer;

use super::{ApiJson, HttpError, api_error, internal};
use crate::access::admin_auth::{
    AdminSession, MAX_PASSWORD_CHARS, check_not_blocked, check_origin, forwarded_of, hash_password,
    new_token, record_failure, secret_eq, session_of, set_cookie, token_hash_of, verify_password,
    wrong_credentials,
};
use crate::access::caller::error;
use crate::access::keys::sha256_hex;
use crate::app::AppState;
use crate::store::{AdminAccount, now_plus};

const MIN_PASSWORD_CHARS: usize = 12;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/admin/session", post(login).delete(logout))
        .route("/api/v1/admin/me", get(me))
        .route("/api/v1/admin/password", post(change_password))
        .layer(SetResponseHeaderLayer::overriding(
            CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
}

fn me_body(email: &str, must_change: bool) -> Json<AdminMe> {
    Json(AdminMe {
        email: email.to_string(),
        must_change_password: must_change,
    })
}

/// The signed-in admin, or the 401 to send.
async fn require(s: &AppState, parts: &Parts) -> Result<AdminSession, Response> {
    match session_of(s, &parts.headers).await {
        Ok(Some(session)) => Ok(session),
        Ok(None) => Err(error(
            ErrorCode::Unauthorized,
            "Your session expired. Sign in again.",
        )),
        Err(e) => Err(e.into_response()),
    }
}

/// `Some(must_change)` when `password` is right for `email`.
async fn authenticate(s: &AppState, email: &str, password: &str) -> anyhow::Result<Option<bool>> {
    if !s.cfg.admin_emails.iter().any(|e| e == email)
        || password.chars().count() > MAX_PASSWORD_CHARS
    {
        return Ok(None);
    }
    let stored = s.store.admin_account(email).await?;
    if let Some(AdminAccount {
        password_hash: Some(hash),
        must_change,
        ..
    }) = stored
    {
        return Ok(verify_password(password, &hash)
            .await
            .then_some(must_change));
    }
    // No password yet: only the configured default opens the account.
    let Some(default) = s.cfg.admin_default_password.as_ref() else {
        return Ok(None);
    };
    if !secret_eq(password, default.expose()) {
        return Ok(None);
    }
    let hash = hash_password(password).await?;
    Ok(s.store
        .init_admin_password(email, &hash)
        .await?
        .then_some(true))
}

async fn login(
    State(s): State<AppState>,
    parts: Parts,
    ApiJson(req): ApiJson<AdminLogin>,
) -> Response {
    let fwd = forwarded_of(&s, &parts);
    if let Err(r) = check_origin(&s, &parts, &fwd) {
        return *r;
    }
    let email = req.email.trim().to_lowercase();
    if let Err(limited) = check_not_blocked(&s.limiter, &fwd, &email) {
        return limited.into_response();
    }
    let must_change = match authenticate(&s, &email, &req.password).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            record_failure(&s.limiter, &fwd, &email);
            return wrong_credentials();
        }
        Err(e) => return internal(e).into_response(),
    };
    let Some(token) = new_token() else {
        return internal("no randomness for an admin session").into_response();
    };
    let ttl = s.cfg.admin_session_ttl;
    if let Err(e) = s
        .store
        .insert_admin_session(&sha256_hex(&token), &email, &now_plus(ttl))
        .await
    {
        return internal(e).into_response();
    }
    tracing::info!(%email, "admin signed in");
    let mut res = me_body(&email, must_change).into_response();
    if let Some(c) = set_cookie(&token, fwd.https, Some(ttl)) {
        res.headers_mut().append(SET_COOKIE, c);
    }
    res
}

/// Works while a password change is pending, and without a valid session.
async fn logout(State(s): State<AppState>, parts: Parts) -> Response {
    let fwd = forwarded_of(&s, &parts);
    if let Err(r) = check_origin(&s, &parts, &fwd) {
        return *r;
    }
    if let Some(hash) = token_hash_of(&parts.headers)
        && let Err(e) = s.store.delete_admin_session(&hash).await
    {
        return internal(e).into_response();
    }
    let mut res = StatusCode::NO_CONTENT.into_response();
    if let Some(c) = set_cookie("", fwd.https, None) {
        res.headers_mut().append(SET_COOKIE, c);
    }
    res
}

async fn me(State(s): State<AppState>, parts: Parts) -> Response {
    match require(&s, &parts).await {
        Ok(a) => me_body(&a.account.email, a.account.must_change).into_response(),
        Err(r) => r,
    }
}

fn invalid(message: &str) -> Response {
    HttpError(api_error(ErrorCode::InvalidRequest, message)).into_response()
}

async fn change_password(
    State(s): State<AppState>,
    parts: Parts,
    ApiJson(req): ApiJson<AdminPasswordChange>,
) -> Response {
    let fwd = forwarded_of(&s, &parts);
    if let Err(r) = check_origin(&s, &parts, &fwd) {
        return *r;
    }
    let session = match require(&s, &parts).await {
        Ok(a) => a,
        Err(r) => return r,
    };
    let email = session.account.email.clone();
    if let Err(limited) = check_not_blocked(&s.limiter, &fwd, &email) {
        return limited.into_response();
    }
    let len = req.new.chars().count();
    if !(MIN_PASSWORD_CHARS..=MAX_PASSWORD_CHARS).contains(&len) {
        return invalid("The new password must be 12 to 256 characters");
    }
    let is_default = s
        .cfg
        .admin_default_password
        .as_ref()
        .is_some_and(|d| secret_eq(&req.new, d.expose()));
    if is_default {
        return invalid("Choose a password different from the default one");
    }
    let current_ok = match &session.account.password_hash {
        Some(hash) if req.current.chars().count() <= MAX_PASSWORD_CHARS => {
            verify_password(&req.current, hash).await
        }
        _ => false,
    };
    if !current_ok {
        record_failure(&s.limiter, &fwd, &email);
        return invalid("Current password is not correct");
    }
    let hash = match hash_password(&req.new).await {
        Ok(h) => h,
        Err(e) => return internal(e).into_response(),
    };
    if let Err(e) = s.store.set_admin_password(&email, &hash).await {
        return internal(e).into_response();
    }
    if req.sign_out_others
        && let Err(e) = s
            .store
            .delete_other_admin_sessions(&email, &session.token_hash)
            .await
    {
        return internal(e).into_response();
    }
    tracing::info!(%email, sign_out_others = req.sign_out_others, "admin password changed");
    me_body(&email, false).into_response()
}
