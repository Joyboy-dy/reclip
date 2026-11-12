//! Startup recovery and periodic expiry, against the fake yt-dlp.

mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::TestServer;
use reclip_core::config::Config;
use reclip_core::jobs::{JobManager, cleanup, recovery};
use reclip_core::store::Store;
use reclip_shared::{CreateJobRequest, ErrorCode, Job, JobState, MediaFormat};
use serde_json::{Value, json};

const OWNER: &str = "web:0123456789abcdef0123456789abcdef";
const DEADLINE: Duration = Duration::from_secs(20);

fn config(dir: &Path) -> Config {
    let mut cfg = Config::from_env(&|_| None);
    cfg.ytdlp_bin = env!("CARGO_BIN_EXE_fake-ytdlp").to_string();
    cfg.downloads_dir = dir.to_path_buf();
    cfg.job_timeout = Duration::from_secs(60);
    cfg.file_ttl = Duration::from_secs(3600);
    cfg
}

fn job(id: &str, state: JobState, format_id: Option<&str>) -> Job {
    Job {
        id: id.into(),
        url: "https://example.com/v".into(),
        format: MediaFormat::Mp4,
        format_id: format_id.map(String::from),
        title: Some("Fake".into()),
        thumbnail: None,
        state,
        filename: None,
        size: None,
        error: None,
        progress: None,
        speed: None,
        eta: None,
        created_at: reclip_core::store::now(),
        clip: None,
        strip_metadata: false,
        resumable: false,
        expires_at: Some(reclip_core::store::now_plus(Duration::from_secs(3600))),
    }
}

fn make_dir(root: &Path, name: &str) {
    std::fs::create_dir_all(root.join(name)).unwrap();
    std::fs::write(root.join(name).join("partial.part"), "x").unwrap();
}

