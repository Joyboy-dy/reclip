//! Client for the ReClip core API.

mod error;
mod part;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::Duration;

pub use error::CoreError;
use part::{PartGuard, part_path};
use reclip_shared::{CreateJobRequest, Job, JobList, LinkResponse, ProbeResponse, PublicConfig};
use reqwest::{Method, RequestBuilder, Response};
use serde::de::DeserializeOwned;
use serde_json::json;
use tokio::io::AsyncWriteExt;

/// The core calls the bot uses. `user` is the Telegram user id.
pub trait Core: Send + Sync {
    fn config(&self) -> impl Future<Output = Result<PublicConfig, CoreError>> + Send;
    fn probe(
        &self,
        user: i64,
        url: &str,
    ) -> impl Future<Output = Result<ProbeResponse, CoreError>> + Send;
    fn create_job(
        &self,
        user: i64,
        req: &CreateJobRequest,
    ) -> impl Future<Output = Result<Job, CoreError>> + Send;
    fn get_job(&self, user: i64, id: &str) -> impl Future<Output = Result<Job, CoreError>> + Send;
    fn list_jobs(&self, user: i64) -> impl Future<Output = Result<Vec<Job>, CoreError>> + Send;
    /// The job as the core left it: `Cancelled` when it was stopped, its
    /// final state when it had already ended.
    fn cancel_job(
        &self,
        user: i64,
        id: &str,
    ) -> impl Future<Output = Result<Job, CoreError>> + Send;
    fn delete_job(&self, user: i64, id: &str)
    -> impl Future<Output = Result<(), CoreError>> + Send;
    /// Streams the file to `dest` and returns the number of bytes written.
    fn download_file(
        &self,
        user: i64,
        id: &str,
        dest: &Path,
    ) -> impl Future<Output = Result<u64, CoreError>> + Send;
    /// Like [`Core::download_file`], but stops as soon as the file is known
    /// to exceed `max` bytes (`Content-Length` or bytes received): the
    /// partial file is removed and the answer is `Ok(None)`.
    fn download_file_capped(
        &self,
        user: i64,
        id: &str,
        dest: &Path,
        max: u64,
    ) -> impl Future<Output = Result<Option<u64>, CoreError>> + Send;
    fn signed_link(
        &self,
        user: i64,
        id: &str,
    ) -> impl Future<Output = Result<LinkResponse, CoreError>> + Send;
}

pub struct HttpCore {
    http: reqwest::Client,
    base: String,
    key: RwLock<String>,
    /// Where the key came from: read again when the core refuses it.
    key_file: Option<PathBuf>,
}

