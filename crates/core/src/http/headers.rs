//! Security headers added to every response.

use axum::Router;
use axum::http::{HeaderName, HeaderValue, header};
use tower_http::set_header::SetResponseHeaderLayer;

/// No `'unsafe-inline'`: the frontend ships no inline script or style.
/// Google Fonts is the only third-party origin; thumbnails come from
/// arbitrary https CDNs.
const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' https://fonts.googleapis.com; font-src https://fonts.gstatic.com; img-src 'self' data: https:; connect-src 'self'; media-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";
const PERMISSIONS: &str = "camera=(), microphone=(), geolocation=(), payment=(), usb=()";

fn set(name: HeaderName, value: &'static str) -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::overriding(name, HeaderValue::from_static(value))
}

/// Wrap `app` so every response (site, API, downloads, errors) carries the headers.
pub fn with_security_headers(app: Router) -> Router {
    app.layer(set(header::CONTENT_SECURITY_POLICY, CSP))
        .layer(set(header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
        .layer(set(header::REFERRER_POLICY, "no-referrer"))
        .layer(set(
            HeaderName::from_static("permissions-policy"),
            PERMISSIONS,
        ))
}
