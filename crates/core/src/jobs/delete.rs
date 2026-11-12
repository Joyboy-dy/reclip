//! Removing a job: a finished one leaves the history only when hidden there,
//! an unfinished one is deleted outright.

use reclip_shared::JobState;

use super::sched::Tracked;
use super::worker::thumb_file;
use super::{JobManager, ManagerError, internal, recovery};

impl JobManager {
    /// Remove a job for its owner.
    ///
    /// A `done` (or already `expired`) job is part of the history: its media
    /// directory is removed and the row is marked `expired`, like an
    /// eviction, keeping its thumbnail. Any other job is deleted with its
    /// files (see [`JobManager::hard_delete`]).
    pub async fn delete(&self, owner: &str, id: &str) -> Result<(), ManagerError> {
        // Ownership first: `id` comes from the request and names a directory.
        let stored = self.owned(owner, id).await?;
        if matches!(stored.state, JobState::Done | JobState::Expired) {
            self.retire(id).await.map_err(internal)?;
        } else {
            self.hard_delete(id).await.map_err(internal)?;
        }
        self.refresh_disk().await;
        self.schedule();
        Ok(())
    }

    /// Expire a finished job now and remove its media, keeping the row.
    async fn retire(&self, id: &str) -> anyhow::Result<()> {
        // Row first, like an eviction: the file is no longer served once
        // its path is cleared. Already `expired` is fine.
        self.store().expire_now(id).await?;
        self.remove_job_dir(id).await;
        self.hub().forget(id);
        Ok(())
    }

    /// Remove the row, stop the job if it is queued or running, then remove
    /// its files.
    ///
    /// The row goes first: from then on a concurrent resume cannot requeue
    /// it, and a worker admitted late fails its first store write before it
    /// creates the directory. A worker that started earlier is still in its
    /// slot when the job is detached, so it is stopped and awaited before the
    /// files are removed.
    async fn hard_delete(&self, id: &str) -> anyhow::Result<()> {
        let thumb = self.store().delete(id).await?;
        match self.detach(id) {
            Tracked::Dequeued => self.schedule(),
            Tracked::Running { cancel, finished } => {
                cancel.cancel();
                finished.cancelled().await;
            }
            Tracked::Untracked => {}
        }
        self.remove_job_dir(id).await;
        self.hub().forget(id);
        // A completion racing the delete may have moved a thumbnail in after
        // the row was gone: remove it whether or not the row named it.
        let own = thumb_file(self.config(), id);
        if let Some(thumb) = thumb.filter(|t| *t != own) {
            recovery::remove_file(&thumb).await;
        }
        recovery::remove_file(&own).await;
        Ok(())
    }

    async fn remove_job_dir(&self, id: &str) {
        // Left behind on failure, startup recovery removes it as an orphan.
        if let Err(e) = recovery::remove_dir(&self.config().downloads_dir.join(id)).await {
            tracing::warn!(job = %id, error = %e, "cannot remove deleted job dir");
        }
    }
}
