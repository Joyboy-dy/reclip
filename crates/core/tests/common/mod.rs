//! Shared integration-test harness: a real server on a random port, running
//! the fake yt-dlp, with clients for anonymous visitors and API-key agents.

// Each test binary uses its own subset of these helpers.
#![allow(dead_code)]

use std::future::Future;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reclip_core::access::keys::create_key;
use reclip_core::app::{AppState, router, serve};
use reclip_core::config::Config;
use reclip_core::jobs::JobManager;
use reclip_core::store::Store;
use reclip_shared::{CreateKeyRequest, Scope};
use reqwest::Response;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue, ORIGIN};
use serde_json::{Value, json};
use tokio::task::JoinHandle;

const DEADLINE: Duration = Duration::from_secs(20);

pub const ON_BEHALF_OF: &str = "x-reclip-on-behalf-of";

/// A running server. Derefs to a default visitor (cookies + same-site Origin).
pub struct TestServer {
    pub base: String,
    pub store: Store,
    pub jobs: JobManager,
    pub downloads: PathBuf,
    pub visitor: Visitor,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

impl Deref for TestServer {
    type Target = Visitor;
    fn deref(&self) -> &Visitor {
        &self.visitor
    }
}

impl TestServer {
    /// Start a server that runs until the test ends.
    pub async fn start(tweak: impl FnOnce(&mut Config)) -> TestServer {
        Self::start_until(tweak, std::future::pending()).await.0
    }

    /// Start a server that shuts down when `signal` completes.
    pub async fn start_until(
        tweak: impl FnOnce(&mut Config),
        signal: impl Future<Output = ()> + Send + 'static,
    ) -> (TestServer, JoinHandle<std::io::Result<()>>) {
        let downloads = tempfile::tempdir().unwrap();
        let web = tempfile::tempdir().unwrap();
        std::fs::write(web.path().join("index.html"), "<h1>ReClip</h1>").unwrap();
        let mut cfg = Config::from_env(&|_| None);
        cfg.ytdlp_bin = env!("CARGO_BIN_EXE_fake-ytdlp").to_string();
        cfg.downloads_dir = downloads.path().to_path_buf();
        cfg.web_dir = web.path().to_path_buf();
        cfg.job_timeout = Duration::from_secs(60);
        tweak(&mut cfg);
        let downloads_dir = cfg.downloads_dir.clone();
        let store = Store::memory().await;
        let state = AppState::new(Arc::new(cfg), store.clone());
        let jobs = state.jobs.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(serve(listener, router(state), jobs.clone(), signal));
        let s = TestServer {
            visitor: Visitor::new(&base),
            base,
            store,
            jobs,
            downloads: downloads_dir,
            _dirs: (downloads, web),
        };
        (s, server)
    }

    /// Create an API key with `scopes` (`None`: the default set).
    pub async fn key(&self, scopes: Option<Vec<Scope>>) -> String {
        let req = CreateKeyRequest {
            name: "test".into(),
            scopes,
            unlimited: false,
            expires_in_days: None,
            limits: None,
        };
        create_key(&self.store, req).await.unwrap().key
    }

    /// An agent holding a fresh key with exactly `scopes`.
    pub async fn agent(&self, scopes: &[Scope]) -> Agent {
        Agent::new(&self.base, &self.key(Some(scopes.to_vec())).await)
    }
}

/// An HTTP client bound to the server, with the request helpers tests use.
pub struct Api {
    pub base: String,
    pub client: reqwest::Client,
}

impl Api {
    fn build(base: &str, headers: HeaderMap, cookies: bool) -> Api {
        Api {
            base: base.to_string(),
            client: reqwest::Client::builder()
                .cookie_store(cookies)
                .default_headers(headers)
                .build()
                .unwrap(),
        }
    }

    pub async fn get(&self, path: &str) -> Response {
        self.client
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap()
    }

    pub async fn post(&self, path: &str, body: Value) -> Response {
        self.client
            .post(format!("{}{path}", self.base))
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    pub async fn delete(&self, path: &str) -> Response {
        self.client
            .delete(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap()
    }

    /// Create a job (asserting 202) and return its id.
    pub async fn create(&self, url: &str) -> String {
        let r = self
            .post(
                "/api/v1/jobs",
                json!({"url": url, "format": "mp4", "title": "Fake ✨ / video"}),
            )
            .await;
        assert_eq!(r.status(), 202);
        r.json::<Value>().await.unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    pub async fn cancel(&self, id: &str) -> Response {
        self.post(&format!("/api/v1/jobs/{id}/cancel"), json!({}))
            .await
    }

    /// Poll the job until its state is `want`; panics after 20 s.
    pub async fn wait_job(&self, id: &str, want: &str) -> Value {
        let end = Instant::now() + DEADLINE;
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
}

/// One anonymous browser: its own cookie jar, and an `Origin` header.
pub struct Visitor(Api);

impl Deref for Visitor {
    type Target = Api;
    fn deref(&self) -> &Api {
        &self.0
    }
}

impl Visitor {
    /// Sends `Origin: <server base URL>`, like a browser on the site.
    pub fn new(base: &str) -> Visitor {
        Self::with_origin(base, base)
    }

    pub fn with_origin(base: &str, origin: &str) -> Visitor {
        let mut h = HeaderMap::new();
        h.insert(ORIGIN, HeaderValue::from_str(origin).unwrap());
        Visitor(Api::build(base, h, true))
    }

    /// No `Origin` header at all (curl, a script, a cross-site form...).
    pub fn without_origin(base: &str) -> Visitor {
        Visitor(Api::build(base, HeaderMap::new(), true))
    }
}

/// An API client: `Authorization: Bearer <key>`, no cookies, no Origin.
pub struct Agent {
    pub key: String,
    api: Api,
}

impl Deref for Agent {
    type Target = Api;
    fn deref(&self) -> &Api {
        &self.api
    }
}

impl Agent {
    pub fn new(base: &str, key: &str) -> Agent {
        Self::with_headers(base, key, HeaderMap::new())
    }

    fn with_headers(base: &str, key: &str, mut h: HeaderMap) -> Agent {
        let auth = HeaderValue::from_str(&format!("Bearer {key}")).unwrap();
        h.insert(AUTHORIZATION, auth);
        Agent {
            key: key.to_string(),
            api: Api::build(base, h, false),
        }
    }

    /// The key's public id (`rck_<id>_<secret>`).
    pub fn key_id(&self) -> &str {
        &self.key[4..12]
    }

    /// The same key, acting for `who` (`X-Reclip-On-Behalf-Of`).
    pub fn on_behalf_of(&self, who: &str) -> Agent {
        let mut h = HeaderMap::new();
        h.insert(ON_BEHALF_OF, HeaderValue::from_str(who).unwrap());
        Self::with_headers(&self.api.base, &self.key, h)
    }
}
