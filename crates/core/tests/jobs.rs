//! JobManager against the fake yt-dlp: queueing, limits, cancellation, expiry.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reclip_core::config::Config;
use reclip_core::jobs::{JobCounts, JobManager, ManagerError};
use reclip_core::store::Store;
use reclip_shared::{CreateJobRequest, Job, JobState, MediaFormat};

const OWNER: &str = "web:0123456789abcdef0123456789abcdef";
const DEADLINE: Duration = Duration::from_secs(20);

struct Env {
    mgr: JobManager,
    store: Store,
    dir: tempfile::TempDir,
}

async fn env(max_concurrent: usize, max_per_site: usize) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::from_env(&|_| None);
    cfg.ytdlp_bin = env!("CARGO_BIN_EXE_fake-ytdlp").to_string();
    cfg.downloads_dir = dir.path().to_path_buf();
    cfg.job_timeout = Duration::from_secs(120);
    cfg.max_concurrent = max_concurrent;
    cfg.max_per_site = max_per_site;
    cfg.file_ttl = Duration::from_secs(3600);
    let store = Store::memory().await;
    Env {
        mgr: JobManager::new(Arc::new(cfg), store.clone()),
        store,
        dir,
    }
}

fn req(url: &str) -> CreateJobRequest {
    CreateJobRequest {
        url: url.into(),
        format: MediaFormat::Mp4,
        format_id: None,
        title: Some("Fake".into()),
        thumbnail: None,
        clip: None,
        strip_metadata: false,
        playlist_item: None,
    }
}

impl Env {
    async fn create(&self, url: &str) -> String {
        let job = self.mgr.create(OWNER, "", req(url)).await.unwrap();
        assert_eq!(job.state, JobState::Queued);
        assert_eq!(job.expires_at, None);
        job.id
    }

    async fn get(&self, id: &str) -> Job {
        self.mgr.get(OWNER, id).await.unwrap()
    }

