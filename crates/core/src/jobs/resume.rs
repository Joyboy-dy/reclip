//! Resuming a stopped download: same job, same options, same directory.

use reclip_shared::{ApiError, ErrorCode, Job, JobState};

use super::{JobManager, ManagerError, internal};

/// The error answered (409) for a job that cannot be resumed.
pub fn not_resumable() -> ApiError {
    ApiError {
        code: ErrorCode::InvalidRequest,
        message: "This download cannot be resumed".into(),
    }
}

impl JobManager {
    /// Put the owner's cancelled (or interrupted) job back in the queue with
    /// its id and options; yt-dlp continues from the partial files left in
    /// its directory. `Ok(None)` if the job is not resumable.
    pub async fn resume(&self, owner: &str, id: &str) -> Result<Option<Job>, ManagerError> {
        let stored = self.owned(owner, id).await?;
        if !stored.resumable {
            return Ok(None);
        }
        // A worker that has just recorded `cancelled` may still hold the slot.
        if let Some(finished) = self.running_finished(id) {
            finished.cancelled().await;
        }
        // Atomic: a concurrent resume, expiry or eviction wins at most once.
        if !self.store().requeue(id).await.map_err(internal)? {
            return Ok(None);
        }
        let job = Job {
            state: JobState::Queued,
            filename: None,
            size: None,
            error: None,
            expires_at: None,
            resumable: false,
            progress: None,
            speed: None,
            eta: None,
            ..stored
        };
        self.enqueue(job.clone(), owner);
        Ok(Some(job))
    }
}
