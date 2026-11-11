//! Live job snapshots, each with a broadcast channel.
//!
//! The hub only holds jobs that are not finished. Publishing a terminal state
//! broadcasts it, then drops the entry: subscribers receive that last update
//! followed by `RecvError::Closed`, and readers fall back to the store, which
//! the worker writes before publishing. Memory stays bounded by live jobs.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use reclip_shared::Job;
use tokio::sync::broadcast;
use tokio::time::Instant;

use crate::store::is_terminal;

/// Buffered updates per job; a slower subscriber gets `Lagged` and the event
/// stream falls back to a fresh snapshot, so a small buffer loses nothing.
pub const CHANNEL_CAPACITY: usize = 16;
/// Minimum gap between two broadcast progress-only updates of one job.
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(500);

struct Entry {
    snapshot: Job,
    tx: broadcast::Sender<Job>,
    last_sent: Instant,
}

#[derive(Clone, Default)]
pub struct Hub {
    entries: Arc<Mutex<HashMap<String, Entry>>>,
}

impl Hub {
    pub fn new() -> Hub {
        Hub::default()
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<String, Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Record `job` as the latest snapshot and broadcast it, unless it only
    /// changes progress less than [`PROGRESS_INTERVAL`] after the last
    /// broadcast. State changes always go out.
    pub fn publish(&self, job: Job) {
        let mut entries = self.entries();
        let now = Instant::now();
        let terminal = is_terminal(job.state);
        let id = job.id.clone();
        match entries.get_mut(&id) {
            Some(e) => {
                let send = e.snapshot.state != job.state
                    || now.duration_since(e.last_sent) >= PROGRESS_INTERVAL;
                e.snapshot = job.clone();
                if send {
                    // No subscriber is fine.
                    let _ = e.tx.send(job);
                    e.last_sent = now;
                }
            }
            None if !terminal => {
                let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);
                let entry = Entry {
                    snapshot: job,
                    tx,
                    last_sent: now,
                };
                entries.insert(id.clone(), entry);
            }
            None => {}
        }
        if terminal {
            entries.remove(&id);
        }
    }

    /// Latest snapshot of a live job.
    pub fn snapshot(&self, id: &str) -> Option<Job> {
        self.entries().get(id).map(|e| e.snapshot.clone())
    }

    /// Latest snapshot plus every later update, atomically.
    pub fn subscribe(&self, id: &str) -> Option<(Job, broadcast::Receiver<Job>)> {
        self.entries()
            .get(id)
            .map(|e| (e.snapshot.clone(), e.tx.subscribe()))
    }

    /// Drop a job; its subscribers see the stream close.
    pub fn forget(&self, id: &str) {
        self.entries().remove(id);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use reclip_shared::{JobState, MediaFormat};
    use tokio::sync::broadcast::error::{RecvError, TryRecvError};

    use super::*;

    fn job(state: JobState, progress: Option<f32>) -> Job {
        Job {
            id: "j1".into(),
            url: "https://x".into(),
            format: MediaFormat::Mp4,
            format_id: None,
            title: None,
            thumbnail: None,
            state,
            filename: None,
            size: None,
            error: None,
            progress,
            speed: None,
            eta: None,
            created_at: "2026-11-11T00:00:00.000Z".into(),
            expires_at: None,
        }
    }

    fn seen(rx: &mut broadcast::Receiver<Job>) -> Vec<(JobState, Option<f32>)> {
        std::iter::from_fn(|| rx.try_recv().ok())
            .map(|j| (j.state, j.progress))
            .collect()
    }

    #[tokio::test(start_paused = true)]
    async fn subscriber_gets_snapshot_then_updates() {
        let hub = Hub::new();
        assert!(hub.subscribe("j1").is_none());
        hub.publish(job(JobState::Queued, None));
        let (snap, mut rx) = hub.subscribe("j1").unwrap();
        assert_eq!(snap.state, JobState::Queued);
        hub.publish(job(JobState::Downloading, Some(0.0)));
        assert_eq!(rx.recv().await.unwrap().state, JobState::Downloading);
        hub.publish(job(JobState::Done, None));
        assert_eq!(rx.recv().await.unwrap().state, JobState::Done);
        // A terminal state closes the stream and leaves the store authoritative.
        assert!(matches!(rx.recv().await, Err(RecvError::Closed)));
        assert!(hub.snapshot("j1").is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn progress_updates_are_throttled_but_state_changes_are_not() {
        use JobState::*;
        let hub = Hub::new();
        hub.publish(job(Queued, None));
        let (_, mut rx) = hub.subscribe("j1").unwrap();
        hub.publish(job(Downloading, Some(0.0)));
        hub.publish(job(Downloading, Some(10.0)));
        // Not broadcast, but the snapshot is fresh.
        assert_eq!(hub.snapshot("j1").unwrap().progress, Some(10.0));
        assert_eq!(hub.subscribe("j1").unwrap().0.progress, Some(10.0));
        tokio::time::advance(Duration::from_millis(499)).await;
        hub.publish(job(Downloading, Some(15.0)));
        tokio::time::advance(Duration::from_millis(2)).await;
        hub.publish(job(Downloading, Some(20.0)));
        hub.publish(job(Downloading, Some(30.0)));
        hub.publish(job(Processing, Some(100.0)));
        assert_eq!(
            seen(&mut rx),
            [
                (Downloading, Some(0.0)),
                (Downloading, Some(20.0)),
                (Processing, Some(100.0))
            ]
        );
        assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
    }

    #[tokio::test]
    async fn forget_closes_subscribers() {
        let hub = Hub::new();
        hub.publish(job(JobState::Queued, None));
        let (_, mut rx) = hub.subscribe("j1").unwrap();
        hub.forget("j1");
        assert!(hub.snapshot("j1").is_none());
        assert!(matches!(rx.recv().await, Err(RecvError::Closed)));
    }
}