async fn wait_for(what: &str, mut ok: impl AsyncFnMut() -> bool) {
    let start = Instant::now();
    while !ok().await {
        assert!(start.elapsed() < DEADLINE, "never {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn recover_fails_running_and_requeues_queued() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory().await;
    store
        .insert(&job("run1", JobState::Downloading, None), OWNER, "")
        .await
        .unwrap();
    store
        .insert(&job("queued1", JobState::Queued, Some("137")), OWNER, "")
        .await
        .unwrap();
    make_dir(dir.path(), "run1");
    let mgr = JobManager::new(Arc::new(config(dir.path())), store.clone());

    let report = recovery::recover(&mgr).await.unwrap();
    assert_eq!((report.interrupted, report.requeued), (1, 1));

    let failed = store.get("run1").await.unwrap().unwrap();
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(failed.error.as_ref().unwrap().code, ErrorCode::Interrupted);
    assert!(failed.resumable);
    // Kept for a resume.
    assert!(dir.path().join("run1/partial.part").exists());

    wait_for("done", async || {
        store.get("queued1").await.unwrap().unwrap().state == JobState::Done
    })
    .await;
    let args = std::fs::read_to_string(dir.path().join("queued1.args.txt")).unwrap();
    let args: Vec<&str> = args.lines().collect();
    assert!(
        args.windows(2)
            .any(|w| w == ["-f", "137+bestaudio[ext=m4a]/137+bestaudio/137/best"]),
        "{args:?}"
    );
    mgr.shutdown().await;
}

#[tokio::test]
async fn recover_removes_orphan_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory().await;
    let done = "0190f5c2a1b47c3d8e9f00112233aa01";
    let orphans = [
        "0190f5c2a1b47c3d8e9f00112233aa02",
        "0190f5c2a1b47c3d8e9f00112233aa03",
    ];
    store
        .insert(&job(done, JobState::Done, None), OWNER, "")
        .await
        .unwrap();
    make_dir(dir.path(), done);
    // A cancelled download keeps its partial files for a resume.
    let cancelled = "0190f5c2a1b47c3d8e9f00112233aa05";
    store
        .insert(&job(cancelled, JobState::Cancelled, None), OWNER, "")
        .await
        .unwrap();
    make_dir(dir.path(), cancelled);
    for orphan in orphans {
        make_dir(dir.path(), orphan);
    }
    // Not job-shaped: someone else's data, never touched.
    make_dir(dir.path(), "my-videos");
    make_dir(dir.path(), "0190F5C2A1B47C3D8E9F00112233AA04");
    std::fs::write(dir.path().join("stray.txt"), "keep").unwrap();
    let mgr = JobManager::new(Arc::new(config(dir.path())), store);

    let report = recovery::recover(&mgr).await.unwrap();
    assert_eq!(report.orphan_dirs, 2);
    assert!(dir.path().join(done).join("partial.part").exists());
    assert!(dir.path().join(cancelled).join("partial.part").exists());
    for orphan in orphans {
        assert!(!dir.path().join(orphan).exists());
    }
    assert!(dir.path().join("my-videos/partial.part").exists());
    assert!(dir.path().join("0190F5C2A1B47C3D8E9F00112233AA04").exists());
    assert!(dir.path().join("stray.txt").exists());
    mgr.shutdown().await;
}

#[tokio::test]
async fn cleanup_expires_and_deletes_files() {
    let s = TestServer::start(|c| {
        c.file_ttl = Duration::from_secs(1);
        c.cleanup_interval = Duration::from_millis(200);
    })
    .await;
    let mgr = s.jobs.clone();
    let cleaner = cleanup::spawn_cleanup(mgr.clone());

    let body = json!({"url": "https://example.com/v", "format": "mp4"});
    let r = s.post("/api/v1/jobs", body).await;
    let id = r.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let url = format!("/api/v1/jobs/{id}");
    wait_for("expired", async || {
        let j: Value = s.get(&url).await.json().await.unwrap();
        j["state"] == "expired"
    })
    .await;
    assert!(!s.downloads.join(&id).exists());
    let r = s.get(&format!("{url}/file")).await;
    assert_eq!(r.status(), 404);

    mgr.shutdown().await;
    tokio::time::timeout(DEADLINE, cleaner)
        .await
        .expect("cleanup stops on shutdown")
        .unwrap();
}

#[tokio::test]
async fn failed_and_cancelled_jobs_expire_then_purge() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path());
    cfg.file_ttl = Duration::from_secs(1);
    let store = Store::memory().await;
    let mgr = JobManager::new(Arc::new(cfg), store.clone());
    let req = |url: &str| CreateJobRequest {
        url: url.into(),
        format: MediaFormat::Mp4,
        format_id: None,
        title: None,
        clip: None,
        strip_metadata: false,
        playlist_item: None,
        thumbnail: None,
    };
    let failed = mgr
        .create(OWNER, "", req("https://error-bot.example/v"))
        .await
        .unwrap()
        .id;
    let cancelled = mgr
        .create(OWNER, "", req("https://slow.example/v"))
        .await
        .unwrap()
        .id;
    wait_for("failed", async || {
        store.get(&failed).await.unwrap().unwrap().state == JobState::Failed
    })
    .await;
    let job = mgr.cancel(OWNER, &cancelled).await.unwrap();
    assert_eq!(job.state, JobState::Cancelled);
    for id in [&failed, &cancelled] {
        let job = store.get(id).await.unwrap().unwrap();
        assert!(job.expires_at.is_some(), "{id} has no expiry: {job:?}");
    }

    // Not due yet.
    cleanup::sweep(&mgr).await.unwrap();
    assert_eq!(
        store.get(&failed).await.unwrap().unwrap().state,
        JobState::Failed
    );
    wait_for("expired", async || {
        cleanup::sweep(&mgr).await.unwrap();
        let f = store.get(&failed).await.unwrap().unwrap().state;
        let c = store.get(&cancelled).await.unwrap().unwrap().state;
        f == JobState::Expired && c == JobState::Expired
    })
    .await;
    let later = reclip_core::store::now_plus(Duration::from_secs(60));
    assert_eq!(store.purge_expired(&later).await.unwrap().len(), 2);
    assert_eq!(store.get(&failed).await.unwrap(), None);
    assert_eq!(store.get(&cancelled).await.unwrap(), None);
    mgr.shutdown().await;
}
