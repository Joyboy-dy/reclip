//! Per-caller quotas: probes per minute, jobs per hour, active jobs.

mod common;

use common::{TestServer, Visitor};
use reclip_core::access::keys::create_key;
use reclip_shared::{CreateKeyRequest, Scope};
use reqwest::Response;
use serde_json::{Value, json};

const JOBS: &str = "/api/v1/jobs";
const PROBE: &str = "/api/v1/probe";

fn job_body(url: &str) -> Value {
    json!({"url": url, "format": "mp4"})
}

fn probe_body() -> Value {
    json!({"url": "https://ok"})
}

fn header<'a>(r: &'a Response, name: &str) -> &'a str {
    r.headers()[name].to_str().unwrap()
}

async fn expect_limited(r: Response) -> Response {
    assert_eq!(r.status(), 429);
    assert!(r.headers().contains_key("retry-after"));
    r
}

#[tokio::test]
async fn visitor_batch_of_62_is_accepted() {
    let s = TestServer::start(|_| {}).await;
    for n in 0..62 {
        let r = s
            .post(JOBS, job_body(&format!("https://slow.example/{n}")))
            .await;
        assert_eq!(r.status(), 202, "job {n}");
    }
}

#[tokio::test]
async fn active_cap() {
    let s = TestServer::start(|c| c.limits.web.max_active = 2).await;
    for n in 0..2 {
        let r = s
            .post(JOBS, job_body(&format!("https://slow.example/{n}")))
            .await;
        assert_eq!(r.status(), 202);
    }
    let r = expect_limited(s.post(JOBS, job_body("https://slow.example/2")).await).await;
    assert_eq!(header(&r, "retry-after"), "30");
    let b: Value = r.json().await.unwrap();
    assert_eq!(b["code"], "rate_limited");
    assert_eq!(b["message"], "Too many downloads in progress (max 2)");
}

#[tokio::test]
async fn active_refusal_keeps_the_hourly_slot() {
    let s = TestServer::start(|c| {
        c.limits.web.max_active = 1;
        c.limits.web.jobs_per_hour = 3;
    })
    .await;
    assert_eq!(
        s.post(JOBS, job_body("https://slow.example/a"))
            .await
            .status(),
        202
    );
    for _ in 0..3 {
        expect_limited(s.post(JOBS, job_body("https://slow.example/b")).await).await;
    }
    // Two of three hourly slots remain: free the active slot and create again.
    let l: Value = s.get(JOBS).await.json().await.unwrap();
    let id = l["jobs"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(s.cancel(&id).await.status(), 200);
    let r = s.post(JOBS, job_body("https://ok")).await;
    assert_eq!(r.status(), 202);
    assert_eq!(header(&r, "ratelimit-remaining"), "1");
}

#[tokio::test]
async fn probe_limit_headers() {
    let s = TestServer::start(|c| c.limits.web.probes_per_minute = 2).await;
    for remaining in ["1", "0"] {
        let r = s.post(PROBE, probe_body()).await;
        assert_eq!(r.status(), 200);
        assert_eq!(header(&r, "ratelimit-limit"), "2");
        assert_eq!(header(&r, "ratelimit-remaining"), remaining);
        assert!(r.headers().contains_key("ratelimit-reset"));
    }
    let r = expect_limited(s.post(PROBE, probe_body()).await).await;
    assert_eq!(header(&r, "ratelimit-remaining"), "0");
    let b: Value = r.json().await.unwrap();
    assert_eq!(b["code"], "rate_limited");
}

#[tokio::test]
async fn create_hourly_limit() {
    let s = TestServer::start(|c| c.limits.web.jobs_per_hour = 2).await;
    for remaining in ["1", "0"] {
        let r = s.post(JOBS, job_body("https://ok")).await;
        assert_eq!(r.status(), 202);
        assert_eq!(header(&r, "ratelimit-limit"), "2");
        assert_eq!(header(&r, "ratelimit-remaining"), remaining);
    }
    expect_limited(s.post(JOBS, job_body("https://ok")).await).await;
}

#[tokio::test]
async fn unlimited_key_never_limited() {
    let s = TestServer::start(|c| {
        c.limits.web.probes_per_minute = 1;
        c.limits.key.probes_per_minute = 1;
        c.limits.key.jobs_per_hour = 1;
        c.limits.key.max_active = 1;
    })
    .await;
    let req = CreateKeyRequest {
        name: "free".into(),
        scopes: None,
        unlimited: true,
        expires_in_days: None,
        limits: None,
    };
    let key = create_key(&s.store, req).await.unwrap().key;
    let agent = common::Agent::new(&s.base, &key);
    for _ in 0..3 {
        let r = agent.post(PROBE, probe_body()).await;
        assert_eq!(r.status(), 200);
        assert!(!r.headers().contains_key("ratelimit-limit"));
    }
    for n in 0..3 {
        let r = agent
            .post(JOBS, job_body(&format!("https://slow.example/{n}")))
            .await;
        assert_eq!(r.status(), 202);
        assert!(!r.headers().contains_key("ratelimit-limit"));
    }
}

#[tokio::test]
async fn tg_limits_apply_to_delegation() {
    let s = TestServer::start(|c| c.limits.tg.max_active = 1).await;
    let mut scopes = Scope::default_set();
    scopes.push(Scope::Delegate);
    let bot = s.agent(&scopes).await;
    let tg = bot.on_behalf_of("tg:42");
    assert_eq!(
        tg.post(JOBS, job_body("https://slow.example/1"))
            .await
            .status(),
        202
    );
    expect_limited(tg.post(JOBS, job_body("https://slow.example/2")).await).await;
    // Another telegram user has their own quota.
    let other = bot.on_behalf_of("tg:43");
    assert_eq!(
        other
            .post(JOBS, job_body("https://slow.example/3"))
            .await
            .status(),
        202
    );
}

#[tokio::test]
async fn quota_shared_by_cookies_of_same_ip() {
    let s = TestServer::start(|c| c.limits.web.jobs_per_hour = 1).await;
    let (a, b) = (Visitor::new(&s.base), Visitor::new(&s.base));
    assert_eq!(a.post(JOBS, job_body("https://ok")).await.status(), 202);
    expect_limited(b.post(JOBS, job_body("https://ok")).await).await;
}
