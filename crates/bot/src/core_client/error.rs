//! Core API failures.

use std::fmt;

use reclip_shared::{ApiError, ErrorCode};
use reqwest::Response;

/// A core API failure. `status` is 0 when the request never got an answer.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreError {
    pub status: u16,
    pub code: ErrorCode,
    pub message: String,
    /// Seconds, from the `Retry-After` header of a 429.
    pub retry_after: Option<u64>,
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "core error {} ({:?}): {}",
            self.status, self.code, self.message
        )
    }
}

impl std::error::Error for CoreError {}

impl CoreError {
    pub(super) fn transport(e: reqwest::Error) -> Self {
        Self {
            status: 0,
            code: ErrorCode::Internal,
            message: e.without_url().to_string(),
            retry_after: None,
        }
    }

    pub(super) fn local(msg: String) -> Self {
        Self {
            status: 0,
            code: ErrorCode::Internal,
            message: msg,
            retry_after: None,
        }
    }

    pub(super) async fn from_response(resp: Response) -> Self {
        let status = resp.status().as_u16();
        let retry_after = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse().ok());
        let text = resp.text().await.unwrap_or_default();
        let text = if text.len() > 2000 && serde_json::from_str::<ApiError>(&text).is_err() {
            text.chars().take(500).collect()
        } else {
            text
        };
        match serde_json::from_str::<ApiError>(&text) {
            Ok(e) => Self {
                status,
                code: e.code,
                message: e.message,
                retry_after,
            },
            Err(_) => Self {
                status,
                code: ErrorCode::Internal,
                message: text,
                retry_after,
            },
        }
    }
}