    /// Poll until `ok` holds, failing after a generous deadline.
    async fn wait(&self, id: &str, what: &str, ok: impl Fn(&Job) -> bool) -> Job {
        let start = Instant::now();
        loop {
            let job = self.get(id).await;
            if ok(&job) {
                return job;
            }
            assert!(start.elapsed() < DEADLINE, "{id} never {what}: {job:?}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn wait_state(&self, id: &str, state: JobState) -> Job {
        self.wait(id, &format!("{state:?}"), |j| j.state == state)
            .await
    }

    /// Wait until yt-dlp reported progress, i.e. it really is running.
    async fn wait_progress(&self, id: &str) -> Job {
        self.wait(id, "reported progress", |j| {
            j.state == JobState::Downloading && j.progress.is_some_and(|p| p > 0.0)
        })
        .await
    }

    fn job_dir(&self, id: &str) -> PathBuf {
        self.dir.path().join(id)
    }

    async fn stored_state(&self, id: &str) -> JobState {
        self.store.get(id).await.unwrap().unwrap().state
    }
}

#[tokio::test]
async fn second_job_waits_for_the_first() {
    let e = env(1, 2).await;
    let a = e.create("https://slow.example/a").await;
    let b = e.create("https://ok.example/b").await;
    e.wait_progress(&a).await;
    assert_eq!(e.get(&b).await.state, JobState::Queued);
    assert!(!e.job_dir(&b).exists());
    let a_job = e.mgr.cancel(OWNER, &a).await.unwrap();
    assert_eq!(a_job.state, JobState::Cancelled);
    e.wait_state(&b, JobState::Done).await;
}

#[tokio::test]
async fn cancel_queued_job_never_runs() {
    let e = env(1, 2).await;
    let a = e.create("https://slow.example/a").await;
    let b = e.create("https://ok.example/b").await;
    e.wait_progress(&a).await;
    let job = e.mgr.cancel(OWNER, &b).await.unwrap();
    assert_eq!(job.state, JobState::Cancelled);
    assert_eq!(e.stored_state(&b).await, JobState::Cancelled);
    assert_eq!(e.get(&a).await.state, JobState::Downloading);
    // Once `a` is gone the slot is free, yet `b` must not start.
    e.mgr.cancel(OWNER, &a).await.unwrap();
    let c = e.create("https://ok.example/c").await;
    e.wait_state(&c, JobState::Done).await;
    assert_eq!(e.get(&b).await.state, JobState::Cancelled);
    assert!(!e.job_dir(&b).exists());
}

#[tokio::test]
async fn cancel_running_job() {
    let e = env(1, 2).await;
    let a = e.create("https://slow.example/a").await;
    e.wait_progress(&a).await;
    assert!(e.job_dir(&a).exists());
    let job = e.mgr.cancel(OWNER, &a).await.unwrap();
    assert_eq!(job.state, JobState::Cancelled);
    assert_eq!(job.error, None);
    assert_eq!(e.stored_state(&a).await, JobState::Cancelled);
    assert!(!e.job_dir(&a).exists());
    // Cancelling again is a no-op returning the same state.
    let again = e.mgr.cancel(OWNER, &a).await.unwrap();
    assert_eq!(again.state, JobState::Cancelled);
}

#[tokio::test]
async fn done_job_has_expiry() {
    let e = env(1, 2).await;
    let before = chrono::Utc::now();
    let id = e.create("https://ok.example/x").await;
    let job = e.wait_state(&id, JobState::Done).await;
    let after = chrono::Utc::now();
    let at = job.expires_at.clone().expect("expires_at");
    assert!(at.ends_with('Z') && at.len() == 24, "{at}");
    let at = chrono::DateTime::parse_from_rfc3339(&at).unwrap();
    let ttl = chrono::Duration::seconds(3600);
    assert!(at >= before + ttl - chrono::Duration::seconds(1), "{at}");
    assert!(at <= after + ttl, "{at}");
    let stored = e.store.get(&id).await.unwrap().unwrap();
    assert_eq!(stored.expires_at, job.expires_at);
    assert_eq!(stored.state, JobState::Done);
    assert!(e.job_dir(&id).join("media.mp4").exists());
}

#[tokio::test]
async fn per_site_limit_applies() {
    let e = env(4, 1).await;
    let a = e.create("https://slow.example/1").await;
    let b = e.create("https://slow.example/2").await;
    let c = e.create("https://slow.other/3").await;
    e.wait_progress(&a).await;
    // Another site is not blocked by the saturated one.
    e.wait_progress(&c).await;
    assert_eq!(e.get(&b).await.state, JobState::Queued);
    e.mgr.cancel(OWNER, &a).await.unwrap();
    e.wait_progress(&b).await;
    e.mgr.cancel(OWNER, &b).await.unwrap();
    e.mgr.cancel(OWNER, &c).await.unwrap();
}

#[tokio::test]
async fn progress_is_monotonic_in_snapshots() {
    let e = env(1, 2).await;
    let a = e.create("https://slow.example/a").await;
    let mut last = 0.0f32;
    for _ in 0..10 {
        let p = e.wait_progress(&a).await.progress.unwrap();
        assert!(p >= last && p < 100.0, "{p} after {last}");
        last = p;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    e.mgr.cancel(OWNER, &a).await.unwrap();
}

#[tokio::test]
async fn delete_running_job_removes_row_and_dir() {
    let e = env(1, 2).await;
    let a = e.create("https://slow.example/a").await;
    let b = e.create("https://ok.example/b").await;
    e.wait_progress(&a).await;
    e.mgr.delete(OWNER, &a).await.unwrap();
    assert!(e.store.get(&a).await.unwrap().is_none());
    assert!(!e.job_dir(&a).exists());
    assert!(matches!(
        e.mgr.get(OWNER, &a).await,
        Err(ManagerError::NotFound)
    ));
    // Its slot was released.
    e.wait_state(&b, JobState::Done).await;
    e.mgr.delete(OWNER, &b).await.unwrap();
    assert!(!e.job_dir(&b).exists());
    assert!(e.mgr.list(OWNER).await.unwrap().is_empty());
}

#[tokio::test]
async fn other_owner_gets_not_found() {
    let e = env(1, 2).await;
    let id = e.create("https://ok.example/x").await;
    let other = "web:ffffffffffffffffffffffffffffffff";
    assert!(matches!(
        e.mgr.get(other, &id).await,
        Err(ManagerError::NotFound)
    ));
    assert!(matches!(
        e.mgr.cancel(other, &id).await,
        Err(ManagerError::NotFound)
    ));
    assert!(matches!(
        e.mgr.delete(other, &id).await,
        Err(ManagerError::NotFound)
    ));
    assert!(matches!(
        e.mgr.subscribe(other, &id).await,
        Err(ManagerError::NotFound)
    ));
    assert!(e.mgr.list(other).await.unwrap().is_empty());
    e.wait_state(&id, JobState::Done).await;
    assert_eq!(e.mgr.list(OWNER).await.unwrap().len(), 1);
}

#[tokio::test]
async fn subscribe_streams_until_terminal() {
    let e = env(1, 2).await;
    let id = e.create("https://ok.example/x").await;
    let sub = e.mgr.subscribe(OWNER, &id).await.unwrap();
    let mut last = sub.snapshot.state;
    if let Some(mut rx) = sub.updates {
        while let Ok(job) = rx.recv().await {
            last = job.state;
        }
    }
    assert_eq!(last, JobState::Done);
    // A finished job gives its snapshot and no stream.
    let sub = e.mgr.subscribe(OWNER, &id).await.unwrap();
    assert_eq!(sub.snapshot.state, JobState::Done);
    assert!(sub.updates.is_none());
}

#[tokio::test]
async fn enqueue_twice_runs_once() {
    let e = env(1, 2).await;
    let a = e.create("https://slow.example/a").await;
    let b = e.create("https://ok.example/b").await;
    e.wait_progress(&a).await;
    let running = e.store.get(&a).await.unwrap().unwrap();
    let queued = e.store.get(&b).await.unwrap().unwrap();
    // Re-enqueueing a running or queued job is ignored.
    e.mgr.enqueue(running, OWNER);
    e.mgr.enqueue(queued, OWNER);
    assert_eq!(
        e.mgr.counts(),
        JobCounts {
            queued: 1,
            running: 1
        }
    );
    e.mgr.cancel(OWNER, &a).await.unwrap();
    e.wait_state(&b, JobState::Done).await;
    let start = Instant::now();
    while e.mgr.counts() != JobCounts::default() {
        assert!(start.elapsed() < DEADLINE, "{:?}", e.mgr.counts());
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(e.get(&a).await.state, JobState::Cancelled);
}
