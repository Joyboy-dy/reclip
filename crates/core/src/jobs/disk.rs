//! Disk budget (`max_disk`) for the job directories under `downloads_dir`.
//!
//! The size is a cached value, measured off the runtime threads after each
//! job, at each cleanup sweep and after a delete; evictions subtract what
//! they remove. When it is over budget, `schedule` starts nothing and asks
//! for room: if the files that may go cover the excess, the oldest stopped
//! downloads' partial files expire first, then the oldest finished files.
//! Otherwise nothing is expired and queued jobs wait; the scheduler retries
//! when a job ends or at the next sweep.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::JobManager;
use super::recovery::{is_job_dir_name, remove_dir};

#[derive(Default)]
pub(super) struct Disk {
    /// Generation handed to the next measurement or adjustment.
    next: AtomicU64,
    /// `(generation, bytes)` of the newest value stored.
    used: Mutex<(u64, u64)>,
    /// An eviction task is running.
    evicting: AtomicBool,
}

impl Disk {
    fn begin(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Store `bytes` measured at generation `generation`, unless a newer value is in.
    fn store(&self, generation: u64, bytes: u64) {
        let mut used = self.used.lock().unwrap_or_else(|e| e.into_inner());
        if generation > used.0 {
            *used = (generation, bytes);
        }
    }

    fn bytes(&self) -> u64 {
        self.used.lock().unwrap_or_else(|e| e.into_inner()).1
    }

    /// Take `bytes` off the newest value, as a new generation.
    fn subtract(&self, bytes: u64) {
        let generation = self.begin();
        let mut used = self.used.lock().unwrap_or_else(|e| e.into_inner());
        *used = (generation, used.1.saturating_sub(bytes));
    }
}

/// Clears `evicting` however the eviction task ends.
struct Evicting<'a>(&'a AtomicBool);

impl Drop for Evicting<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
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

/// Size of a file or directory tree; 0 if it does not exist.
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

/// The bytes held by each candidate's directory, in order, skipping those
/// with no directory (blocking).
fn sized(root: &Path, ids: Vec<String>) -> Vec<(String, PathBuf, u64)> {
    ids.into_iter()
        .map(|id| {
            let dir = root.join(&id);
            (id, dir)
        })
        .filter(|(_, dir)| dir.is_dir())
        .map(|(id, dir)| {
            let size = tree_size(&dir);
            (id, dir, size)
        })
        .collect()
}

impl JobManager {
    /// Measure the job directories, cache the size (unless a newer value
    /// came in meanwhile) and return it.
    pub async fn refresh_disk(&self) -> u64 {
        let disk = &self.inner.disk;
        let generation = disk.begin();
        let root = self.config().downloads_dir.clone();
        match tokio::task::spawn_blocking(move || job_dirs_size(&root)).await {
            Ok(bytes) => {
                disk.store(generation, bytes);
                bytes
            }
            Err(e) => {
                tracing::warn!(error = %e, "cannot measure the downloads dir");
                disk.bytes()
            }
        }
    }

    /// Whether the cached size is over `max_disk`.
    pub(super) fn disk_full(&self) -> bool {
        self.inner.disk.bytes() > self.config().max_disk
    }

    /// Start freeing space for queued jobs, unless that is already running.
    /// Call it without the scheduler lock.
    pub(super) fn request_room(&self) {
        if self.inner.disk.evicting.swap(true, Ordering::AcqRel) {
            return;
        }
        let mgr = self.clone();
        let task = async move {
            {
                let _guard = Evicting(&mgr.inner.disk.evicting);
                if let Err(e) = mgr.evict_until_fits().await {
                    tracing::error!(error = %e, "cannot free disk space");
                }
            }
            // Read after the flag is down, so room made meanwhile is not missed.
            if !mgr.disk_full() {
                mgr.schedule();
            }
        };
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => drop(rt.spawn(task)),
            Err(_) => self.inner.disk.evicting.store(false, Ordering::Release),
        }
    }

    /// Expire finished jobs holding files, in eviction order, until the
    /// directories fit in `max_disk`; nothing if they could not.
    async fn evict_until_fits(&self) -> anyhow::Result<()> {
        let used = self.refresh_disk().await;
        let max = self.config().max_disk;
        if used <= max {
            return Ok(());
        }
        let ids = self.store().eviction_candidates().await?;
        let root = self.config().downloads_dir.clone();
        let candidates = tokio::task::spawn_blocking(move || sized(&root, ids)).await?;
        let evictable: u64 = candidates.iter().map(|(_, _, size)| size).sum();
        if evictable < used - max {
            tracing::info!(used, max, evictable, "disk budget exceeded, jobs wait");
            return Ok(());
        }
        for (id, dir, size) in candidates {
            // Checked in the update: a job resumed meanwhile keeps its files.
            if !self.store().expire_now(&id).await? {
                continue;
            }
            if let Err(e) = remove_dir(&dir).await {
                tracing::warn!(job = %id, error = %e, "cannot remove evicted job dir");
            }
            self.inner.disk.subtract(size);
            tracing::info!(job = %id, "expired early to free disk space");
            if let Some(job) = self.store().get(&id).await? {
                self.hub().publish(job);
            }
            if !self.disk_full() {
                break;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Disk, job_dirs_size};

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

    #[test]
    fn a_stale_measurement_never_wins() {
        let disk = Disk::default();
        let (old, new) = (disk.begin(), disk.begin());
        disk.store(new, 50);
        disk.store(old, 90);
        assert_eq!(disk.bytes(), 50);
        // An adjustment is newer than a walk that started before it.
        let walk = disk.begin();
        disk.subtract(20);
        disk.store(walk, 50);
        assert_eq!(disk.bytes(), 30);
    }
}
