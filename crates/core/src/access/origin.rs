//! Cross-site request check for cookie-authenticated (visitor) requests.

use axum::http::HeaderMap;
use axum::http::header::ORIGIN;
use url::Url;

use super::ip::Forwarded;

/// `host[:port]` of a URL, the port only when not the scheme's default.
fn authority(u: &Url) -> Option<String> {
    let host = u.host_str()?;
    Some(match u.port() {
        Some(p) => format!("{host}:{p}"),
        None => host.to_string(),
    })
}

/// Whether the `Origin` header names this site: its authority equals the
/// request's host or the public URL's. Missing, `null` or unparseable
/// origins are refused.
pub fn same_origin(headers: &HeaderMap, fwd: &Forwarded, public_url: Option<&str>) -> bool {
    let Some(origin) = headers.get(ORIGIN).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let Ok(origin) = Url::parse(origin.trim()) else {
        return false;
    };
    if !matches!(origin.scheme(), "http" | "https") {
        return false;
    }
    let Some(want) = authority(&origin) else {
        return false;
    };
    // Parse the host with the Origin's scheme so default ports compare equal.
    let host = fwd
        .host
        .as_deref()
        .filter(|h| !h.contains(['@', '/', '\\', '?', '#']))
        .and_then(|h| Url::parse(&format!("{}://{h}", origin.scheme())).ok())
        .and_then(|u| authority(&u));
    let public = public_url
        .and_then(|u| Url::parse(u).ok())
        .and_then(|u| authority(&u));
    host.as_deref() == Some(want.as_str()) || public.as_deref() == Some(want.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use std::net::{IpAddr, Ipv4Addr};

    fn fwd(host: Option<&str>) -> Forwarded {
        Forwarded {
            client_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            trusted_peer: false,
            https: false,
            host: host.map(String::from),
        }
    }

    fn origin(v: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(ORIGIN, HeaderValue::from_str(v).unwrap());
        h
    }

    #[test]
    fn origin_matches_host_or_public_url() {
        let f = fwd(Some("R.example:80"));
        assert!(same_origin(&origin("http://r.example"), &f, None));
        assert!(!same_origin(&origin("https://r.example"), &f, None));
        assert!(!same_origin(&origin("null"), &f, None));
        assert!(!same_origin(&HeaderMap::new(), &f, None));
        let public = Some("https://pub.example");
        let none = fwd(None);
        assert!(same_origin(&origin("https://pub.example"), &none, public));
        assert!(!same_origin(&origin("https://evil.example"), &none, public));
        let tricky = fwd(Some("evil.example@r.example"));
        assert!(!same_origin(&origin("http://r.example"), &tricky, None));
    }
}
