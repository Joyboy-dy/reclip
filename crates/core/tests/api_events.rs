//! Live job events (SSE), cancel and delete routes.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use reclip_core::app::{AppState, router, serve};
use reclip_core::config::Config;
use reclip_core::store::Store;
use reclip_shared::{ErrorCode, JobState};
use reqwest::Response;
use serde_json::{Value, json};

struct Server {
    visitor: Visitor,
    base: String,
    downloads: std::path::PathBuf,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

impl std::ops::Deref for Server {
    type Target = Visitor;
    fn deref(&self) -> &Visitor {
        &self.visitor
    }
}

/// One anonymous browser: its own cookie jar.
struct Visitor {
    base: String,
    client: reqwest::Client,
}

async fn start() -> Server {
    let downloads = tempfile::tempdir().unwrap();
    let web = tempfile::tempdir().unwrap();
    std::fs::write(web.path().join("index.html"), "<h1>ReClip</h1>").unwrap();
    let mut cfg = Config::from_env(&|_| None);
    cfg.ytdlp_bin = env!("CARGO_BIN_EXE_fake-ytdlp").to_string();
    cfg.downloads_dir = downloads.path().to_path_buf();
    cfg.web_dir = web.path().to_path_buf();
    cfg.job_timeout = Duration::from_secs(60);
    let dl = cfg.downloads_dir.clone();
    let store = Store::memory().await;
    let state = AppState::new(Arc::new(cfg), store);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router(state)).await.unwrap() });
    Server {
        visitor: Visitor::new(&base),
        base,
        downloads: dl,
        _dirs: (downloads, web),
    }
}

impl Visitor {
    fn new(base: &str) -> Visitor {
        Visitor {
            base: base.to_string(),
            client: reqwest::Client::builder()
                .cookie_store(true)
                .build()
                .unwrap(),
        }
    }
    async fn get(&self, path: &str) -> Response {
        self.client
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap()
    }
    async fn post(&self, path: &str, body: Value) -> Response {
        self.client
            .post(format!("{}{path}", self.base))
            .json(&body)
            .send()
            .await
            .unwrap()
    }
    async fn delete(&self, path: &str) -> Response {
        self.client
            .delete(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap()
    }
    async fn create(&self, url: &str) -> String {
        let r = self
            .post("/api/v1/jobs", json!({"url": url, "format": "mp4"}))
            .await;
        assert_eq!(r.status(), 202);
        r.json::<Value>().await.unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    }
    async fn cancel(&self, id: &str) -> Response {
        self.post(&format!("/api/v1/jobs/{id}/cancel"), json!({}))
            .await
    }
    async fn wait_state(&self, id: &str, want: &str) -> Value {
        let end = Instant::now() + Duration::from_secs(20);
        while Instant::now() < end {
            let j: Value = self
                .get(&format!("/api/v1/jobs/{id}"))
                .await
                .json()
                .await
                .unwrap();
            if j["state"] == want {
                return j;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("job never reached {want}");
    }
    async fn events(&self, id: &str) -> Events {
        let r = self.get(&format!("/api/v1/jobs/{id}/events")).await;
        assert_eq!(r.status(), 200);
        Events::new(r)
    }
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
    s.wait_state(&id, "done").await;
    let evs = s.events(&id).await.all().await;
    assert_eq!(evs.len(), 1);
    assert_eq!(evs[0]["state"], "done");
}

#[tokio::test]
async fn two_subscribers_see_cancellation() {
    let s = start().await;
    let id = s.create("https://slow.example/v").await;
    let mut a = s.events(&id).await;
    let mut b = s.events(&id).await;
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
    drop(s.events(&id).await);
    s.wait_state(&id, "downloading").await;
    let first = s.events(&id).await.next().await.unwrap();
    assert_eq!(first["state"], "downloading");
    s.cancel(&id).await;
}

#[tokio::test]
async fn cancel_is_idempotent() {
    let s = start().await;
    let id = s.create("https://slow.example/v").await;
    s.wait_state(&id, "downloading").await;
    for _ in 0..2 {
        let r = s.cancel(&id).await;
        assert_eq!(r.status(), 200);
        assert_eq!(r.json::<Value>().await.unwrap()["state"], "cancelled");
    }
    let done = s.create("https://ok.example/v").await;
    s.wait_state(&done, "done").await;
    let r = s.cancel(&done).await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.json::<Value>().await.unwrap()["state"], "done");
}

#[tokio::test]
async fn delete_removes_job_and_file() {
    let s = start().await;
    let id = s.create("https://ok.example/v").await;
    s.wait_state(&id, "done").await;
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
    let downloads = tempfile::tempdir().unwrap();
    let web = tempfile::tempdir().unwrap();
    let mut cfg = Config::from_env(&|_| None);
    cfg.ytdlp_bin = env!("CARGO_BIN_EXE_fake-ytdlp").to_string();
    cfg.downloads_dir = downloads.path().to_path_buf();
    cfg.web_dir = web.path().to_path_buf();
    cfg.job_timeout = Duration::from_secs(60);
    // One slot: the second job stays queued, and only the shutdown can end
    // its stream (no worker will ever publish a terminal state for it).
    cfg.max_concurrent = 1;
    let store = Store::memory().await;
    let state = AppState::new(Arc::new(cfg), store.clone());
    let jobs = state.jobs.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let signal = async move {
        let _ = stopped.await;
    };
    let server = tokio::spawn(serve(listener, router(state), jobs, signal));

    let v = Visitor::new(&base);
    let id = v.create("https://slow.example/v").await;
    v.wait_state(&id, "downloading").await;
    let queued = v.create("https://slow.example/q").await;
    let mut events = v.events(&id).await;
    assert!(events.next().await.is_some());
    let mut waiting = v.events(&queued).await;
    assert_eq!(waiting.next().await.unwrap()["state"], "queued");

    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(10), server)
        .await
        .expect("server stops within 10 s despite an open event stream")
        .unwrap()
        .unwrap();
    // Both streams were ended by the server, not left hanging.
    events.all().await;
    waiting.all().await;
    let job = store.get(&id).await.unwrap().unwrap();
    assert_eq!(job.state, JobState::Failed);
    assert_eq!(job.error.unwrap().code, ErrorCode::Interrupted);
    let queued = store.get(&queued).await.unwrap().unwrap();
    assert_eq!(queued.state, JobState::Queued);
}
