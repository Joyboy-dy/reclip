//! Disk budget (`max_disk`) for the job directories under `downloads_dir`.
//!
//! The size is a cached value, recomputed off the runtime threads after each
//! job, at each cleanup sweep and after evictions. When it is over budget,
//! `schedule` starts nothing and asks for room: the oldest stopped
//! downloads' partial files expire first, then the oldest finished files.
//! If that is not enough, queued jobs wait; the scheduler retries when a job
//! ends or at the next sweep.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::JobManager;
use super::recovery::{is_job_dir_name, remove_dir};

#[derive(Default)]
pub(super) struct Disk {
    /// Bytes in job directories, as last computed.
    used: AtomicU64,
    /// An eviction task is running.
    evicting: AtomicBool,
}

/// Bytes in the job directories directly below `root` (blocking). Other
/// entries are not ReClip's and are not counted: `RECLIP_DOWNLOADS_DIR` may
/// hold other things. Symlinks are not followed.
fn job_dirs_size(root: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_str().is_some_and(is_job_dir_name))
        .map(|e| tree_size(&e.path()))
        .sum()
}

fn tree_size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| tree_size(&e.path()))
                .sum()
        })
        .unwrap_or(0)
}

impl JobManager {
    /// Recompute the cached size of the job directories and return it.
    pub async fn refresh_disk(&self) -> u64 {
        let root = self.config().downloads_dir.clone();
        let used = tokio::task::spawn_blocking(move || job_dirs_size(&root))
            .await
            .unwrap_or_else(|e| {
                tracing::warn!(error = %e, "cannot measure the downloads dir");
                self.inner.disk.used.load(Ordering::Relaxed)
            });
        self.inner.disk.used.store(used, Ordering::Relaxed);
        used
    }

    /// Whether the cached size is over `max_disk`.
    pub(super) fn disk_full(&self) -> bool {
        self.inner.disk.used.load(Ordering::Relaxed) > self.config().max_disk
    }

    /// Start freeing space for queued jobs, unless that is already running.
    /// Call it without the scheduler lock.
    pub(super) fn request_room(&self) {
        let disk = &self.inner.disk;
        if disk.evicting.swap(true, Ordering::AcqRel) {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            disk.evicting.store(false, Ordering::Release);
            return;
        };
        let mgr = self.clone();
        rt.spawn(async move {
            if let Err(e) = mgr.evict_until_fits().await {
                tracing::error!(error = %e, "cannot free disk space");
            }
            mgr.inner.disk.evicting.store(false, Ordering::Release);
            // Read after the flag is down, so room made meanwhile is not missed.
            if !mgr.disk_full() {
                mgr.schedule();
            }
        });
    }

    /// Expire finished jobs holding files, in eviction order, until the
    /// directories fit in `max_disk` or nothing is left to expire.
    async fn evict_until_fits(&self) -> anyhow::Result<()> {
        self.refresh_disk().await;
        if !self.disk_full() {
            return Ok(());
        }
        let root = &self.config().downloads_dir;
        for id in self.store().eviction_candidates().await? {
            let dir = root.join(&id);
            if !tokio::fs::try_exists(&dir).await.unwrap_or(false) {
                continue;
            }
            // Checked in the update: a job resumed meanwhile keeps its files.
            if !self.store().expire_now(&id).await? {
                continue;
            }
            if let Err(e) = remove_dir(&dir).await {
                tracing::warn!(job = %id, error = %e, "cannot remove evicted job dir");
            }
            tracing::info!(job = %id, "expired early to free disk space");
            if let Some(job) = self.store().get(&id).await? {
                self.hub().publish(job);
            }
            self.refresh_disk().await;
            if !self.disk_full() {
                break;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::job_dirs_size;

    #[test]
    fn counts_job_dirs_only() {
        let root = tempfile::tempdir().unwrap();
        let job = root.path().join("0190f5c2a1b47c3d8e9f00112233aa01");
        std::fs::create_dir_all(job.join("sub")).unwrap();
        std::fs::write(job.join("media.mp4"), [0u8; 10]).unwrap();
        std::fs::write(job.join("sub/media.mp4.part"), [0u8; 5]).unwrap();
        std::fs::create_dir_all(root.path().join("my-videos")).unwrap();
        std::fs::write(root.path().join("my-videos/big.mp4"), [0u8; 100]).unwrap();
        std::fs::write(root.path().join("stray.txt"), [0u8; 100]).unwrap();
        assert_eq!(job_dirs_size(root.path()), 15);
        assert_eq!(job_dirs_size(&root.path().join("missing")), 0);
    }
}
