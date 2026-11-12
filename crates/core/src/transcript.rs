//! The transcript service: scope check, URL checks, probe quota, yt-dlp.
//! Called by the HTTP route and by the MCP server, so both apply the same
//! rules to the same caller.

use reclip_shared::{Scope, Transcript, TranscriptRequest};

use crate::access::caller::Caller;
use crate::access::limits::RateInfo;
use crate::access::quota::check_probe;
use crate::access::ssrf::check_target;
use crate::app::AppState;
use crate::http::validate_url;
pub use crate::service::ServiceError;
use crate::ytdlp::{runner, subs::validate_language};

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
    caller.require(Scope::Probe)?;
    check_target(&url, &s.resolver).await?;
    let rate = check_probe(s, caller).await?;
    let found = runner::transcript(&s.cfg, &url, language.as_deref()).await?;
    Ok((found, rate))
}
