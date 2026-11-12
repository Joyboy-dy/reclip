//! `/api/v1/admin/session`, `/me`, `/password`: admin sign-in (pending
//! handlers). Outside `caller_layer`, like `/api/v1/admin/keys`.

use axum::Router;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use reclip_shared::{ApiError, ErrorCode};

use crate::app::AppState;

async fn pending() -> Response {
    let body = ApiError {
        code: ErrorCode::Internal,
        message: "Not implemented yet".into(),
    };
    (StatusCode::NOT_IMPLEMENTED, axum::Json(body)).into_response()
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/admin/session", post(pending).delete(pending))
        .route("/api/v1/admin/me", get(pending))
        .route("/api/v1/admin/password", post(pending))
}
