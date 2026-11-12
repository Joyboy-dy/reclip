//! Helpers for stopped downloads: the fake yt-dlp's `partial` URLs leave a
//! `media.mp4.part` and run until stopped (see `bin/fake-ytdlp.rs`).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use reclip_core::jobs::cleanup;
use reclip_shared::JobState;

use super::TestServer;

const DEADLINE: Duration = Duration::from_secs(20);
/// An expiry already due.
pub const PAST: &str = "2000-01-01T00:00:00.000Z";

pub async fn wait_for(what: &str, mut ok: impl AsyncFnMut() -> bool) {
    let start = Instant::now();
    while !ok().await {
        assert!(start.elapsed() < DEADLINE, "never {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

pub fn part(s: &TestServer, id: &str) -> PathBuf {
    s.downloads.join(id).join("media.mp4.part")
}

/// Start a `partial` download and wait until its `.part` file is written.
pub async fn start_partial(s: &TestServer) -> String {
    let id = s.create("https://partial.example/v").await;
    wait_for("wrote a .part", async || part(s, &id).is_file()).await;
    id
}

/// [`start_partial`], then cancel it.
pub async fn cancelled_partial(s: &TestServer) -> String {
    let id = start_partial(s).await;
    assert_eq!(s.cancel(&id).await.status(), 200);
    id
}

pub async fn state(s: &TestServer, id: &str) -> JobState {
    s.store.get(id).await.unwrap().unwrap().state
}

/// Make a finished job due, then sweep.
pub async fn expire(s: &TestServer, id: &str) {
    s.store.set_expires_at(id, PAST).await.unwrap();
    cleanup::sweep(&s.jobs).await.unwrap();
    assert_eq!(state(s, id).await, JobState::Expired);
}
