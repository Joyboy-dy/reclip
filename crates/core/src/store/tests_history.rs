//! Store queries behind resume, disk eviction and the history.

use std::path::{Path, PathBuf};
use std::time::Duration;

use reclip_shared::api::{Job, JobState, MediaFormat};
use reclip_shared::errors::{ApiError, ErrorCode};

use super::{Store, now, now_minus, now_plus};

const OWNER: &str = "web:a";

fn job(id: &str, state: JobState, code: Option<ErrorCode>) -> Job {
    Job {
        id: id.into(),
        url: "https://example.com/v".into(),
        format: MediaFormat::Mp4,
        format_id: None,
        title: None,
        thumbnail: None,
        state,
        // Like real rows: only a `done` outcome sets it, expiry keeps it.
        filename: matches!(state, JobState::Done | JobState::Expired).then(|| "f.mp4".into()),
        size: None,
        error: code.map(|code| ApiError {
            code,
            message: "x".into(),
        }),
        progress: None,
        speed: None,
        eta: None,
        created_at: now(),
        expires_at: Some(now_plus(Duration::from_secs(3600))),
        clip: None,
        strip_metadata: false,
        resumable: false,
        playlist_item: None,
    }
}

async fn finish(store: &Store, id: &str, state: JobState, code: Option<ErrorCode>) {
    let error = code.map(|code| ApiError {
        code,
        message: "x".into(),
    });
    let ttl = Duration::from_secs(3600);
    store
        .set_finished(id, state, None, None, error.as_ref(), ttl)
        .await
        .unwrap();
}

#[tokio::test]
async fn requeue_only_resumable_jobs() {
    let store = Store::memory().await;
    let cases = [
        ("cancelled", JobState::Cancelled, None, true),
        (
            "interrupted",
            JobState::Failed,
            Some(ErrorCode::Interrupted),
            true,
        ),
        ("timeout", JobState::Failed, Some(ErrorCode::Timeout), true),
        ("bot", JobState::Failed, Some(ErrorCode::BotCheck), false),
        ("done", JobState::Done, None, false),
        ("expired", JobState::Expired, None, false),
    ];
    for (id, state, code, _) in &cases {
        store
            .insert(&job(id, *state, *code), OWNER, "")
            .await
            .unwrap();
    }
    let mut past = job("past", JobState::Cancelled, None);
    past.expires_at = Some(now_minus(Duration::from_secs(1)));
    store.insert(&past, OWNER, "").await.unwrap();

    for (id, _, _, want) in cases {
        assert_eq!(store.requeue(id).await.unwrap(), want, "{id}");
    }
    assert!(!store.requeue("past").await.unwrap());
    let back = store.get("interrupted").await.unwrap().unwrap();
    assert_eq!(
        (back.state, back.error, back.expires_at),
        (JobState::Queued, None, None)
    );
    // Once queued it is no longer resumable.
    assert!(!store.requeue("interrupted").await.unwrap());
}

#[tokio::test]
async fn eviction_order_and_expire_now() {
    let store = Store::memory().await;
    for id in ["done1", "cancel1", "done2", "fail1", "queued"] {
        store
            .insert(&job(id, JobState::Queued, None), OWNER, "")
            .await
            .unwrap();
    }
    // Finished in this order: stopped downloads still come first.
    finish(&store, "done1", JobState::Done, None).await;
    tokio::time::sleep(Duration::from_millis(5)).await;
    finish(&store, "cancel1", JobState::Cancelled, None).await;
    tokio::time::sleep(Duration::from_millis(5)).await;
    finish(&store, "done2", JobState::Done, None).await;
    tokio::time::sleep(Duration::from_millis(5)).await;
    finish(
        &store,
        "fail1",
        JobState::Failed,
        Some(ErrorCode::Interrupted),
    )
    .await;
    let order = store.eviction_candidates().await.unwrap();
    assert_eq!(order, ["cancel1", "fail1", "done1", "done2"]);

    assert!(store.expire_now("done1").await.unwrap());
    assert_eq!(
        store.get("done1").await.unwrap().unwrap().state,
        JobState::Expired
    );
    assert!(!store.expire_now("done1").await.unwrap());
    assert!(!store.expire_now("queued").await.unwrap());
}

