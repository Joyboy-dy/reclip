//! The two HTML pages (`index.html`, `admin.html`) served with a fresh CSP
//! nonce on every response. Other files of the web dir go through `ServeDir`.

use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use super::not_found;
use crate::app::AppState;

const PLACEHOLDER: &str = "{{CSP_NONCE}}";
const META_PLACEHOLDER: &str = "{{WEBMCP_META}}";

/// The WebMCP origin-trial tag, or nothing without a token. The value is
/// escaped for a double-quoted HTML attribute.
fn webmcp_meta(token: Option<&str>) -> String {
    let Some(token) = token else {
        return String::new();
    };
    let value = token
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!("<meta http-equiv=\"origin-trial\" content=\"{value}\">")
}

/// CSP of an HTML page: like the default one, plus the response's nonce on
/// styles. No `'unsafe-inline'`.
fn html_csp(nonce: &str) -> String {
    format!(
        "default-src 'self'; script-src 'self'; style-src 'self' 'nonce-{nonce}' \
         https://fonts.googleapis.com; font-src https://fonts.gstatic.com; \
         img-src 'self' data: https:; connect-src 'self'; media-src 'self'; \
         object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'"
    )
}

/// 16 random bytes, base64url.
fn new_nonce() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("system randomness");
    URL_SAFE_NO_PAD.encode(bytes)
}

async fn page(state: &AppState, file: &str) -> Response {
    let Ok(html) = tokio::fs::read_to_string(state.cfg.web_dir.join(file)).await else {
        return not_found("Page not found");
    };
    let nonce = new_nonce();
    let meta = webmcp_meta(state.cfg.webmcp_trial_token.as_deref());
    let html = html
        .replace(PLACEHOLDER, &nonce)
        .replace(META_PLACEHOLDER, &meta);
    let mut res = (StatusCode::OK, html).into_response();
    let h = res.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(csp) = HeaderValue::from_str(&html_csp(&nonce)) {
        h.insert(header::CONTENT_SECURITY_POLICY, csp);
    }
    res
}

pub async fn index(State(s): State<AppState>) -> Response {
    page(&s, "index.html").await
}

pub async fn admin(State(s): State<AppState>) -> Response {
    page(&s, "admin.html").await
}
