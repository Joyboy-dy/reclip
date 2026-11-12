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
        };
    };
    if !is_trusted(&peer) {
        return Forwarded {
            client_ip: peer,
            trusted_peer: false,
            https: false,
            host: host_header(),
        };
    }

    // Walk X-Forwarded-For right to left, skipping our own proxies.
    let chain: Option<Vec<IpAddr>> = header_str(headers, "x-forwarded-for").and_then(|v| {
        v.split(',')
            .map(|p| p.trim().parse::<IpAddr>().ok().map(normalize))
            .collect()
    });
    let client_ip = chain
        .and_then(|c| {
            c.iter()
                .rev()
                .find(|ip| !is_trusted(ip))
                .or_else(|| c.first())
                .copied()
        })
        .unwrap_or(peer);
    let https = header_str(headers, "x-forwarded-proto")
        .is_some_and(|v| v.split(',').next().is_some_and(|p| p.trim() == "https"));
    let host = header_str(headers, "x-forwarded-host")
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .map(String::from)
        .or_else(host_header);
    Forwarded {
        client_ip,
        trusted_peer: true,
        https,
        host,
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
        assert!(f.trusted_peer && f.https);
        assert_eq!(f.host.as_deref(), Some("r.example"));
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
    fn spoofed_forwarded_for_from_untrusted_peer_is_ignored() {
        let h = hm(&[
            ("x-forwarded-for", "1.1.1.1"),
            ("x-forwarded-proto", "https"),
            ("x-forwarded-host", "evil"),
            ("host", "real"),
        ]);
        let f = forwarded(Some(ip("198.51.100.2")), &h, &nets());
        assert_eq!(f.client_ip, ip("198.51.100.2"));
        assert!(!f.trusted_peer && !f.https);
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
