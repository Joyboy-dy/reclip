//! Real client address behind trusted reverse proxies.

use axum::http::{HeaderMap, header};
use ipnet::IpNet;
use std::net::{IpAddr, Ipv4Addr};

/// What a request looks like once proxy headers are accounted for.
#[derive(Debug, Clone, PartialEq)]
pub struct Forwarded {
    pub client_ip: IpAddr,
    /// The direct peer is a trusted proxy.
    pub trusted_peer: bool,
    pub https: bool,
    pub host: Option<String>,
    /// A trusted proxy sent `X-Forwarded-Proto` (http or https) and
    /// `X-Forwarded-Host`: `https` and `host` name the public origin.
    pub forwarded_origin: bool,
}

/// Map IPv4-mapped IPv6 addresses (`::ffff:a.b.c.d`) to plain IPv4.
fn normalize(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        v4 => v4,
    }
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// Comma-separated entries of every header line named `name`, in order.
fn list_entries<'a>(headers: &'a HeaderMap, name: &str) -> Vec<&'a str> {
    headers
        .get_all(name)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .collect()
}

pub fn forwarded(peer: Option<IpAddr>, headers: &HeaderMap, trusted: &[IpNet]) -> Forwarded {
    let is_trusted = |ip: &IpAddr| trusted.iter().any(|n| n.contains(ip));
    let host_header = || {
        header_str(headers, header::HOST.as_str())
            .map(str::trim)
            .filter(|h| !h.is_empty())
            .map(String::from)
    };
    let Some(peer) = peer.map(normalize) else {
        return Forwarded {
            client_ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            trusted_peer: false,
            https: false,
            host: host_header(),
            forwarded_origin: false,
        };
    };
    if !is_trusted(&peer) {
        return Forwarded {
            client_ip: peer,
            trusted_peer: false,
            https: false,
            host: host_header(),
            forwarded_origin: false,
        };
    }

    // Walk X-Forwarded-For right to left across all header lines, skipping our
    // own proxies. Stop at the first unreadable entry.
    let mut last_trusted = None;
    let mut found = None;
    for entry in list_entries(headers, "x-forwarded-for").iter().rev() {
        let Ok(ip) = entry.parse::<IpAddr>() else {
            break;
        };
        let ip = normalize(ip);
        if !is_trusted(&ip) {
            found = Some(ip);
            break;
        }
        last_trusted = Some(ip);
    }
    let client_ip = found.or(last_trusted).unwrap_or(peer);
    let proto = list_entries(headers, "x-forwarded-proto").last().copied();
    let https = proto.is_some_and(|p| p.eq_ignore_ascii_case("https"));
    let fwd_host = header_str(headers, "x-forwarded-host")
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .map(String::from);
    let web_proto = proto.is_some_and(|p| https || p.eq_ignore_ascii_case("http"));
    Forwarded {
        client_ip,
        trusted_peer: true,
        https,
        forwarded_origin: web_proto && fwd_host.is_some(),
        host: fwd_host.or_else(host_header),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nets() -> Vec<IpNet> {
        ["10.0.0.0/8", "127.0.0.0/8"]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect()
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn hm(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        h
    }

    #[test]
    fn trusted_peer_uses_rightmost_untrusted() {
        let h = hm(&[
            ("x-forwarded-for", "203.0.113.7, 10.0.0.3"),
            ("x-forwarded-proto", "https"),
            ("x-forwarded-host", "r.example, other"),
            ("host", "internal"),
        ]);
        let f = forwarded(Some(ip("10.0.1.9")), &h, &nets());
        assert_eq!(f.client_ip, ip("203.0.113.7"));
        assert!(f.trusted_peer && f.https && f.forwarded_origin);
        assert_eq!(f.host.as_deref(), Some("r.example"));
        // The host alone, or a proto that is not http(s), is not an origin.
        let h = hm(&[("x-forwarded-host", "r.example"), ("host", "internal")]);
        assert!(!forwarded(Some(ip("10.0.1.9")), &h, &nets()).forwarded_origin);
        let h = hm(&[
            ("x-forwarded-proto", "wss"),
            ("x-forwarded-host", "r.example"),
        ]);
        assert!(!forwarded(Some(ip("10.0.1.9")), &h, &nets()).forwarded_origin);
        let h = hm(&[
            ("x-forwarded-proto", "http"),
            ("x-forwarded-host", "r.example"),
        ]);
        assert!(forwarded(Some(ip("10.0.1.9")), &h, &nets()).forwarded_origin);
        // A forged leftmost entry does not win over the real one.
        let h = hm(&[("x-forwarded-for", "1.1.1.1, 203.0.113.7, 10.0.0.3")]);
        let f = forwarded(Some(ip("10.0.1.9")), &h, &nets());
        assert_eq!(f.client_ip, ip("203.0.113.7"));
        // All trusted: leftmost; unreadable or absent: the peer.
        let h = hm(&[("x-forwarded-for", "10.0.0.5, 10.0.0.3")]);
        let f = forwarded(Some(ip("10.0.1.9")), &h, &nets());
        assert_eq!(f.client_ip, ip("10.0.0.5"));
        let h = hm(&[("x-forwarded-for", "garbage")]);
        let f = forwarded(Some(ip("10.0.1.9")), &h, &nets());
        assert_eq!(f.client_ip, ip("10.0.1.9"));
        let f = forwarded(Some(ip("10.0.1.9")), &HeaderMap::new(), &nets());
        assert_eq!(f.client_ip, ip("10.0.1.9"));
        assert!(!f.https && f.host.is_none());
    }

    #[test]
    fn repeated_lines_and_unreadable_entries() {
        let peer = Some(ip("10.0.1.9"));
        let mut h = HeaderMap::new();
        h.append("x-forwarded-for", "1.1.1.1".parse().unwrap());
        h.append("x-forwarded-for", "203.0.113.7, 10.0.0.3".parse().unwrap());
        h.append("x-forwarded-proto", "https".parse().unwrap());
        h.append("x-forwarded-proto", "HTTP".parse().unwrap());
        let f = forwarded(peer, &h, &nets());
        assert_eq!(f.client_ip, ip("203.0.113.7"));
        assert!(!f.https);
        h.append("x-forwarded-proto", "HTTPS".parse().unwrap());
        assert!(forwarded(peer, &h, &nets()).https);
        let h = hm(&[("x-forwarded-for", "x, 203.0.113.7")]);
        assert_eq!(forwarded(peer, &h, &nets()).client_ip, ip("203.0.113.7"));
        let h = hm(&[("x-forwarded-for", "203.0.113.7, x")]);
        assert_eq!(forwarded(peer, &h, &nets()).client_ip, ip("10.0.1.9"));
    }

    #[test]
    fn spoofed_forwarded_for_from_untrusted_peer_is_ignored() {
        let h = hm(&[
            ("x-forwarded-for", "1.1.1.1"),
            ("x-forwarded-proto", "https"),
            ("x-forwarded-host", "evil"),
            ("host", "real"),
        ]);
        let f = forwarded(Some(ip("198.51.100.2")), &h, &nets());
        assert_eq!(f.client_ip, ip("198.51.100.2"));
        assert!(!f.trusted_peer && !f.https && !f.forwarded_origin);
        assert_eq!(f.host.as_deref(), Some("real"));
    }

    #[test]
    fn mapped_ipv6_normalized() {
        let f = forwarded(
            Some(ip("::ffff:10.0.1.9")),
            &hm(&[("x-forwarded-for", "::ffff:203.0.113.7")]),
            &nets(),
        );
        assert!(f.trusted_peer);
        assert_eq!(f.client_ip, ip("203.0.113.7"));
    }

    #[test]
    fn missing_peer_is_untrusted() {
        let f = forwarded(None, &hm(&[("x-forwarded-for", "1.1.1.1")]), &nets());
        assert_eq!(f.client_ip, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        assert!(!f.trusted_peer);
    }
}
