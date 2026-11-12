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
    nonce_of(s.get("/admin/").await, "admin").await;
    nonce_of(s.get("/index.html").await, "index").await;
    nonce_of(s.get("/admin.html").await, "admin").await;
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

/// A server with both pages carrying the WebMCP placeholder.
async fn with_meta_pages(token: Option<&str>) -> (TestServer, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for f in ["index.html", "admin.html"] {
        std::fs::write(dir.path().join(f), "<head>{{WEBMCP_META}}</head>").unwrap();
    }
    let web = dir.path().to_path_buf();
    let token = token.map(str::to_string);
    let s = TestServer::start(move |c| {
        c.web_dir = web;
        c.webmcp_trial_token = token;
    })
    .await;
    (s, dir)
}

#[tokio::test]
async fn webmcp_meta_only_with_a_token_and_escaped() {
    let (s, _d) = with_meta_pages(None).await;
    for path in ["/", "/admin"] {
        assert_eq!(s.get(path).await.text().await.unwrap(), "<head></head>");
    }
    let (s, _d) = with_meta_pages(Some("tok=en")).await;
    for path in ["/", "/admin"] {
        let body = s.get(path).await.text().await.unwrap();
        assert_eq!(
            body,
            "<head><meta http-equiv=\"origin-trial\" content=\"tok=en\"></head>"
        );
    }
    let (s, _d) = with_meta_pages(Some("a\"><script>&")).await;
    let body = s.get("/").await.text().await.unwrap();
    assert!(
        body.contains("content=\"a&quot;&gt;&lt;script&gt;&amp;\""),
        "{body}"
    );
    assert!(!body.contains("<script>"), "{body}");
    // The token is inserted after the nonce: it is never rewritten.
    let (s, _d) = with_meta_pages(Some("{{CSP_NONCE}}")).await;
    let body = s.get("/").await.text().await.unwrap();
    assert!(body.contains("content=\"{{CSP_NONCE}}\""), "{body}");
}
