use super::Store;
use reclip_shared::admin::{ApiKeyInfo, KeyLimits, Scope};
use reclip_shared::api::{Job, JobState, MediaFormat};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::str::FromStr;

fn job(id: &str, state: JobState) -> Job {
    Job {
        id: id.into(),
        url: "https://example.com/v".into(),
        format: MediaFormat::Mp4,
        format_id: None,
        title: None,
        thumbnail: None,
        state,
        filename: None,
        size: None,
        error: None,
        progress: None,
        speed: None,
        eta: None,
        created_at: "2026-11-11T10:00:00.000Z".into(),
        expires_at: None,
    }
}

fn key(id: &str, created_at: &str) -> ApiKeyInfo {
    ApiKeyInfo {
        id: id.into(),
        name: format!("name-{id}"),
        scopes: vec![Scope::Probe, Scope::JobsCreate],
        unlimited: false,
        limits: KeyLimits {
            probes_per_minute: Some(5),
            jobs_per_hour: None,
            max_active: Some(2),
        },
        created_at: created_at.into(),
        expires_at: Some("2027-01-01T00:00:00.000Z".into()),
        revoked_at: None,
        last_used_at: None,
        request_count: 0,
    }
}

#[tokio::test]
async fn count_active_by_quota_key() {
    let store = Store::memory().await;
    let ip = "ip:1.2.3.4";
    for (id, state) in [
        ("a", JobState::Queued),
        ("b", JobState::Queued),
        ("c", JobState::Downloading),
        ("d", JobState::Done),
    ] {
        store.insert(&job(id, state), "web:x", ip).await.unwrap();
    }
    assert_eq!(store.count_active(ip).await.unwrap(), 3);
    assert_eq!(store.count_active("ip:other").await.unwrap(), 0);
}

#[tokio::test]
async fn key_roundtrip_and_revoke() {
    let store = Store::memory().await;
    let info = key("k1", "2026-11-11T10:00:00.000Z");
    store.insert_key(&info, "abc123").await.unwrap();

    let (got, hash) = store.key_with_hash("k1").await.unwrap().unwrap();
    assert_eq!(got, info);
    assert_eq!(hash, "abc123");
    assert!(store.key_with_hash("nope").await.unwrap().is_none());

    assert!(store.revoke_key("k1").await.unwrap());
    let revoked = store.key_with_hash("k1").await.unwrap().unwrap().0;
    assert!(revoked.revoked_at.is_some());
    assert!(!store.revoke_key("unknown").await.unwrap());
}

#[tokio::test]
async fn list_keys_newest_first() {
    let store = Store::memory().await;
    store
        .insert_key(&key("old", "2026-11-11T10:00:00.000Z"), "h1")
        .await
        .unwrap();
    store
        .insert_key(&key("new", "2026-11-11T12:00:00.000Z"), "h2")
        .await
        .unwrap();
    let ids: Vec<_> = store
        .list_keys()
        .await
        .unwrap()
        .into_iter()
        .map(|k| k.id)
        .collect();
    assert_eq!(ids, ["new", "old"]);
}

#[tokio::test]
async fn touch_key_counts() {
    let store = Store::memory().await;
    store
        .insert_key(&key("k1", "2026-11-11T10:00:00.000Z"), "h")
        .await
        .unwrap();
    store.touch_key("k1").await.unwrap();
    store.touch_key("k1").await.unwrap();
    let got = store.key_with_hash("k1").await.unwrap().unwrap().0;
    assert_eq!(got.request_count, 2);
    assert!(got.last_used_at.is_some());
}

#[tokio::test]
async fn settings_set_if_absent() {
    let store = Store::memory().await;
    assert_eq!(store.setting("k").await.unwrap(), None);
    assert!(store.set_setting_if_absent("k", "first").await.unwrap());
    assert!(!store.set_setting_if_absent("k", "second").await.unwrap());
    assert_eq!(store.setting("k").await.unwrap().as_deref(), Some("first"));
    store.set_setting("k", "third").await.unwrap();
    assert_eq!(store.setting("k").await.unwrap().as_deref(), Some("third"));
}

#[tokio::test]
async fn legacy_failed_rows_get_expiry() {
    // A database that only knows migrations 0001-0003.
    let options = SqliteConnectOptions::from_str("sqlite::memory:").unwrap();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .idle_timeout(None)
        .max_lifetime(None)
        .connect_with(options)
        .await
        .unwrap();
    for sql in [
        include_str!("../../migrations/0001_jobs.sql"),
        include_str!("../../migrations/0002_owner_progress.sql"),
        include_str!("../../migrations/0003_legacy_expiry.sql"),
    ] {
        sqlx::raw_sql(sql).execute(&pool).await.unwrap();
    }
    for (id, state) in [("f", "failed"), ("c", "cancelled"), ("q", "queued")] {
        sqlx::query(
            "INSERT INTO jobs (id, url, format, state, created_at) \
             VALUES (?, 'u', 'mp4', ?, '2026-11-11T10:00:00.000Z')",
        )
        .bind(id)
        .bind(state)
        .execute(&pool)
        .await
        .unwrap();
    }
    sqlx::raw_sql(include_str!("../../migrations/0004_access.sql"))
        .execute(&pool)
        .await
        .unwrap();

    let store = Store(pool);
    for id in ["f", "c"] {
        assert!(store.get(id).await.unwrap().unwrap().expires_at.is_some());
    }
    assert!(store.get("q").await.unwrap().unwrap().expires_at.is_none());
}
