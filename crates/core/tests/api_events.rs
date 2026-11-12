//! Live job events (SSE), cancel and delete routes.

mod common;

use std::time::{Duration, Instant};

use common::{Api, TestServer, Visitor};
use futures_util::StreamExt;
use reclip_shared::{ErrorCode, JobState};
use reqwest::Response;
use serde_json::Value;

async fn start() -> TestServer {
    TestServer::start(|_| {}).await
}

/// Open the job's event stream (asserting 200).
async fn events(v: &Api, id: &str) -> Events {
    let r = v.get(&format!("/api/v1/jobs/{id}/events")).await;
    assert_eq!(r.status(), 200);
    Events::new(r)
}

type ByteStream =
    std::pin::Pin<Box<dyn futures_util::Stream<Item = reqwest::Result<bytes::Bytes>>>>;

/// Raw SSE reader: parses `event:`/`data:` lines out of the byte stream.
struct Events {
    stream: ByteStream,
    buf: String,
}

impl Events {
    fn new(r: Response) -> Events {
        Events {
            stream: Box::pin(r.bytes_stream()),
            buf: String::new(),
        }
    }
    /// Next `job` event, `None` once the stream ends. Panics after 20 s.
    async fn next(&mut self) -> Option<Value> {
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            while let Some(i) = self.buf.find("\n\n") {
                let block: String = self.buf.drain(..i + 2).collect();
                let mut event = "";
                let mut data = String::new();
                for line in block.lines() {
                    if let Some(v) = line.strip_prefix("event:") {
                        event = v.trim();
                    } else if let Some(v) = line.strip_prefix("data:") {
                        data.push_str(v.trim());
                    }
                }
                if event == "job" {
                    return Some(serde_json::from_str(&data).unwrap());
                }
            }
            let left = end.saturating_duration_since(Instant::now());
            let chunk = tokio::time::timeout(left, self.stream.next())
                .await
                .expect("timed out waiting for SSE data");
            match chunk {
                Some(c) => {
                    let c = c.unwrap();
                    self.buf
                        .push_str(&std::str::from_utf8(&c).unwrap().replace("\r\n", "\n"));
                }
                None => return None,
            }
        }
    }
    async fn all(&mut self) -> Vec<Value> {
        let mut v = Vec::new();
        while let Some(e) = self.next().await {
            v.push(e);
        }
        v
    }
}

#[tokio::test]
async fn events_stream_snapshot_then_done() {
    let s = start().await;
    let id = s.create("https://ok.example/v").await;
    let r = s.get(&format!("/api/v1/jobs/{id}/events")).await;
    assert_eq!(r.status(), 200);
    assert!(
        r.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    assert_eq!(r.headers()["x-accel-buffering"], "no");
    assert_eq!(r.headers()["cache-control"], "no-store");
    let evs = Events::new(r).all().await;
    let first = evs.first().unwrap()["state"].as_str().unwrap();
    assert!(
        ["queued", "downloading", "processing", "done"].contains(&first),
        "{first}"
    );
    assert_eq!(evs.last().unwrap()["state"], "done");
    assert_eq!(evs.last().unwrap()["id"], id);
}

#[tokio::test]
async fn events_for_finished_job_send_one_and_close() {
    let s = start().await;
    let id = s.create("https://ok.example/v").await;
    s.wait_job(&id, "done").await;
    let evs = events(&s, &id).await.all().await;
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0]["state"], "done");
}

#[tokio::test]
async fn two_subscribers_see_cancellation() {
    let s = start().await;
    let id = s.create("https://slow.example/v").await;
    let mut a = events(&s, &id).await;
    let mut b = events(&s, &id).await;
    assert!(a.next().await.is_some());
    assert!(b.next().await.is_some());
    assert_eq!(s.cancel(&id).await.status(), 200);
    for ev in [a.all().await, b.all().await] {
        assert_eq!(ev.last().unwrap()["state"], "cancelled");
    }
}

#[tokio::test]
async fn reconnect_starts_with_current_snapshot() {
    let s = start().await;
    let id = s.create("https://slow.example/v").await;
    drop(events(&s, &id).await);
    s.wait_job(&id, "downloading").await;
    let first = events(&s, &id).await.next().await.unwrap();
    assert_eq!(first["state"], "downloading");
    s.cancel(&id).await;
}

#[tokio::test]
async fn cancel_is_idempotent() {
    let s = start().await;
    let id = s.create("https://slow.example/v").await;
    s.wait_job(&id, "downloading").await;
    for _ in 0..2 {
        let r = s.cancel(&id).await;
        assert_eq!(r.status(), 200);
        assert_eq!(r.json::<Value>().await.unwrap()["state"], "cancelled");
    }
    let done = s.create("https://ok.example/v").await;
    s.wait_job(&done, "done").await;
    let r = s.cancel(&done).await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.json::<Value>().await.unwrap()["state"], "done");
}

#[tokio::test]
async fn delete_removes_job_and_file() {
    let s = start().await;
    let id = s.create("https://ok.example/v").await;
    s.wait_job(&id, "done").await;
    assert!(s.downloads.join(&id).exists());
    let r = s.delete(&format!("/api/v1/jobs/{id}")).await;
    assert_eq!(r.status(), 204);
    assert!(r.bytes().await.unwrap().is_empty());
    assert_eq!(s.get(&format!("/api/v1/jobs/{id}")).await.status(), 404);
    assert!(!s.downloads.join(&id).exists());
}

#[tokio::test]
async fn other_visitor_gets_404_everywhere() {
    let s = start().await;
    let id = s.create("https://ok.example/v").await;
    let other = Visitor::new(&s.base);
    let events = other.get(&format!("/api/v1/jobs/{id}/events")).await;
    assert_eq!(events.status(), 404);
    assert_eq!(other.cancel(&id).await.status(), 404);
    let del = other.delete(&format!("/api/v1/jobs/{id}")).await;
    assert_eq!(del.status(), 404);
    assert_eq!(s.get(&format!("/api/v1/jobs/{id}")).await.status(), 200);
}

#[tokio::test]
async fn api_responses_are_not_cached() {
    let s = start().await;
    let r = s.get("/api/v1/jobs").await;
    assert_eq!(r.headers()["cache-control"], "no-store");
}

#[tokio::test]
async fn shutdown_ends_open_event_streams() {
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let signal = async move {
        let _ = stopped.await;
    };
    // One slot: the second job stays queued, and only the shutdown can end
    // its stream (no worker will ever publish a terminal state for it).
    let (s, server) = TestServer::start_until(|c| c.max_concurrent = 1, signal).await;

    let v = Visitor::new(&s.base);
    let id = v.create("https://slow.example/v").await;
    v.wait_job(&id, "downloading").await;
    let queued = v.create("https://slow.example/q").await;
    let mut running = events(&v, &id).await;
    assert!(running.next().await.is_some());
    let mut waiting = events(&v, &queued).await;
    assert_eq!(waiting.next().await.unwrap()["state"], "queued");

    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), server)
        .await
        .expect("server stops within 10 s despite an open event stream")
        .unwrap()
        .unwrap();
    // Both streams were ended by the server, not left hanging.
    running.all().await;
    waiting.all().await;
    let job = s.store.get(&id).await.unwrap().unwrap();
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.error.unwrap().code, ErrorCode::Interrupted);
    let queued = s.store.get(&queued).await.unwrap().unwrap();
    assert_eq!(queued.state, JobState::Queued);
}
