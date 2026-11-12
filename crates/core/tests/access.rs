//! Who may call the API: visitors (cookie + Origin), API keys, delegation.

mod common;

use std::time::Duration;

use common::{TestServer, Visitor};
use reclip_core::access::keys::sha256_hex;
use reclip_core::store::{now, now_minus};
use reclip_shared::{ApiKeyInfo, Scope};
use reqwest::Response;
use serde_json::{Value, json};

fn job_body() -> Value {
    json!({"url": "https://ok", "format": "mp4"})
}

/// Assert the status and error code; return the JSON body.
async fn expect_error(r: Response, status: u16, code: &str) -> Value {
    assert_eq!(r.status(), status);
    let b: Value = r.json().await.unwrap();
    assert_eq!(b["code"], code, "{b}");
    b
}

fn with_delegate() -> Vec<Scope> {
    let mut s = Scope::default_set();
    s.push(Scope::Delegate);
    s
}

#[tokio::test]
async fn visitor_without_origin_cannot_post() {
    let s = TestServer::start(|_| {}).await;
    for v in [
        Visitor::without_origin(&s.base),
        Visitor::with_origin(&s.base, "https://evil.example"),
        Visitor::with_origin(&s.base, "null"),
    ] {
        let b = expect_error(v.post("/api/v1/jobs", job_body()).await, 403, "forbidden").await;
        assert_eq!(b["message"], "Cross-site request refused");
    }
    let id = s.create("https://ok").await;
    let bare = Visitor::without_origin(&s.base);
    let r = bare.delete(&format!("/api/v1/jobs/{id}")).await;
    expect_error(r, 403, "forbidden").await;
    expect_error(bare.cancel(&id).await, 403, "forbidden").await;
    let l: Value = s.get("/api/v1/jobs").await.json().await.unwrap();
    assert_eq!(l["jobs"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn origin_may_be_the_public_url() {
    let s = TestServer::start(|c| c.public_url = Some("https://r.example".into())).await;
    let v = Visitor::with_origin(&s.base, "https://r.example");
    assert_eq!(v.post("/api/v1/jobs", job_body()).await.status(), 202);
    let v = Visitor::with_origin(&s.base, "https://r.example:8443");
    let r = v.post("/api/v1/jobs", job_body()).await;
    expect_error(r, 403, "forbidden").await;
}

#[tokio::test]
async fn visitor_get_needs_no_origin() {
    let s = TestServer::start(|_| {}).await;
    let v = Visitor::without_origin(&s.base);
    assert_eq!(v.get("/api/v1/jobs").await.status(), 200);
    assert_eq!(v.get("/api/v1/config").await.status(), 200);
}

#[tokio::test]
async fn key_owns_its_jobs() {
    let s = TestServer::start(|_| {}).await;
    let a = s.agent(&Scope::default_set()).await;
    let b = s.agent(&Scope::default_set()).await;
    let id = a.create("https://ok").await;
    let l: Value = a.get("/api/v1/jobs").await.json().await.unwrap();
    assert_eq!(l["jobs"].as_array().unwrap().len(), 1);
    assert_eq!(l["jobs"][0]["id"], id);
    assert_eq!(a.get(&format!("/api/v1/jobs/{id}")).await.status(), 200);
    let owner = s.store.owner_of(&id).await.unwrap().unwrap();
    assert_eq!(owner, format!("key:{}", a.key_id()));
    for other in [&*b, &**s] {
        let r = other.get(&format!("/api/v1/jobs/{id}")).await;
        assert_eq!(r.status(), 404);
        let l: Value = other.get("/api/v1/jobs").await.json().await.unwrap();
        assert!(l["jobs"].as_array().unwrap().is_empty());
    }
}

#[tokio::test]
async fn bad_key_401() {
    let s = TestServer::start(|_| {}).await;
    let secret = "z".repeat(43);
    let revoked = s.key(None).await;
    assert!(s.store.revoke_key(&revoked[4..12]).await.unwrap());
    let expired = ApiKeyInfo {
        id: "expired1".into(),
        name: "old".into(),
        scopes: Scope::default_set(),
        unlimited: false,
        limits: Default::default(),
        created_at: now(),
        expires_at: Some(now_minus(Duration::from_secs(5))),
        revoked_at: None,
        last_used_at: None,
        request_count: 0,
    };
    s.store
        .insert_key(&expired, &sha256_hex(&secret))
        .await
        .unwrap();
    let valid = s.key(None).await;
    let c = reqwest::Client::new();
    let url = format!("{}/api/v1/jobs", s.base);
    for auth in [
        format!("Bearer rck_nosuchid_{secret}"),
        "Bearer hello".to_string(),
        format!("Bearer {revoked}"),
        format!("Bearer rck_expired1_{secret}"),
        format!("Basic {valid}"),
        valid.clone(),
        "Bearer".to_string(),
    ] {
        let r = c
            .get(&url)
            .header("Authorization", &auth)
            .send()
            .await
            .unwrap();
        assert_eq!(r.headers()["www-authenticate"], "Bearer", "{auth}");
        assert!(!r.headers().contains_key("set-cookie"), "{auth}");
        let b = expect_error(r, 401, "unauthorized").await;
        assert_eq!(b["message"], "Invalid or expired API key");
    }
    let r = c
        .get(&url)
        .header("Authorization", format!("bearer {valid}"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
}

#[tokio::test]
async fn key_scope_enforced() {
    let s = TestServer::start(|_| {}).await;
    let reader = s.agent(&[Scope::JobsRead]).await;
    let r = reader.post("/api/v1/jobs", job_body()).await;
    expect_error(r, 403, "forbidden").await;
    let r = reader
        .post("/api/v1/probe", json!({"url": "https://ok"}))
        .await;
    expect_error(r, 403, "forbidden").await;
    assert_eq!(reader.get("/api/v1/jobs").await.status(), 200);

    let writer = s.agent(&[Scope::JobsCreate]).await;
    let id = writer.create("https://ok").await;
    for path in ["", "/events", "/file"] {
        let r = writer.get(&format!("/api/v1/jobs/{id}{path}")).await;
        expect_error(r, 403, "forbidden").await;
    }
    expect_error(writer.get("/api/v1/jobs").await, 403, "forbidden").await;
    expect_error(writer.cancel(&id).await, 403, "forbidden").await;
    let r = writer.delete(&format!("/api/v1/jobs/{id}")).await;
    expect_error(r, 403, "forbidden").await;
}

#[tokio::test]
async fn delegation_requires_scope() {
    let s = TestServer::start(|_| {}).await;
    let plain = s.agent(&Scope::default_set()).await.on_behalf_of("tg:42");
    let r = plain.post("/api/v1/jobs", job_body()).await;
    expect_error(r, 403, "forbidden").await;

    let bot = s.agent(&with_delegate()).await;
    let (tg42, tg43) = (bot.on_behalf_of("tg:42"), bot.on_behalf_of("tg:43"));
    let id = tg42.create("https://ok").await;
    assert_eq!(s.store.owner_of(&id).await.unwrap().unwrap(), "tg:42");
    assert_eq!(tg42.get(&format!("/api/v1/jobs/{id}")).await.status(), 200);
    for other in [&*tg43, &*bot] {
        assert_eq!(other.get(&format!("/api/v1/jobs/{id}")).await.status(), 404);
    }
    let l: Value = tg43.get("/api/v1/jobs").await.json().await.unwrap();
    assert!(l["jobs"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn delegation_header_validated() {
    let s = TestServer::start(|_| {}).await;
    let bot = s.agent(&with_delegate()).await;
    let too_long = format!("tg:{}", "1".repeat(21));
    for who in ["tg:abc", "tg:", "42", "tg:-1", "TG:42", "web:42", &too_long] {
        let r = bot.on_behalf_of(who).get("/api/v1/jobs").await;
        expect_error(r, 400, "invalid_request").await;
    }
    let max = format!("tg:{}", "9".repeat(20));
    assert_eq!(
        bot.on_behalf_of(&max).get("/api/v1/jobs").await.status(),
        200
    );
}

#[tokio::test]
async fn private_mode_requires_key() {
    let s = TestServer::start(|c| c.public_web = false).await;
    let r = s.post("/api/v1/probe", json!({"url": "https://ok"})).await;
    assert_eq!(r.headers()["www-authenticate"], "Bearer");
    expect_error(r, 401, "unauthorized").await;
    expect_error(s.get("/api/v1/jobs").await, 401, "unauthorized").await;
    assert_eq!(s.get("/api/v1/config").await.status(), 200);
    let agent = s.agent(&Scope::default_set()).await;
    let r = agent
        .post("/api/v1/probe", json!({"url": "https://ok"}))
        .await;
    assert_eq!(r.status(), 200);
}

#[tokio::test]
async fn key_requests_set_no_cookie() {
    let s = TestServer::start(|_| {}).await;
    let agent = s.agent(&Scope::default_set()).await;
    let r = agent.get("/api/v1/jobs").await;
    assert_eq!(r.status(), 200);
    assert!(!r.headers().contains_key("set-cookie"));
    assert_eq!(r.headers()["cache-control"], "no-store");
    let r = agent.post("/api/v1/jobs", job_body()).await;
    assert_eq!(r.status(), 202);
    assert!(!r.headers().contains_key("set-cookie"));
}
