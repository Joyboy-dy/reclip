//! reqwest implementation of the Bot API client.

use std::path::Path;
use std::time::Duration;

use reqwest::multipart::{Form, Part};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio_util::io::ReaderStream;

use super::Telegram;
use super::error::{SentMessage, TgError, parse};
use super::types::{InlineKeyboardMarkup, MessageId, Update};

const UPLOAD_CHUNK: usize = 64 * 1024;
const JSON_TIMEOUT: Duration = Duration::from_secs(30);
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

pub struct HttpTelegram {
    http: reqwest::Client,
    base: String,
}

impl HttpTelegram {
    pub fn new(api_url: &str, token: &str) -> Result<Self, TgError> {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            // A redirect would carry the token URL into reqwest's logs.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(TgError::transport)?;
        Ok(Self {
            http,
            base: format!("{}/bot{}", api_url.trim_end_matches('/'), token),
        })
    }

    fn url(&self, method: &str) -> String {
        format!("{}/{method}", self.base)
    }

    async fn post_json<T: DeserializeOwned>(
        &self,
        method: &str,
        body: &Value,
        timeout: Duration,
    ) -> Result<T, TgError> {
        let resp = self
            .http
            .post(self.url(method))
            .timeout(timeout)
            .json(body)
            .send()
            .await
            .map_err(TgError::transport)?;
        parse(resp).await
    }

    /// One attempt: a 429 goes back to the caller, which paces the chat
    /// and retries ([`crate::send`]).
    async fn json_call<T: DeserializeOwned>(
        &self,
        method: &str,
        body: Value,
    ) -> Result<T, TgError> {
        self.post_json(method, &body, JSON_TIMEOUT).await
    }

    async fn file_part(path: &Path) -> Result<Part, TgError> {
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|e| TgError::local(format!("cannot open {}: {e}", path.display())))?;
        let len = file
            .metadata()
            .await
            .map_err(|e| TgError::local(e.to_string()))?
            .len();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let body = reqwest::Body::wrap_stream(ReaderStream::with_capacity(file, UPLOAD_CHUNK));
        Part::stream_with_length(body, len)
            .file_name(name)
            .mime_str(mime_for(path))
            .map_err(TgError::transport)
    }

    async fn upload(
        &self,
        method: &str,
        field: &str,
        path: &Path,
        fields: &[(&str, String)],
    ) -> Result<(), TgError> {
        let mut form = Form::new().part(field.to_string(), Self::file_part(path).await?);
        for (k, v) in fields {
            form = form.text(k.to_string(), v.clone());
        }
        let resp = self
            .http
            .post(self.url(method))
            .timeout(UPLOAD_TIMEOUT)
            .multipart(form)
            .send()
            .await
            .map_err(TgError::transport)?;
        let _: Value = parse(resp).await?;
        Ok(())
    }
}

/// One update of a batch. An update the bot cannot read becomes an empty
/// one, so the offset still moves past it; one without an id is dropped.
fn decode_update(v: Value) -> Option<Update> {
    let update_id = v.get("update_id").and_then(Value::as_i64)?;
    match serde_json::from_value(v) {
        Ok(u) => Some(u),
        Err(e) => {
            tracing::warn!(update_id, error = %e, "undecodable update skipped");
            Some(Update {
                update_id,
                message: None,
                callback_query: None,
            })
        }
    }
}

fn mime_for(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    match ext.as_deref() {
        Some("mp4") => "video/mp4",
        Some("mkv") => "video/x-matroska",
        Some("mov") => "video/quicktime",
        Some("mp3") => "audio/mpeg",
        Some("m4a") => "audio/mp4",
        Some("opus") => "audio/ogg",
        Some("flac") => "audio/flac",
        Some("wav") => "audio/wav",
        _ => "application/octet-stream",
    }
}

fn kb(k: Option<&InlineKeyboardMarkup>) -> Value {
    serde_json::to_value(k).unwrap_or(Value::Null)
}

