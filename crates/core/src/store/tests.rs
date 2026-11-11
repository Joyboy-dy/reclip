use super::Store;
use reclip_shared::api::{Job, JobState, MediaFormat};
use reclip_shared::errors::{ApiError, ErrorCode};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// `ts` (a store timestamp) plus one hour, in the store format.
fn plus_hour(ts: &str) -> String {
    let at = chrono::DateTime::parse_from_rfc3339(ts).unwrap() + chrono::TimeDelta::hours(1);
    at.with_timezone(&chrono::Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn sample(id: &str) -> Job {
    Job {
        id: id.into(),
        url: "https://example.com/v".into(),
        format: MediaFormat::Mp4,
        format_id: Some("137".into()),
        title: Some("A title".into()),
        thumbnail: Some("https://example.com/t.jpg".into()),
        state: JobState::Queued,
        filename: Some("a.mp4".into()),
        size: Some(1234),
        error: Some(ApiError {
            code: ErrorCode::Timeout,
            message: "slow".into(),
        }),
        progress: None,
        speed: None,
        eta: None,
        created_at: "2026-11-11T10:00:00.000Z".into(),
        expires_at: None,
    }
}

#[tokio::test]
async fn insert_then_get_round_trips() {
    let store = Store::memory().await;
    let job = sample("j1");
    store.insert(&job, "").await.unwrap();
    assert_eq!(store.get("j1").await.unwrap(), Some(job));

    let mut mp3 = sample("j2");
    mp3.format = MediaFormat::Mp3;
    mp3.title = None;
    mp3.filename = None;
    mp3.size = None;
    mp3.error = None;
    mp3.format_id = None;
    mp3.thumbnail = None;
    store.insert(&mp3, "").await.unwrap();
    assert_eq!(store.get("j2").await.unwrap(), Some(mp3));
}

#[tokio::test]
async fn set_state_records_error() {
    let store = Store::memory().await;
    let mut job = sample("j1");
    job.error = None;
    store.insert(&job, "").await.unwrap();
    let err = ApiError {
        code: ErrorCode::BotCheck,
        message: "m".into(),
    };
    store
        .set_state("j1", JobState::Failed, None, None, Some(&err))
        .await
        .unwrap();
    let got = store.get("j1").await.unwrap().unwrap();
    assert_eq!(got.state, JobState::Failed);
    assert_eq!(got.error, Some(err));
}

#[tokio::test]
async fn get_unknown_is_none() {
    let store = Store::memory().await;
    assert_eq!(store.get("nope").await.unwrap(), None);
    assert_eq!(store.file_path("nope").await.unwrap(), None);
}

#[tokio::test]
async fn set_state_updates_file_info_and_finished_at() {
    let store = Store::memory().await;
    let mut job = sample("j1");
    job.filename = None;
    job.size = None;
    job.error = None;
    store.insert(&job, "").await.unwrap();
    store
        .set_state("j1", JobState::Downloading, None, None, None)
        .await
        .unwrap();
    assert!(store.finished_at("j1").await.is_none());
    store
        .set_state("j1", JobState::Done, Some("x.mp4"), Some(9), None)
        .await
        .unwrap();
    let got = store.get("j1").await.unwrap().unwrap();
    assert_eq!(got.state, JobState::Done);
    assert_eq!(got.filename.as_deref(), Some("x.mp4"));
    assert_eq!(got.size, Some(9));
    assert!(store.finished_at("j1").await.is_some());
}

#[tokio::test]
async fn file_path_round_trips() {
    let store = Store::memory().await;
    store.insert(&sample("j1"), "").await.unwrap();
    assert_eq!(store.file_path("j1").await.unwrap(), None);
    store
        .set_file_path("j1", Path::new("/tmp/x.mp4"))
        .await
        .unwrap();
    assert_eq!(
        store.file_path("j1").await.unwrap(),
        Some(PathBuf::from("/tmp/x.mp4"))
    );
}

#[tokio::test]
async fn open_creates_parent_dir_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("reclip.db");
    {
        let store = Store::open(&path).await.unwrap();
        store.insert(&sample("j1"), "").await.unwrap();
    }
    let store = Store::open(&path).await.unwrap();
    assert!(store.get("j1").await.unwrap().is_some());
}

#[tokio::test]
async fn set_state_none_clears_previous_error() {
    let store = Store::memory().await;
    let mut job = sample("j1");
    job.error = None;
    store.insert(&job, "").await.unwrap();
    let err = ApiError {
        code: ErrorCode::BotCheck,
        message: "m".into(),
    };
    store
        .set_state("j1", JobState::Failed, None, None, Some(&err))
        .await
        .unwrap();
    store
        .set_state("j1", JobState::Queued, None, None, None)
        .await
        .unwrap();
    let got = store.get("j1").await.unwrap().unwrap();
    assert_eq!(got.state, JobState::Queued);
    assert_eq!(got.error, None);
}

fn plain(id: &str, created_at: &str) -> Job {
    let mut j = sample(id);
    j.filename = None;
    j.size = None;
    j.error = None;
    j.created_at = created_at.into();
    j
}

#[tokio::test]
async fn insert_keeps_owner_format_id_and_thumbnail() {
    let store = Store::memory().await;
    store.insert(&sample("j1"), "web:a").await.unwrap();
    let got = store.get("j1").await.unwrap().unwrap();
    assert_eq!(got.format_id.as_deref(), Some("137"));
    assert_eq!(got.thumbnail.as_deref(), Some("https://example.com/t.jpg"));
    assert_eq!(got.progress, None);
    assert_eq!(got.speed, None);
    assert_eq!(got.eta, None);
    assert_eq!(
        store.owner_of("j1").await.unwrap().as_deref(),
        Some("web:a")
    );
    assert_eq!(store.owner_of("nope").await.unwrap(), None);
}

#[tokio::test]
async fn get_owned_hides_other_owners() {
    let store = Store::memory().await;
    store.insert(&sample("j1"), "web:a").await.unwrap();
    assert!(store.get_owned("j1", "web:a").await.unwrap().is_some());
    assert_eq!(store.get_owned("j1", "web:b").await.unwrap(), None);
}

#[tokio::test]
async fn list_by_owner_newest_first_without_expired() {
    let store = Store::memory().await;
    store
        .insert(&plain("a", "2026-11-11T10:00:00.000Z"), "web:a")
        .await
        .unwrap();
    store
        .insert(&plain("b", "2026-11-11T11:00:00.000Z"), "web:a")
        .await
        .unwrap();
    store
        .insert(&plain("c", "2026-11-11T12:00:00.000Z"), "web:a")
        .await
        .unwrap();
    store
        .insert(&plain("d", "2026-11-11T13:00:00.000Z"), "web:other")
        .await
        .unwrap();
    store
        .set_state("c", JobState::Expired, None, None, None)
        .await
        .unwrap();
    let ids = |jobs: Vec<Job>| jobs.into_iter().map(|j| j.id).collect::<Vec<_>>();
    assert_eq!(
        ids(store.list_by_owner("web:a", 10).await.unwrap()),
        ["b", "a"]
    );
    assert_eq!(ids(store.list_by_owner("web:a", 1).await.unwrap()), ["b"]);
}

#[tokio::test]
async fn queued_returns_jobs_with_owner_in_creation_order() {
    let store = Store::memory().await;
    store
        .insert(&plain("late", "2026-11-11T12:00:00.000Z"), "web:b")
        .await
        .unwrap();
    store
        .insert(&plain("early", "2026-11-11T10:00:00.000Z"), "web:a")
        .await
        .unwrap();
    let mut running = plain("run", "2026-11-11T11:00:00.000Z");
    running.state = JobState::Downloading;
    store.insert(&running, "web:a").await.unwrap();
    let q = store.queued().await.unwrap();
    let got: Vec<(String, String)> = q.into_iter().map(|(j, o)| (j.id, o)).collect();
    assert_eq!(
        got,
        [
            ("early".into(), "web:a".into()),
            ("late".into(), "web:b".into())
        ]
    );
}

#[tokio::test]
async fn mark_interrupted_fails_running_jobs() {
    let store = Store::memory().await;
    for (id, st) in [
        ("q", JobState::Queued),
        ("d", JobState::Downloading),
        ("p", JobState::Processing),
        ("ok", JobState::Done),
    ] {
        let mut j = plain(id, "2026-11-11T10:00:00.000Z");
        j.state = st;
        store.insert(&j, "web:a").await.unwrap();
    }
    let mut ids = store
        .mark_interrupted(Duration::from_secs(3600))
        .await
        .unwrap();
    ids.sort();
    assert_eq!(ids, ["d", "p"]);
    for id in ["d", "p"] {
        let j = store.get(id).await.unwrap().unwrap();
        assert_eq!(j.state, JobState::Failed);
        let e = j.error.unwrap();
        assert_eq!(e.code, ErrorCode::Interrupted);
        assert_eq!(e.message, "Interrupted by a server restart");
        let finished = store.finished_at(id).await.unwrap();
        assert_eq!(j.expires_at.unwrap(), plus_hour(&finished));
    }
    assert_eq!(
        store.get("q").await.unwrap().unwrap().state,
        JobState::Queued
    );
    assert_eq!(
        store.get("ok").await.unwrap().unwrap().state,
        JobState::Done
    );
}

#[tokio::test]
async fn expire_due_and_purge() {
    let store = Store::memory().await;
    for id in ["old", "future", "noexp"] {
        let mut j = plain(id, "2026-11-11T10:00:00.000Z");
        j.state = JobState::Done;
        store.insert(&j, "web:a").await.unwrap();
        store
            .set_file_path(id, Path::new(&format!("/tmp/{id}.mp4")))
            .await
            .unwrap();
    }
    store
        .set_expires_at("old", "2026-11-11T11:00:00.000Z")
        .await
        .unwrap();
    store
        .set_expires_at("future", "2026-11-12T11:00:00.000Z")
        .await
        .unwrap();
    let due = store.expire_due("2026-11-11T12:00:00.000Z").await.unwrap();
    assert_eq!(
        due,
        [("old".to_string(), Some(PathBuf::from("/tmp/old.mp4")))]
    );
    assert_eq!(
        store.get("old").await.unwrap().unwrap().state,
        JobState::Expired
    );
    assert_eq!(store.file_path("old").await.unwrap(), None);
    assert_eq!(
        store.get("future").await.unwrap().unwrap().state,
        JobState::Done
    );
    assert!(
        store
            .expire_due("2026-11-11T12:00:00.000Z")
            .await
            .unwrap()
            .is_empty()
    );

    let finished = store.finished_at("old").await.unwrap();
    assert_eq!(store.purge_expired(&finished).await.unwrap(), 0);
    assert_eq!(
        store
            .purge_expired("9999-01-01T00:00:00.000Z")
            .await
            .unwrap(),
        1
    );
    assert_eq!(store.get("old").await.unwrap(), None);
    assert!(store.get("future").await.unwrap().is_some());
}

#[tokio::test]
async fn delete_removes_row() {
    let store = Store::memory().await;
    store.insert(&sample("j1"), "").await.unwrap();
    store.delete("j1").await.unwrap();
    assert_eq!(store.get("j1").await.unwrap(), None);
}

#[tokio::test]
async fn set_state_unknown_id_errors() {
    let store = Store::memory().await;
    let err = store
        .set_state("nope", JobState::Done, None, None, None)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("job not found"));
}

