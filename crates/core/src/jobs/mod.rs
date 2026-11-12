//! Job lifecycle: a fair bounded queue, a live hub and download workers.
//!
//! Scheduling state (queue, running slots, per-site counters) sits behind a
//! `std::sync::Mutex` that is never held across an `.await`. Scheduling runs
//! on create and resume, when a worker finishes, when a queued job is
//! cancelled or a job deleted, after a cleanup sweep and after disk evictions
//! (see `disk.rs`).
//! Running downloads are only ever stopped through their `CancellationToken`,
//! and callers that need the worker gone await its `finished` token: dropping
//! a download future would orphan yt-dlp's children.

pub mod cleanup;
mod delete;
mod disk;
pub mod hub;
pub mod progress_tracker;
pub mod queue;
pub mod recovery;
mod resume;
mod sched;
#[cfg(test)]
mod tests;
mod worker;

use std::sync::{Arc, Mutex};

use reclip_shared::{ApiError, CreateJobRequest, Job, JobState};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use sched::{Sched, Tracked};

pub use hub::Hub;
pub use queue::{Queue, site_of};
pub use resume::not_resumable;
pub use sched::JobCounts;

use crate::config::Config;
use crate::store::Store;

/// Jobs listed per owner.
const LIST_LIMIT: u32 = 100;

/// Failure of an owner-scoped manager call.
#[derive(Debug)]
pub enum ManagerError {
    /// No such job, or it belongs to someone else (the HTTP layer answers 404).
    NotFound,
    Api(ApiError),
}

impl From<ApiError> for ManagerError {
    fn from(e: ApiError) -> Self {
        ManagerError::Api(e)
    }
}

/// Store/IO failure: logged, reported as a generic internal error.
fn internal(e: impl std::fmt::Display) -> ApiError {
    crate::http::internal(e).0
}

/// What [`JobManager::subscribe`] returns.
#[derive(Debug)]
pub struct Subscription {
    /// Current state: first thing to send.
    pub snapshot: Job,
    /// Later updates, closed after the terminal one. `None` when the job is
    /// not live (already finished): send the snapshot and stop.
    pub updates: Option<broadcast::Receiver<Job>>,
}

struct Inner {
    cfg: Arc<Config>,
    store: Store,
    hub: Hub,
    sched: Mutex<Sched>,
    disk: disk::Disk,
    /// Cancelled by `shutdown`, for background loops.
    shutdown: CancellationToken,
}

/// Owns job scheduling and execution. Cheap to clone.
#[derive(Clone)]
pub struct JobManager {
    inner: Arc<Inner>,
}

impl JobManager {
    pub fn new(cfg: Arc<Config>, store: Store) -> JobManager {
        let inner = Inner {
            sched: Mutex::new(Sched::new(&cfg)),
            disk: disk::Disk::default(),
            cfg,
            store,
            hub: Hub::new(),
            shutdown: CancellationToken::new(),
        };
        JobManager {
            inner: Arc::new(inner),
        }
    }

    pub fn config(&self) -> &Config {
        &self.inner.cfg
    }

    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    /// Cancelled when [`JobManager::shutdown`] starts.
    pub fn shutdown_token(&self) -> CancellationToken {
        self.inner.shutdown.clone()
    }

    pub fn hub(&self) -> &Hub {
        &self.inner.hub
    }

    /// Store a new `queued` job for `owner`, counted against `quota_key`, and schedule it.
    pub async fn create(
        &self,
        owner: &str,
        quota_key: &str,
        req: CreateJobRequest,
    ) -> Result<Job, ApiError> {
        let job = Job {
            id: uuid::Uuid::now_v7().simple().to_string(),
            url: req.url,
            format: req.format,
            format_id: req.format_id,
            title: req.title,
            thumbnail: req.thumbnail,
            state: JobState::Queued,
            filename: None,
            size: None,
            error: None,
            progress: None,
            speed: None,
            eta: None,
            created_at: crate::store::now(),
            expires_at: None,
            clip: req.clip,
            strip_metadata: req.strip_metadata,
            resumable: false,
            playlist_item: req.playlist_item,
        };
        self.store()
            .insert(&job, owner, quota_key)
            .await
            .map_err(internal)?;
        self.enqueue(job.clone(), owner);
        Ok(job)
    }

    /// Queue a job already stored as `queued` (creation, startup recovery).
    /// A job already queued or running here is left alone (warning logged).
    pub fn enqueue(&self, job: Job, owner: &str) {
        let id = job.id.clone();
        if self.admit(job, owner) {
            self.schedule();
        } else {
            tracing::warn!(job = %id, "already queued or running, not enqueued again");
        }
    }

    async fn owned(&self, owner: &str, id: &str) -> Result<Job, ManagerError> {
        let job = self.store().get_owned(id, owner).await.map_err(internal)?;
        job.ok_or(ManagerError::NotFound)
    }

    /// The live snapshot if the job is running or queued, else the stored row.
    pub async fn get(&self, owner: &str, id: &str) -> Result<Job, ManagerError> {
        // Snapshot first: a job leaving the hub is already final in the store.
        let live = self.hub().snapshot(id);
        let stored = self.owned(owner, id).await?;
        Ok(live.unwrap_or(stored))
    }

    /// The owner's recent jobs, newest first, with live snapshots overlaid.
    pub async fn list(&self, owner: &str) -> Result<Vec<Job>, ApiError> {
        let mut jobs = self
            .store()
            .list_by_owner(owner, LIST_LIMIT)
            .await
            .map_err(internal)?;
        for job in &mut jobs {
            if let Some(live) = self.hub().snapshot(&job.id) {
                *job = live;
            }
        }
        Ok(jobs)
    }

    pub async fn subscribe(&self, owner: &str, id: &str) -> Result<Subscription, ManagerError> {
        self.owned(owner, id).await?;
        if let Some((snapshot, rx)) = self.hub().subscribe(id) {
            return Ok(Subscription {
                snapshot,
                updates: Some(rx),
            });
        }
        // Not live: re-read, it may have finished since the ownership check.
        let snapshot = self.owned(owner, id).await?;
        Ok(Subscription {
            snapshot,
            updates: None,
        })
    }

    /// Cancel a job. Queued: removed and marked `cancelled` (yt-dlp never
    /// runs). Running: stopped, waiting for the worker to record `cancelled`.
    /// Finished: returned unchanged.
    pub async fn cancel(&self, owner: &str, id: &str) -> Result<Job, ManagerError> {
        let stored = self.owned(owner, id).await?;
        match self.detach(id) {
            Tracked::Dequeued => {
                let ttl = self.config().file_ttl;
                let expires_at = self
                    .store()
                    .set_finished(id, JobState::Cancelled, None, None, None, ttl)
                    .await
                    .map_err(internal)?;
                let job = Job {
                    state: JobState::Cancelled,
                    resumable: crate::store::is_resumable(
                        JobState::Cancelled,
                        None,
                        Some(&expires_at),
                    ),
                    expires_at: Some(expires_at),
                    ..stored
                };
                self.hub().publish(job.clone());
                self.schedule();
                Ok(job)
            }
            Tracked::Running { cancel, finished } => {
                cancel.cancel();
                finished.cancelled().await;
                self.owned(owner, id).await
            }
            // The worker may have finished since `stored` was read: re-read
            // rather than return a pre-completion row.
            Tracked::Untracked => match self.hub().snapshot(id) {
                Some(live) => Ok(live),
                None => self.owned(owner, id).await,
            },
        }
    }
}
