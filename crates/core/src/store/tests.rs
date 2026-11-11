use super::Store;
use reclip_shared::api::{Job, JobState, MediaFormat};
use reclip_shared::errors::{ApiError, ErrorCode};
use std::path::{Path, PathBuf};

fn sample(id: &str) -> Job {
    Job {
        id: id.into(),
        url: "https://example.com/v".into(),
        format: MediaFormat::Mp4,
        title: Some("A title".into()),
        state: JobState::Queued,
        filename: Some("a.mp4".into()),
        size: Some(1234),
        error: Some(ApiError {
            code: ErrorCode::Timeout,
            message: "slow".into(),
        }),
        created_at: "2026-11-11T10:00:00+00:00".into(),
    }
}

#[tokio::test]
async fn insert_then_get_round_trips() {
    let store = Store::memory().await;
    let job = sample("j1");
    store.insert(&job).await.unwrap();
    assert_eq!(store.get("j1").await.unwrap(), Some(job));

    let mut mp3 = sample("j2");
    mp3.format = MediaFormat::Mp3;
    mp3.title = None;
    mp3.filename = None;
    mp3.size = None;
    mp3.error = None;
    store.insert(&mp3).await.unwrap();
    assert_eq!(store.get("j2").await.unwrap(), Some(mp3));
}

#[tokio::test]
async fn set_state_records_error() {
    let store = Store::memory().await;
    let mut job = sample("j1");
    job.error = None;
    store.insert(&job).await.unwrap();
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
    store.insert(&job).await.unwrap();
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
    store.insert(&sample("j1")).await.unwrap();
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
        store.insert(&sample("j1")).await.unwrap();
    }
    let store = Store::open(&path).await.unwrap();
    assert!(store.get("j1").await.unwrap().is_some());
}
