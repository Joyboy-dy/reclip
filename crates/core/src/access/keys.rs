//! API keys: `rck_<id>_<secret>`.
//!
//! `id` is 8 chars of `[0-9a-z]` (public, indexed); `secret` is 43 base62
//! chars (~256 bits). Only the hex SHA-256 of the secret is stored. The secret
//! has enough entropy that a fast hash is sufficient, and lookups stay cheap.

use reclip_shared::ApiError;
use reclip_shared::ErrorCode;
use reclip_shared::admin::{ApiKeyInfo, CreateKeyRequest, CreatedKey, Scope};
use sha2::{Digest, Sha256};

use crate::http::{api_error, internal};
use crate::store::{Store, now, now_plus};

const PREFIX: &str = "rck_";
const ID_LEN: usize = 8;
const SECRET_LEN: usize = 43;
const MAX_NAME_CHARS: usize = 64;
const ID_ATTEMPTS: usize = 5;
/// Longest key lifetime (100 years): later dates would leave the timestamp
/// format and compare as already expired.
pub const MAX_EXPIRES_DAYS: u32 = 36_500;
const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const BASE36: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// Why a bearer token was not accepted.
#[derive(Debug, PartialEq, Eq)]
pub enum KeyError {
    /// Malformed, unknown, wrong secret, revoked or expired (indistinguishable).
    Invalid,
    /// The key store failed; the caller should answer 500, not 401.
    Internal,
}

/// `len` chars from `alphabet`, by rejection sampling on OS randomness.
fn random_from(alphabet: &[u8], len: usize) -> anyhow::Result<String> {
    // Largest multiple of the alphabet size that fits in a byte: bytes at or
    // above it are dropped so the modulo stays unbiased.
    let limit = 256 - 256 % alphabet.len();
    let mut out = String::with_capacity(len);
    let mut buf = [0u8; 64];
    while out.len() < len {
        getrandom::fill(&mut buf).map_err(|e| anyhow::anyhow!("getrandom: {e}"))?;
        for &b in &buf {
            if (b as usize) < limit && out.len() < len {
                out.push(alphabet[b as usize % alphabet.len()] as char);
            }
        }
    }
    Ok(out)
}

pub fn random_base62(len: usize) -> anyhow::Result<String> {
    random_from(BASE62, len)
}

pub fn sha256_hex(s: &str) -> String {
    use std::fmt::Write;
    Sha256::digest(s.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut acc, b| {
            let _ = write!(acc, "{b:02x}");
            acc
        })
}

/// Constant-time equality (the length is not secret here: hashes are fixed size).
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Split `rck_<id>_<secret>` into `(id, secret)`; anything else is `None`.
pub fn parse_key(raw: &str) -> Option<(&str, &str)> {
    let rest = raw.strip_prefix(PREFIX)?;
    let (id, secret) = rest.split_once('_')?;
    let id_ok = id.len() == ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase());
    let secret_ok = secret.len() == SECRET_LEN && secret.bytes().all(|b| b.is_ascii_alphanumeric());
    (id_ok && secret_ok).then_some((id, secret))
}

/// Create and store a key. The full key is only available in the result.
pub async fn create_key(store: &Store, req: CreateKeyRequest) -> Result<CreatedKey, ApiError> {
    let name = req.name.trim();
    if name.is_empty() || name.chars().count() > MAX_NAME_CHARS {
        return Err(api_error(
            ErrorCode::InvalidRequest,
            "Key name must be 1 to 64 characters",
        ));
    }
    if req
        .expires_in_days
        .is_some_and(|d| !(1..=MAX_EXPIRES_DAYS).contains(&d))
    {
        return Err(api_error(
            ErrorCode::InvalidRequest,
            "expires_in_days must be between 1 and 36500",
        ));
    }
    let mut scopes: Vec<Scope> = Vec::new();
    for s in req.scopes.unwrap_or_else(Scope::default_set) {
        if !scopes.contains(&s) {
            scopes.push(s);
        }
    }

    let id = free_id(store).await?;
    let secret = random_base62(SECRET_LEN).map_err(|e| internal(e).0)?;
    let info = ApiKeyInfo {
        id,
        name: name.to_string(),
        scopes,
        unlimited: req.unlimited,
        limits: req.limits.unwrap_or_default(),
        created_at: now(),
        expires_at: req
            .expires_in_days
            .map(|d| now_plus(std::time::Duration::from_secs(u64::from(d) * 86_400))),
        revoked_at: None,
        last_used_at: None,
        request_count: 0,
    };
    store
        .insert_key(&info, &sha256_hex(&secret))
        .await
        .map_err(|e| internal(e).0)?;
    Ok(CreatedKey {
        key: format!("{PREFIX}{}_{secret}", info.id),
        info,
    })
}

/// Draw ids until one is unused (collisions are astronomically rare).
async fn free_id(store: &Store) -> Result<String, ApiError> {
    for _ in 0..ID_ATTEMPTS {
        let id = random_from(BASE36, ID_LEN).map_err(|e| internal(e).0)?;
        if store
            .key_with_hash(&id)
            .await
            .map_err(|e| internal(e).0)?
            .is_none()
        {
            return Ok(id);
        }
    }
    Err(internal("could not find a free api key id").0)
}

/// Check a raw `rck_...` token (without the `Bearer ` prefix).
pub async fn verify_key(store: &Store, bearer: &str) -> Result<ApiKeyInfo, KeyError> {
    let (id, secret) = parse_key(bearer).ok_or(KeyError::Invalid)?;
    let presented = sha256_hex(secret);
    let found = store.key_with_hash(id).await.map_err(|e| {
        internal(e);
        KeyError::Internal
    })?;
    let (info, stored) = found.ok_or(KeyError::Invalid)?;
    if !ct_eq(presented.as_bytes(), stored.as_bytes()) {
        return Err(KeyError::Invalid);
    }
    if info.revoked_at.is_some()
        || info
            .expires_at
            .as_deref()
            .is_some_and(|e| e <= now().as_str())
    {
        return Err(KeyError::Invalid);
    }
    let (store, id) = (store.clone(), info.id.clone());
    tokio::spawn(async move {
        if let Err(e) = store.touch_key(&id).await {
            tracing::warn!(error = %e, "failed to record api key use");
        }
    });
    Ok(info)
}

#[cfg(test)]
#[path = "keys_tests.rs"]
mod tests;
