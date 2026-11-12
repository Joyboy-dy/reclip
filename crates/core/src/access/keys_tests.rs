//! Tests of API key creation, parsing and verification.

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

#[tokio::test]
async fn create_bounds_expiry() {
    let (store, _d) = store().await;
    for bad in [0, 36_501, u32::MAX] {
        let mut r = req("a");
        r.expires_in_days = Some(bad);
        let e = create_key(&store, r).await.unwrap_err();
        assert_eq!(e.code, reclip_shared::ErrorCode::InvalidRequest, "{bad}");
        assert_eq!(e.message, "expires_in_days must be between 1 and 36500");
    }
    let mut r = req("a");
    r.expires_in_days = Some(36_500);
    let created = create_key(&store, r).await.unwrap();
    let exp = created.info.expires_at.unwrap();
    assert!(
        exp.as_str() > now().as_str() && !exp.starts_with('+'),
        "{exp}"
    );
    assert!(verify_key(&store, &created.key).await.is_ok());
}
