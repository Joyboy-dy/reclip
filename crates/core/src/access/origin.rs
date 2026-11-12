//! Cross-site request check for cookie-authenticated (visitor) requests.

use axum::http::HeaderMap;
use axum::http::header::ORIGIN;
use url::Url;

use super::ip::Forwarded;

/// `scheme://host[:port]` of an http(s) URL, the port only when not the
/// scheme's default.
fn origin_of(u: &Url) -> Option<String> {
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    let host = u.host_str()?;
    Some(match u.port() {
        Some(p) => format!("{}://{host}:{p}", u.scheme()),
        None => format!("{}://{host}", u.scheme()),
    })
}

/// The normalized origin of `raw` when it is a bare http(s) origin: no
/// credentials, path, query or fragment. `None` otherwise.
pub fn bare_origin(raw: &str) -> Option<String> {
    let u = Url::parse(raw).ok()?;
    let bare = u.username().is_empty()
        && u.password().is_none()
        && u.path() == "/"
        && u.query().is_none()
        && u.fragment().is_none();
    if bare { origin_of(&u) } else { None }
}

/// Whether the `Origin` header names this site: same scheme and authority
/// as the request (`https` only when a trusted proxy says so) or as the
/// public URL. Missing, `null` or unparseable origins are refused.
pub fn same_origin(headers: &HeaderMap, fwd: &Forwarded, public_url: Option<&str>) -> bool {
    let Some(origin) = headers
        .get(ORIGIN)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| bare_origin(v.trim()))
    else {
        return false;
    };
    let scheme = if fwd.https { "https" } else { "http" };
    let host = fwd
        .host
        .as_deref()
        .and_then(|h| bare_origin(&format!("{scheme}://{h}")));
    let public = public_url
        .and_then(|u| Url::parse(u).ok())
        .and_then(|u| origin_of(&u));
    host.as_deref() == Some(origin.as_str()) || public.as_deref() == Some(origin.as_str())
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
            forwarded_origin: false,
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
        for bad in ["r.example/x", "r.example?x", "r.example#", r"r.example\x"] {
            assert!(!same_origin(
                &origin("http://r.example"),
                &fwd(Some(bad)),
                None
            ));
        }
    }

    #[test]
    fn origin_scheme_must_match() {
        let mut f = fwd(Some("r.example"));
        f.https = true;
        assert!(same_origin(&origin("https://r.example"), &f, None));
        assert!(!same_origin(&origin("http://r.example"), &f, None));
        let public = Some("https://pub.example/app");
        let none = fwd(None);
        assert!(same_origin(&origin("https://pub.example"), &none, public));
        assert!(!same_origin(&origin("http://pub.example"), &none, public));
        let plain = Some("http://pub.example:8080");
        assert!(same_origin(
            &origin("http://pub.example:8080"),
            &none,
            plain
        ));
        assert!(!same_origin(
            &origin("https://pub.example:8080"),
            &none,
            plain
        ));
    }

    #[test]
    fn bare_origin_rejects_extras() {
        assert_eq!(
            bare_origin("HTTPS://R.example:443").as_deref(),
            Some("https://r.example")
        );
        assert_eq!(
            bare_origin("http://[::1]:8080").as_deref(),
            Some("http://[::1]:8080")
        );
        for bad in [
            "https://a@r.example",
            "https://r.example/x",
            "ftp://r.example",
            "null",
        ] {
            assert_eq!(bare_origin(bad), None, "{bad}");
        }
    }
}
