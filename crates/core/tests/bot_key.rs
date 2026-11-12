//! The Telegram bot key written by the core at startup.

use std::path::Path;

use reclip_core::access::bot_key::{BOT_KEY_NAME, ensure_bot_key, ensure_configured};
use reclip_core::access::key_usage::KeyUsage;
use reclip_core::access::keys::sha256_hex;
use reclip_core::access::keys::{create_key, verify_key};
use reclip_core::store::Store;
use reclip_shared::{ApiKeyInfo, CreateKeyRequest, KeyLimits, Scope};

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

async fn assert_no_active_keys(store: &Store) {
    let keys = store.list_keys().await.unwrap();
    assert!(keys.iter().all(|k| k.revoked_at.is_some()), "{keys:?}");
}

#[tokio::test]
async fn path_that_is_a_directory_fails_without_a_live_key() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path()).await;
    let path = dir.path().join("bot.key");
    std::fs::create_dir(&path).unwrap();
    let err = ensure_bot_key(&store, &path).await.unwrap_err();
    assert!(format!("{err:#}").contains("bot.key"));
    assert_no_active_keys(&store).await;
}

#[cfg(unix)]
#[tokio::test]
async fn unwritable_directory_fails_without_a_key() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path()).await;
    let locked = dir.path().join("locked");
    std::fs::create_dir(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o500)).unwrap();
    if std::fs::File::create(locked.join("probe")).is_ok() {
        return; // running as root: permissions do not apply
    }
    let path = locked.join("bot.key");
    let err = ensure_bot_key(&store, &path).await.unwrap_err();
    assert!(format!("{err:#}").contains("bot.key"));
    assert_no_active_keys(&store).await;
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[tokio::test]
async fn replaces_an_expired_key() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path()).await;
    let path = dir.path().join("bot.key");
    let secret = "A".repeat(43);
    let info = ApiKeyInfo {
        id: "abcd1234".into(),
        name: "old".into(),
        scopes: vec![Scope::Delegate],
        unlimited: false,
        limits: KeyLimits::default(),
        created_at: "2000-01-01T00:00:00.000Z".into(),
        expires_at: Some("2000-01-02T00:00:00.000Z".into()),
        revoked_at: None,
        last_used_at: None,
        request_count: 0,
    };
    store.insert_key(&info, &sha256_hex(&secret)).await.unwrap();
    let old = format!("rck_abcd1234_{secret}");
    std::fs::write(
        &path,
        format!(
            "{old}
"
        ),
    )
    .unwrap();
    ensure_bot_key(&store, &path).await.unwrap();
    let new = read_key(&path);
    assert_ne!(new, old);
    assert!(valid(&store, &new).await);
}

#[cfg(unix)]
#[tokio::test]
async fn reused_file_is_tightened_to_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path()).await;
    let path = dir.path().join("bot.key");
    ensure_bot_key(&store, &path).await.unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    ensure_bot_key(&store, &path).await.unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    assert_eq!(store.list_keys().await.unwrap().len(), 1);
}

fn plain_request(name: &str) -> CreateKeyRequest {
    CreateKeyRequest {
        name: name.into(),
        scopes: None,
        unlimited: false,
        expires_in_days: None,
        limits: None,
    }
}

#[tokio::test]
async fn new_key_revokes_other_active_bot_keys_only() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path()).await;
    let path = dir.path().join("bot.key");
    let orphan = create_key(&store, plain_request(BOT_KEY_NAME))
        .await
        .unwrap();
    let other = create_key(&store, plain_request("someone-else"))
        .await
        .unwrap();
    // The file is missing, so a new key is made and the orphan is revoked.
    ensure_bot_key(&store, &path).await.unwrap();
    let new = read_key(&path);
    assert!(valid(&store, &new).await);
    assert!(!valid(&store, &orphan.key).await);
    assert!(valid(&store, &other.key).await);
    let active_bot_keys = store
        .list_keys()
        .await
        .unwrap()
        .into_iter()
        .filter(|k| k.name == BOT_KEY_NAME && k.revoked_at.is_none())
        .count();
    assert_eq!(active_bot_keys, 1);
}

#[tokio::test]
async fn a_valid_key_file_revokes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path()).await;
    let path = dir.path().join("bot.key");
    ensure_bot_key(&store, &path).await.unwrap();
    let kept = read_key(&path);
    let extra = create_key(&store, plain_request(BOT_KEY_NAME))
        .await
        .unwrap();
    ensure_bot_key(&store, &path).await.unwrap();
    assert_eq!(read_key(&path), kept);
    assert!(valid(&store, &extra.key).await);
}
