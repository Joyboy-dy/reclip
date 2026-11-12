//! Cancelled downloads keep their partial files and can be resumed.

mod common;

use std::time::Duration;

use common::partial::{cancelled_partial, expire, part, start_partial, state, wait_for};
use common::{TestServer, Visitor};
use reclip_core::jobs::JobCounts;
use reclip_shared::{Job, JobState, Scope};
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(20);

fn resume_path(id: &str) -> String {
    format!("/api/v1/jobs/{id}/resume")
}

#[tokio::test]
async fn resume_continues_partial() {
    let s = TestServer::start(|_| {}).await;
    let id = start_partial(&s).await;
    let job: Value = s.cancel(&id).await.json().await.unwrap();
    assert_eq!(
        (job["state"].as_str(), job["resumable"].as_bool()),
        (Some("cancelled"), Some(true))
    );
    assert!(part(&s, &id).is_file(), "cancel keeps the partial file");

    // Someone else cannot resume it.
    let other = Visitor::new(&s.base);
    assert_eq!(other.post(&resume_path(&id), json!({})).await.status(), 404);

    let r = s.post(&resume_path(&id), json!({})).await;
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
    let r = s.post(&resume_path(&id), json!({})).await;
    assert_eq!(r.status(), 409);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["code"], "invalid_request");
    assert_eq!(body["message"], "This download cannot be resumed");
}

#[tokio::test]
async fn resume_counts_against_the_creation_quota() {
    let s = TestServer::start(|c| c.limits.web.jobs_per_hour = 1).await;
    let id = cancelled_partial(&s).await;
    let r = s.post(&resume_path(&id), json!({})).await;
    assert_eq!(r.status(), 429);
    assert_eq!(state(&s, &id).await, JobState::Cancelled);
}

#[tokio::test]
async fn resume_needs_the_create_scope() {
    let s = TestServer::start(|_| {}).await;
    let agent = s.agent(&[Scope::JobsRead, Scope::JobsCancel]).await;
    let r = agent.post(&resume_path("abc"), json!({})).await;
    assert_eq!(r.status(), 403);
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
        .filter(|b| b.lines().any(|l| l.replace(' ', "") == "event:job"))
        .filter_map(|b| b.lines().find_map(|l| l.strip_prefix("data:")))
        .map(|d| serde_json::from_str::<Job>(d.trim()).unwrap())
        .last()
        .expect("a job event");
    assert_eq!((last.state, last.resumable), (JobState::Cancelled, true));
}

#[tokio::test]
async fn cancel_keeps_partial_remove_deletes() {
    let s = TestServer::start(|_| {}).await;
    let removed = cancelled_partial(&s).await;
    assert!(part(&s, &removed).is_file());
    let r = s.delete(&format!("/api/v1/jobs/{removed}")).await;
    assert_eq!(r.status(), 204);
    assert!(!s.downloads.join(&removed).exists());

    // Expiry removes a cancelled job's partial files too.
    let expired = cancelled_partial(&s).await;
    expire(&s, &expired).await;
    assert!(!s.downloads.join(&expired).exists());
    let r = s.post(&resume_path(&expired), json!({})).await;
    assert_eq!(r.status(), 409);
}

#[tokio::test]
async fn delete_racing_resume_leaves_nothing() {
    let s = TestServer::start(|_| {}).await;
    for _ in 0..8 {
        let id = cancelled_partial(&s).await;
        let owner = s.store.owner_of(&id).await.unwrap().unwrap();
        let (_resumed, deleted) =
            tokio::join!(s.jobs.resume(&owner, &id), s.jobs.delete(&owner, &id));
        deleted.unwrap();
        wait_for("workers gone", async || {
            s.jobs.counts() == JobCounts::default()
        })
        .await;
        // Give a late worker the time to recreate the directory.
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(s.store.get(&id).await.unwrap().is_none());
        assert!(!s.downloads.join(&id).exists(), "deleted {id} left a dir");
    }
}
