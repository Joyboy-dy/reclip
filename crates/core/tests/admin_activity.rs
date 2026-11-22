//! Who uses ReClip (activity), blocking, and the server view.

mod common;

use std::time::{Duration, Instant};

use common::*;
use reclip_shared::Scope;
use serde_json::{Value, json};

/// The activity report once `ready` holds (activity is written in the background).
async fn activity_until(s: &TestServer, ready: impl Fn(&Value) -> bool) -> Value {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let r: Value = s
            .admin()
            .get("/api/v1/admin/activity")
            .await
            .json()
            .await
            .unwrap();
        if ready(&r) || Instant::now() > end {
            return r;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn client<'a>(report: &'a Value, kind: &str) -> Option<&'a Value> {
    report["clients"]
        .as_array()?
        .iter()
        .find(|c| c["kind"] == kind)
}

#[tokio::test]
async fn a_visitor_shows_up_with_its_address_and_downloads() {
    let s = TestServer::start(|_| {}).await;
    let id = s.visitor.create("https://ok").await;
    s.visitor.wait_job(&id, "done").await;
    let r = activity_until(&s, |r| {
        client(r, "web").is_some_and(|c| c["downloads_total"] == 1)
    })
    .await;
    let c = client(&r, "web").expect("the visitor is listed");
    assert!(c["owner"].as_str().unwrap().starts_with("web:"));
    assert_eq!(c["last_ip"], "127.0.0.1");
    assert_eq!(c["ips"][0]["ip"], "127.0.0.1");
    assert_eq!(
        (
            c["downloads_day"].as_u64(),
            c["bytes_total"].as_u64().map(|b| b > 0)
        ),
        (Some(1), Some(true))
    );
    assert_eq!(r["retention_days"], 30);
    assert_eq!(r["new_devices_hour"], 1);
}

#[tokio::test]
async fn blocking_a_device_then_lifting_it() {
    let s = TestServer::start(|_| {}).await;
    let probe = || {
        s.visitor
            .post("/api/v1/probe", json!({"url": "https://ok"}))
    };
    assert_eq!(probe().await.status(), 200);
    let r = activity_until(&s, |r| client(r, "web").is_some()).await;
    let owner = client(&r, "web").unwrap()["owner"]
        .as_str()
        .unwrap()
        .to_string();

    let made = s
        .admin()
        .post(
            "/api/v1/admin/blocks",
            json!({"kind": "owner", "target": owner, "reason": "shared link"}),
        )
        .await;
    assert_eq!(made.status(), 201);
    let block: Value = made.json().await.unwrap();
    let refused = probe().await;
    assert_eq!(refused.status(), 403);
    let body: Value = refused.json().await.unwrap();
    assert_eq!(body["code"], "blocked");
    // The reason stays with the admins.
    assert!(!body["message"].as_str().unwrap().contains("shared link"));
    let create = s
        .visitor
        .post(
            "/api/v1/jobs",
            json!({"url": "https://ok", "format": "mp4"}),
        )
        .await;
    assert_eq!(create.status(), 403);
    // Another browser is not concerned.
    let other = Visitor::new(&s.base);
    assert_eq!(
        other
            .post("/api/v1/probe", json!({"url": "https://ok"}))
            .await
            .status(),
        200
    );

    let r = activity_until(&s, |_| true).await;
    let listed = r["clients"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["owner"] == owner)
        .unwrap();
    assert_eq!(listed["block"]["reason"], "shared link");

    let lifted = s
        .admin()
        .delete(&format!("/api/v1/admin/blocks/{}", block["id"]))
        .await;
    assert_eq!(lifted.status(), 204);
    assert_eq!(probe().await.status(), 200);
    assert_eq!(
        s.admin()
            .delete(&format!("/api/v1/admin/blocks/{}", block["id"]))
            .await
            .status(),
        404
    );
}

#[tokio::test]
async fn blocking_an_address_or_range() {
    let s = TestServer::start(|_| {}).await;
    let admin = s.admin();
    for wide in ["0.0.0.0/0", "127.0.0.0/8", "not an ip"] {
        let r = admin
            .post(
                "/api/v1/admin/blocks",
                json!({"kind": "ip", "target": wide}),
            )
            .await;
        assert_eq!(r.status(), 400, "{wide}");
    }
    let r = admin
        .post(
            "/api/v1/admin/blocks",
            json!({"kind": "ip", "target": "127.0.0.1"}),
        )
        .await;
    assert_eq!(r.status(), 201);
    let b: Value = r.json().await.unwrap();
    assert_eq!(b["target"], "127.0.0.1/32");
    assert_eq!(b["created_by"], Value::Null);
    let probe = s
        .visitor
        .post("/api/v1/probe", json!({"url": "https://ok"}))
        .await;
    assert_eq!(probe.status(), 403);
    // A key called from the same address is blocked too.
    let agent = s.agent(&[Scope::Probe]).await;
    assert_eq!(
        agent
            .post("/api/v1/probe", json!({"url": "https://ok"}))
            .await
            .status(),
        403
    );
    let list: Value = admin
        .get("/api/v1/admin/blocks")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(list["blocks"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn the_bot_passes_the_telegram_profile() {
    let s = TestServer::start(|_| {}).await;
    let bot = s.agent(&[Scope::Probe, Scope::Delegate]).await;
    let r = reqwest::Client::new()
        .post(format!("{}/api/v1/probe", s.base))
        .bearer_auth(&bot.key)
        .header(ON_BEHALF_OF, "tg:42")
        .header("x-reclip-tg-username", "jean_d")
        .header("x-reclip-tg-name", "Jos%C3%A9%20D")
        .json(&json!({"url": "https://ok"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let report = activity_until(&s, |r| client(r, "telegram").is_some()).await;
    let c = client(&report, "telegram").unwrap();
    assert_eq!(
        (c["owner"].as_str(), c["label"].as_str()),
        (Some("tg:42"), Some("@jean_d"))
    );
    assert_eq!(c["tg_name"], "José D");
    // The bot's own address is not the user's.
    assert_eq!(c["last_ip"], Value::Null);
    let blocked = s
        .admin()
        .post(
            "/api/v1/admin/blocks",
            json!({"kind": "owner", "target": "tg:42"}),
        )
        .await;
    assert_eq!(blocked.status(), 201);
    let again = bot
        .on_behalf_of("tg:42")
        .post("/api/v1/probe", json!({"url": "https://ok"}))
        .await;
    assert_eq!(again.json::<Value>().await.unwrap()["code"], "blocked");
}

#[tokio::test]
async fn admin_routes_need_the_admin() {
    let s = TestServer::start(|_| {}).await;
    for path in [
        "/api/v1/admin/activity",
        "/api/v1/admin/blocks",
        "/api/v1/admin/server",
    ] {
        assert_eq!(s.visitor.get(path).await.status(), 401, "{path}");
    }
    let stats: Value = s
        .admin()
        .get("/api/v1/admin/server")
        .await
        .json()
        .await
        .unwrap();
    assert!(stats["cpu_cores"].as_u64().unwrap() >= 1);
    assert_eq!(stats["cpu_percent"], Value::Null);
    assert_eq!(stats["jobs_running"], 0);
    // The second call measures rates over the time in between.
    let again: Value = s
        .admin()
        .get("/api/v1/admin/server")
        .await
        .json()
        .await
        .unwrap();
    assert!(again["uptime_secs"].is_u64());
}
