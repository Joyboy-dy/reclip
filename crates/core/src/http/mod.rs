//! HTTP layer: error mapping and request helpers.

pub mod admin;
pub mod admin_session;
pub mod config;
pub mod events;
pub mod files;
pub mod headers;
pub mod history;
pub mod jobs;
pub mod links;
pub mod pages;
pub mod probe;
pub mod transcript;

use axum::Json;
use axum::extract::{FromRequest, Request};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use reclip_shared::{ApiError, ErrorCode};
use serde::de::DeserializeOwned;

use crate::jobs::ManagerError;

/// Newtype so `ApiError` (defined in `reclip-shared`) can be an axum response.
#[derive(Debug)]
pub struct HttpError(pub ApiError);

impl From<ApiError> for HttpError {
    fn from(e: ApiError) -> Self {
        HttpError(e)
    }
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.0.code.http_status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(self.0)).into_response()
    }
}

pub fn api_error(code: ErrorCode, message: &str) -> ApiError {
    ApiError {
        code,
        message: message.to_string(),
    }
}

/// Store/IO failure: logged, never leaked to the client.
pub fn internal(e: impl std::fmt::Display) -> HttpError {
    tracing::error!(error = %e, "internal error");
    HttpError(api_error(ErrorCode::Internal, "Internal error"))
}

/// 404 with the standard JSON error body.
pub fn not_found(message: &str) -> Response {
    let body = api_error(ErrorCode::InvalidRequest, message);
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}

pub async fn api_not_found() -> Response {
    not_found("Unknown API route")
}

impl From<HttpError> for ManagerError {
    fn from(e: HttpError) -> Self {
        ManagerError::Api(e.0)
    }
}

impl IntoResponse for ManagerError {
    fn into_response(self) -> Response {
        match self {
            ManagerError::NotFound => not_found("Job not found"),
            ManagerError::Api(e) => HttpError(e).into_response(),
        }
    }
}

/// JSON body extractor whose rejection is our `invalid_request` error.
pub struct ApiJson<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for ApiJson<T> {
    type Rejection = HttpError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(v)) => Ok(ApiJson(v)),
            Err(_) => Err(HttpError(api_error(
                ErrorCode::InvalidRequest,
                "Invalid request body",
            ))),
        }
    }
}

/// Trim and check that the input is an http(s) URL.
pub fn validate_url(raw: &str) -> Result<String, ApiError> {
    let trimmed = raw.trim();
    match url::Url::parse(trimmed) {
        Ok(u) if matches!(u.scheme(), "http" | "https") && u.host_str().is_some() => {
            Ok(trimmed.to_string())
        }
        _ => Err(api_error(
            ErrorCode::InvalidRequest,
            "Please paste a valid http(s) link",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::validate_url;

    #[test]
    fn validates_urls() {
        assert_eq!(validate_url("  https://a.b/c ").unwrap(), "https://a.b/c");
        for bad in ["", "ftp://x/y", "javascript:alert(1)", "hello", "http://"] {
            assert!(validate_url(bad).is_err(), "{bad}");
        }
    }
}
