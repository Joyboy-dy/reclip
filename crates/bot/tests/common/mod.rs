//! In-memory fakes of the Telegram and core clients, and update builders.
#![allow(dead_code)]

mod fake_core;
mod fake_tg;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

pub use fake_core::*;
pub use fake_tg::*;
use reclip_bot::app::{App, Settings};
use reclip_bot::router;
use reclip_bot::state::StateStore;
use reclip_bot::telegram::*;
use reclip_shared::*;
use serde_json::json;

pub fn job(id: &str, state: JobState, progress: Option<f32>, size: Option<u64>) -> Job {
    Job {
        id: id.into(),
        url: "https://v.example/1".into(),
        format: MediaFormat::Mp4,
        format_id: None,
        title: Some("T".into()),
        thumbnail: None,
        state,
        filename: None,
        size,
        error: None,
        progress,
        speed: None,
        eta: None,
        created_at: "2026-11-12T00:00:00Z".into(),
        expires_at: None,
        clip: None,
        strip_metadata: false,
        resumable: false,
        playlist_item: None,
    }
}

pub fn video(title: &str, thumb: Option<&str>, heights: &[u32]) -> ProbeResponse {
    ProbeResponse::Video(VideoInfo {
        title: title.into(),
        thumbnail: thumb.map(Into::into),
        duration: Some(185.0),
        uploader: Some("Uploader".into()),
        formats: heights
            .iter()
            .map(|h| FormatOption {
                id: format!("f{h}"),
                label: format!("{h}p"),
                height: *h,
                codec: None,
            })
            .collect(),
    })
}

pub type TestApp = Arc<App<FakeTg, FakeCore>>;

pub struct Env {
    pub app: TestApp,
    pub dir: tempfile::TempDir,
}

pub fn settings(dir: &Path) -> Settings {
    Settings {
        max_upload: 1000,
        public_url: Some("https://reclip.example".into()),
        tmp_dir: dir.join("tmp"),
        poll_interval: Duration::from_millis(2),
        choice_ttl: Duration::from_secs(1800),
        error_pause: Duration::from_millis(2),
        long_poll_secs: 50,
        // No gate unless a test asks for one.
        edit_gap: Duration::ZERO,
        max_deliveries: 3,
        upload_attempts: 3,
    }
}

pub fn env_with(core: FakeCore, edit: impl FnOnce(&mut Settings)) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let mut s = settings(dir.path());
    edit(&mut s);
    let state = StateStore::load(&dir.path().join("state.json")).unwrap();
    Env {
        app: App::new(FakeTg::default(), core, s, state),
        dir,
    }
}

pub fn env(core: FakeCore) -> Env {
    env_with(core, |_| {})
}

/// Handles one update, then waits for every task it started.
pub async fn send(app: &TestApp, u: Update) {
    router::handle_update(app, u).await;
    settle(app).await;
}

pub async fn settle(app: &TestApp) {
    tokio::time::timeout(Duration::from_secs(10), app.settle())
        .await
        .expect("tasks did not finish");
}

pub fn text_update(update_id: i64, user: i64, lang: &str, text: &str) -> Update {
    serde_json::from_value(json!({
        "update_id": update_id,
        "message": {
            "message_id": 1, "chat": {"id": user, "type": "private"},
            "from": {"id": user, "language_code": lang}, "text": text
        }
    }))
    .unwrap()
}

pub fn group_update(user: i64, text: &str) -> Update {
    serde_json::from_value(json!({
        "update_id": 1,
        "message": {
            "message_id": 1, "chat": {"id": -100, "type": "supergroup"},
            "from": {"id": user, "language_code": "en"}, "text": text
        }
    }))
    .unwrap()
}

/// A click by `user` on a message of `chat`; `msg_text` None = a photo message.
pub fn click(user: i64, chat: i64, msg: MessageId, msg_text: Option<&str>, data: &str) -> Update {
    let mut message = json!({ "message_id": msg, "chat": {"id": chat, "type": "private"} });
    if let Some(t) = msg_text {
        message["text"] = json!(t);
    }
    serde_json::from_value(json!({
        "update_id": 2,
        "callback_query": {
            "id": format!("cb-{user}-{data}"), "from": {"id": user, "language_code": "en"},
            "message": message, "data": data
        }
    }))
    .unwrap()
}

/// The callback data of every button of a keyboard.
pub fn buttons(kb: &Option<InlineKeyboardMarkup>) -> Vec<(String, String)> {
    kb.iter()
        .flat_map(|k| k.inline_keyboard.iter().flatten())
        .map(|b| (b.text.clone(), b.callback_data.clone()))
        .collect()
}