#[tokio::test]
async fn requeue_clears_finished_at() {
    let store = Store::memory().await;
    store
        .insert(&plain("j1", "2026-11-11T10:00:00.000Z"), "")
        .await
        .unwrap();
    store
        .set_state("j1", JobState::Failed, None, None, None)
        .await
        .unwrap();
    assert!(store.finished_at("j1").await.is_some());
    store
        .set_state("j1", JobState::Queued, None, None, None)
        .await
        .unwrap();
    assert!(store.finished_at("j1").await.is_none());
}

#[tokio::test]
async fn set_finished_stamps_expiry_from_finished_at() {
    let store = Store::memory().await;
    store
        .insert(&plain("j1", "2026-11-11T10:00:00.000Z"), "web:a")
        .await
        .unwrap();
    let err = ApiError {
        code: ErrorCode::Unavailable,
        message: "gone".into(),
    };
    let at = store
        .set_finished(
            "j1",
            JobState::Failed,
            None,
            None,
            Some(&err),
            Duration::from_secs(3600),
        )
        .await
        .unwrap();
    let j = store.get("j1").await.unwrap().unwrap();
    assert_eq!(j.state, JobState::Failed);
    assert_eq!(j.error.unwrap().code, ErrorCode::Unavailable);
    assert_eq!(j.expires_at.as_deref(), Some(at.as_str()));
    assert_eq!(at, plus_hour(&store.finished_at("j1").await.unwrap()));
    assert!(
        store
            .set_finished("j1", JobState::Queued, None, None, None, Duration::ZERO)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn expire_due_covers_failed_and_cancelled() {
    let store = Store::memory().await;
    for (id, state) in [
        ("f", JobState::Failed),
        ("c", JobState::Cancelled),
        ("q", JobState::Queued),
        ("d", JobState::Downloading),
    ] {
        let mut j = plain(id, "2026-11-11T10:00:00.000Z");
        j.state = state;
        store.insert(&j, "web:a").await.unwrap();
        store
            .set_expires_at(id, "2026-11-11T11:00:00.000Z")
            .await
            .unwrap();
    }
    let mut due = store.expire_due("2026-11-11T12:00:00.000Z").await.unwrap();
    due.sort();
    assert_eq!(due, [("c".to_string(), None), ("f".to_string(), None)]);
    for id in ["f", "c"] {
        assert_eq!(
            store.get(id).await.unwrap().unwrap().state,
            JobState::Expired
        );
    }
    // Live jobs never expire, whatever their expiry says.
    assert_eq!(
        store.get("q").await.unwrap().unwrap().state,
        JobState::Queued
    );
    assert_eq!(
        store.get("d").await.unwrap().unwrap().state,
        JobState::Downloading
    );
    assert_eq!(
        store
            .purge_expired("9999-01-01T00:00:00.000Z")
            .await
            .unwrap(),
        2
    );
    assert_eq!(store.get("f").await.unwrap(), None);
    assert_eq!(store.get("c").await.unwrap(), None);
}

