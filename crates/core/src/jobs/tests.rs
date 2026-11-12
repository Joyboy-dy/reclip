//! Manager behaviour that needs test-only hooks (see `worker::PANIC_URL`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use reclip_shared::{CreateJobRequest, ErrorCode, Job, JobState, MediaFormat};
use tokio::sync::broadcast::error::RecvError;

use super::JobManager;
use super::worker::PANIC_URL;
use crate::config::Config;
use crate::store::Store;

const OWNER: &str = "web:0123456789abcdef0123456789abcdef";
const DEADLINE: Duration = Duration::from_secs(20);

fn req(url: &str) -> CreateJobRequest {
    CreateJobRequest {
        url: url.into(),
        format: MediaFormat::Mp4,
        format_id: None,
        title: None,
        thumbnail: None,
        clip: None,
        strip_metadata: false,
        playlist_item: None,
    }
}

async fn wait_terminal(mgr: &JobManager, id: &str) -> Job {
    let start = Instant::now();
    loop {
        let job = mgr.get(OWNER, id).await.unwrap();
        if crate::store::is_terminal(job.state) {
            return job;
        }
        assert!(start.elapsed() < DEADLINE, "{id} never finished: {job:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn crashed_worker_fails_job_closes_stream_and_frees_slot() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::from_env(&|_| None);
    // No real download is needed: a missing binary fails the second job fast.
    cfg.ytdlp_bin = "reclip-test-no-such-ytdlp".into();
    cfg.downloads_dir = dir.path().to_path_buf();
    cfg.max_concurrent = 1;
    let mgr = JobManager::new(Arc::new(cfg), Store::memory().await);

    let crash = mgr.create(OWNER, "", req(PANIC_URL)).await.unwrap();
    std::fs::create_dir_all(dir.path().join(&crash.id)).unwrap();
    // Synchronous right after `create`: the worker has not been polled yet.
    let (_, mut rx) = mgr.hub().subscribe(&crash.id).unwrap();
    let closed = tokio::time::timeout(DEADLINE, async {
        loop {
            match rx.recv().await {
                Err(RecvError::Closed) => break,
                Err(RecvError::Lagged(_)) | Ok(_) => {}
            }
        }
    });
    closed.await.expect("subscriber stream never closed");

    let job = wait_terminal(&mgr, &crash.id).await;
    assert_eq!(job.state, JobState::Failed);
    let error = job.error.expect("error");
    assert_eq!(
        (error.code, error.message.as_str()),
        (ErrorCode::Internal, "Job crashed")
    );
    assert!(!dir.path().join(&crash.id).exists());

    // The slot was released: the next job still runs (and fails normally).
    let next = mgr
        .create(OWNER, "", req("https://ok.test/"))
        .await
        .unwrap();
    let job = wait_terminal(&mgr, &next.id).await;
    assert_eq!(job.state, JobState::Failed);
    assert_ne!(job.error.unwrap().message, "Job crashed");
}
