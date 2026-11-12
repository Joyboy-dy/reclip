//! The Telegram bot key written by the core at startup.

use std::path::Path;

use reclip_core::access::bot_key::{BOT_KEY_NAME, ensure_bot_key, ensure_configured};
use reclip_core::access::key_usage::KeyUsage;
use reclip_core::access::keys::{create_key, verify_key};
use reclip_core::store::Store;
use reclip_shared::{CreateKeyRequest, Scope};

async fn store(dir: &Path) -> Store {
    Store::open(&dir.join("t.db")).await.unwrap()
}

fn read_key(path: &Path) -> String {
    let raw = std::fs::read_to_string(path).unwrap();
    assert!(raw.ends_with('\n'));
    raw.trim().to_string()
}

async fn valid(store: &Store, key: &str) -> bool {
    verify_key(store, &KeyUsage::default(), key).await.is_ok()
}

#[tokio::test]
async fn no_path_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path()).await;
    ensure_configured(&store, None).await.unwrap();
    assert!(store.list_keys().await.unwrap().is_empty());
}

#[tokio::test]
async fn creates_a_delegate_key_once() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path()).await;
    let path = dir.path().join("run/reclip/bot.key");
    ensure_bot_key(&store, &path).await.unwrap();
    let first = read_key(&path);
    ensure_bot_key(&store, &path).await.unwrap();
    assert_eq!(read_key(&path), first);
    assert_eq!(store.list_keys().await.unwrap().len(), 1);

    let info = verify_key(&store, &KeyUsage::default(), &first)
        .await
        .unwrap();
    assert_eq!(info.name, BOT_KEY_NAME);
    assert_eq!(
        info.scopes,
        vec![
            Scope::Probe,
            Scope::JobsCreate,
            Scope::JobsRead,
            Scope::JobsCancel,
            Scope::Delegate
        ]
    );
    assert!(!info.unlimited);
    assert_eq!(info.expires_at, None);
}

#[tokio::test]
async fn replaces_a_revoked_key() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path()).await;
    let path = dir.path().join("bot.key");
    ensure_bot_key(&store, &path).await.unwrap();
    let old = read_key(&path);
    let id = store.list_keys().await.unwrap()[0].id.clone();
    store.revoke_key(&id).await.unwrap();
    ensure_bot_key(&store, &path).await.unwrap();
    let new = read_key(&path);
    assert_ne!(new, old);
    assert!(valid(&store, &new).await);
}

#[tokio::test]
async fn replaces_garbage() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path()).await;
    let path = dir.path().join("bot.key");
    std::fs::write(&path, "not a key\n").unwrap();
    ensure_bot_key(&store, &path).await.unwrap();
    assert!(valid(&store, &read_key(&path)).await);
}

#[tokio::test]
async fn replaces_a_key_without_delegate() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path()).await;
    let path = dir.path().join("bot.key");
    let plain = create_key(
        &store,
        CreateKeyRequest {
            name: "x".into(),
            scopes: None,
            unlimited: false,
            expires_in_days: None,
            limits: None,
        },
    )
    .await
    .unwrap();
    std::fs::write(&path, format!("{}\n", plain.key)).unwrap();
    ensure_bot_key(&store, &path).await.unwrap();
    assert_ne!(read_key(&path), plain.key);
}

#[cfg(unix)]
#[tokio::test]
async fn file_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path()).await;
    let path = dir.path().join("bot.key");
    ensure_bot_key(&store, &path).await.unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}
