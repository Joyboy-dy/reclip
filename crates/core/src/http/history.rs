//! `/api/v1/history`: the visitor's download history (pending handlers).

use axum::Json;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use reclip_shared::{ErrorCode, Scope};

use super::api_error;
use crate::access::caller::Caller;

/// Placeholder answered until the real handler lands: 501 `internal`, after
/// the scope check.
pub(super) fn pending(caller: &Caller, scope: Scope) -> Response {
    if let Err(e) = caller.require(scope) {
        return e.into_response();
    }
    let body = api_error(ErrorCode::Internal, "Not implemented yet");
    (StatusCode::NOT_IMPLEMENTED, Json(body)).into_response()
}

pub async fn list(caller: Caller) -> Response {
    pending(&caller, Scope::JobsRead)
}

pub async fn remove(caller: Caller, Path(_id): Path<String>) -> Response {
    pending(&caller, Scope::JobsCancel)
}

pub async fn clean(caller: Caller) -> Response {
    pending(&caller, Scope::JobsCancel)
}

pub async fn thumb(caller: Caller, Path(_id): Path<String>) -> Response {
    pending(&caller, Scope::JobsRead)
}
