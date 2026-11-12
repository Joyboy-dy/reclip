//! Shared limits: the per-chat send rate and the running deliveries.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{Mutex as AsyncMutex, Semaphore, SemaphorePermit};
use tokio_util::sync::CancellationToken;

/// The last send or edit in one chat; held while a send waits its turn.
type Slot = Arc<AsyncMutex<Option<Instant>>>;

/// Spaces every send and edit to a chat by `gap` (Telegram allows about
/// one message per second per chat, edits included). Messages that must
/// arrive wait for their turn, in order; progress edits are skipped instead.
pub struct ChatGate {
    gap: Duration,
    chats: Mutex<HashMap<i64, Slot>>,
}

impl ChatGate {
    pub fn new(gap: Duration) -> ChatGate {
        ChatGate {
            gap,
            chats: Mutex::new(HashMap::new()),
        }
    }

    fn slot(&self, chat: i64, now: Instant) -> Slot {
        let mut chats = self.chats.lock().unwrap_or_else(|e| e.into_inner());
        if chats.len() > 10_000 {
            let gap = self.gap;
            // Unused and quiet for a while: forgotten.
            chats.retain(|_, s| match s.try_lock() {
                Err(_) => true,
                Ok(last) => {
                    Arc::strong_count(s) > 1
                        || last.is_some_and(|t| now.saturating_duration_since(t) < gap)
                }
            });
        }
        Arc::clone(chats.entry(chat).or_default())
    }

    /// Whether a progress edit may go to `chat` now: not while another
    /// message waits for its turn, nor within `gap` of the last one. If
    /// so, it is counted.
    pub fn allow(&self, chat: i64, now: Instant) -> bool {
        let slot = self.slot(chat, now);
        let Ok(mut last) = slot.try_lock() else {
            return false;
        };
        if last.is_some_and(|t| now.saturating_duration_since(t) < self.gap) {
            return false;
        }
        *last = Some(now);
        true
    }

    /// Waits until a message may go to `chat` and counts it.
    pub async fn turn(&self, chat: i64) {
        let slot = self.slot(chat, Instant::now());
        let mut last = slot.lock().await;
        if let Some(t) = *last
            && !self.gap.is_zero()
        {
            tokio::time::sleep_until((t + self.gap).into()).await;
        }
        *last = Some(Instant::now());
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

    #[tokio::test]
    async fn every_send_waits_for_its_turn() {
        let gap = Duration::from_millis(60);
        let g = Arc::new(ChatGate::new(gap));
        let t0 = Instant::now();
        g.turn(1).await;
        assert!(t0.elapsed() < gap, "the first send goes at once");
        g.turn(2).await;
        assert!(t0.elapsed() < gap, "other chats are independent");
        let waiter = tokio::spawn({
            let g = Arc::clone(&g);
            async move { g.turn(1).await }
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(
            !g.allow(1, Instant::now()),
            "progress skipped while a send waits"
        );
        waiter.await.unwrap();
        assert!(t0.elapsed() >= gap * 3 / 4, "the second send waited");
        assert!(!g.allow(1, Instant::now()), "and counts for the gap");
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
