//! Startup recovery: interrupted jobs, queued jobs, orphan directories.

use std::collections::HashSet;
use std::path::Path;

use super::JobManager;

/// What [`recover`] did, logged at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Running jobs marked `failed`/`interrupted`.
    pub interrupted: usize,
    /// Queued jobs put back in the queue.
    pub requeued: usize,
    /// Directories in `downloads_dir` matching no `queued`/`done` job.
    pub orphan_dirs: usize,
}

/// Remove `dir` and everything below it; a missing directory is fine.
pub(super) async fn remove_dir(dir: &Path) -> std::io::Result<()> {
    match tokio::fs::remove_dir_all(dir).await {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Bring the store and `downloads_dir` back to a consistent state. Run it
/// before the server listens and before any `create`.
///
/// Order: fail running jobs and delete their directories, requeue `queued`
/// jobs (owner and `format_id` kept), then delete every direct child
/// directory of `downloads_dir` that is not a `queued` or `done` job. The
/// keep-set is computed before requeueing, so a worker that already started
/// on a requeued job never loses its directory. Symlinks are not followed
/// and plain files are left alone.
pub async fn recover(manager: &JobManager) -> anyhow::Result<RecoveryReport> {
    let store = manager.store();
    let root = &manager.config().downloads_dir;

    let interrupted = store.mark_interrupted().await?;
    for id in &interrupted {
        if let Err(e) = remove_dir(&root.join(id)).await {
            tracing::warn!(job = %id, error = %e, "cannot remove interrupted job dir");
        }
    }

    let queued = store.queued().await?;
    let mut keep: HashSet<String> = store.done_ids().await?.into_iter().collect();
    keep.extend(queued.iter().map(|(job, _)| job.id.clone()));
    let requeued = queued.len();
    for (job, owner) in queued {
        manager.enqueue(job, &owner);
    }

    let orphan_dirs = remove_orphans(root, &keep).await?;
    Ok(RecoveryReport {
        interrupted: interrupted.len(),
        requeued,
        orphan_dirs,
    })
}

async fn remove_orphans(root: &Path, keep: &HashSet<String>) -> anyhow::Result<usize> {
    let mut removed = 0;
    let mut entries = match tokio::fs::read_dir(root).await {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e.into()),
    };
    while let Some(entry) = entries.next_entry().await? {
        // `file_type` does not follow symlinks: a link is never a directory.
        if !entry.file_type().await.is_ok_and(|t| t.is_dir()) {
            continue;
        }
        if entry.file_name().to_str().is_some_and(|n| keep.contains(n)) {
            continue;
        }
        match remove_dir(&entry.path()).await {
            Ok(()) => removed += 1,
            Err(e) => {
                tracing::warn!(dir = %entry.path().display(), error = %e, "cannot remove orphan dir");
            }
        }
    }
    Ok(removed)
}
