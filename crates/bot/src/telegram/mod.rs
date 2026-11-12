//! Telegram Bot API access.

pub mod client;
pub mod types;

use std::future::Future;
use std::path::Path;

pub use client::{HttpTelegram, TgError};
pub use types::*;

/// The Bot API calls the bot uses. Used with generics, never `dyn`.
pub trait Telegram: Send + Sync {
    fn get_updates(
        &self,
        offset: i64,
        timeout_s: u64,
    ) -> impl Future<Output = Result<Vec<Update>, TgError>> + Send;

    fn send_message(
        &self,
        chat: i64,
        text: &str,
        keyboard: Option<&InlineKeyboardMarkup>,
    ) -> impl Future<Output = Result<MessageId, TgError>> + Send;

    /// "message is not modified" counts as success.
    fn edit_message(
        &self,
        chat: i64,
        msg: MessageId,
        text: &str,
        keyboard: Option<&InlineKeyboardMarkup>,
    ) -> impl Future<Output = Result<(), TgError>> + Send;

    fn send_photo_url(
        &self,
        chat: i64,
        url: &str,
        caption: &str,
        keyboard: Option<&InlineKeyboardMarkup>,
    ) -> impl Future<Output = Result<MessageId, TgError>> + Send;

    fn send_video(
        &self,
        chat: i64,
        path: &Path,
        caption: &str,
        duration: Option<u32>,
        width: Option<u32>,
        height: Option<u32>,
    ) -> impl Future<Output = Result<(), TgError>> + Send;

    fn send_audio(
        &self,
        chat: i64,
        path: &Path,
        title: Option<&str>,
        performer: Option<&str>,
        duration: Option<u32>,
    ) -> impl Future<Output = Result<(), TgError>> + Send;

    fn answer_callback(
        &self,
        id: &str,
        text: Option<&str>,
    ) -> impl Future<Output = Result<(), TgError>> + Send;

    fn delete_message(
        &self,
        chat: i64,
        msg: MessageId,
    ) -> impl Future<Output = Result<(), TgError>> + Send;
}
