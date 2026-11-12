use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    BotCheck,
    UnsupportedUrl,
    Unavailable,
    Private,
    GeoBlocked,
    TooLarge,
    TooLong,
    Timeout,
    Interrupted,
    Cancelled,
    RateLimited,
    ForbiddenTarget,
    InvalidRequest,
    Internal,
}

impl ErrorCode {
    /// HTTP status used when this error is returned by the API.
    pub fn http_status(&self) -> u16 {
        match self {
            Self::InvalidRequest | Self::UnsupportedUrl => 400,
            Self::ForbiddenTarget => 403,
            Self::RateLimited => 429,
            Self::Internal => 500,
            _ => 422,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
}
