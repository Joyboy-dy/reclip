//! Client for the ReClip core API.

use std::fmt;
use std::future::Future;
use std::path::Path;
use std::time::Duration;

use reclip_shared::{
    ApiError, CreateJobRequest, ErrorCode, Job, JobList, LinkResponse, ProbeResponse, PublicConfig,
};
use reqwest::{Method, RequestBuilder, Response};
use serde::de::DeserializeOwned;
use serde_json::json;
use tokio::io::AsyncWriteExt;

/// A core API failure. `status` is 0 when the request never got an answer.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreError {
    pub status: u16,
    pub code: ErrorCode,
    pub message: String,
    /// Seconds, from the `Retry-After` header of a 429.
    pub retry_after: Option<u64>,
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "core error {} ({:?}): {}",
            self.status, self.code, self.message
        )
    }
}

impl std::error::Error for CoreError {}

impl CoreError {
    fn transport(e: reqwest::Error) -> Self {
        Self {
            status: 0,
            code: ErrorCode::Internal,
            message: e.without_url().to_string(),
            retry_after: None,
        }
    }

    fn local(msg: String) -> Self {
        Self {
            status: 0,
            code: ErrorCode::Internal,
            message: msg,
            retry_after: None,
        }
    }

    async fn from_response(resp: Response) -> Self {
        let status = resp.status().as_u16();
        let retry_after = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse().ok());
        let text = resp.text().await.unwrap_or_default();
        match serde_json::from_str::<ApiError>(&text) {
            Ok(e) => Self {
                status,
                code: e.code,
                message: e.message,
                retry_after,
            },
            Err(_) => Self {
                status,
                code: ErrorCode::Internal,
                message: text,
                retry_after,
            },
        }
    }
}

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
    fn cancel_job(&self, user: i64, id: &str)
    -> impl Future<Output = Result<(), CoreError>> + Send;
    fn delete_job(&self, user: i64, id: &str)
    -> impl Future<Output = Result<(), CoreError>> + Send;
    /// Streams the file to `dest` and returns the number of bytes written.
    fn download_file(
        &self,
        user: i64,
        id: &str,
        dest: &Path,
    ) -> impl Future<Output = Result<u64, CoreError>> + Send;
    fn signed_link(
        &self,
        user: i64,
        id: &str,
    ) -> impl Future<Output = Result<LinkResponse, CoreError>> + Send;
}

pub struct HttpCore {
    http: reqwest::Client,
    base: String,
    key: String,
}

impl HttpCore {
    pub fn new(base: &str, key: &str) -> Result<Self, CoreError> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(CoreError::transport)?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').into(),
            key: key.into(),
        })
    }

    fn req(&self, method: Method, path: &str, user: Option<i64>) -> RequestBuilder {
        let mut r = self
            .http
            .request(method, format!("{}/api/v1{path}", self.base))
            .bearer_auth(&self.key)
            .timeout(Duration::from_secs(60));
        if let Some(u) = user {
            r = r.header("X-Reclip-On-Behalf-Of", format!("tg:{u}"));
        }
        r
    }

    async fn send(r: RequestBuilder) -> Result<Response, CoreError> {
        let resp = r.send().await.map_err(CoreError::transport)?;
        if resp.status().is_success() {
            Ok(resp)
        } else {
            Err(CoreError::from_response(resp).await)
        }
    }

    async fn json<T: DeserializeOwned>(r: RequestBuilder) -> Result<T, CoreError> {
        Self::send(r)
            .await?
            .json()
            .await
            .map_err(CoreError::transport)
    }
}

impl Core for HttpCore {
    async fn config(&self) -> Result<PublicConfig, CoreError> {
        Self::json(self.req(Method::GET, "/config", None)).await
    }

    async fn probe(&self, user: i64, url: &str) -> Result<ProbeResponse, CoreError> {
        // Probing can take a while (yt-dlp), so allow more than the default.
        let r = self
            .req(Method::POST, "/probe", Some(user))
            .timeout(Duration::from_secs(180))
            .json(&json!({ "url": url }));
        Self::json(r).await
    }

    async fn create_job(&self, user: i64, req: &CreateJobRequest) -> Result<Job, CoreError> {
        Self::json(self.req(Method::POST, "/jobs", Some(user)).json(req)).await
    }

    async fn get_job(&self, user: i64, id: &str) -> Result<Job, CoreError> {
        Self::json(self.req(Method::GET, &format!("/jobs/{id}"), Some(user))).await
    }

    async fn list_jobs(&self, user: i64) -> Result<Vec<Job>, CoreError> {
        let list: JobList = Self::json(self.req(Method::GET, "/jobs", Some(user))).await?;
        Ok(list.jobs)
    }

    async fn cancel_job(&self, user: i64, id: &str) -> Result<(), CoreError> {
        Self::send(self.req(Method::POST, &format!("/jobs/{id}/cancel"), Some(user)))
            .await
            .map(|_| ())
    }

    async fn delete_job(&self, user: i64, id: &str) -> Result<(), CoreError> {
        Self::send(self.req(Method::DELETE, &format!("/jobs/{id}"), Some(user)))
            .await
            .map(|_| ())
    }

    async fn download_file(&self, user: i64, id: &str, dest: &Path) -> Result<u64, CoreError> {
        let r = self
            .req(Method::GET, &format!("/jobs/{id}/file"), Some(user))
            .timeout(Duration::from_secs(30 * 60));
        let mut resp = Self::send(r).await?;
        let io =
            |e: std::io::Error| CoreError::local(format!("cannot write {}: {e}", dest.display()));
        let mut file = tokio::fs::File::create(dest).await.map_err(io)?;
        let mut total = 0u64;
        while let Some(chunk) = resp.chunk().await.map_err(CoreError::transport)? {
            file.write_all(&chunk).await.map_err(io)?;
            total += chunk.len() as u64;
        }
        file.flush().await.map_err(io)?;
        Ok(total)
    }

    async fn signed_link(&self, user: i64, id: &str) -> Result<LinkResponse, CoreError> {
        Self::json(self.req(Method::POST, &format!("/jobs/{id}/link"), Some(user))).await
    }
}
