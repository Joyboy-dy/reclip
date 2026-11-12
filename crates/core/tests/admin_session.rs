//! Admin accounts: email + password sign-in, cookie sessions, forced
//! password change, throttling and session access to the key routes.

mod common;

use common::{TestServer, Visitor};
use reclip_core::config::{Config, Secret};
use reqwest::Response;
use serde_json::{Value, json};

const EMAIL: &str = "admin@example.com";
const DEFAULT_PW: &str = "default-password-123";
const NEW_PW: &str = "a-much-better-password";
const SESSION: &str = "/api/v1/admin/session";
const ME: &str = "/api/v1/admin/me";
const PASSWORD: &str = "/api/v1/admin/password";
const KEYS: &str = "/api/v1/admin/keys";
const WRONG: &str = "Wrong email or password";

fn configure(c: &mut Config) {
    c.admin_emails = vec![EMAIL.to_string()];
    c.admin_default_password = Some(Secret::new(DEFAULT_PW));
}

async fn server() -> TestServer {
    TestServer::start(configure).await
}

async fn login(v: &Visitor, email: &str, pw: &str) -> Response {
    v.post(SESSION, json!({"email": email, "password": pw}))
        .await
}

/// A visitor signed in with the default password (must change it).
async fn first_login(s: &TestServer) -> Visitor {
    let v = Visitor::new(&s.base);
    assert_eq!(login(&v, EMAIL, DEFAULT_PW).await.status(), 200);
    v
}

async fn change(v: &Visitor, current: &str, new: &str, others: bool) -> Response {
    let body = json!({"current": current, "new": new, "sign_out_others": others});
    v.post(PASSWORD, body).await
}

async fn expect_error(r: Response, status: u16, code: &str, message: &str) {
    assert_eq!(r.status(), status);
    let b: Value = r.json().await.unwrap();
    assert_eq!(b["code"], code, "{b}");
    assert_eq!(b["message"], message, "{b}");
}

#[tokio::test]
async fn default_password_only_once() {
    let s = server().await;
    let v = Visitor::new(&s.base);
    // Unlisted email and wrong password are refused alike.
    let r = login(&v, "other@example.com", DEFAULT_PW).await;
    expect_error(r, 401, "unauthorized", WRONG).await;
    expect_error(login(&v, EMAIL, "nope").await, 401, "unauthorized", WRONG).await;
    // The email is matched in lowercase.
    let r = login(&v, "Admin@Example.com", DEFAULT_PW).await;
    assert_eq!(r.status(), 200);
    let me: Value = r.json().await.unwrap();
    assert_eq!(me, json!({"email": EMAIL, "must_change_password": true}));
    // Until it is changed, the account's password is still the default.
    let other = Visitor::new(&s.base);
    assert_eq!(login(&other, EMAIL, DEFAULT_PW).await.status(), 200);
    assert_eq!(change(&v, DEFAULT_PW, NEW_PW, false).await.status(), 200);
    // Once changed, the default password is spent for good.
    expect_error(
        login(&other, EMAIL, DEFAULT_PW).await,
        401,
        "unauthorized",
        WRONG,
    )
    .await;
    let r = login(&other, EMAIL, NEW_PW).await;
    assert_eq!(r.status(), 200);
    let me: Value = r.json().await.unwrap();
    assert_eq!(me["must_change_password"], false);
}

#[tokio::test]
async fn must_change_blocks_keys() {
    let s = server().await;
    let v = first_login(&s).await;
    let msg = "Change your password first";
    expect_error(v.get(KEYS).await, 403, "forbidden", msg).await;
    let r = v.post(KEYS, json!({"name": "x"})).await;
    expect_error(r, 403, "forbidden", msg).await;
    assert_eq!(v.get(ME).await.status(), 200);
    assert_eq!(
        change(&v, DEFAULT_PW, "short-pw", false).await.status(),
        400
    );
    assert_eq!(
        change(&v, DEFAULT_PW, DEFAULT_PW, false).await.status(),
        400
    );
    let r = change(&v, "not-current", NEW_PW, false).await;
    expect_error(r, 400, "invalid_request", "Current password is not correct").await;
    let anon = Visitor::new(&s.base);
    let r = anon.get(ME).await;
    expect_error(
        r,
        401,
        "unauthorized",
        "Your session expired. Sign in again.",
    )
    .await;
    assert_eq!(v.get(KEYS).await.status(), 403);
    assert_eq!(change(&v, DEFAULT_PW, NEW_PW, false).await.status(), 200);
    assert_eq!(v.get(KEYS).await.status(), 200);
    let me: Value = v.get(ME).await.json().await.unwrap();
    assert_eq!(me["must_change_password"], false);
}