fn opt_field(
    fields: &mut Vec<(&'static str, String)>,
    name: &'static str,
    v: Option<impl ToString>,
) {
    if let Some(v) = v {
        fields.push((name, v.to_string()));
    }
}

fn is_not_modified(e: &TgError) -> bool {
    e.code == Some(400) && e.description.contains("message is not modified")
}

impl Telegram for HttpTelegram {
    async fn get_updates(&self, offset: i64, timeout_s: u64) -> Result<Vec<Update>, TgError> {
        let body = json!({
            "offset": offset,
            "timeout": timeout_s,
            "allowed_updates": ["message", "callback_query"],
        });
        let raw: Vec<Value> = self
            .post_json("getUpdates", &body, Duration::from_secs(timeout_s + 15))
            .await?;
        Ok(raw.into_iter().filter_map(decode_update).collect())
    }

    async fn send_message(
        &self,
        chat: i64,
        text: &str,
        keyboard: Option<&InlineKeyboardMarkup>,
    ) -> Result<MessageId, TgError> {
        let mut body = json!({"chat_id": chat, "text": text, "disable_web_page_preview": true});
        if keyboard.is_some() {
            body["reply_markup"] = kb(keyboard);
        }
        Ok(self
            .json_call::<SentMessage>("sendMessage", body)
            .await?
            .message_id)
    }

    async fn edit_message(
        &self,
        chat: i64,
        msg: MessageId,
        text: &str,
        keyboard: Option<&InlineKeyboardMarkup>,
    ) -> Result<(), TgError> {
        let empty = InlineKeyboardMarkup::default();
        let body = json!({
            "chat_id": chat,
            "message_id": msg,
            "text": text,
            "disable_web_page_preview": true,
            // An empty keyboard removes the previous one.
            "reply_markup": kb(Some(keyboard.unwrap_or(&empty))),
        });
        match self.json_call::<Value>("editMessageText", body).await {
            Err(e) if is_not_modified(&e) => Ok(()),
            other => other.map(|_| ()),
        }
    }

    async fn send_photo_url(
        &self,
        chat: i64,
        url: &str,
        caption: &str,
        keyboard: Option<&InlineKeyboardMarkup>,
    ) -> Result<MessageId, TgError> {
        let mut body = json!({"chat_id": chat, "photo": url, "caption": caption});
        if keyboard.is_some() {
            body["reply_markup"] = kb(keyboard);
        }
        Ok(self
            .json_call::<SentMessage>("sendPhoto", body)
            .await?
            .message_id)
    }

    async fn send_video(
        &self,
        chat: i64,
        path: &Path,
        caption: &str,
        duration: Option<u32>,
        width: Option<u32>,
        height: Option<u32>,
    ) -> Result<(), TgError> {
        let mut f = vec![
            ("chat_id", chat.to_string()),
            ("caption", caption.to_string()),
            ("supports_streaming", "true".to_string()),
        ];
        opt_field(&mut f, "duration", duration);
        opt_field(&mut f, "width", width);
        opt_field(&mut f, "height", height);
        self.upload("sendVideo", "video", path, &f).await
    }

    async fn send_audio(
        &self,
        chat: i64,
        path: &Path,
        title: Option<&str>,
        performer: Option<&str>,
        duration: Option<u32>,
    ) -> Result<(), TgError> {
        let mut f = vec![("chat_id", chat.to_string())];
        opt_field(&mut f, "title", title);
        opt_field(&mut f, "performer", performer);
        opt_field(&mut f, "duration", duration);
        self.upload("sendAudio", "audio", path, &f).await
    }

    async fn answer_callback(&self, id: &str, text: Option<&str>) -> Result<(), TgError> {
        let mut body = json!({"callback_query_id": id});
        if let Some(t) = text {
            body["text"] = json!(t);
        }
        self.json_call::<Value>("answerCallbackQuery", body)
            .await
            .map(|_| ())
    }

    async fn delete_message(&self, chat: i64, msg: MessageId) -> Result<(), TgError> {
        let body = json!({"chat_id": chat, "message_id": msg});
        self.json_call::<Value>("deleteMessage", body)
            .await
            .map(|_| ())
    }
}
