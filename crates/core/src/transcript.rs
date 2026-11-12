//! The transcript service: scope check, URL checks, probe quota, yt-dlp.
//! Called by the HTTP route and by the MCP server, so both apply the same
//! rules to the same caller.

use axum::response::{IntoResponse, Response};
use reclip_shared::{ApiError, Scope, Transcript, TranscriptRequest};

use crate::access::caller::Caller;
use crate::access::limits::{Limited, RateInfo};
use crate::access::quota::check_probe;
use crate::access::ssrf::check_target;
use crate::app::AppState;
use crate::http::{HttpError, validate_url};
use crate::ytdlp::{runner, subs::validate_language};

/// Why a service call was refused.
#[derive(Debug)]
pub enum ServiceError {
    Api(ApiError),
    /// A quota refused the call (429, with `Retry-After`).
    Limited(Limited),
}

impl From<ApiError> for ServiceError {
    fn from(e: ApiError) -> Self {
        ServiceError::Api(e)
    }
}

impl From<Limited> for ServiceError {
    fn from(e: Limited) -> Self {
        ServiceError::Limited(e)
    }
}

impl IntoResponse for ServiceError {
    fn into_response(self) -> Response {
        match self {
            ServiceError::Api(e) => HttpError(e).into_response(),
            ServiceError::Limited(l) => l.into_response(),
        }
    }
}

/// Subtitles of `req.url` as text. Needs the `probe` scope and counts as one
/// probe in the caller's per-minute bucket; the rate info (if the caller is
/// limited) comes back for the `RateLimit-*` headers.
pub async fn transcript(
    s: &AppState,
    caller: &Caller,
    req: TranscriptRequest,
) -> Result<(Transcript, Option<RateInfo>), ServiceError> {
    let url = validate_url(&req.url)?;
    let language = req.language.as_deref().map(validate_language).transpose()?;
    caller.require(Scope::Probe).map_err(|e| e.0)?;
    check_target(&url, &s.resolver).await?;
    let rate = check_probe(s, caller).await?;
    let found = runner::transcript(&s.cfg, &url, language.as_deref()).await?;
    Ok((found, rate))
}
