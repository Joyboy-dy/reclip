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
mod tests {
    use super::*;
    use reclip_shared::admin::{CreateKeyRequest, Scope};

    async fn store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("t.db")).await.unwrap();
        (store, dir)
    }

    fn req(name: &str) -> CreateKeyRequest {
        CreateKeyRequest {
            name: name.to_string(),
            scopes: None,
            unlimited: false,
            expires_in_days: None,
            limits: None,
        }
    }

    #[tokio::test]
    async fn key_format() {
        let (store, _d) = store().await;
        let created = create_key(&store, req("a")).await.unwrap();
        let rest = created.key.strip_prefix("rck_").unwrap();
        let (id, secret) = rest.split_once('_').unwrap();
        assert_eq!(id.len(), 8);
        assert!(
            id.bytes()
                .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
        );
        assert_eq!(secret.len(), 43);
        assert!(secret.bytes().all(|b| b.is_ascii_alphanumeric()));
        assert_eq!(created.info.id, id);
        assert_eq!(created.info.scopes, Scope::default_set());
    }

    #[test]
    fn parse_rejects_malformed() {
        let secret = "A".repeat(43);
        assert_eq!(
            parse_key(&format!("rck_abcd1234_{secret}")),
            Some(("abcd1234", secret.as_str()))
        );
        assert!(parse_key("rck_abc_x").is_none());
        assert!(parse_key(&format!("rck_ABCDEFGH_{secret}")).is_none());
        assert!(parse_key(&format!("Bearer rck_abcd1234_{secret}")).is_none());
        assert!(parse_key(&format!("rck_abcd1234_{secret}x")).is_none());
        assert!(parse_key(&format!("rck_abcd1234_{}-", "A".repeat(42))).is_none());
        assert!(parse_key("").is_none());
    }

    #[tokio::test]
    async fn verify_ok_then_revoked() {
        let (store, _d) = store().await;
        let created = create_key(&store, req("a")).await.unwrap();
        let info = verify_key(&store, &created.key).await.unwrap();
        assert_eq!(info.id, created.info.id);
        assert!(store.revoke_key(&info.id).await.unwrap());
        assert!(matches!(
            verify_key(&store, &created.key).await,
            Err(KeyError::Invalid)
        ));
    }

    #[tokio::test]
    async fn verify_expired() {
        let (store, _d) = store().await;
        let mut r = req("a");
        r.expires_in_days = Some(1);
        let created = create_key(&store, r).await.unwrap();
        assert!(created.info.expires_at.is_some());
        assert!(verify_key(&store, &created.key).await.is_ok());

        let secret = created.key.rsplit('_').next().unwrap();
        let mut past = created.info.clone();
        past.id = "pastpast".to_string();
        past.expires_at = Some(crate::store::now_minus(std::time::Duration::from_secs(5)));
        store.insert_key(&past, &sha256_hex(secret)).await.unwrap();
        let key = format!("rck_pastpast_{secret}");
        assert!(matches!(
            verify_key(&store, &key).await,
            Err(KeyError::Invalid)
        ));
    }

    #[tokio::test]
    async fn verify_wrong_secret() {
        let (store, _d) = store().await;
        let created = create_key(&store, req("a")).await.unwrap();
        let wrong = format!("rck_{}_{}", created.info.id, "z".repeat(43));
        assert!(matches!(
            verify_key(&store, &wrong).await,
            Err(KeyError::Invalid)
        ));
        let unknown = format!("rck_nosuchid_{}", "z".repeat(43));
        assert!(matches!(
            verify_key(&store, &unknown).await,
            Err(KeyError::Invalid)
        ));
    }

    #[tokio::test]
    async fn stored_hash_is_not_secret() {
        let (store, _d) = store().await;
        let created = create_key(&store, req("a")).await.unwrap();
        let secret = created.key.rsplit('_').next().unwrap();
        let (_, hash) = store
            .key_with_hash(&created.info.id)
            .await
            .unwrap()
            .unwrap();
        assert_ne!(hash, secret);
        assert_eq!(hash, sha256_hex(secret));
        assert_eq!(hash.len(), 64);
    }

    #[tokio::test]
    async fn create_validates_name_and_dedups_scopes() {
        let (store, _d) = store().await;
        for bad in ["", "   ", &"x".repeat(65)] {
            let e = create_key(&store, req(bad)).await.unwrap_err();
            assert_eq!(e.code, reclip_shared::ErrorCode::InvalidRequest);
        }
        let mut r = req(" ok ");
        r.scopes = Some(vec![Scope::Probe, Scope::Probe, Scope::JobsRead]);
        let created = create_key(&store, r).await.unwrap();
        assert_eq!(created.info.name, "ok");
        assert_eq!(created.info.scopes, vec![Scope::Probe, Scope::JobsRead]);
    }

    #[test]
    fn ct_eq_basic() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"abcd"));
        assert!(ct_eq(b"", b""));
    }

    #[test]
    fn base62_len_and_alphabet() {
        let s = random_base62(100).unwrap();
        assert_eq!(s.len(), 100);
        assert!(s.bytes().all(|b| b.is_ascii_alphanumeric()));
    }
}
