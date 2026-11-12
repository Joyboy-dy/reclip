//! Coalesced API key usage: one `UPDATE` per key per minute at most, instead
//! of one per request. Uses in between are counted in memory and written with
//! the next touch, so `request_count` stays exact (except for uses not yet
//! written when the process stops).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A key is written at most once per interval.
const INTERVAL: Duration = Duration::from_secs(60);

/// What to write for one key: `request_count += count`, `last_used_at`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Touch {
    pub id: String,
    pub count: u64,
    pub used_at: String,
}

#[derive(Debug)]
struct Pending {
    written_at: Instant,
    /// Uses since `written_at`, not written yet.
    count: u64,
    used_at: String,
}

#[derive(Debug)]
struct Inner {
    keys: HashMap<String, Pending>,
    pruned_at: Instant,
}

/// Shared by every request of the process.
#[derive(Debug, Clone)]
pub struct KeyUsage(Arc<Mutex<Inner>>);

impl Default for KeyUsage {
    fn default() -> Self {
        KeyUsage(Arc::new(Mutex::new(Inner {
            keys: HashMap::new(),
            pruned_at: Instant::now(),
        })))
    }
}

impl KeyUsage {
    /// Count one use of `id` at `at` (`used_at` is its stored timestamp) and
    /// return what is due for writing: this key when it was not written in
    /// the last interval, plus the pending uses of keys idle for a whole
    /// interval, which are forgotten (pruned) at the same time.
    pub fn record(&self, id: &str, at: Instant, used_at: &str) -> Vec<Touch> {
        let mut inner = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let mut due = Vec::new();
        let stale = |p: &Pending| at.saturating_duration_since(p.written_at) >= INTERVAL;
        if at.saturating_duration_since(inner.pruned_at) >= INTERVAL {
            inner.pruned_at = at;
            inner.keys.retain(|k, p| {
                if k == id || !stale(p) {
                    return true;
                }
                if p.count > 0 {
                    due.push(Touch {
                        id: k.clone(),
                        count: p.count,
                        used_at: std::mem::take(&mut p.used_at),
                    });
                }
                false
            });
        }
        match inner.keys.get_mut(id) {
            Some(p) if !stale(p) => {
                p.count += 1;
                p.used_at = used_at.to_string();
            }
            Some(p) => {
                due.push(Touch {
                    id: id.to_string(),
                    count: p.count + 1,
                    used_at: used_at.to_string(),
                });
                p.count = 0;
                p.written_at = at;
            }
            None => {
                let first = Pending {
                    written_at: at,
                    count: 0,
                    used_at: String::new(),
                };
                inner.keys.insert(id.to_string(), first);
                due.push(Touch {
                    id: id.to_string(),
                    count: 1,
                    used_at: used_at.to_string(),
                });
            }
        }
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(id: &str, count: u64, used_at: &str) -> Touch {
        Touch {
            id: id.into(),
            count,
            used_at: used_at.into(),
        }
    }

    #[test]
    fn writes_once_per_interval_with_exact_counts() {
        let usage = KeyUsage::default();
        let t0 = Instant::now();
        let s = Duration::from_secs;
        assert_eq!(usage.record("a", t0, "t0"), vec![touch("a", 1, "t0")]);
        assert!(usage.record("a", t0 + s(1), "t1").is_empty());
        assert!(usage.record("a", t0 + s(59), "t59").is_empty());
        // The use that triggers the write counts too: 2 pending + itself.
        assert_eq!(
            usage.record("a", t0 + s(60), "t60"),
            vec![touch("a", 3, "t60")]
        );
        assert!(usage.record("a", t0 + s(61), "t61").is_empty());
        // Another key is independent.
        assert_eq!(
            usage.record("b", t0 + s(61), "b61"),
            vec![touch("b", 1, "b61")]
        );
    }

    #[test]
    fn idle_keys_are_flushed_and_pruned() {
        let usage = KeyUsage::default();
        let t0 = Instant::now();
        let s = Duration::from_secs;
        usage.record("a", t0, "a0");
        usage.record("a", t0 + s(10), "a10");
        usage.record("idle", t0, "i0");
        // `a` idles; a later use of `b` writes a's pending use and forgets it.
        let due = usage.record("b", t0 + s(130), "b130");
        assert_eq!(due, vec![touch("a", 1, "a10"), touch("b", 1, "b130")]);
        assert_eq!(usage.0.lock().unwrap().keys.len(), 1);
        // `a` starts over: its next use is written right away.
        assert_eq!(
            usage.record("a", t0 + s(131), "a131"),
            vec![touch("a", 1, "a131")]
        );
    }
}
