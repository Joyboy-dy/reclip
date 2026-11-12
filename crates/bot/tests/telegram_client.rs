use std::time::Instant;

use reclip_bot::telegram::{
    ChatKind, HttpTelegram, InlineKeyboardButton, InlineKeyboardMarkup, Telegram,
};
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "123:SECRET-TOKEN";

async fn setup() -> (MockServer, HttpTelegram) {
    let server = MockServer::start().await;
    let tg = HttpTelegram::new(&server.uri(), TOKEN).unwrap();
    (server, tg)
}

fn ok(result: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"ok": true, "result": result}))
}

fn keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup {
        inline_keyboard: vec![vec![InlineKeyboardButton::new("✖ Cancel", "x:abc")]],
    }
}

fn api(m: &str) -> String {
    format!("/bot{TOKEN}/{m}")
}

#[tokio::test]
async fn get_updates_sends_offset_and_parses() {
    let (server, tg) = setup().await;
    Mock::given(method("POST"))
        .and(path(api("getUpdates")))
        .and(body_json(json!({
            "offset": 7, "timeout": 30, "allowed_updates": ["message", "callback_query"]
        })))
        .respond_with(ok(json!([
            {"update_id": 7, "message": {"message_id": 1, "chat": {"id": 5, "type": "private"},
              "from": {"id": 5, "language_code": "fr"}, "text": "hi"}},
            {"update_id": 8, "callback_query": {"id": "cb", "from": {"id": 5},
              "data": "x:1", "message": {"message_id": 2, "chat": {"id": -9, "type": "supergroup"}}}}
        ])))
        .expect(1)
        .mount(&server)
        .await;
    let ups = tg.get_updates(7, 30).await.unwrap();
    assert_eq!(ups.len(), 2);
    let m = ups[0].message.as_ref().unwrap();
    assert_eq!(m.chat.kind, ChatKind::Private);
    assert_eq!(
        m.from.as_ref().unwrap().language_code.as_deref(),
        Some("fr")
    );
    let cb = ups[1].callback_query.as_ref().unwrap();
    assert_eq!(cb.message.as_ref().unwrap().chat.kind, ChatKind::Other);
    assert_eq!(cb.data.as_deref(), Some("x:1"));
}

#[tokio::test]
async fn send_message_with_keyboard_returns_id() {
    let (server, tg) = setup().await;
    Mock::given(method("POST"))
        .and(path(api("sendMessage")))
        .and(body_json(json!({
            "chat_id": 5, "text": "hello", "disable_web_page_preview": true,
            "reply_markup": {"inline_keyboard": [[{"text": "✖ Cancel", "callback_data": "x:abc"}]]}
        })))
        .respond_with(ok(json!({"message_id": 42})))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        tg.send_message(5, "hello", Some(&keyboard()))
            .await
            .unwrap(),
        42
    );
}

#[tokio::test]
async fn edit_without_keyboard_clears_it() {
    let (server, tg) = setup().await;
    Mock::given(method("POST"))
        .and(path(api("editMessageText")))
        .and(body_json(json!({
            "chat_id": 5, "message_id": 42, "text": "done", "disable_web_page_preview": true,
            "reply_markup": {"inline_keyboard": []}
        })))
        .respond_with(ok(json!(true)))
        .expect(1)
        .mount(&server)
        .await;
    tg.edit_message(5, 42, "done", None).await.unwrap();
}

#[tokio::test]
async fn message_not_modified_is_success() {
    let (server, tg) = setup().await;
    Mock::given(method("POST"))
        .and(path(api("editMessageText")))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "ok": false, "error_code": 400,
            "description": "Bad Request: message is not modified: specified new message content"
        })))
        .expect(1)
        .mount(&server)
        .await;
    tg.edit_message(5, 42, "same", None).await.unwrap();
}

