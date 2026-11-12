//! `getUpdates` answers with an update the bot cannot decode.

use reclip_bot::telegram::{HttpTelegram, Telegram};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "123:SECRET-TOKEN";

#[tokio::test]
async fn an_undecodable_update_is_skipped_but_counted() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path(format!("/bot{TOKEN}/getUpdates")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"ok": true, "result": [
                {"update_id": 10, "message": {"message_id": 1, "chat": {"id": 5, "type": "private"},
                    "from": {"id": 5}, "text": "hi"}},
                {"update_id": 11, "message": {"message_id": "not a number", "chat": 5}},
                {"update_id": 12, "message": {"message_id": 2, "chat": {"id": 5, "type": "private"},
                    "from": {"id": 5}, "text": "again"}}
            ]})),
        )
        .mount(&server)
        .await;
    let tg = HttpTelegram::new(&server.uri(), TOKEN).unwrap();
    let ups = tg.get_updates(0, 1).await.unwrap();
    let ids: Vec<i64> = ups.iter().map(|u| u.update_id).collect();
    assert_eq!(ids, [10, 11, 12], "the offset can move past the bad one");
    assert!(ups[1].message.is_none() && ups[1].callback_query.is_none());
    assert_eq!(
        ups[2].message.as_ref().unwrap().text.as_deref(),
        Some("again")
    );
}
