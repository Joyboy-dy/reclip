//! `/api/v1/admin/keys`: API key management with the admin token.

use axum::extract::{Path, Request, State};
use axum::http::header::{AUTHORIZATION, CACHE_CONTROL};
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get};
use axum::{Json, Router};
use reclip_shared::{ApiKeyList, CreateKeyRequest, CreatedKey};

use super::{ApiJson, HttpError, internal, not_found};
use crate::access::caller::{bearer, unauthorized};
use crate::access::keys::create_key;
use crate::app::AppState;

pub fn routes(state: AppState) -> Router<AppState> {
    Router::new()
        .route("/api/v1/admin/keys", get(list).post(create))
        .route("/api/v1/admin/keys/{id}", delete(revoke))
        .route_layer(middleware::from_fn_with_state(state, admin_layer))
}

/// `Authorization: Bearer <admin token>` or 401. Never cached.
async fn admin_layer(State(s): State<AppState>, req: Request, next: Next) -> Response {
    let ok = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(bearer)
        .is_some_and(|t| s.site.is_admin(t));
    let mut res = if ok {
        next.run(req).await
    } else {
        unauthorized("Invalid admin token")
    };
    res.headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
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