#[tokio::test]
async fn rate_limit_waits_then_retries_once() {
    let (server, tg) = setup().await;
    Mock::given(method("POST"))
        .and(path(api("editMessageText")))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({
            "ok": false, "error_code": 429, "description": "Too Many Requests: retry after 1",
            "parameters": {"retry_after": 1}
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(api("editMessageText")))
        .respond_with(ok(json!(true)))
        .mount(&server)
        .await;
    let t = Instant::now();
    tg.edit_message(5, 42, "50 %", None).await.unwrap();
    assert!(t.elapsed().as_millis() >= 900, "waited for retry_after");
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn persistent_rate_limit_gives_up_after_one_retry() {
    let (server, tg) = setup().await;
    Mock::given(method("POST"))
        .and(path(api("sendMessage")))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({
            "ok": false, "error_code": 429, "description": "Too Many Requests",
            "parameters": {"retry_after": 1}
        })))
        .mount(&server)
        .await;
    let err = tg.send_message(5, "x", None).await.unwrap_err();
    assert_eq!(err.code, Some(429));
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn photo_callback_and_delete_requests() {
    let (server, tg) = setup().await;
    Mock::given(method("POST"))
        .and(path(api("sendPhoto")))
        .and(body_json(
            json!({"chat_id": 5, "photo": "https://i/x.jpg", "caption": "cap"}),
        ))
        .respond_with(ok(json!({"message_id": 9})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(api("answerCallbackQuery")))
        .and(body_json(json!({"callback_query_id": "cb", "text": "ok"})))
        .respond_with(ok(json!(true)))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(api("deleteMessage")))
        .and(body_json(json!({"chat_id": 5, "message_id": 9})))
        .respond_with(ok(json!(true)))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        tg.send_photo_url(5, "https://i/x.jpg", "cap", None)
            .await
            .unwrap(),
        9
    );
    tg.answer_callback("cb", Some("ok")).await.unwrap();
    tg.delete_message(5, 9).await.unwrap();
}

#[tokio::test]
async fn send_video_and_audio_are_multipart() {
    let (server, tg) = setup().await;
    Mock::given(method("POST"))
        .and(path(api("sendVideo")))
        .respond_with(ok(json!({"message_id": 1})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(api("sendAudio")))
        .respond_with(ok(json!({"message_id": 2})))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("clip.mp4");
    std::fs::write(&video, b"VIDEO-BYTES").unwrap();
    let audio = dir.path().join("song.mp3");
    std::fs::write(&audio, b"AUDIO-BYTES").unwrap();

    tg.send_video(5, &video, "cap", Some(12), Some(1280), Some(720))
        .await
        .unwrap();
    tg.send_audio(5, &audio, Some("Song"), Some("Artist"), Some(200))
        .await
        .unwrap();

    let reqs = server.received_requests().await.unwrap();
    let body = |i: usize| String::from_utf8_lossy(&reqs[i].body).into_owned();
    let ct = reqs[0]
        .headers
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(ct.starts_with("multipart/form-data"));
    let v = body(0);
    for needle in [
        "name=\"video\"; filename=\"clip.mp4\"",
        "VIDEO-BYTES",
        "name=\"chat_id\"",
        "name=\"caption\"",
        "name=\"duration\"",
        "name=\"width\"",
        "name=\"height\"",
        "name=\"supports_streaming\"",
    ] {
        assert!(v.contains(needle), "video body lacks {needle}");
    }
    let a = body(1);
    for needle in [
        "name=\"audio\"; filename=\"song.mp3\"",
        "AUDIO-BYTES",
        "name=\"title\"",
        "name=\"performer\"",
        "name=\"duration\"",
    ] {
        assert!(a.contains(needle), "audio body lacks {needle}");
    }
}

#[tokio::test]
async fn upload_is_retried_after_429_with_a_fresh_form() {
    let (server, tg) = setup().await;
    Mock::given(method("POST"))
        .and(path(api("sendVideo")))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({
            "ok": false, "error_code": 429, "description": "slow down",
            "parameters": {"retry_after": 1}
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(api("sendVideo")))
        .respond_with(ok(json!({"message_id": 1})))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let video = dir.path().join("clip.mp4");
    std::fs::write(&video, b"VIDEO-BYTES").unwrap();
    tg.send_video(5, &video, "", None, None, None)
        .await
        .unwrap();
    let reqs = server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 2);
    assert!(String::from_utf8_lossy(&reqs[1].body).contains("VIDEO-BYTES"));
}

#[tokio::test]
async fn errors_never_contain_the_token() {
    // Telegram-side error.
    let (server, tg) = setup().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "ok": false, "error_code": 403, "description": "Forbidden: bot was blocked by the user"
        })))
        .mount(&server)
        .await;
    let err = tg.send_message(5, "x", None).await.unwrap_err();
    assert_eq!(err.code, Some(403));
    assert!(!format!("{err} {err:?}").contains("SECRET-TOKEN"));

    // Transport error: nothing listens on this port, reqwest would put the URL in the message.
    let dead = HttpTelegram::new("http://127.0.0.1:1", TOKEN).unwrap();
    let err = dead.send_message(5, "x", None).await.unwrap_err();
    assert_eq!(err.code, None);
    assert!(!format!("{err} {err:?}").contains("SECRET-TOKEN"));
}
