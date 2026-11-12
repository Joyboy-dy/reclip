//! Periodic expiry of finished files and purge of old `expired` rows.

use std::time::Duration;

use tokio::task::JoinHandle;
use tokio::time::{MissedTickBehavior, interval};

use super::JobManager;
use super::recovery::{remove_dir, remove_file};

const DAY: u64 = 24 * 3600;

/// Run [`sweep`] every `cleanup_interval`. The first run is one interval
/// after the call (recovery already ran at startup). Errors are logged and
/// the loop goes on. It stops when the manager shuts down
/// ([`JobManager::shutdown_token`]).
pub fn spawn_cleanup(manager: JobManager) -> JoinHandle<()> {
    tokio::spawn(async move {
        let period = manager
            .config()
            .cleanup_interval
            .max(Duration::from_millis(1));
        let mut ticker = interval(period);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        ticker.tick().await; // the first tick is immediate
        let stop = manager.shutdown_token();
        loop {
            tokio::select! {
                _ = stop.cancelled() => break,
                _ = ticker.tick() => {}
            }
            if let Err(e) = sweep(&manager).await {
                tracing::error!(error = %e, "cleanup failed");
            }
        }
    })
}

/// Expire due `done`, `failed` and `cancelled` jobs (delete their
/// directories, publish the `expired` job), purge `expired` rows (and their
/// thumbnails) older than `history_days`, then measure the disk use and
/// start the queued jobs it now allows.
pub async fn sweep(manager: &JobManager) -> anyhow::Result<()> {
    let store = manager.store();
    let root = &manager.config().downloads_dir;
    for (id, _path) in store.expire_due(&crate::store::now()).await? {
        if let Err(e) = remove_dir(&root.join(&id)).await {
            tracing::warn!(job = %id, error = %e, "cannot remove expired job dir");
        }
        if let Some(job) = store.get(&id).await? {
            manager.hub().publish(job);
        }
    }
    let keep = Duration::from_secs(u64::from(manager.config().history_days) * DAY);
    let purged = store.purge_expired(&crate::store::now_minus(keep)).await?;
    for thumb in purged.iter().flatten() {
        remove_file(thumb).await;
    }
    if !purged.is_empty() {
        tracing::info!(purged = purged.len(), "purged expired jobs");
    }
    manager.refresh_disk().await;
    manager.schedule();
    Ok(())
}
