//! A new WARP exit when YouTube asks to "confirm you're not a bot".
//!
//! YouTube sometimes blocks the Cloudflare address the `warp` container goes
//! out through; restarting that container usually gets another one. The two
//! containers share a folder (`RECLIP_WARP_SIGNAL_DIR`):
//!
//! - the warp healthcheck (deploy/compose.yaml) touches `alive` while WARP
//!   is connected, and when it finds `rotate` it removes both files and
//!   stops its proxy: the container exits and Docker starts it again;
//! - the core, on a bot check, writes `rotate`, waits for a new `alive`,
//!   then tries the failed call once more.
//!
//! The core never touches Docker. Nothing runs while all is well: the files
//! are only read after a failure. One rotation at most per [`COOLDOWN`]: if
//! the new exit is blocked too, the error goes to the user as before.

use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use reclip_shared::{ApiError, ErrorCode};
use tokio_util::sync::CancellationToken;

const ROTATE: &str = "rotate";
const ALIVE: &str = "alive";
/// Shortest time between two rotations.
pub const COOLDOWN: Duration = Duration::from_secs(120);
/// `alive` older than this: no warp container is watching the folder.
const ALIVE_WITHIN: Duration = Duration::from_secs(60);

/// The rotation state of this process (there is one proxy).
pub struct Rotation {
    /// When the last rotation was asked.
    last: Mutex<Option<SystemTime>>,
    poll: Duration,
    ready_timeout: Duration,
}

impl Rotation {
    pub const fn new(poll: Duration, ready_timeout: Duration) -> Rotation {
        Rotation {
            last: Mutex::new(None),
            poll,
            ready_timeout,
        }
    }