#[tokio::test]
async fn history_window_hiding_and_thumbs() {
    let store = Store::memory().await;
    let since = now_minus(Duration::from_secs(86400));
    for (id, state) in [
        ("a", JobState::Done),
        ("b", JobState::Expired),
        ("c", JobState::Cancelled),
    ] {
        store
            .insert(&job(id, state, None), OWNER, "")
            .await
            .unwrap();
    }
    let mut old = job("old", JobState::Done, None);
    old.created_at = now_minus(Duration::from_secs(2 * 86400));
    store.insert(&old, OWNER, "").await.unwrap();
    store
        .insert(&job("other", JobState::Done, None), "web:b", "")
        .await
        .unwrap();
    store
        .set_file_path("a", Path::new("/d/a/media.mp4"))
        .await
        .unwrap();
    store
        .set_thumb_path("b", Path::new("/t/b.jpg"))
        .await
        .unwrap();

    let rows = store.history(OWNER, &since).await.unwrap();
    let ids: Vec<&str> = rows.iter().map(|(e, _)| e.id.as_str()).collect();
    assert_eq!(ids.len(), 2);
    let (a, path) = rows.iter().find(|(e, _)| e.id == "a").unwrap();
    assert!(a.file_available && !a.has_thumbnail);
    assert_eq!(path.as_deref(), Some(Path::new("/d/a/media.mp4")));
    let (b, _) = rows.iter().find(|(e, _)| e.id == "b").unwrap();
    assert!(!b.file_available && b.has_thumbnail);

    assert!(!store.hide("web:b", &since, "a").await.unwrap());
    assert!(!store.hide(OWNER, &since, "c").await.unwrap());
    assert_eq!(store.hide_many(OWNER, &since, true, None).await.unwrap(), 1);
    assert_eq!(
        store
            .hide_many(OWNER, &since, false, Some(&since))
            .await
            .unwrap(),
        0
    );
    assert!(store.hide(OWNER, &since, "a").await.unwrap());
    assert!(store.history(OWNER, &since).await.unwrap().is_empty());

    assert_eq!(
        store.thumb_path(OWNER, "b").await.unwrap(),
        Some(PathBuf::from("/t/b.jpg"))
    );
    assert_eq!(store.thumb_path("web:b", "b").await.unwrap(), None);
    assert_eq!(
        store.delete("b").await.unwrap(),
        Some(PathBuf::from("/t/b.jpg"))
    );
    assert_eq!(store.delete("b").await.unwrap(), None);
}

#[tokio::test]
async fn purge_returns_thumbnails() {
    let store = Store::memory().await;
    store
        .insert(&job("e", JobState::Queued, None), OWNER, "")
        .await
        .unwrap();
    store
        .set_thumb_path("e", Path::new("/t/e.jpg"))
        .await
        .unwrap();
    finish(&store, "e", JobState::Done, None).await;
    store.expire_now("e").await.unwrap();
    let purged = store
        .purge_expired("9999-01-01T00:00:00.000Z")
        .await
        .unwrap();
    assert_eq!(purged, [Some(PathBuf::from("/t/e.jpg"))]);
}

#[tokio::test]
async fn expired_failed_and_cancelled_jobs_stay_out_of_history() {
    let store = Store::memory().await;
    let since = now_minus(Duration::from_secs(86400));
    for id in ["done", "cancelled", "failed"] {
        store
            .insert(&job(id, JobState::Queued, None), OWNER, "")
            .await
            .unwrap();
    }
    let ttl = Duration::from_secs(3600);
    store
        .set_finished("done", JobState::Done, Some("a.mp4"), Some(1), None, ttl)
        .await
        .unwrap();
    store
        .insert(
            &Job {
                playlist_item: Some(3),
                ..job("item", JobState::Done, None)
            },
            OWNER,
            "",
        )
        .await
        .unwrap();
    finish(&store, "cancelled", JobState::Cancelled, None).await;
    finish(
        &store,
        "failed",
        JobState::Failed,
        Some(ErrorCode::BotCheck),
    )
    .await;
    for id in ["done", "cancelled", "failed"] {
        assert!(store.expire_now(id).await.unwrap(), "{id}");
    }
    let rows = store.history(OWNER, &since).await.unwrap();
    let items: Vec<_> = rows
        .iter()
        .map(|(e, _)| (e.id.as_str(), e.playlist_item))
        .collect();
    assert_eq!(items, [("item", Some(3)), ("done", None)]);
}
