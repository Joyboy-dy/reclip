//! In-memory Bot API: records every call.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Instant;

use reclip_bot::telegram::*;

#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    Send {
        chat: i64,
        text: String,
        kb: Option<InlineKeyboardMarkup>,
    },
    Edit {
        chat: i64,
        msg: MessageId,
        text: String,
        kb: Option<InlineKeyboardMarkup>,
    },
    Photo {
        chat: i64,
        url: String,
        caption: String,
        kb: Option<InlineKeyboardMarkup>,
    },
    Video {
        chat: i64,
        name: String,
        bytes: u64,
        caption: String,
        duration: Option<u32>,
    },
    Audio {
        chat: i64,
        name: String,
        bytes: u64,
        title: Option<String>,
        performer: Option<String>,
    },
    Answer {
        id: String,
        text: Option<String>,
    },
    Delete {
        chat: i64,
        msg: MessageId,
    },
}

#[derive(Default)]
pub struct FakeTg {
    calls: Mutex<Vec<Call>>,
    next_id: AtomicI64,
    /// Scripted `getUpdates` answers; when empty, it waits forever.
    pub updates: Mutex<VecDeque<Result<Vec<Update>, TgError>>>,
    pub offsets: Mutex<Vec<i64>>,
    /// Every edit fails with a 429 (`retry_after` 0).
    pub fail_edits: AtomicBool,
    /// Per chat: how many of the next sends, edits and photos fail with a
    /// 429 (`retry_after` 0). Those attempts are not recorded as calls.
    pub throttle: Mutex<HashMap<i64, usize>>,
    /// (chat, when) of every attempt to send or edit in a chat.
    pub chat_log: Mutex<Vec<(i64, Instant)>>,
    /// An edit with this text waits for `release` before answering.
    pub hold_edit: Mutex<Option<String>>,
    pub release: tokio::sync::Notify,
    /// Errors returned by the next uploads, one per upload (still recorded).
    pub upload_errors: Mutex<VecDeque<TgError>>,
    /// (chat, when, text) of every edit.
    pub edit_log: Mutex<Vec<(i64, Instant, String)>>,
}

pub fn too_many(retry_after: u64) -> TgError {
    TgError {
        code: Some(429),
        description: "Too Many Requests".into(),
        retry_after: Some(retry_after),
    }
}

impl FakeTg {
    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }
    fn push(&self, c: Call) {
        self.calls.lock().unwrap().push(c);
    }
    /// Logs an attempt in `chat`; a 429 when the chat is throttled.
    fn attempt(&self, chat: i64) -> Result<(), TgError> {
        self.chat_log.lock().unwrap().push((chat, Instant::now()));
        let mut throttle = self.throttle.lock().unwrap();
        match throttle.get_mut(&chat) {
            Some(n) if *n > 0 => {
                *n -= 1;
                Err(too_many(0))
            }
            _ => Ok(()),
        }
    }
    fn new_id(&self) -> MessageId {
        100 + self.next_id.fetch_add(1, Ordering::SeqCst)
    }
    fn upload_result(&self) -> Result<(), TgError> {
        match self.upload_errors.lock().unwrap().pop_front() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
    /// Texts of sent and edited messages, photo captions and answers.
    pub fn texts(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Send { text, .. } | Call::Edit { text, .. } => Some(text),
                Call::Photo { caption, .. } => Some(caption),
                Call::Answer { text, .. } => text,
                _ => None,
            })
            .collect()
    }
    pub fn answers(&self) -> Vec<Option<String>> {
        self.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Answer { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }
}

fn file_info(path: &Path) -> (String, u64) {
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    (
        name,
        std::fs::metadata(path).map(|m| m.len()).unwrap_or(u64::MAX),
    )
}

impl Telegram for FakeTg {
    async fn get_updates(&self, offset: i64, _timeout_s: u64) -> Result<Vec<Update>, TgError> {
        self.offsets.lock().unwrap().push(offset);
        let next = self.updates.lock().unwrap().pop_front();
        match next {
            Some(r) => r,
            None => std::future::pending().await,
        }
    }
    async fn send_message(
        &self,
        chat: i64,
        text: &str,
        kb: Option<&InlineKeyboardMarkup>,
    ) -> Result<MessageId, TgError> {
        self.attempt(chat)?;
        self.push(Call::Send {
            chat,
            text: text.into(),
            kb: kb.cloned(),
        });
        Ok(self.new_id())
    }
    async fn edit_message(
        &self,
        chat: i64,
        msg: MessageId,
        text: &str,
        kb: Option<&InlineKeyboardMarkup>,
    ) -> Result<(), TgError> {
        self.attempt(chat)?;
        self.push(Call::Edit {
            chat,
            msg,
            text: text.into(),
            kb: kb.cloned(),
        });
        let entry = (chat, Instant::now(), text.to_string());
        self.edit_log.lock().unwrap().push(entry);
        let held = self.hold_edit.lock().unwrap().as_deref() == Some(text);
        if held {
            self.release.notified().await;
        }
        if self.fail_edits.load(Ordering::SeqCst) {
            return Err(too_many(0));
        }
        Ok(())
    }
    async fn send_photo_url(
        &self,
        chat: i64,
        url: &str,
        caption: &str,
        kb: Option<&InlineKeyboardMarkup>,
    ) -> Result<MessageId, TgError> {
        self.attempt(chat)?;
        self.push(Call::Photo {
            chat,
            url: url.into(),
            caption: caption.into(),
            kb: kb.cloned(),
        });
        Ok(self.new_id())
    }
    async fn send_video(
        &self,
        chat: i64,
        path: &Path,
        caption: &str,
        duration: Option<u32>,
        _w: Option<u32>,
        _h: Option<u32>,
    ) -> Result<(), TgError> {
        self.chat_log.lock().unwrap().push((chat, Instant::now()));
        let (name, bytes) = file_info(path);
        self.push(Call::Video {
            chat,
            name,
            bytes,
            caption: caption.into(),
            duration,
        });
        self.upload_result()
    }
    async fn send_audio(
        &self,
        chat: i64,
        path: &Path,
        title: Option<&str>,
        performer: Option<&str>,
        _d: Option<u32>,
    ) -> Result<(), TgError> {
        self.chat_log.lock().unwrap().push((chat, Instant::now()));
        let (name, bytes) = file_info(path);
        self.push(Call::Audio {
            chat,
            name,
            bytes,
            title: title.map(Into::into),
            performer: performer.map(Into::into),
        });
        self.upload_result()
    }
    async fn answer_callback(&self, id: &str, text: Option<&str>) -> Result<(), TgError> {
        self.push(Call::Answer {
            id: id.into(),
            text: text.map(Into::into),
        });
        Ok(())
    }
    async fn delete_message(&self, chat: i64, msg: MessageId) -> Result<(), TgError> {
        self.push(Call::Delete { chat, msg });
        Ok(())
    }
}
