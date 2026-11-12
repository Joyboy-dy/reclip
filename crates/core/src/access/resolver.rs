//! DNS resolution for the SSRF guard, with a global bound on concurrent
//! lookups: `getaddrinfo` runs on a blocking thread that no timeout can stop,
//! so an unbounded flood of slow names would pile those threads up.

use std::net::IpAddr;
use std::sync::{Arc, LazyLock};

use futures_util::future::BoxFuture;
use tokio::sync::Semaphore;

/// At most this many system lookups run at once, process-wide.
const MAX_LOOKUPS: usize = 16;

/// Turns `(host, port)` into addresses. Injectable so tests stay offline.
pub type Resolver =
    Arc<dyn Fn(String, u16) -> BoxFuture<'static, std::io::Result<Vec<IpAddr>>> + Send + Sync>;

static LOOKUPS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(MAX_LOOKUPS)));

/// The real resolver (`tokio::net::lookup_host`), bounded by [`MAX_LOOKUPS`].
pub fn system_resolver() -> Resolver {
    bounded(LOOKUPS.clone(), |host, port| {
        Box::pin(async move {
            let addrs = tokio::net::lookup_host((host, port)).await?;
            Ok(addrs.map(|a| a.ip()).collect())
        })
    })
}

/// A resolver that answers `ip` for every name.
pub fn static_resolver(ip: IpAddr) -> Resolver {
    Arc::new(move |_, _| Box::pin(async move { Ok(vec![ip]) }))
}

/// Run `lookup` only while holding one of `permits`. The lookup runs in its
/// own task that keeps the permit until the lookup ends, even when the
/// caller gives up (timeout): the blocking thread is still busy until then.
/// Waiting for a permit is part of the returned future, so the caller's
/// timeout covers it.
fn bounded(
    permits: Arc<Semaphore>,
    lookup: impl Fn(String, u16) -> BoxFuture<'static, std::io::Result<Vec<IpAddr>>>
    + Send
    + Sync
    + 'static,
) -> Resolver {
    let lookup = Arc::new(lookup);
    Arc::new(move |host, port| {
        let (permits, lookup) = (permits.clone(), lookup.clone());
        Box::pin(async move {
            let permit = permits
                .acquire_owned()
                .await
                .map_err(std::io::Error::other)?;
            let task = tokio::spawn(async move {
                let _permit = permit;
                lookup(host, port).await
            });
            task.await.map_err(std::io::Error::other)?
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// A lookup that takes 1 s and records the peak number running at once.
    fn tracked(running: Arc<AtomicUsize>, peak: Arc<AtomicUsize>) -> Resolver {
        bounded(Arc::new(Semaphore::new(3)), move |_, _| {
            let (running, peak) = (running.clone(), peak.clone());
            Box::pin(async move {
                let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_secs(1)).await;
                running.fetch_sub(1, Ordering::SeqCst);
                Ok(vec![IpAddr::from([93, 184, 216, 34])])
            })
        })
    }

    #[tokio::test(start_paused = true)]
    async fn lookups_are_bounded() {
        let (running, peak) = (Arc::default(), Arc::default());
        let r = tracked(Arc::clone(&running), Arc::clone(&peak));
        let all: Vec<_> = (0..10).map(|_| r("a.example".into(), 443)).collect();
        for res in futures_util::future::join_all(all).await {
            assert_eq!(res.unwrap().len(), 1);
        }
        assert_eq!(peak.load(Ordering::SeqCst), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn abandoned_lookups_keep_their_permit() {
        let (running, peak) = (Arc::default(), Arc::default());
        let r = tracked(Arc::clone(&running), Arc::clone(&peak));
        for _ in 0..10 {
            let late = tokio::time::timeout(Duration::from_millis(10), r("a".into(), 80));
            assert!(late.await.is_err());
        }
        // The abandoned lookups still run, never more than three at once.
        tokio::time::sleep(Duration::from_secs(5)).await;
        assert_eq!(peak.load(Ordering::SeqCst), 3);
        assert_eq!(running.load(Ordering::SeqCst), 0);
    }
}
