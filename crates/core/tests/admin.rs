//! Admin key management, the learned public address and the public config.

mod common;

use common::{Agent, TestServer, Visitor};
use reqwest::Response;
use serde_json::{Value, json};

/// Assert the status and error code; return the JSON body.
async fn expect_error(r: Response, status: u16, code: &str) -> Value {
    assert_eq!(r.status(), status);
    let b: Value = r.json().await.unwrap();
    assert_eq!(b["code"], code, "{b}");
    b
}

const KEYS: &str = "/api/v1/admin/keys";

#[tokio::test]
async fn admin_key_lifecycle() {
    let s = TestServer::start(|_| {}).await;
    let admin = s.admin();
    let r = admin
        .post(
            KEYS,
            json!({"name": "bot", "scopes": ["probe", "jobs:read"]}),
        )
        .await;
    assert_eq!(r.status(), 201);
    assert_eq!(r.headers()["cache-control"], "no-store");
    let created: Value = r.json().await.unwrap();
    let key = created["key"].as_str().unwrap().to_string();
    let id = created["info"]["id"].as_str().unwrap().to_string();
    assert_eq!(created["info"]["scopes"], json!(["probe", "jobs:read"]));

    let agent = Agent::new(&s.base, &key);
    assert_eq!(agent.get("/api/v1/jobs").await.status(), 200);

    let r = admin.get(KEYS).await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["cache-control"], "no-store");
    let text = r.text().await.unwrap();
    let secret = key.rsplit('_').next().unwrap();
    assert!(!text.contains(secret), "{text}");
    let list: Value = serde_json::from_str(&text).unwrap();
    let keys = list["keys"].as_array().unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(
        (&keys[0]["id"], &keys[0]["name"]),
        (&json!(id), &json!("bot"))
    );
    assert!(keys[0].get("key").is_none());

    let r = admin.delete(&format!("{KEYS}/{id}")).await;
    assert_eq!(r.status(), 204);
    let r = agent.get("/api/v1/jobs").await;
    expect_error(r, 401, "unauthorized").await;
    let r = admin.delete(&format!("{KEYS}/nosuchid")).await;
    assert_eq!(r.status(), 404);
}

#[tokio::test]
async fn admin_create_validates_body() {
    let s = TestServer::start(|_| {}).await;
    let admin = s.admin();
    for body in [
        json!({"name": ""}),
        json!({"scopes": ["probe"]}),
        json!({"name": "x", "scopes": ["root"]}),
        json!("bot"),
    ] {
        expect_error(admin.post(KEYS, body).await, 400, "invalid_request").await;
    }
    let r = admin
        .client
        .post(format!("{}{KEYS}", s.base))
        .body("{")
        .send()
        .await
        .unwrap();
    expect_error(r, 400, "invalid_request").await;
    let list: Value = admin.get(KEYS).await.json().await.unwrap();
    assert!(list["keys"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn admin_requires_token() {
    let s = TestServer::start(|_| {}).await;
    let key = s.key(None).await;
    let clients = [Visitor::without_origin(&s.base), Visitor::new(&s.base)];
    let agents = [
        Agent::new(&s.base, "rca_wrong-token"),
        Agent::new(&s.base, &key),
        Agent::new(&s.base, &format!("{}x", common::ADMIN_TOKEN)),
    ];
    let apis = clients
        .iter()
        .map(|v| &**v)
        .chain(agents.iter().map(|a| &**a));
    for api in apis {
        for r in [
            api.get(KEYS).await,
            api.post(KEYS, json!({"name": "x"})).await,
            api.delete(&format!("{KEYS}/abcd1234")).await,
        ] {
            assert_eq!(r.headers()["www-authenticate"], "Bearer");
            assert_eq!(r.headers()["cache-control"], "no-store");
            assert!(!r.headers().contains_key("set-cookie"));
            let b = expect_error(r, 401, "unauthorized").await;
            assert_eq!(b["message"], "Invalid admin token");
        }
    }
    assert_eq!(s.store.list_keys().await.unwrap().len(), 1);
}

#[tokio::test]
async fn config_exposes_limits() {
    let s = TestServer::start(|c| {
        c.public_web = false;
        c.max_duration = 600;
        c.limits.web.jobs_per_hour = 7;
    })
    .await;
    let r = s.get("/api/v1/config").await;
    assert_eq!(r.status(), 200);
    let c: Value = r.json().await.unwrap();
    assert_eq!(c["public_web"], false);
    assert_eq!(c["max_duration"], 600);
    assert_eq!(
        c["limits"],
        json!({"probes_per_minute": 120, "jobs_per_hour": 7, "max_active": 80})
    );
}

#[tokio::test]
async fn learned_public_url_is_a_valid_origin() {
    let s = TestServer::start(|_| {}).await;
    let job = json!({"url": "https://ok", "format": "mp4"});
    let visitor = Visitor::with_origin(&s.base, "https://r.example");
    let r = visitor.post("/api/v1/jobs", job.clone()).await;
    expect_error(r, 403, "forbidden").await;
    assert_eq!(s.site.public_url(), None);

    // The trusted proxy (127.0.0.1 here) forwards the public origin.
    let r = s
        .client
        .get(format!("{}/api/v1/config", s.base))
        .header("x-forwarded-proto", "https")
        .header("x-forwarded-host", "r.example")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(s.site.public_url().as_deref(), Some("https://r.example"));
    let stored = s.store.setting("public_url").await.unwrap();
    assert_eq!(stored.as_deref(), Some("https://r.example"));

    assert_eq!(
        visitor.post("/api/v1/jobs", job.clone()).await.status(),
        202
    );
    let http = Visitor::with_origin(&s.base, "http://r.example");
    expect_error(http.post("/api/v1/jobs", job).await, 403, "forbidden").await;
}
