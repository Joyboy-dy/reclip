//! Admin sign-in hardening: forced session revocation, atomic throttling,
//! throttle keys, and sessions of removed admins.

mod common;

use common::{TestServer, Visitor};
use reclip_core::access::keys::sha256_hex;
use reclip_core::config::{Config, Secret};
use reclip_core::store::now_plus;
use serde_json::json;
use std::time::Duration;

const EMAIL: &str = "admin@example.com";
const DEFAULT_PW: &str = "default-password-123";
const NEW_PW: &str = "a-much-better-password";
const SESSION: &str = "/api/v1/admin/session";
const ME: &str = "/api/v1/admin/me";
const KEYS: &str = "/api/v1/admin/keys";

fn configure(c: &mut Config) {
    c.admin_emails = vec![EMAIL.to_string(), "second@example.com".to_string()];
    c.admin_default_password = Some(Secret::new(DEFAULT_PW));
    c.trusted_proxies = vec!["127.0.0.0/8".parse().unwrap()];
}

/// One login attempt from `ip` (through the trusted proxy) with `email`.
async fn attempt(s: &TestServer, ip: &str, email: &str, pw: &str) -> reqwest::Response {
    reqwest::Client::new()
        .post(format!("{}{SESSION}", s.base))
        .header("origin", &s.base)
        .header("x-forwarded-for", ip)
        .json(&json!({"email": email, "password": pw}))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn pending_change_ends_other_sessions() {
    let s = TestServer::start(configure).await;
    let (a, b) = (Visitor::new(&s.base), Visitor::new(&s.base));
    let body = json!({"email": EMAIL, "password": DEFAULT_PW});
    assert_eq!(a.post(SESSION, body.clone()).await.status(), 200);
    assert_eq!(b.post(SESSION, body).await.status(), 200);
    // Even with sign_out_others false: the account still had the default.
    let change = json!({"current": DEFAULT_PW, "new": NEW_PW, "sign_out_others": false});
    assert_eq!(a.post("/api/v1/admin/password", change).await.status(), 200);
    assert_eq!(a.get(ME).await.status(), 200);
    assert_eq!(b.get(ME).await.status(), 401);
}

#[tokio::test]
async fn parallel_bad_logins_stay_within_the_limit() {
    let s = TestServer::start(configure).await;
    let calls = (0..10).map(|_| attempt(&s, "203.0.113.9", EMAIL, "wrong"));
    let statuses: Vec<u16> = futures_util::future::join_all(calls)
        .await
        .iter()
        .map(|r| r.status().as_u16())
        .collect();
    let refused = statuses.iter().filter(|&&c| c == 401).count();
    let limited = statuses.iter().filter(|&&c| c == 429).count();
    assert_eq!((refused, limited), (5, 5), "{statuses:?}");
}

#[tokio::test]
async fn email_key_throttles_across_addresses() {
    let s = TestServer::start(configure).await;
    for n in 1..=5 {
        let r = attempt(&s, &format!("203.0.113.{n}"), EMAIL, "wrong").await;
        assert_eq!(r.status(), 401);
    }
    // A new address, same email: refused before any check.
    let r = attempt(&s, "203.0.113.100", EMAIL, DEFAULT_PW).await;
    assert_eq!(r.status(), 429);
    assert!(r.headers().contains_key("retry-after"));
    // Another email from that address is unaffected.
    let r = attempt(&s, "203.0.113.100", "second@example.com", DEFAULT_PW).await;
    assert_eq!(r.status(), 200);
}

#[tokio::test]
async fn ip_key_throttles_across_emails() {
    let s = TestServer::start(configure).await;
    for n in 0..5 {
        let r = attempt(&s, "203.0.113.7", &format!("nobody{n}@example.com"), "x").await;
        assert_eq!(r.status(), 401);
    }
    let r = attempt(&s, "203.0.113.7", EMAIL, DEFAULT_PW).await;
    assert_eq!(r.status(), 429);
    let r = attempt(&s, "203.0.113.8", EMAIL, DEFAULT_PW).await;
    assert_eq!(r.status(), 200);
}

#[tokio::test]
async fn successful_logins_do_not_use_up_the_limit() {
    let s = TestServer::start(configure).await;
    for _ in 0..8 {
        assert_eq!(
            attempt(&s, "203.0.113.7", EMAIL, DEFAULT_PW).await.status(),
            200
        );
    }
}

#[tokio::test]
async fn session_dies_when_email_is_removed_from_admin_emails() {
    let s = TestServer::start(configure).await;
    let gone = "gone@example.com";
    let token = "ab".repeat(32);
    s.store
        .init_admin_password(gone, "$argon2id$x")
        .await
        .unwrap();
    s.store
        .set_admin_password(gone, "$argon2id$x", None)
        .await
        .unwrap();
    let expires = now_plus(Duration::from_secs(3600));
    s.store
        .insert_admin_session(&sha256_hex(&token), gone, &expires)
        .await
        .unwrap();
    let cookie = format!("reclip_admin={token}");
    let raw = reqwest::Client::new();
    for path in [ME, KEYS] {
        let r = raw
            .get(format!("{}{path}", s.base))
            .header("cookie", &cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 401, "{path}");
    }
}