    /// The process-wide state: a 1 s poll, up to 90 s for WARP to come back.
    pub fn shared() -> &'static Rotation {
        static SHARED: OnceLock<Rotation> = OnceLock::new();
        SHARED.get_or_init(|| Rotation::new(Duration::from_secs(1), Duration::from_secs(90)))
    }

    /// After a call that began at `started` failed with `err`: true when a
    /// fresh exit is up and the call is worth one more try.
    ///
    /// A bot check asks for a rotation (unless one happened less than
    /// [`COOLDOWN`] ago). A call that was running while another one asked
    /// for a rotation may have failed because the proxy restarted: it waits
    /// for the new exit too, without asking for another one.
    pub async fn recover(
        &self,
        dir: Option<&Path>,
        err: &ApiError,
        started: SystemTime,
        cancel: &CancellationToken,
    ) -> bool {
        let Some(dir) = dir else { return false };
        // A dead proxy shows up as a generic (internal) yt-dlp failure.
        if !matches!(err.code, ErrorCode::BotCheck | ErrorCode::Internal) {
            return false;
        }
        let asked = {
            let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
            match *last {
                Some(t) if t > started => t,
                _ if err.code != ErrorCode::BotCheck => return false,
                Some(t) if t.elapsed().unwrap_or_default() < COOLDOWN => {
                    tracing::warn!("YouTube bot check right after a new WARP exit: giving up");
                    return false;
                }
                _ => {
                    if !fresh(&dir.join(ALIVE), ALIVE_WITHIN) {
                        return false;
                    }
                    if let Err(e) = std::fs::write(dir.join(ROTATE), b"") {
                        tracing::warn!(error = %e, "cannot ask for a new WARP exit");
                        return false;
                    }
                    tracing::warn!("YouTube bot check: asking WARP for a new exit");
                    let now = SystemTime::now();
                    *last = Some(now);
                    now
                }
            }
        };
        self.wait_ready(dir, asked, cancel).await
    }

    /// The request was taken (`rotate` gone) and the restarted container is
    /// connected (`alive` written after `asked`).
    async fn wait_ready(&self, dir: &Path, asked: SystemTime, cancel: &CancellationToken) -> bool {
        let deadline = tokio::time::Instant::now() + self.ready_timeout;
        while tokio::time::Instant::now() < deadline {
            tokio::select! {
                () = cancel.cancelled() => return false,
                () = tokio::time::sleep(self.poll) => {}
            }
            let taken = !dir.join(ROTATE).exists();
            let alive = modified(&dir.join(ALIVE)).is_some_and(|m| m > asked);
            if taken && alive {
                tracing::info!("new WARP exit ready, trying again");
                return true;
            }
        }
        tracing::warn!("WARP did not come back in time");
        false
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// `path` exists and was written less than `within` ago.
fn fresh(path: &Path, within: Duration) -> bool {
    modified(path).is_some_and(|m| m.elapsed().is_ok_and(|age| age < within))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bot() -> ApiError {
        ApiError {
            code: ErrorCode::BotCheck,
            message: "not a bot".into(),
        }
    }

    fn err(code: ErrorCode) -> ApiError {
        ApiError {
            code,
            message: "x".into(),
        }
    }

    fn quick() -> Rotation {
        Rotation::new(Duration::from_millis(20), Duration::from_millis(600))
    }

    /// The warp healthcheck: takes the request, then reports a new exit.
    fn fake_warp(dir: &Path) -> tokio::task::JoinHandle<()> {
        let dir = dir.to_path_buf();
        tokio::spawn(async move {
            while !dir.join(ROTATE).exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let _ = std::fs::remove_file(dir.join(ALIVE));
            std::fs::remove_file(dir.join(ROTATE)).unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
            std::fs::write(dir.join(ALIVE), b"").unwrap();
        })
    }

    #[tokio::test]
    async fn rotates_once_then_respects_the_cooldown() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join(ALIVE), b"").unwrap();
        let r = quick();
        let never = CancellationToken::new();
        let warp = fake_warp(dir);
        assert!(
            r.recover(Some(dir), &bot(), SystemTime::now(), &never)
                .await
        );
        warp.await.unwrap();
        // Blocked again on the new exit: no second rotation, no retry.
        assert!(
            !r.recover(Some(dir), &bot(), SystemTime::now(), &never)
                .await
        );
        assert!(!dir.join(ROTATE).exists());
    }

    #[tokio::test]
    async fn a_call_cut_by_the_restart_waits_for_it() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join(ALIVE), b"").unwrap();
        let r = quick();
        let never = CancellationToken::new();
        let started = SystemTime::now();
        let warp = fake_warp(dir);
        let (blocked, cut) = (bot(), err(ErrorCode::Internal));
        let (a, b) = tokio::join!(r.recover(Some(dir), &blocked, started, &never), async {
            // Another download, running meanwhile, loses its proxy.
            tokio::time::sleep(Duration::from_millis(5)).await;
            r.recover(Some(dir), &cut, started, &never).await
        });
        warp.await.unwrap();
        assert!(a && b);
    }

    #[tokio::test]
    async fn nothing_happens_without_a_live_warp() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let never = CancellationToken::new();
        let now = SystemTime::now();
        // No `alive`: WARP is not used, or not watching.
        assert!(!quick().recover(Some(dir), &bot(), now, &never).await);
        assert!(!dir.join(ROTATE).exists());
        // Not configured, or not a bot check.
        assert!(!quick().recover(None, &bot(), now, &never).await);
        std::fs::write(dir.join(ALIVE), b"").unwrap();
        for code in [
            ErrorCode::Internal,
            ErrorCode::Private,
            ErrorCode::Cancelled,
        ] {
            assert!(!quick().recover(Some(dir), &err(code), now, &never).await);
        }
        assert!(!dir.join(ROTATE).exists());
    }

    #[tokio::test]
    async fn gives_up_when_warp_does_not_come_back_or_on_cancel() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join(ALIVE), b"").unwrap();
        let never = CancellationToken::new();
        assert!(
            !quick()
                .recover(Some(dir), &bot(), SystemTime::now(), &never)
                .await
        );
        std::fs::remove_file(dir.join(ROTATE)).unwrap();
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert!(
            !quick()
                .recover(Some(dir), &bot(), SystemTime::now(), &cancel)
                .await
        );
    }
}
