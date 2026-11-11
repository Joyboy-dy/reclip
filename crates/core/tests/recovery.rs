//! Startup recovery and periodic expiry, against the fake yt-dlp.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reclip_core::app::{AppState, router};
use reclip_core::config::Config;
use reclip_core::jobs::{JobManager, cleanup, recovery};
use reclip_core::store::Store;
use reclip_shared::{ErrorCode, Job, JobState, MediaFormat};
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
        .insert(&job("run1", JobState::Downloading, None), OWNER)
        .await
        .unwrap();
    store
        .insert(&job("queued1", JobState::Queued, Some("137")), OWNER)
        .await
        .unwrap();
    make_dir(dir.path(), "run1");
    let mgr = JobManager::new(Arc::new(config(dir.path())), store.clone());

    let report = recovery::recover(&mgr).await.unwrap();
    assert_eq!((report.interrupted, report.requeued), (1, 1));

    let failed = store.get("run1").await.unwrap().unwrap();
    assert_eq!(failed.state, JobState::Failed);
    assert_eq!(failed.error.unwrap().code, ErrorCode::Interrupted);
    assert!(!dir.path().join("run1").exists());

    wait_for("done", async || {
        store.get("queued1").await.unwrap().unwrap().state == JobState::Done
    })
    .await;
    let args = std::fs::read_to_string(dir.path().join("queued1.args.txt")).unwrap();
    let args: Vec<&str> = args.lines().collect();
    assert!(
        args.windows(2).any(|w| w == ["-f", "137+bestaudio/best"]),
        "{args:?}"
    );
    mgr.shutdown().await;
}

#[tokio::test]
async fn recover_removes_orphan_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::memory().await;
    store
        .insert(&job("done1", JobState::Done, None), OWNER)
        .await
        .unwrap();
    make_dir(dir.path(), "done1");
    make_dir(dir.path(), "orphan1");
    make_dir(dir.path(), "orphan2");
    std::fs::write(dir.path().join("stray.txt"), "keep").unwrap();
    let mgr = JobManager::new(Arc::new(config(dir.path())), store);

    let report = recovery::recover(&mgr).await.unwrap();
    assert_eq!(report.orphan_dirs, 2);
    assert!(dir.path().join("done1/partial.part").exists());
    assert!(!dir.path().join("orphan1").exists());
    assert!(!dir.path().join("orphan2").exists());
    assert!(dir.path().join("stray.txt").exists());
    mgr.shutdown().await;
}

#[tokio::test]
async fn cleanup_expires_and_deletes_files() {
    let downloads = tempfile::tempdir().unwrap();
    let mut cfg = config(downloads.path());
    cfg.file_ttl = Duration::from_secs(1);
    cfg.cleanup_interval = Duration::from_millis(200);
    let web = tempfile::tempdir().unwrap();
    cfg.web_dir = web.path().to_path_buf();
    let state = AppState::new(Arc::new(cfg), Store::memory().await);
    let mgr = state.jobs.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router(state)).await.unwrap() });
    let cleaner = cleanup::spawn_cleanup(mgr.clone());
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .build()
        .unwrap();

    let body = json!({"url": "https://example.com/v", "format": "mp4"});
    let r = client
        .post(format!("{base}/api/v1/jobs"))
        .json(&body)
        .send()
        .await
        .unwrap();
    let id = r.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let url = format!("{base}/api/v1/jobs/{id}");
    wait_for("expired", async || {
        let j: Value = client.get(&url).send().await.unwrap().json().await.unwrap();
        j["state"] == "expired"
    })
    .await;
    assert!(!downloads.path().join(&id).exists());
    let r = client.get(format!("{url}/file")).send().await.unwrap();
    assert_eq!(r.status(), 404);

    mgr.shutdown().await;
    tokio::time::timeout(DEADLINE, cleaner)
        .await
        .expect("cleanup stops on shutdown")
        .unwrap();
}
