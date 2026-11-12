mod common;

use common::TestServer;
use serde_json::Value;

/// A server whose web dir holds both pages (and one asset).
async fn with_pages() -> (TestServer, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("index.html"),
        "<style nonce=\"{{CSP_NONCE}}\"></style>index",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("admin.html"),
        "<style nonce=\"{{CSP_NONCE}}\"></style>admin",
    )
    .unwrap();
    std::fs::write(dir.path().join("app.js"), "console.log(1)").unwrap();
    let web = dir.path().to_path_buf();
    let s = TestServer::start(move |c| c.web_dir = web).await;
    (s, dir)
}

/// The nonce of a response: from the CSP, checked against the body.
async fn nonce_of(r: reqwest::Response, page: &str) -> String {
    let csp = r.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .to_string();
    let nonce = csp
        .split("'nonce-")
        .nth(1)
        .and_then(|s| s.split('\'').next())
        .unwrap_or_else(|| panic!("no nonce in {csp}"))
        .to_string();
    assert!(csp.contains("script-src 'self';"), "{csp}");
    assert!(!csp.contains("unsafe-inline"), "{csp}");
    let body = r.text().await.unwrap();
    assert!(body.contains(page), "{body}");
    assert!(body.contains(&format!("nonce=\"{nonce}\"")), "{body}");
    assert!(!body.contains("{{CSP_NONCE}}"));
    // 16 random bytes, base64url without padding.
    assert_eq!(nonce.len(), 22, "{nonce}");
    assert!(
        nonce
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_".contains(c))
    );
    nonce
}

#[tokio::test]
async fn pages_get_a_fresh_nonce_each_response() {
    let (s, _dir) = with_pages().await;
    let a = nonce_of(s.get("/").await, "index").await;
    let b = nonce_of(s.get("/").await, "index").await;
    assert_ne!(a, b);
    // Client-side routes fall back to the main page.
    nonce_of(s.get("/some/route").await, "index").await;
    nonce_of(s.get("/admin").await, "admin").await;
    nonce_of(s.get("/admin/keys").await, "admin").await;
}

#[tokio::test]
async fn assets_keep_the_default_csp_and_api_stays_json_404() {
    let (s, _dir) = with_pages().await;
    let r = s.get("/app.js").await;
    assert_eq!(r.status(), 200);
    assert!(
        !r.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("nonce")
    );
    let r = s.get("/api/x").await;
    assert_eq!(r.status(), 404);
    let b: Value = r.json().await.unwrap();
    assert_eq!(b["code"], "invalid_request");
}

#[tokio::test]
async fn admin_page_is_404_when_missing() {
    let s = TestServer::start(|_| {}).await;
    assert_eq!(s.get("/admin").await.status(), 404);
    assert_eq!(s.get("/admin/keys").await.status(), 404);
    assert_eq!(s.get("/").await.status(), 200);
}

#[tokio::test]
async fn pending_routes_answer_501() {
    let s = TestServer::start(|_| {}).await;
    for (method, path) in [
        ("POST", "/api/v1/jobs/abc/resume"),
        ("GET", "/api/v1/history"),
        ("DELETE", "/api/v1/history/abc"),
        ("POST", "/api/v1/history/clean"),
        ("GET", "/api/v1/history/abc/thumb"),
        ("POST", "/api/v1/admin/session"),
        ("DELETE", "/api/v1/admin/session"),
        ("GET", "/api/v1/admin/me"),
        ("POST", "/api/v1/admin/password"),
    ] {
        let r = match method {
            "GET" => s.get(path).await,
            "DELETE" => s.delete(path).await,
            _ => s.post(path, serde_json::json!({})).await,
        };
        assert_eq!(r.status(), 501, "{method} {path}");
        let b: Value = r.json().await.unwrap();
        assert_eq!(b["code"], "internal", "{path}");
        assert_eq!(b["message"], "Not implemented yet", "{path}");
    }
}
