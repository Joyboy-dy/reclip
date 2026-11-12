//! Everything the bot sends to a chat goes through here: each send or edit
//! waits for the chat's turn ([`crate::limits::ChatGate`]) and a Telegram
//! 429 is retried, up to `send_attempts` attempts in total.

use std::future::Future;
use std::hash::{BuildHasher, RandomState};
use std::path::Path;
use std::time::Duration;

use crate::app::{App, MAX_ANSWER, MAX_TEXT, clip};
use crate::core_client::Core;
use crate::telegram::{InlineKeyboardMarkup, MessageId, Telegram, TgError};

/// Longest wait honoured after a 429.
pub const MAX_RETRY_WAIT: u64 = 60;

/// Up to half a second, so tasks refused together do not retry together.
fn jitter() -> Duration {
    let n = RandomState::new().hash_one(std::time::Instant::now());
    Duration::from_millis(n % 500)
}

impl<T: Telegram + 'static, C: Core + 'static> App<T, C> {
    /// Runs `f` (one call to `chat`) in the chat's turn; on 429 waits
    /// `retry_after` (capped, plus jitter) and tries again.
    pub(crate) async fn to_chat<R, F, Fut>(&self, chat: i64, f: F) -> Result<R, TgError>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = Result<R, TgError>>,
    {
        let attempts = self.settings.send_attempts.max(1);
        let mut attempt = 1;
        loop {
            self.gate.turn(chat).await;
            match f().await {
                Err(e) if e.code == Some(429) && attempt < attempts => {
                    let secs = e.retry_after.unwrap_or(1).min(MAX_RETRY_WAIT);
                    let wait = match secs {
                        0 => Duration::ZERO,
                        s => Duration::from_secs(s) + jitter(),
                    };
                    let wait_ms = wait.as_millis() as u64;
                    tracing::warn!(chat, attempt, wait_ms, "rate-limited by Telegram");
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                }
                other => return other,
            }
        }
    }

    /// Sends a text; failures are logged.
    pub async fn say(
        &self,
        chat: i64,
        text: &str,
        kb: Option<&InlineKeyboardMarkup>,
    ) -> Option<MessageId> {
        let text = clip(text, MAX_TEXT);
        match self
            .to_chat(chat, || self.tg.send_message(chat, &text, kb))
            .await
        {
            Ok(id) => Some(id),
            Err(e) => {
                tracing::warn!(chat, error = %e, "sendMessage failed");
                None
            }
        }
    }

    /// Edits a message in the chat's turn (a message that must arrive:
    /// progress edits use the gate's `allow` instead).
    pub async fn edit(
        &self,
        chat: i64,
        msg: MessageId,
        text: &str,
        kb: Option<&InlineKeyboardMarkup>,
    ) -> Result<(), TgError> {
        let text = clip(text, MAX_TEXT);
        self.to_chat(chat, || self.tg.edit_message(chat, msg, &text, kb))
            .await
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
            match self.edit(chat, m, text, kb).await {
                Ok(()) => return Some(m),
                Err(e) => tracing::warn!(chat, msg = m, error = %e, "edit failed"),
            }
        }
        self.say(chat, text, kb).await
    }

    pub async fn photo(
        &self,
        chat: i64,
        url: &str,
        caption: &str,
        kb: Option<&InlineKeyboardMarkup>,
    ) -> Result<MessageId, TgError> {
        self.to_chat(chat, || self.tg.send_photo_url(chat, url, caption, kb))
            .await
    }

    /// Sends the file as audio (title and performer) or as video (caption).
    pub async fn send_file(
        &self,
        chat: i64,
        path: &Path,
        audio: bool,
        title: &str,
        performer: Option<&str>,
        duration: Option<u32>,
    ) -> Result<(), TgError> {
        if audio {
            self.to_chat(chat, || {
                self.tg
                    .send_audio(chat, path, Some(title), performer, duration)
            })
            .await
        } else {
            let caption = clip(title, crate::app::MAX_CAPTION);
            self.to_chat(chat, || {
                self.tg
                    .send_video(chat, path, &caption, duration, None, None)
            })
            .await
        }
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
