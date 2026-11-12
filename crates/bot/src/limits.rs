//! Shared limits: the per-chat edit rate and the running deliveries.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{Semaphore, SemaphorePermit};
use tokio_util::sync::CancellationToken;

/// At most one progress edit per chat every `gap` (Telegram allows about
/// one message per second per chat, edits included).
pub struct ChatGate {
    gap: Duration,
    last: Mutex<HashMap<i64, Instant>>,
}

impl ChatGate {
    pub fn new(gap: Duration) -> ChatGate {
        ChatGate {
            gap,
            last: Mutex::new(HashMap::new()),
        }
    }

    /// Whether a progress edit may go to `chat` now; if so, it is counted.
    pub fn allow(&self, chat: i64, now: Instant) -> bool {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if last
            .get(&chat)
            .is_some_and(|t| now.saturating_duration_since(*t) < self.gap)
        {
            return false;
        }
        if last.len() > 10_000 {
            let gap = self.gap;
            last.retain(|_, t| now.saturating_duration_since(*t) < gap);
        }
        last.insert(chat, now);
        true
    }
}

/// Deliveries (download from the core, upload to Telegram): a bound on how
/// many run at once, and a way to abort one (Cancel pressed meanwhile).
pub struct Deliveries {
    slots: Semaphore,
    running: Mutex<HashMap<String, CancellationToken>>,
}

impl Deliveries {
    pub fn new(max: usize) -> Deliveries {
        Deliveries {
            slots: Semaphore::new(max.max(1)),
            running: Mutex::new(HashMap::new()),
        }
    }

    /// Waits for a free delivery slot.
    pub async fn slot(&self) -> SemaphorePermit<'_> {
        self.slots
            .acquire()
            .await
            .expect("the delivery semaphore is never closed")
    }

    /// Registers the delivery of `job`; it stays registered while the
    /// returned guard lives.
    pub fn start(self: &Arc<Self>, job: &str) -> Delivering {
        let token = CancellationToken::new();
        self.map().insert(job.to_string(), token.clone());
        Delivering {
            owner: Arc::clone(self),
            job: job.to_string(),
            token,
        }
    }

    /// Aborts the delivery of `job`; false if none is running.
    pub fn abort(&self, job: &str) -> bool {
        match self.map().get(job) {
            Some(t) => {
                t.cancel();
                true
            }
            None => false,
        }
    }

    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, CancellationToken>> {
        self.running.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub struct Delivering {
    owner: Arc<Deliveries>,
    job: String,
    pub token: CancellationToken,
}

impl Drop for Delivering {
    fn drop(&mut self) {
        self.owner.map().remove(&self.job);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_spaces_edits_per_chat() {
        let g = ChatGate::new(Duration::from_millis(1100));
        let t0 = Instant::now();
        assert!(g.allow(1, t0));
        assert!(g.allow(2, t0), "other chats are independent");
        assert!(!g.allow(1, t0 + Duration::from_millis(1099)));
        assert!(g.allow(1, t0 + Duration::from_millis(1100)));
    }

    #[test]
    fn deliveries_register_and_abort() {
        let d = Arc::new(Deliveries::new(2));
        assert!(!d.abort("j"));
        let running = d.start("j");
        assert!(d.abort("j"));
        assert!(running.token.is_cancelled());
        drop(running);
        assert!(!d.abort("j"), "unregistered when done");
    }
}
