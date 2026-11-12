mod common;

use common::TestServer;

const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data: https:; connect-src 'self'; media-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";

#[tokio::test]
async fn security_headers_on_all_routes() {
    let s = TestServer::start(|_| {}).await;
    for path in [
        "/",
        "/api/v1/config",
        "/api/v1/nope",
        "/healthz",
        "/d/not-a-token",
        "/admin/keys",
    ] {
        let r = s.get(path).await;
        let h = r.headers();
        let get = |n: &str| {
            h.get(n)
                .unwrap_or_else(|| panic!("{path}: no {n}"))
                .to_str()
                .unwrap()
        };
        if path == "/" {
            // HTML pages carry their own CSP with a per-response nonce.
            let csp = get("content-security-policy");
            assert!(csp.contains("style-src 'self' 'nonce-"), "{csp}");
            assert!(!csp.contains("unsafe-inline"));
        } else {
            assert_eq!(get("content-security-policy"), CSP, "{path}");
        }
        assert_eq!(get("x-content-type-options"), "nosniff", "{path}");
        assert_eq!(get("referrer-policy"), "no-referrer", "{path}");
        assert_eq!(
            get("permissions-policy"),
            "camera=(), microphone=(), geolocation=(), payment=(), usb=()",
            "{path}"
        );
        assert!(!CSP.contains("unsafe-inline"));
    }
}
