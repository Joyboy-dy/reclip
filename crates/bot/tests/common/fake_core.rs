//! In-memory core: scripted job states, owner checks like the real core.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use reclip_bot::core_client::{Core, CoreError};
use reclip_shared::*;

use super::job;

#[derive(Default)]
pub struct FakeCore {
    pub probes: Mutex<HashMap<String, Result<ProbeResponse, CoreError>>>,
    /// States a new job goes through (the last one repeats).
    pub script: Mutex<Vec<Job>>,
    /// id -> (owner, remaining states).
    pub jobs: Mutex<HashMap<String, (i64, VecDeque<Job>)>>,
    pub created: Mutex<Vec<(i64, CreateJobRequest)>>,
    pub cancelled: Mutex<Vec<(i64, String)>>,
    pub deleted: Mutex<Vec<(i64, String)>>,
    pub downloads: Mutex<Vec<String>>,
    pub links: Mutex<Vec<String>>,
    /// Bytes written by `download_file`.
    pub file_bytes: Mutex<u64>,
    /// When set, downloads write the file then never finish.
    pub hold_downloads: AtomicBool,
    /// Origin of signed links (default `https://reclip.example`).
    pub link_base: Mutex<Option<String>>,
    /// Id given to the next created job instead of `job<n>`.
    pub next_job_id: Mutex<Option<String>>,
    /// Errors returned by the next downloads, one per call.
    pub download_errors: Mutex<VecDeque<CoreError>>,
    /// Errors returned by the next signed-link requests, one per call.
    pub link_errors: Mutex<VecDeque<CoreError>>,
}

/// The core is unreachable (restart, network).
pub fn unreachable() -> CoreError {
    CoreError {
        status: 0,
        code: ErrorCode::Internal,
        message: "connection refused".into(),
        retry_after: None,
    }
}

pub fn not_found() -> CoreError {
    CoreError {
        status: 404,
        code: ErrorCode::InvalidRequest,
        message: "Job not found".into(),
        retry_after: None,
    }
}

impl FakeCore {
    pub fn probe(&self, url: &str, r: Result<ProbeResponse, CoreError>) {
        self.probes.lock().unwrap().insert(url.into(), r);
    }
    pub fn script(&self, jobs: Vec<Job>) {
        *self.script.lock().unwrap() = jobs;
    }
    /// A job that already exists (bot restart); every state gets the first id.
    pub fn add_job(&self, owner: i64, mut states: Vec<Job>) {
        let id = states[0].id.clone();
        for s in states.iter_mut() {
            s.id = id.clone();
        }
        self.jobs.lock().unwrap().insert(id, (owner, states.into()));
    }
    fn owned(&self, user: i64, id: &str) -> Result<(), CoreError> {
        match self.jobs.lock().unwrap().get(id) {
            Some((owner, _)) if *owner == user => Ok(()),
            _ => Err(not_found()),
        }
    }
}

impl Core for FakeCore {
    async fn config(&self) -> Result<PublicConfig, CoreError> {
        Err(not_found())
    }
    async fn probe(&self, _user: i64, url: &str) -> Result<ProbeResponse, CoreError> {
        let r = self.probes.lock().unwrap().get(url).cloned();
        r.unwrap_or_else(|| {
            Err(CoreError {
                status: 400,
                code: ErrorCode::UnsupportedUrl,
                message: "no".into(),
                retry_after: None,
            })
        })
    }
    async fn create_job(&self, user: i64, req: &CreateJobRequest) -> Result<Job, CoreError> {
        let id = {
            let mut created = self.created.lock().unwrap();
            created.push((user, req.clone()));
            let forced = self.next_job_id.lock().unwrap().take();
            forced.unwrap_or_else(|| format!("job{}", created.len()))
        };
        let mut states: VecDeque<Job> = self.script.lock().unwrap().clone().into();
        if states.is_empty() {
            states.push_back(job("x", JobState::Queued, None, None));
        }
        for j in states.iter_mut() {
            j.id = id.clone();
            j.format = req.format;
        }
        let first = states[0].clone();
        self.jobs.lock().unwrap().insert(id, (user, states));
        Ok(first)
    }
    async fn get_job(&self, user: i64, id: &str) -> Result<Job, CoreError> {
        self.owned(user, id)?;
        let mut jobs = self.jobs.lock().unwrap();
        let q = &mut jobs.get_mut(id).unwrap().1;
        Ok(if q.len() > 1 {
            q.pop_front().unwrap()
        } else {
            q[0].clone()
        })
    }
    async fn list_jobs(&self, _user: i64) -> Result<Vec<Job>, CoreError> {
        Ok(vec![])
    }
    async fn cancel_job(&self, user: i64, id: &str) -> Result<Job, CoreError> {
        self.owned(user, id)?;
        self.cancelled.lock().unwrap().push((user, id.into()));
        let mut jobs = self.jobs.lock().unwrap();
        let states = &mut jobs.get_mut(id).unwrap().1;
        // Like the core: a finished job is returned unchanged.
        let current = states[0].clone();
        if matches!(
            current.state,
            JobState::Queued | JobState::Downloading | JobState::Processing
        ) {
            *states = vec![job(id, JobState::Cancelled, None, None)].into();
            return Ok(states[0].clone());
        }
        Ok(current)
    }
    async fn delete_job(&self, user: i64, id: &str) -> Result<(), CoreError> {
        self.owned(user, id)?;
        self.deleted.lock().unwrap().push((user, id.into()));
        Ok(())
    }
    async fn download_file(&self, user: i64, id: &str, dest: &Path) -> Result<u64, CoreError> {
        let n = self.download_file_capped(user, id, dest, u64::MAX).await?;
        Ok(n.unwrap_or(0))
    }
    async fn download_file_capped(
        &self,
        user: i64,
        id: &str,
        dest: &Path,
        max: u64,
    ) -> Result<Option<u64>, CoreError> {
        self.owned(user, id)?;
        self.downloads.lock().unwrap().push(id.into());
        if let Some(e) = self.download_errors.lock().unwrap().pop_front() {
            return Err(e);
        }
        let n = *self.file_bytes.lock().unwrap();
        if n > max {
            return Ok(None); // the real client stops and removes the part
        }
        std::fs::write(dest, vec![0u8; n as usize]).unwrap();
        if self.hold_downloads.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        Ok(Some(n))
    }
    async fn signed_link(&self, user: i64, id: &str) -> Result<LinkResponse, CoreError> {
        self.owned(user, id)?;
        if let Some(e) = self.link_errors.lock().unwrap().pop_front() {
            return Err(e);
        }
        self.links.lock().unwrap().push(id.into());
        let base = self.link_base.lock().unwrap().clone();
        let base = base.unwrap_or_else(|| "https://reclip.example".into());
        Ok(LinkResponse {
            url: format!("{base}/d/{id}?sig=s"),
            expires_at: "2026-11-13T00:00:00Z".into(),
        })
    }
}
