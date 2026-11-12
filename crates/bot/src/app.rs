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
use crate::pending::Pending;
use crate::state::{StateStore, Tracked};
use crate::telegram::{InlineKeyboardButton, InlineKeyboardMarkup, MessageId, Telegram};
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
    /// `getUpdates` long-poll timeout, in seconds.
    pub long_poll_secs: u64,
}

impl Settings {
    pub fn from_config(cfg: &Config, tmp_dir: PathBuf) -> Settings {
        Settings {
            max_upload: cfg.max_upload,
            public_url: cfg.public_url.clone(),
            tmp_dir,
            poll_interval: Duration::from_secs(3),
            choice_ttl: Duration::from_secs(30 * 60),
            error_pause: Duration::from_secs(5),
            long_poll_secs: 50,
        }
    }
}

pub struct App<T, C> {
    pub tg: T,
    pub core: C,
    pub settings: Settings,
    pub state: StateStore,
    pending: Mutex<Pending>,
    /// Cancelled on SIGTERM / Ctrl-C: the loop and the trackers stop.
    pub shutdown: CancellationToken,
    /// Update handlers and trackers.
    pub tasks: TaskTracker,
}

impl<T: Telegram + 'static, C: Core + 'static> App<T, C> {
    pub fn new(tg: T, core: C, settings: Settings, state: StateStore) -> Arc<Self> {
        let pending = Mutex::new(Pending::new(settings.choice_ttl));
        Arc::new(App {
            tg,
            core,
            settings,
            state,
            pending,
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
    pub async fn run(self: Arc<Self>) {
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
                Err(e) => {
                    let wait = e
                        .retry_after
                        .map(Duration::from_secs)
                        .unwrap_or(self.settings.error_pause);
                    tracing::warn!(error = %e, wait_ms = wait.as_millis() as u64, "getUpdates failed");
                    if !self.pause(wait).await {
                        break;
                    }
                }
            }
        }
        tracing::info!("update loop stopped");
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

    /// Sends a text; failures are logged.
    pub async fn say(
        &self,
        chat: i64,
        text: &str,
        kb: Option<&InlineKeyboardMarkup>,
    ) -> Option<MessageId> {
        match self.tg.send_message(chat, &clip(text, MAX_TEXT), kb).await {
            Ok(id) => Some(id),
            Err(e) => {
                tracing::warn!(chat, error = %e, "sendMessage failed");
                None
            }
        }
    }

    /// Edits `msg` if there is one, otherwise (or if the edit fails) sends
    /// a new message. Returns the message that shows the text.
    pub async fn replace(
        &self,
        chat: i64,
        msg: Option<MessageId>,
        text: &str,
        kb: Option<&InlineKeyboardMarkup>,
    ) -> Option<MessageId> {
        if let Some(m) = msg {
            match self
                .tg
                .edit_message(chat, m, &clip(text, MAX_TEXT), kb)
                .await
            {
                Ok(()) => return Some(m),
                Err(e) => tracing::warn!(chat, msg = m, error = %e, "edit failed"),
            }
        }
        self.say(chat, text, kb).await
    }

    /// Answers a button click (stops the spinner); failures are logged.
    pub async fn answer(&self, id: &str, text: Option<&str>) {
        let text = text.map(|t| clip(t, MAX_ANSWER));
        if let Err(e) = self.tg.answer_callback(id, text.as_deref()).await {
            tracing::warn!(error = %e, "answerCallbackQuery failed");
        }
    }

    pub async fn delete(&self, chat: i64, msg: MessageId) {
        if let Err(e) = self.tg.delete_message(chat, msg).await {
            tracing::warn!(chat, msg, error = %e, "deleteMessage failed");
        }
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
