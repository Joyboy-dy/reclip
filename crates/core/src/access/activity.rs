//! Records who calls (identity, address, device) for the admin's view.
//!
//! One write per identity and address per [`EVERY`] at most, made in the
//! background: a page that probes ten links costs one row update, and a
//! request never waits for it.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::caller::Caller;
use super::cookie::ip_quota_key;
use crate::store::Store;

const EVERY: Duration = Duration::from_secs(60);
/// Entries kept before the old ones are dropped.
const MAX_ENTRIES: usize = 10_000;

#[derive(Clone, Default)]
pub struct Activity(Arc<Mutex<HashMap<String, Instant>>>);

/// How an address is stored: as is for IPv4, its /64 for IPv6 (one host
/// usually owns the whole /64 and rotates within it).
pub fn ip_text(ip: IpAddr) -> String {
    ip_quota_key(ip).trim_start_matches("ip:").to_string()
}

impl Activity {
    /// Whether `key` is due for a write now (and mark it written).
    fn due(&self, key: String) -> bool {
        let now = Instant::now();
        let mut seen = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if seen
            .get(&key)
            .is_some_and(|at| now.duration_since(*at) < EVERY)
        {
            return false;
        }
        if seen.len() >= MAX_ENTRIES {
            seen.retain(|_, at| now.duration_since(*at) < EVERY);
        }
        seen.insert(key, now);
        true
    }

    /// Note a request of `caller`, in the background.
    pub fn record(&self, store: &Store, caller: &Caller) {
        let ip = caller.origin.ip.map(ip_text);
        let key = format!("{}|{}", caller.owner, ip.as_deref().unwrap_or(""));
        if !self.due(key) {
            return;
        }
        let (store, owner, origin) = (store.clone(), caller.owner.clone(), caller.origin.clone());
        tokio::spawn(async move {
            let res = store
                .touch_client(
                    &owner,
                    ip.as_deref(),
                    origin.device.as_deref(),
                    origin.tg_username.as_deref(),
                    origin.tg_name.as_deref(),
                )
                .await;
            if let Err(e) = res {
                tracing::warn!(error = %e, "cannot record activity");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_are_coalesced() {
        let a = Activity::default();
        assert!(a.due("web:x|1.2.3.4".into()));
        assert!(!a.due("web:x|1.2.3.4".into()));
        assert!(a.due("web:x|5.6.7.8".into()));
        assert_eq!(
            ip_text("2001:db8:1:2:3:4:5:6".parse().unwrap()),
            "2001:db8:1:2::/64"
        );
        assert_eq!(ip_text("203.0.113.7".parse().unwrap()), "203.0.113.7");
    }
}
