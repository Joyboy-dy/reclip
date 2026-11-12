//! The bot: clients, settings, pending choices and tracked jobs, plus the
//! update loop.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::config::Config;
use crate::core_client::{Core, CoreError};
use crate::i18n::{self, Lang};
use crate::limits::{ChatGate, Deliveries};
use crate::pending::Pending;
use crate::state::{StateStore, Tracked};
use crate::telegram::{InlineKeyboardButton, InlineKeyboardMarkup, Telegram};
use crate::{router, tracker};

/// Telegram limits, in UTF-16 code units.
pub const MAX_TEXT: usize = 4096;
pub const MAX_CAPTION: usize = 1024;
pub const MAX_ANSWER: usize = 200;
/// Titles are cut to this many units everywhere.
pub const MAX_TITLE: usize = 200;

#[derive(Debug, Clone)]
pub struct Settings {
    /// Largest file sent through Telegram, in bytes; above, a signed link.
    pub max_upload: u64,
    pub public_url: Option<String>,
    /// Where files are downloaded before being sent.
    pub tmp_dir: PathBuf,
    /// Delay between two job reads while tracking.
    pub poll_interval: Duration,
    /// How long a format choice stays valid.
    pub choice_ttl: Duration,
    /// Pause after a failed `getUpdates` without `retry_after`.
    pub error_pause: Duration,
    /// Pause after a `getUpdates` conflict (409: webhook or second poller).
    pub conflict_pause: Duration,
    /// `getUpdates` long-poll timeout, in seconds.
    pub long_poll_secs: u64,
    /// Smallest gap between two sends or edits in one chat.
    pub edit_gap: Duration,
    /// Deliveries running at once.
    pub max_deliveries: usize,
    /// Attempts of a send, edit or upload when Telegram answers 429.
    pub send_attempts: u32,
}

impl Settings {
    pub fn from_config(cfg: &Config) -> Settings {
        Settings {
            max_upload: cfg.max_upload,
            public_url: cfg.public_url.clone(),
            tmp_dir: cfg.tmp_dir.clone(),
            poll_interval: Duration::from_secs(3),
            choice_ttl: Duration::from_secs(30 * 60),
            error_pause: Duration::from_secs(5),
            conflict_pause: Duration::from_secs(30),
            long_poll_secs: 50,
            edit_gap: Duration::from_millis(1100),
            max_deliveries: cfg.max_deliveries,
            send_attempts: 3,
        }
    }
}

pub struct App<T, C> {
    pub tg: T,
    pub core: C,
    pub settings: Settings,
    pub state: StateStore,
    pending: Mutex<Pending>,
    /// Sends and edits per chat.
    pub gate: ChatGate,
    pub deliveries: Arc<Deliveries>,
    /// Cancelled on SIGTERM / Ctrl-C: the loop and the trackers stop.
    pub shutdown: CancellationToken,
    /// Update handlers and trackers.
    pub tasks: TaskTracker,
}

impl<T: Telegram + 'static, C: Core + 'static> App<T, C> {
    pub fn new(tg: T, core: C, settings: Settings, state: StateStore) -> Arc<Self> {
        let pending = Mutex::new(Pending::new(settings.choice_ttl));
        let gate = ChatGate::new(settings.edit_gap);
        let deliveries = Arc::new(Deliveries::new(settings.max_deliveries));
        Arc::new(App {
            tg,
            core,
            settings,
            state,
            pending,
            gate,
            deliveries,
            shutdown: CancellationToken::new(),
            tasks: TaskTracker::new(),
        })
    }

    pub(crate) fn pending(&self) -> MutexGuard<'_, Pending> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Restarts tracking of every job in the state; returns how many.
    pub fn resume(self: &Arc<Self>) -> usize {
        let all = self.state.snapshot();
        for t in &all {
            self.spawn_tracker(t.clone(), String::new());
        }
        all.len()
    }

    /// Follows a job until it is delivered, failed or gone. `shown` is the
    /// text the progress message already shows.
    pub fn spawn_tracker(self: &Arc<Self>, t: Tracked, shown: String) {
        let app = Arc::clone(self);
        self.tasks.spawn(tracker::track(app, t, shown));
    }

