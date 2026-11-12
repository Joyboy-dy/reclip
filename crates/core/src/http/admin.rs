//! `/api/v1/admin/keys`: API key management with the admin token.

use axum::extract::{Path, Request, State};
use axum::http::header::{AUTHORIZATION, CACHE_CONTROL};
use axum::http::request::Parts;
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get};
use axum::{Json, Router};
use reclip_shared::{ApiKeyList, CreateKeyRequest, CreatedKey};

use super::{ApiJson, HttpError, internal, not_found};
use crate::access::admin_auth::{check_origin, forwarded_of, must_change_error, session_of};
use crate::access::caller::{bearer, unauthorized};
use crate::access::keys::create_key;
use crate::app::AppState;

pub fn routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/admin/keys", get(list).post(create))
        .route("/api/v1/admin/keys/{id}", delete(revoke))
        .route_layer(middleware::from_fn_with_state(state, admin_layer))
}

/// `Authorization: Bearer <admin token>`, or an admin session cookie (with a
/// same-site `Origin` on requests that change something and no pending
/// password change), or 401. Never cached.
async fn admin_layer(State(s): State<AppState>, req: Request, next: Next) -> Response {
    let (parts, body) = req.into_parts();
    let mut res = match authorize(&s, &parts).await {
        Ok(()) => next.run(Request::from_parts(parts, body)).await,
        Err(res) => res,
    };
    res.headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

async fn authorize(s: &AppState, parts: &Parts) -> Result<(), Response> {
    let invalid = || unauthorized("Invalid admin token");
    if let Some(auth) = parts.headers.get(AUTHORIZATION) {
        let ok = bearer(auth).is_some_and(|t| s.site.is_admin(t));
        return if ok { Ok(()) } else { Err(invalid()) };
    }
    let session = session_of(s, &parts.headers)
        .await
        .map_err(IntoResponse::into_response)?
        .ok_or_else(invalid)?;
    check_origin(s, parts, &forwarded_of(s, parts)).map_err(|r| *r)?;
    if session.account.must_change {
        return Err(must_change_error());
    }
    Ok(())
}

async fn create(
    State(s): State<AppState>,
    ApiJson(req): ApiJson<CreateKeyRequest>,
) -> Result<(StatusCode, Json<CreatedKey>), HttpError> {
    let created = create_key(&s.store, req).await?;
    tracing::info!(id = %created.info.id, "api key created");
    Ok((StatusCode::CREATED, Json(created)))
}

async fn list(State(s): State<AppState>) -> Result<Json<ApiKeyList>, HttpError> {
    let keys = s.store.list_keys().await.map_err(internal)?;
    Ok(Json(ApiKeyList { keys }))
}

async fn revoke(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    match s.store.revoke_key(&id).await {
        Ok(true) => {
            tracing::info!(%id, "api key revoked");
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => not_found("API key not found"),
        Err(e) => internal(e).into_response(),
    }
}
