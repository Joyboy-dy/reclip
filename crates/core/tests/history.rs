//! Resumable cancelled downloads, the disk budget and the visitor history.

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::{TestServer, Visitor};
use reclip_core::jobs::cleanup;
use reclip_shared::{Job, JobState};
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(20);
const PAST: &str = "2000-01-01T00:00:00.000Z";

async fn wait_for(what: &str, mut ok: impl AsyncFnMut() -> bool) {
    let start = Instant::now();
    while !ok().await {
        assert!(start.elapsed() < DEADLINE, "never {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn part(s: &TestServer, id: &str) -> std::path::PathBuf {
    s.downloads.join(id).join("media.mp4.part")
}

/// Start a `partial` download and wait until its `.part` file is written.
async fn start_partial(s: &TestServer) -> String {
    let id = s.create("https://partial.example/v").await;
    wait_for("wrote a .part", async || part(s, &id).is_file()).await;
    id
}

async fn state(s: &TestServer, id: &str) -> JobState {
    s.store.get(id).await.unwrap().unwrap().state
}

/// Make a finished job due, then sweep.
async fn expire(s: &TestServer, id: &str) {
    s.store.set_expires_at(id, PAST).await.unwrap();
    cleanup::sweep(&s.jobs).await.unwrap();
    assert_eq!(state(s, id).await, JobState::Expired);
}

async fn history(v: &Visitor) -> Vec<Value> {
    let r = v.get("/api/v1/history").await;
    assert_eq!(r.status(), 200);
    let body: Value = r.json().await.unwrap();
    body["entries"].as_array().unwrap().clone()
}

fn ids(entries: &[Value]) -> Vec<&str> {
    entries.iter().map(|e| e["id"].as_str().unwrap()).collect()
}

#[tokio::test]
async fn resume_continues_partial() {
    let s = TestServer::start(|_| {}).await;
    let id = start_partial(&s).await;
    let r = s.cancel(&id).await;
    let job: Value = r.json().await.unwrap();
    assert_eq!(
        (job["state"].as_str(), job["resumable"].as_bool()),
        (Some("cancelled"), Some(true))
    );
    assert!(part(&s, &id).is_file(), "cancel keeps the partial file");

    // Someone else cannot resume it.
    let other = Visitor::new(&s.base);
    let r = other
        .post(&format!("/api/v1/jobs/{id}/resume"), json!({}))
        .await;
    assert_eq!(r.status(), 404);

    let r = s
        .post(&format!("/api/v1/jobs/{id}/resume"), json!({}))
        .await;
    assert_eq!(r.status(), 202);
    let job: Value = r.json().await.unwrap();
    assert_eq!(job["id"], id.as_str());
    assert_eq!(job["state"], "queued");
    assert_eq!(job["resumable"], false);

    let done = s.wait_job(&id, "done").await;
    assert_eq!(done["format"], "mp4");
    let r = s.get(&format!("/api/v1/jobs/{id}/file")).await;
    assert_eq!(r.text().await.unwrap(), "resumed after 7 bytes");

    // A finished job cannot be resumed.
    let r = s
        .post(&format!("/api/v1/jobs/{id}/resume"), json!({}))
        .await;
    assert_eq!(r.status(), 409);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["code"], "invalid_request");
    assert_eq!(body["message"], "This download cannot be resumed");
}

#[tokio::test]
async fn resume_counts_against_the_creation_quota() {
    let s = TestServer::start(|c| c.limits.web.jobs_per_hour = 1).await;
    let id = start_partial(&s).await;
    s.cancel(&id).await;
    let r = s
        .post(&format!("/api/v1/jobs/{id}/resume"), json!({}))
        .await;
    assert_eq!(r.status(), 429);
    assert_eq!(state(&s, &id).await, JobState::Cancelled);
}

#[tokio::test]
async fn cancelled_event_is_resumable() {
    let s = TestServer::start(|_| {}).await;
    let id = start_partial(&s).await;
    let events = s.get(&format!("/api/v1/jobs/{id}/events")).await;
    assert_eq!(events.status(), 200);
    s.cancel(&id).await;
    let text = tokio::time::timeout(DEADLINE, events.text())
        .await
        .unwrap()
        .unwrap();
    let last = text
        .replace("\r\n", "\n")
        .split("\n\n")
        .filter(|b| {
            b.lines()
                .any(|l| l.trim() == "event: job" || l.trim() == "event:job")
        })
        .filter_map(|b| b.lines().find_map(|l| l.strip_prefix("data:")))
        .map(|d| serde_json::from_str::<Job>(d.trim()).unwrap())
        .last()
        .expect("a job event");
    assert_eq!((last.state, last.resumable), (JobState::Cancelled, true));
}

#[tokio::test]
async fn cancel_keeps_partial_remove_deletes() {
    let s = TestServer::start(|_| {}).await;
    let removed = start_partial(&s).await;
    s.cancel(&removed).await;
    assert!(part(&s, &removed).is_file());
    let r = s.delete(&format!("/api/v1/jobs/{removed}")).await;
    assert_eq!(r.status(), 204);
    assert!(!s.downloads.join(&removed).exists());

    // Expiry removes a cancelled job's partial files too.
    let expired = start_partial(&s).await;
    s.cancel(&expired).await;
    expire(&s, &expired).await;
    assert!(!s.downloads.join(&expired).exists());
    let r = s
        .post(&format!("/api/v1/jobs/{expired}/resume"), json!({}))
        .await;
    assert_eq!(r.status(), 409);
}

#[tokio::test]
async fn disk_cap_evicts_then_waits() {
    // Done jobs hold 10 bytes ("fake media"), a cancelled partial one 7.
    let s = TestServer::start(|c| {
        c.max_disk = 25;
        c.max_concurrent = 2;
    })
    .await;
    let a = s.create("https://ok.example/a").await;
    s.wait_job(&a, "done").await;
    let p = start_partial(&s).await;
    s.cancel(&p).await;
    let b = s.create("https://ok.example/b").await;
    s.wait_job(&b, "done").await;

    // 27 bytes: the cancelled partial goes first, before any finished file.
    let c = s.create("https://ok.example/c").await;
    s.wait_job(&c, "done").await;
    assert_eq!(state(&s, &p).await, JobState::Expired);
    assert!(!s.downloads.join(&p).exists());
    assert_eq!(state(&s, &a).await, JobState::Done);

    // 30 bytes: then the oldest finished file.
    let d = s.create("https://ok.example/d").await;
    s.wait_job(&d, "done").await;
    assert_eq!(state(&s, &a).await, JobState::Expired);
    assert!(!s.downloads.join(&a).exists());
    assert_eq!(state(&s, &b).await, JobState::Done);

    // Space nothing can free: every file goes, the next job still waits.
    let stuck = s.downloads.join("ffffffffffffffffffffffffffffffff");
    std::fs::create_dir_all(&stuck).unwrap();
    std::fs::write(stuck.join("big.part"), [0u8; 100]).unwrap();
    let e = s.create("https://ok.example/e").await;
    for id in [&b, &c, &d] {
        wait_for("evicted", async || state(&s, id).await == JobState::Expired).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(state(&s, &e).await, JobState::Queued);

    // The next cleanup sees the room and starts it.
    std::fs::remove_dir_all(&stuck).unwrap();
    cleanup::sweep(&s.jobs).await.unwrap();
    s.wait_job(&e, "done").await;
}

#[tokio::test]
async fn history_lists_hides_and_cleans() {
    let s = TestServer::start(|_| {}).await;
    let mut done = Vec::new();
    for n in 0..3 {
        let id = s.create(&format!("https://ok.example/{n}")).await;
        s.wait_job(&id, "done").await;
        done.push(id);
    }
    let (a, b, c) = (&done[0], &done[1], &done[2]);
    // Not in the history: cancelled jobs, other visitors' jobs, old rows.
    let cancelled = start_partial(&s).await;
    s.cancel(&cancelled).await;
    let owner = s.store.owner_of(a).await.unwrap().unwrap();
    let mut old: Job = s.store.get(a).await.unwrap().unwrap();
    old.id = "0190f5c2a1b47c3d8e9f00112233aa01".into();
    old.created_at = reclip_core::store::now_minus(Duration::from_secs(31 * 86400));
    s.store.insert(&old, &owner, "").await.unwrap();
    assert!(history(&Visitor::new(&s.base)).await.is_empty());

    let entries = history(&s).await;
    assert_eq!(ids(&entries), [c.as_str(), b, a]);
    let first = &entries[0];
    assert_eq!(first["format"], "mp4");
    assert_eq!(first["size"], 10);
    assert_eq!(first["file_available"], true);
    assert_eq!(first["has_thumbnail"], false);
    assert_eq!(first["title"], "Fake ✨ / video");

    expire(&s, a).await;
    let entries = history(&s).await;
    assert_eq!(ids(&entries), [c.as_str(), b, a]);
    assert_eq!(entries[2]["file_available"], false);

    let r = s.delete(&format!("/api/v1/history/{b}")).await;
    assert_eq!(r.status(), 204);
    assert_eq!(
        s.delete(&format!("/api/v1/history/{b}")).await.status(),
        404
    );
    assert_eq!(ids(&history(&s).await), [c.as_str(), a]);
    // Hiding is not deleting: the file is still there.
    assert_eq!(s.get(&format!("/api/v1/jobs/{b}/file")).await.status(), 200);

    let clean = async |body: Value| {
        let r = s.post("/api/v1/history/clean", body).await;
        let status = r.status();
        (status.as_u16(), r.json::<Value>().await.unwrap())
    };
    assert_eq!(
        clean(json!({"mode": "expired"})).await,
        (200, json!({"removed": 1}))
    );
    assert_eq!(ids(&history(&s).await), [c.as_str()]);
    assert_eq!(clean(json!({"mode": "older_than"})).await.0, 400);
    let one_day = json!({"mode": "older_than", "days": 1});
    assert_eq!(clean(one_day).await, (200, json!({"removed": 0})));
    assert_eq!(
        clean(json!({"mode": "all"})).await,
        (200, json!({"removed": 1}))
    );
    assert!(history(&s).await.is_empty());
}

fn thumb_of(data: &Path, id: &str) -> std::path::PathBuf {
    data.join("thumbs").join(format!("{id}.jpg"))
}

#[tokio::test]
async fn thumb_served_and_purged() {
    let s = TestServer::start(|_| {}).await;
    let id = s.create("https://thumb.example/v").await;
    s.wait_job(&id, "done").await;
    assert!(thumb_of(&s.data, &id).is_file());
    assert!(!s.downloads.join(&id).join("thumb.jpg").exists());
    let r = s.get(&format!("/api/v1/jobs/{id}/file")).await;
    assert_eq!(r.text().await.unwrap(), "fake media");
    assert_eq!(history(&s).await[0]["has_thumbnail"], true);

    let url = format!("/api/v1/history/{id}/thumb");
    let r = s.get(&url).await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "image/jpeg");
    assert_eq!(r.headers()["cache-control"], "private, max-age=86400");
    assert_eq!(r.text().await.unwrap(), "fake jpeg");
    assert_eq!(Visitor::new(&s.base).get(&url).await.status(), 404);
    let plain = s.create("https://ok.example/v").await;
    s.wait_job(&plain, "done").await;
    let r = s.get(&format!("/api/v1/history/{plain}/thumb")).await;
    assert_eq!(r.status(), 404);

    // Removing the job removes its thumbnail.
    s.delete(&format!("/api/v1/jobs/{id}")).await;
    assert!(!thumb_of(&s.data, &id).exists());

    // Expired rows (and their thumbnails) go after `history_days`.
    let s = TestServer::start(|c| c.history_days = 0).await;
    let id = s.create("https://thumb.example/v").await;
    s.wait_job(&id, "done").await;
    assert!(thumb_of(&s.data, &id).is_file());
    s.store.set_expires_at(&id, PAST).await.unwrap();
    wait_for("purged", async || {
        cleanup::sweep(&s.jobs).await.unwrap();
        s.store.get(&id).await.unwrap().is_none()
    })
    .await;
    assert!(!thumb_of(&s.data, &id).exists());
}