    /// Reads updates until shutdown; each update runs in its own task.
    /// Fails when Telegram rejects the token (401): nothing would work.
    pub async fn run(self: Arc<Self>) -> anyhow::Result<()> {
        let mut offset = 0i64;
        loop {
            let got = tokio::select! {
                _ = self.shutdown.cancelled() => break,
                r = self.tg.get_updates(offset, self.settings.long_poll_secs) => r,
            };
            match got {
                Ok(updates) => {
                    for u in updates {
                        offset = offset.max(u.update_id + 1);
                        let app = Arc::clone(&self);
                        self.tasks
                            .spawn(async move { router::handle_update(&app, u).await });
                    }
                }
                Err(e) if e.code == Some(401) => {
                    tracing::error!(error = %e, "Telegram rejected TELEGRAM_BOT_TOKEN: check the token");
                    anyhow::bail!("TELEGRAM_BOT_TOKEN rejected by Telegram (401)");
                }
                Err(e) => {
                    let wait = if e.code == Some(409) {
                        tracing::error!(
                            error = %e,
                            "getUpdates conflict: a webhook is set on this token \
                             (call deleteWebhook) or another bot instance polls with it"
                        );
                        self.settings.conflict_pause
                    } else {
                        let wait = e.retry_after.map(Duration::from_secs);
                        let wait = wait.unwrap_or(self.settings.error_pause);
                        tracing::warn!(error = %e, wait_ms = wait.as_millis() as u64, "getUpdates failed");
                        wait
                    };
                    if !self.pause(wait).await {
                        break;
                    }
                }
            }
        }
        tracing::info!("update loop stopped");
        Ok(())
    }

    /// Sleeps; returns false if the bot is shutting down.
    pub async fn pause(&self, d: Duration) -> bool {
        tokio::select! {
            _ = self.shutdown.cancelled() => false,
            _ = tokio::time::sleep(d) => true,
        }
    }

    /// Waits for every running task (tests, shutdown).
    pub async fn settle(&self) {
        self.tasks.close();
        self.tasks.wait().await;
        self.tasks.reopen();
    }
}

/// The text for a core failure.
pub fn core_error_text(l: Lang, e: &CoreError) -> String {
    i18n::error(l, e.code, e.retry_after)
}

/// The progress keyboard: one Cancel button carrying the job id (none if
/// the id would not fit Telegram's 64-byte button data).
pub fn cancel_keyboard(l: Lang, job_id: &str) -> InlineKeyboardMarkup {
    let data = format!("x:{job_id}");
    let rows = if data.len() <= 64 {
        vec![vec![InlineKeyboardButton::new(i18n::btn_cancel(l), data)]]
    } else {
        vec![]
    };
    InlineKeyboardMarkup {
        inline_keyboard: rows,
    }
}

/// Cuts `s` to at most `max` UTF-16 code units (Telegram's unit), on a
/// character boundary, adding "…" when something was cut.
pub fn clip(s: &str, max: usize) -> String {
    if s.encode_utf16().count() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 1; // the ellipsis
    for c in s.chars() {
        used += c.len_utf16();
        if used > max {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_counts_utf16_and_keeps_chars_whole() {
        assert_eq!(clip("abc", 3), "abc");
        assert_eq!(clip("abcd", 3), "ab…");
        // An emoji is two UTF-16 units.
        let s = "😀😀😀";
        let c = clip(s, 5);
        assert_eq!(c, "😀😀…");
        assert!(c.encode_utf16().count() <= 5);
        let uuid = "0193a5c4-7e2b-7c3d-9f00-123456789abc";
        assert_eq!(
            cancel_keyboard(Lang::En, uuid).inline_keyboard[0][0].callback_data,
            format!("x:{uuid}")
        );
        assert!(
            cancel_keyboard(Lang::En, &"j".repeat(63))
                .inline_keyboard
                .is_empty()
        );
    }
}
