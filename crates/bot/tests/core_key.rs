//! A key rejected by the core (401) is re-read from its file once.

use reclip_bot::core_client::{Core, HttpCore};
use serde_json::{Value, json};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn job_json() -> Value {
    json!({
        "id": "j1", "url": "https://v/1", "format": "mp4", "format_id": null, "title": "T",
        "thumbnail": null, "state": "queued", "filename": null, "size": null, "error": null,
        "progress": null, "speed": null, "eta": null, "created_at": "2026-01-01T00:00:00Z",
        "expires_at": null, "clip": null, "strip_metadata": false, "resumable": false,
        "playlist_item": null
    })
}

/// Answers with the job for the `new` key, 401 for anything else.
async fn server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/j1"))
        .and(header("authorization", "Bearer new"))
        .respond_with(ResponseTemplate::new(200).set_body_json(job_json()))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/j1"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "code": "unauthorized", "message": "invalid key"
        })))
        .mount(&server)
        .await;
    server
}

async fn requests(s: &MockServer) -> usize {
    s.received_requests().await.unwrap().len()
}

#[tokio::test]
async fn a_replaced_key_file_is_read_again_after_401() {
    let s = server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bot.key");
    std::fs::write(&file, "old\n").unwrap();
    let core = HttpCore::new(&s.uri(), "old").unwrap().with_key_file(&file);
    // The core wrote a new key since the bot started.
    std::fs::write(&file, "new\n").unwrap();
    assert_eq!(core.get_job(9, "j1").await.unwrap().id, "j1");
    assert_eq!(requests(&s).await, 2);
    // The new key is kept.
    core.get_job(9, "j1").await.unwrap();
    assert_eq!(requests(&s).await, 3);
}

#[tokio::test]
async fn an_unchanged_key_file_gives_the_401() {
    let s = server().await;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bot.key");
    std::fs::write(&file, "old\n").unwrap();
    let core = HttpCore::new(&s.uri(), "old").unwrap().with_key_file(&file);
    let err = core.get_job(9, "j1").await.unwrap_err();
    assert_eq!(err.status, 401);
    assert_eq!(requests(&s).await, 1, "no retry with the same key");

    // A key given directly (no file) is never re-read.
    let s = server().await;
    let core = HttpCore::new(&s.uri(), "old").unwrap();
    assert_eq!(core.get_job(9, "j1").await.unwrap_err().status, 401);
    assert_eq!(requests(&s).await, 1);
}