impl HttpCore {
    pub fn new(base: &str, key: &str) -> Result<Self, CoreError> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(CoreError::transport)?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').into(),
            key: RwLock::new(key.into()),
            key_file: None,
        })
    }

    /// The key was read from `path`: on a 401 the file is read again (the
    /// core writes a new key when the old one was revoked or lost).
    pub fn with_key_file(mut self, path: impl Into<PathBuf>) -> Self {
        self.key_file = Some(path.into());
        self
    }

    fn key(&self) -> String {
        self.key.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Reads the key file again; true when it holds a different key, which
    /// is then used.
    fn reload_key(&self) -> bool {
        let Some(path) = &self.key_file else {
            return false;
        };
        let fresh = match std::fs::read_to_string(path) {
            Ok(raw) => raw.trim().to_string(),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "cannot read the bot key file");
                return false;
            }
        };
        let mut key = self.key.write().unwrap_or_else(|e| e.into_inner());
        if fresh.is_empty() || *key == fresh {
            return false;
        }
        *key = fresh;
        tracing::info!("bot key reloaded from its file");
        true
    }

    fn req(&self, method: Method, path: &str, user: Option<i64>) -> RequestBuilder {
        let mut r = self
            .http
            .request(method, format!("{}/api/v1{path}", self.base))
            .bearer_auth(self.key())
            .timeout(Duration::from_secs(60));
        if let Some(u) = user {
            r = r.header("X-Reclip-On-Behalf-Of", format!("tg:{u}"));
        }
        r
    }

    /// Sends the request `build` makes. When the core refuses the key
    /// (401), the key file is read once more and, if it changed, the
    /// request is made again with the new key.
    async fn send(&self, build: impl Fn() -> RequestBuilder) -> Result<Response, CoreError> {
        let first = Self::send_once(build()).await;
        let retried = match first {
            Err(e) if e.status == 401 && self.reload_key() => Self::send_once(build()).await,
            other => other,
        };
        if let Err(e) = &retried
            && e.status == 401
        {
            tracing::error!(
                "the core rejected the bot key: restart the core (it writes a new key), \
                 or check RECLIP_BOT_KEY"
            );
        }
        retried
    }

    async fn send_once(r: RequestBuilder) -> Result<Response, CoreError> {
        let resp = r.send().await.map_err(CoreError::transport)?;
        if resp.status().is_success() {
            Ok(resp)
        } else {
            Err(CoreError::from_response(resp).await)
        }
    }

    /// Streams the job file to `<dest>.part`, then renames it to `dest`.
    /// With `max`, gives up (`Ok(None)`) once the file is known to be
    /// larger. The partial file is removed on any early exit.
    async fn fetch_file(
        &self,
        user: i64,
        id: &str,
        dest: &Path,
        max: Option<u64>,
    ) -> Result<Option<u64>, CoreError> {
        let file = format!("/jobs/{id}/file");
        let mut resp = self
            .send(|| {
                self.req(Method::GET, &file, Some(user))
                    .timeout(Duration::from_secs(30 * 60))
            })
            .await?;
        let over = |n: u64| max.is_some_and(|m| n > m);
        if resp.content_length().is_some_and(over) {
            return Ok(None);
        }
        let part = part_path(dest);
        // Declared before the file so the file is closed first when dropped.
        let mut guard = PartGuard {
            path: part.clone(),
            committed: false,
        };
        let io =
            |e: std::io::Error| CoreError::local(format!("cannot write {}: {e}", dest.display()));
        let mut file = tokio::fs::File::create(&part).await.map_err(io)?;
        let mut total = 0u64;
        while let Some(chunk) = resp.chunk().await.map_err(CoreError::transport)? {
            total += chunk.len() as u64;
            if over(total) {
                return Ok(None);
            }
            file.write_all(&chunk).await.map_err(io)?;
        }
        file.flush().await.map_err(io)?;
        drop(file);
        tokio::fs::rename(&part, dest).await.map_err(io)?;
        guard.committed = true;
        Ok(Some(total))
    }

    async fn json<T: DeserializeOwned>(
        &self,
        build: impl Fn() -> RequestBuilder,
    ) -> Result<T, CoreError> {
        self.send(build)
            .await?
            .json()
            .await
            .map_err(CoreError::transport)
    }
}

impl Core for HttpCore {
    async fn config(&self) -> Result<PublicConfig, CoreError> {
        self.json(|| self.req(Method::GET, "/config", None)).await
    }

    async fn probe(&self, user: i64, url: &str) -> Result<ProbeResponse, CoreError> {
        // Probing can take a while (yt-dlp), so allow more than the default.
        self.json(|| {
            self.req(Method::POST, "/probe", Some(user))
                .timeout(Duration::from_secs(180))
                .json(&json!({ "url": url }))
        })
        .await
    }

    async fn create_job(&self, user: i64, req: &CreateJobRequest) -> Result<Job, CoreError> {
        self.json(|| self.req(Method::POST, "/jobs", Some(user)).json(req))
            .await
    }

    async fn get_job(&self, user: i64, id: &str) -> Result<Job, CoreError> {
        let path = format!("/jobs/{id}");
        self.json(|| self.req(Method::GET, &path, Some(user))).await
    }

    async fn list_jobs(&self, user: i64) -> Result<Vec<Job>, CoreError> {
        let list: JobList = self
            .json(|| self.req(Method::GET, "/jobs", Some(user)))
            .await?;
        Ok(list.jobs)
    }

    async fn cancel_job(&self, user: i64, id: &str) -> Result<Job, CoreError> {
        let path = format!("/jobs/{id}/cancel");
        self.json(|| self.req(Method::POST, &path, Some(user)))
            .await
    }

    async fn delete_job(&self, user: i64, id: &str) -> Result<(), CoreError> {
        let path = format!("/jobs/{id}");
        self.send(|| self.req(Method::DELETE, &path, Some(user)))
            .await
            .map(|_| ())
    }

    async fn download_file(&self, user: i64, id: &str, dest: &Path) -> Result<u64, CoreError> {
        let n = self.fetch_file(user, id, dest, None).await?;
        Ok(n.unwrap_or(0)) // never None without a cap
    }

    async fn download_file_capped(
        &self,
        user: i64,
        id: &str,
        dest: &Path,
        max: u64,
    ) -> Result<Option<u64>, CoreError> {
        self.fetch_file(user, id, dest, Some(max)).await
    }

    async fn signed_link(&self, user: i64, id: &str) -> Result<LinkResponse, CoreError> {
        let path = format!("/jobs/{id}/link");
        self.json(|| self.req(Method::POST, &path, Some(user)))
            .await
    }
}