#[tokio::test]
async fn session_cookie_flags() {
    let s = TestServer::start(|c| {
        configure(c);
        c.trusted_proxies = vec!["127.0.0.0/8".parse().unwrap()];
    })
    .await;
    let body = json!({"email": EMAIL, "password": DEFAULT_PW});
    let post = |origin: &str| {
        reqwest::Client::new()
            .post(format!("{}{SESSION}", s.base))
            .header("origin", origin)
            .json(&body)
    };
    let r = post(&s.base).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["cache-control"], "no-store");
    let c = r.headers()["set-cookie"].to_str().unwrap().to_string();
    assert!(c.starts_with("reclip_admin="), "{c}");
    for flag in ["HttpOnly", "SameSite=Strict", "Path=/", "Max-Age=43200"] {
        assert!(c.contains(flag), "{c}");
    }
    assert!(!c.contains("Secure"), "{c}");
    let token = c
        .split(';')
        .next()
        .unwrap()
        .trim_start_matches("reclip_admin=");
    assert_eq!(token.len(), 64);
    // Behind a trusted HTTPS proxy the cookie is Secure.
    let r = post("https://r.example")
        .header("x-forwarded-proto", "https")
        .header("x-forwarded-host", "r.example")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let c = r.headers()["set-cookie"].to_str().unwrap();
    assert!(c.contains("; Secure"), "{c}");
}

#[tokio::test]
async fn wrong_password_throttled() {
    let s = server().await;
    let v = Visitor::new(&s.base);
    for _ in 0..5 {
        assert_eq!(login(&v, EMAIL, "wrong").await.status(), 401);
    }
    // Even the right password is refused while the block lasts.
    let r = login(&v, EMAIL, DEFAULT_PW).await;
    assert_eq!(r.status(), 429);
    let wait: u64 = r.headers()["retry-after"]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=900).contains(&wait), "{wait}");
    let b: Value = r.json().await.unwrap();
    assert_eq!(b["code"], "rate_limited");
}

#[tokio::test]
async fn keys_with_session_need_origin() {
    let s = server().await;
    let v = first_login(&s).await;
    assert_eq!(change(&v, DEFAULT_PW, NEW_PW, false).await.status(), 200);
    assert_eq!(v.get(KEYS).await.status(), 200);
    let r = v.post(KEYS, json!({"name": "bot"})).await;
    assert_eq!(r.status(), 201);
    let created: Value = r.json().await.unwrap();
    let id = created["info"]["id"].as_str().unwrap().to_string();

    // The same cookie without a same-site Origin may read but not change.
    let cookie = format!("reclip_admin={}", session_cookie(&s).await);
    let raw = reqwest::Client::new();
    let keys = format!("{}{KEYS}", s.base);
    let r = raw
        .get(&keys)
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let r = raw
        .post(&keys)
        .header("cookie", &cookie)
        .json(&json!({"name": "x"}))
        .send()
        .await
        .unwrap();
    expect_error(r, 403, "forbidden", "Cross-site request refused").await;
    let r = raw
        .post(&keys)
        .header("cookie", &cookie)
        .header("origin", "https://evil.example")
        .json(&json!({"name": "x"}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    let r = raw
        .delete(format!("{keys}/{id}"))
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    assert_eq!(v.delete(&format!("{KEYS}/{id}")).await.status(), 204);

    // No credentials: 401. The admin token still works.
    assert_eq!(Visitor::new(&s.base).get(KEYS).await.status(), 401);
    assert_eq!(s.admin().get(KEYS).await.status(), 200);
}

/// Sign in again and return the raw session token from `Set-Cookie`.
async fn session_cookie(s: &TestServer) -> String {
    session_cookie_for(s, NEW_PW).await
}

async fn session_cookie_for(s: &TestServer, pw: &str) -> String {
    let r = reqwest::Client::new()
        .post(format!("{}{SESSION}", s.base))
        .header("origin", &s.base)
        .json(&json!({"email": EMAIL, "password": pw}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let c = r.headers()["set-cookie"].to_str().unwrap();
    c.split(';')
        .next()
        .unwrap()
        .trim_start_matches("reclip_admin=")
        .to_string()
}

#[tokio::test]
async fn logout_and_sign_out_others() {
    let s = server().await;
    // Logout works even while the password change is pending.
    let pending = first_login(&s).await;
    assert_eq!(pending.get(KEYS).await.status(), 403);
    let r = pending.delete(SESSION).await;
    assert_eq!(r.status(), 204);
    assert!(
        r.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .contains("Max-Age=0")
    );
    assert_eq!(pending.get(ME).await.status(), 401);

    // A password change can end the other sessions.
    let a = first_login(&s).await;
    assert_eq!(change(&a, DEFAULT_PW, NEW_PW, false).await.status(), 200);
    let b = Visitor::new(&s.base);
    assert_eq!(login(&b, EMAIL, NEW_PW).await.status(), 200);
    assert_eq!(b.get(ME).await.status(), 200);
    let next = "yet-another-password";
    assert_eq!(change(&a, NEW_PW, next, true).await.status(), 200);
    assert_eq!(a.get(ME).await.status(), 200);
    assert_eq!(b.get(ME).await.status(), 401);
    // Logout needs a same-site Origin.
    let cookie = format!("reclip_admin={}", session_cookie_for(&s, next).await);
    let url = format!("{}{SESSION}", s.base);
    let raw = reqwest::Client::new();
    let r = raw
        .delete(&url)
        .header("cookie", &cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    let r = raw
        .delete(&url)
        .header("cookie", &cookie)
        .header("origin", &s.base)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204);
}
