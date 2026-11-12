//! Signed, time-limited download links: `POST /api/v1/jobs/{id}/link` mints
//! one, `GET /d/{token}` redeems it without a cookie or API key.

use axum::Json;
use axum::extract::{Path, Request, State};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, SecondsFormat, Utc};
use hmac::{Hmac, Mac};
use reclip_shared::{ErrorCode, JobState, LinkResponse, Scope};
use sha2::Sha256;

use super::files::serve_job_file;
use super::{HttpError, api_error, internal, not_found};
use crate::access::caller::Caller;
use crate::access::ip::Forwarded;
use crate::access::keys::ct_eq;
use crate::app::AppState;

/// Hex characters of the HMAC kept in a token (128 bits).
const SIG_CHARS: usize = 32;

fn signature(key: &[u8; 32], id: &str, exp: i64) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(format!("{id}.{exp}").as_bytes());
    let hex: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    hex[..SIG_CHARS].to_string()
}

/// `"{id}.{exp}.{sig}"`, `exp` in Unix seconds.
pub fn sign(key: &[u8; 32], id: &str, exp: i64) -> String {
    format!("{id}.{exp}.{}", signature(key, id, exp))
}

/// The job id when `token` is well formed, correctly signed and not expired.
pub fn verify(key: &[u8; 32], token: &str, now: i64) -> Option<String> {
    let mut parts = token.split('.');
    let (id, exp, sig) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let well_formed_id =
        id.len() == 32 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    let exp: i64 = exp.parse().ok()?;
    if !well_formed_id || exp <= now {
        return None;
    }
    ct_eq(sig.as_bytes(), signature(key, id, exp).as_bytes()).then(|| id.to_string())
}

/// Public origin for link URLs: configured address, the origin a trusted
/// proxy forwarded, the learned address, then the request's own host.
fn base_url(s: &AppState, fwd: &Forwarded) -> String {
    let own = || {
        let scheme = if fwd.https { "https" } else { "http" };
        let host = fwd.host.as_deref().unwrap_or("localhost");
        format!("{scheme}://{host}")
    };
    let base = if let Some(u) = &s.cfg.public_url {
        u.clone()
    } else if fwd.forwarded_origin {
        own()
    } else {
        s.site.public_url().unwrap_or_else(own)
    };
    base.trim_end_matches('/').to_string()
}

pub async fn create(
    State(s): State<AppState>,
    caller: Caller,
    fwd: Forwarded,
    Path(id): Path<String>,
) -> Response {
    if let Err(e) = caller.require(Scope::JobsRead) {
        return e.into_response();
    }
    let job = match s.store.get_owned(&id, &caller.owner).await {
        Ok(Some(j)) if j.state == JobState::Done => j,
        Ok(_) => return not_found("File not available"),
        Err(e) => return internal(e).into_response(),
    };
    // Whole seconds: the token's `exp` and the reported expiry must agree.
    let ttl = i64::try_from(s.cfg.link_ttl.as_secs()).unwrap_or(i64::MAX);
    let mut exp = Utc::now()
        .timestamp()
        .saturating_add(ttl)
        .min(DateTime::<Utc>::MAX_UTC.timestamp());
    if let Some(at) = job.expires_at.as_deref() {
        match DateTime::parse_from_rfc3339(at) {
            Ok(at) => exp = exp.min(at.timestamp()),
            Err(e) => {
                // Fail closed: a link must never outlive the file.
                tracing::warn!(id = %job.id, error = %e, "unreadable file expiry, no link");
                return HttpError(api_error(ErrorCode::Internal, "Internal error")).into_response();
            }
        }
    }
    let expiry = DateTime::<Utc>::from_timestamp(exp, 0).unwrap_or(DateTime::<Utc>::MAX_UTC);
    let token = sign(&s.site.link_key, &job.id, exp);
    Json(LinkResponse {
        url: format!("{}/d/{token}", base_url(&s, &fwd)),
        expires_at: expiry.to_rfc3339_opts(SecondsFormat::Millis, true),
    })
    .into_response()
}

pub async fn redeem(
    State(s): State<AppState>,
    Path(token): Path<String>,
    req: Request,
) -> Response {
    let gone = || not_found("Link expired or invalid");
    let Some(id) = verify(&s.site.link_key, &token, Utc::now().timestamp()) else {
        return gone();
    };
    let job = match s.store.get(&id).await {
        Ok(Some(j)) if j.state == JobState::Done => j,
        Ok(_) => return gone(),
        Err(e) => return internal(e).into_response(),
    };
    let mut res = serve_job_file(&s, job, req).await;
    res.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    res
}

#[cfg(test)]
mod tests {
    use super::{sign, verify};

    const KEY: [u8; 32] = [7; 32];
    const ID: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn sign_verify_roundtrip() {
        let t = sign(&KEY, ID, 2_000);
        assert!(t.starts_with(&format!("{ID}.2000.")));
        assert_eq!(t.rsplit('.').next().unwrap().len(), 32);
        assert_eq!(verify(&KEY, &t, 1_000).as_deref(), Some(ID));
        assert_eq!(verify(&[8; 32], &t, 1_000), None);
    }

    #[test]
    fn tampered_token_rejected() {
        let t = sign(&KEY, ID, 2_000);
        let other = "fedcba9876543210fedcba9876543210";
        let sig = t.rsplit('.').next().unwrap();
        assert_eq!(verify(&KEY, &t.replacen(ID, other, 1), 1_000), None);
        assert_eq!(verify(&KEY, &t.replace(".2000.", ".9000."), 1_000), None);
        let last = if sig.ends_with('0') { '1' } else { '0' };
        let flipped = format!("{}{last}", &sig[..31]);
        assert_eq!(verify(&KEY, &t.replace(sig, &flipped), 1_000), None);
        for bad in ["", "a.b", &format!("{t}.x"), &t.to_uppercase()] {
            assert_eq!(verify(&KEY, bad, 1_000), None, "{bad}");
        }
    }

    #[test]
    fn expired_token_rejected() {
        let t = sign(&KEY, ID, 2_000);
        assert!(verify(&KEY, &t, 1_999).is_some());
        assert_eq!(verify(&KEY, &t, 2_000), None);
        assert_eq!(verify(&KEY, &t, 3_000), None);
    }
}
