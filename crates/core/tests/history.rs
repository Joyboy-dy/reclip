//! The disk budget and the visitor history (with thumbnails).

mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use common::partial::{PAST, cancelled_partial, expire, state, wait_for};
use common::{TestServer, Visitor};
use reclip_core::jobs::cleanup;
use reclip_core::store::now_minus;
use reclip_shared::{Job, JobState, Scope};
use serde_json::{Value, json};

const DAY: u64 = 86400;

async fn history(v: &Visitor) -> Vec<Value> {
    let r = v.get("/api/v1/history").await;
    assert_eq!(r.status(), 200);
    let body: Value = r.json().await.unwrap();
    body["entries"].as_array().unwrap().clone()
}

fn ids(entries: &[Value]) -> Vec<&str> {
    entries.iter().map(|e| e["id"].as_str().unwrap()).collect()
}

async fn done(s: &TestServer, url: &str) -> String {
    let id = s.create(url).await;
    s.wait_job(&id, "done").await;
    id
}

#[tokio::test]
async fn disk_cap_evicts_then_waits() {
    // Done jobs hold 10 bytes ("fake media"), a cancelled partial one 7.
    let s = TestServer::start(|c| {
        c.max_disk = 25;
        c.max_concurrent = 2;
    })
    .await;
    let a = done(&s, "https://ok.example/a").await;
    let p = cancelled_partial(&s).await;
    let b = done(&s, "https://ok.example/b").await;

    // 27 bytes: the cancelled partial goes first, before any finished file.
    let c = done(&s, "https://ok.example/c").await;
    assert_eq!(state(&s, &p).await, JobState::Expired);
    assert!(!s.downloads.join(&p).exists());
    assert_eq!(state(&s, &a).await, JobState::Done);

    // 30 bytes: then the oldest finished file.
    let d = done(&s, "https://ok.example/d").await;
    assert_eq!(state(&s, &a).await, JobState::Expired);
    assert!(!s.downloads.join(&a).exists());
    assert_eq!(state(&s, &b).await, JobState::Done);

    // Over budget by more than the files could free: nothing is expired
    // for nothing, the next job waits.
    let stuck = s.downloads.join("ffffffffffffffffffffffffffffffff");
    std::fs::create_dir_all(&stuck).unwrap();
    std::fs::write(stuck.join("big.part"), [0u8; 100]).unwrap();
    let e = s.create("https://ok.example/e").await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(state(&s, &e).await, JobState::Queued);
    for id in [&b, &c, &d] {
        assert_eq!(state(&s, id).await, JobState::Done);
    }

    // The next cleanup sees what can be freed and starts it.
    std::fs::remove_dir_all(&stuck).unwrap();
    cleanup::sweep(&s.jobs).await.unwrap();
    s.wait_job(&e, "done").await;
    assert_eq!(state(&s, &b).await, JobState::Expired);
    assert_eq!(state(&s, &c).await, JobState::Done);
}

#[tokio::test]
async fn history_lists_hides_and_cleans() {
    let s = TestServer::start(|_| {}).await;
    let a = done(&s, "https://ok.example/0").await;
    let b = done(&s, "https://ok.example/1").await;
    let c = done(&s, "https://ok.example/2").await;
    // Not in the history: cancelled jobs, other visitors' jobs, old rows.
    cancelled_partial(&s).await;
    let owner = s.store.owner_of(&a).await.unwrap().unwrap();
    let row: Job = s.store.get(&a).await.unwrap().unwrap();
    let insert = async |id: &str, days: u64| {
        let mut j = row.clone();
        j.id = id.into();
        j.created_at = now_minus(Duration::from_secs(days * DAY));
        s.store.insert(&j, &owner, "").await.unwrap();
    };
    insert("0190f5c2a1b47c3d8e9f00112233aa01", 31).await;
    let two_days = "0190f5c2a1b47c3d8e9f00112233aa02";
    insert(two_days, 2).await;
    assert!(history(&Visitor::new(&s.base)).await.is_empty());

    let entries = history(&s).await;
    assert_eq!(ids(&entries), [c.as_str(), &b, &a, two_days]);
    let first = &entries[0];
    assert_eq!(first["format"], "mp4");
    assert_eq!(first["size"], 10);
    assert_eq!(first["file_available"], true);
    assert_eq!(first["has_thumbnail"], false);
    assert_eq!(first["title"], "Fake ✨ / video");

    expire(&s, &a).await;
    let entries = history(&s).await;
    assert_eq!(ids(&entries), [c.as_str(), &b, &a, two_days]);
    assert_eq!(entries[2]["file_available"], false);

    let hide = format!("/api/v1/history/{b}");
    assert_eq!(s.delete(&hide).await.status(), 204);
    assert_eq!(s.delete(&hide).await.status(), 404);
    assert_eq!(ids(&history(&s).await), [c.as_str(), &a, two_days]);
    // Hiding is not deleting: the file is still there.
    assert_eq!(s.get(&format!("/api/v1/jobs/{b}/file")).await.status(), 200);

    let clean = async |body: Value| {
        let r = s.post("/api/v1/history/clean", body).await;
        let status = r.status();
        (status.as_u16(), r.json::<Value>().await.unwrap())
    };
    let removed = |n: u32| (200, json!({ "removed": n }));
    assert_eq!(clean(json!({"mode": "expired"})).await, removed(1));
    assert_eq!(ids(&history(&s).await), [c.as_str(), two_days]);
    assert_eq!(clean(json!({"mode": "older_than"})).await.0, 400);
    let one_day = json!({"mode": "older_than", "days": 1});
    assert_eq!(clean(one_day).await, removed(1));
    assert_eq!(ids(&history(&s).await), [c.as_str()]);
    assert_eq!(clean(json!({"mode": "all"})).await, removed(1));
    assert!(history(&s).await.is_empty());
}

