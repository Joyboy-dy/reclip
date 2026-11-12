//! Scheduling state of the [`JobManager`]: queue, running slots, counters.
//!
//! Everything here sits behind one `std::sync::Mutex`, never held across an
//! `.await`. Workers are spawned after the lock is released.

use std::collections::HashMap;
use std::sync::MutexGuard;

use reclip_shared::Job;
use tokio_util::sync::CancellationToken;

use super::queue::{Queue, site_of};
use super::{JobManager, worker};
use crate::config::Config;

pub(super) struct Slot {
    site: String,
    cancel: CancellationToken,
    /// Fired once the worker has recorded its outcome and freed the slot.
    finished: CancellationToken,
}

pub(super) struct Sched {
    queue: Queue,
    /// Queued jobs, by id, as handed to their worker.
    waiting: HashMap<String, Job>,
    running: HashMap<String, Slot>,
    per_site: HashMap<String, usize>,
    max_concurrent: usize,
    stopping: bool,
}

impl Sched {
    pub(super) fn new(cfg: &Config) -> Sched {
        Sched {
            max_concurrent: cfg.max_concurrent,
            queue: Queue::new(cfg.max_concurrent, cfg.max_per_site),
            waiting: HashMap::new(),
            running: HashMap::new(),
            per_site: HashMap::new(),
            stopping: false,
        }
    }
}

/// How many jobs this process is holding, for health and tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JobCounts {
    pub queued: usize,
    pub running: usize,
}

/// Where a job stood in the scheduler when it was detached from the queue.
pub(super) enum Tracked {
    /// Was waiting; now removed from the queue (it will never start).
    Dequeued,
    Running {
        cancel: CancellationToken,
        finished: CancellationToken,
    },
    /// Neither queued nor running: finished, or not scheduled by this process.
    Untracked,
}

impl JobManager {
    pub(super) fn sched(&self) -> MutexGuard<'_, Sched> {
        self.inner.sched.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Stop scheduling, stop every running download through its token and
    /// wait for the workers (they record `failed`/`interrupted`). Queued jobs
    /// stay `queued` in the store for the next start.
    pub async fn shutdown(&self) {
        self.inner.shutdown.cancel();
        let finished: Vec<CancellationToken> = {
            let mut s = self.sched();
            s.stopping = true;
            s.running
                .values()
                .map(|slot| {
                    slot.cancel.cancel();
                    slot.finished.clone()
                })
                .collect()
        };
        for f in finished {
            f.cancelled().await;
        }
    }

    pub fn counts(&self) -> JobCounts {
        let s = self.sched();
        JobCounts {
            queued: s.queue.len(),
            running: s.running.len(),
        }
    }

    pub(super) fn stopping(&self) -> bool {
        self.sched().stopping
    }

    /// Add a job to the queue, unless it is already queued or running (a
    /// second slot would leak a per-site count and start two workers).
    pub(super) fn admit(&self, job: Job, owner: &str) -> bool {
        let mut s = self.sched();
        if s.waiting.contains_key(&job.id) || s.running.contains_key(&job.id) {
            return false;
        }
        // Published under the lock (order: sched, then hub) so a worker that a
        // concurrent `schedule` starts cannot be overtaken by this snapshot.
        self.hub().publish(job.clone());
        s.queue.push(&job.id, owner, &site_of(&job.url));
        s.waiting.insert(job.id.clone(), job);
        true
    }

    /// Remove `id` from the queue if it waits there, else report its slot.
    pub(super) fn detach(&self, id: &str) -> Tracked {
        let mut s = self.sched();
        if s.queue.remove(id) {
            s.waiting.remove(id);
            return Tracked::Dequeued;
        }
        match s.running.get(id) {
            Some(slot) => Tracked::Running {
                cancel: slot.cancel.clone(),
                finished: slot.finished.clone(),
            },
            None => Tracked::Untracked,
        }
    }

    /// The `finished` token of a running job.
    pub(super) fn running_finished(&self, id: &str) -> Option<CancellationToken> {
        self.sched()
            .running
            .get(id)
            .map(|slot| slot.finished.clone())
    }

    /// Start every queued job the limits allow. Over the disk budget nothing
    /// starts and room is requested instead (see [`super::disk`]).
    pub(super) fn schedule(&self) {
        let full = self.disk_full();
        let mut need_room = false;
        let starts = {
            let mut guard = self.sched();
            let s = &mut *guard;
            let mut starts = Vec::new();
            if full && !s.stopping && !s.queue.is_empty() && s.running.len() < s.max_concurrent {
                need_room = true;
            }
            while !s.stopping && !full {
                let Some(id) = s.queue.next(s.running.len(), &s.per_site) else {
                    break;
                };
                let Some(job) = s.waiting.remove(&id) else {
                    continue;
                };
                let site = site_of(&job.url);
                *s.per_site.entry(site.clone()).or_insert(0) += 1;
                let cancel = CancellationToken::new();
                let slot = Slot {
                    site,
                    cancel: cancel.clone(),
                    finished: CancellationToken::new(),
                };
                s.running.insert(id, slot);
                starts.push((job, cancel));
            }
            starts
        };
        if need_room {
            self.request_room();
        }
        // Outside a runtime (process teardown) nothing can start anyway.
        let Ok(rt) = tokio::runtime::Handle::try_current() else {
            return;
        };
        for (job, cancel) in starts {
            rt.spawn(worker::run(self.clone(), job, cancel));
        }
    }

    /// Free a worker's slot, signal waiters, then schedule the next jobs.
    pub(super) fn release(&self, id: &str) {
        let finished = {
            let mut s = self.sched();
            let slot = s.running.remove(id);
            if let Some(slot) = &slot
                && let Some(n) = s.per_site.get_mut(&slot.site)
            {
                *n -= 1;
                if *n == 0 {
                    s.per_site.remove(&slot.site);
                }
            }
            slot.map(|slot| slot.finished)
        };
        if let Some(finished) = finished {
            finished.cancel();
        }
        self.schedule();
    }
}
