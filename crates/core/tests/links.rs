//! Signed download links: minting, redeeming without credentials, expiry.

mod common;

use std::time::Duration;

use common::{TestServer, Visitor};
use reclip_shared::Scope;
use reqwest::Client;
use serde_json::{Value, json};

async fn mint(s: &TestServer, id: &str) -> Value {
    let r = s.post(&format!("/api/v1/jobs/{id}/link"), json!({})).await;
    assert_eq!(r.status(), 200);
    r.json().await.unwrap()
}

#[tokio::test]
async fn link_downloads_without_cookie() {
    let s = TestServer::start(|_| {}).await;
    let id = s.create("https://ok").await;
    s.wait_job(&id, "done").await;
    let link = mint(&s, &id).await;
    let url = link["url"].as_str().unwrap();
    assert!(url.starts_with(&format!("{}/d/", s.base)), "{url}");

    let anon = Client::new();
    let r = anon.get(url).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["cache-control"], "private, no-store");
    let disposition = r.headers()["content-disposition"].to_str().unwrap();
    assert!(disposition.starts_with("attachment"), "{disposition}");
    let r = anon
        .get(url)
        .header("Range", "bytes=0-3")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 206);
    assert_eq!(r.bytes().await.unwrap().len(), 4);
    assert_eq!(anon.head(url).send().await.unwrap().status(), 200);
}

#[tokio::test]
async fn link_for_unfinished_job_404() {
    let s = TestServer::start(|_| {}).await;
    let id = s.create("https://slow.example/1").await;
    let r = s.post(&format!("/api/v1/jobs/{id}/link"), json!({})).await;
    assert_eq!(r.status(), 404);
}

#[tokio::test]
async fn link_of_other_owner_404() {
    let s = TestServer::start(|_| {}).await;
    let id = s.create("https://ok").await;
    s.wait_job(&id, "done").await;
    let path = format!("/api/v1/jobs/{id}/link");
    let other = Visitor::new(&s.base);
    assert_eq!(other.post(&path, json!({})).await.status(), 404);
    let agent = s.agent(&[Scope::JobsRead]).await;
    assert_eq!(agent.post(&path, json!({})).await.status(), 404);
}

#[tokio::test]
async fn link_capped_by_file_expiry() {
    let s = TestServer::start(|c| {
        c.file_ttl = Duration::from_secs(60);
        c.link_ttl = Duration::from_secs(86400);
    })
    .await;
    let id = s.create("https://ok").await;
    let job = s.wait_job(&id, "done").await;
    let link = mint(&s, &id).await;
    let millis = |v: &Value| {
        chrono::DateTime::parse_from_rfc3339(v.as_str().unwrap())
            .unwrap()
            .timestamp_millis()
    };
    let diff = (millis(&link["expires_at"]) - millis(&job["expires_at"])).abs();
    assert!(diff <= 1000, "diff {diff} ms");
}

#[tokio::test]
async fn link_dead_after_job_deleted() {
    let s = TestServer::start(|_| {}).await;
    let id = s.create("https://ok").await;
    s.wait_job(&id, "done").await;
    let link = mint(&s, &id).await;
    let url = link["url"].as_str().unwrap();
    assert_eq!(s.delete(&format!("/api/v1/jobs/{id}")).await.status(), 204);
    let r = Client::new().get(url).send().await.unwrap();
    assert_eq!(r.status(), 404);
    assert_eq!(
        r.json::<Value>().await.unwrap()["message"],
        "Link expired or invalid"
    );
}

/// The token carries whole seconds: the reported expiry is exactly its `exp`.
#[tokio::test]
async fn reported_expiry_matches_the_token() {
    let s = TestServer::start(|_| {}).await;
    let id = s.create("https://ok").await;
    s.wait_job(&id, "done").await;
    let link = mint(&s, &id).await;
    let at = link["expires_at"].as_str().unwrap();
    assert!(at.ends_with(".000Z"), "{at}");
    let token = link["url"].as_str().unwrap().rsplit('/').next().unwrap();
    let exp: i64 = token.split('.').nth(1).unwrap().parse().unwrap();
    let at = chrono::DateTime::parse_from_rfc3339(at).unwrap();
    assert_eq!(at.timestamp(), exp);
}

/// A file expiry that does not parse refuses the link instead of skipping
/// the cap.
#[tokio::test]
async fn unreadable_file_expiry_fails_closed() {
    let s = TestServer::start(|_| {}).await;
    let id = s.create("https://ok").await;
    s.wait_job(&id, "done").await;
    s.store.set_expires_at(&id, "not a date").await.unwrap();
    let r = s.post(&format!("/api/v1/jobs/{id}/link"), json!({})).await;
    assert_eq!(r.status(), 500);
    assert_eq!(r.json::<Value>().await.unwrap()["code"], "internal");
}