#[tokio::test]
async fn hiding_needs_the_cancel_scope() {
    let s = TestServer::start(|_| {}).await;
    let agent = s.agent(&[Scope::JobsRead, Scope::JobsCreate]).await;
    assert_eq!(agent.get("/api/v1/history").await.status(), 200);
    assert_eq!(agent.delete("/api/v1/history/abc").await.status(), 403);
    let r = agent
        .post("/api/v1/history/clean", json!({"mode": "all"}))
        .await;
    assert_eq!(r.status(), 403);
}

fn thumb_of(data: &Path, id: &str) -> PathBuf {
    data.join("thumbs").join(format!("{id}.jpg"))
}

#[tokio::test]
async fn thumb_served_and_purged() {
    let s = TestServer::start(|_| {}).await;
    let id = done(&s, "https://thumb.example/v").await;
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
    let plain = done(&s, "https://ok.example/v").await;
    let r = s.get(&format!("/api/v1/history/{plain}/thumb")).await;
    assert_eq!(r.status(), 404);

    // Expired rows (and their thumbnails) go after `history_days`.
    let s = TestServer::start(|c| c.history_days = 0).await;
    let id = done(&s, "https://thumb.example/v").await;
    assert!(thumb_of(&s.data, &id).is_file());
    s.store.set_expires_at(&id, PAST).await.unwrap();
    wait_for("purged", async || {
        cleanup::sweep(&s.jobs).await.unwrap();
        s.store.get(&id).await.unwrap().is_none()
    })
    .await;
    assert!(!thumb_of(&s.data, &id).exists());
}

#[tokio::test]
async fn removing_a_finished_job_keeps_it_in_history() {
    let s = TestServer::start(|_| {}).await;
    let id = done(&s, "https://thumb.example/v").await;
    assert_eq!(s.delete(&format!("/api/v1/jobs/{id}")).await.status(), 204);
    // The file goes, the row stays as `expired` with its thumbnail.
    assert!(!s.downloads.join(&id).exists());
    assert_eq!(state(&s, &id).await, JobState::Expired);
    assert!(thumb_of(&s.data, &id).is_file());
    let entries = history(&s).await;
    assert_eq!(ids(&entries), [id.as_str()]);
    assert_eq!(entries[0]["file_available"], false);
    assert_eq!(entries[0]["has_thumbnail"], true);
    let r = s.get(&format!("/api/v1/jobs/{id}/file")).await;
    assert_eq!(r.status(), 404);
    // It is no longer one of the visitor's jobs, and removing it again is fine.
    let jobs: Value = s.get("/api/v1/jobs").await.json().await.unwrap();
    assert!(jobs["jobs"].as_array().unwrap().is_empty());
    assert_eq!(s.delete(&format!("/api/v1/jobs/{id}")).await.status(), 204);
    assert_eq!(ids(&history(&s).await), [id.as_str()]);
}

#[tokio::test]
async fn removing_an_unfinished_job_deletes_it_and_any_thumbnail() {
    let s = TestServer::start(|_| {}).await;
    let id = cancelled_partial(&s).await;
    // As if a completion racing the removal had moved a thumbnail in.
    let thumb = thumb_of(&s.data, &id);
    std::fs::create_dir_all(thumb.parent().unwrap()).unwrap();
    std::fs::write(&thumb, "fake jpeg").unwrap();
    assert_eq!(s.delete(&format!("/api/v1/jobs/{id}")).await.status(), 204);
    assert!(s.store.get(&id).await.unwrap().is_none());
    assert!(!s.downloads.join(&id).exists());
    assert!(!thumb.exists());
    assert!(history(&s).await.is_empty());
}
