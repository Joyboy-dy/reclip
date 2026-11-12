//! SSRF guard: refuse links that point at private, loopback, link-local or
//! metadata addresses before yt-dlp ever runs.
//!
//! Limits: only the submitted URL is checked. Redirects followed later by
//! yt-dlp are not covered here; network isolation of the runtime covers them
//! (spec section 4.5). DNS is resolved once for the check and again by
//! yt-dlp, so DNS rebinding is likewise left to network isolation.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use reclip_shared::{ApiError, ErrorCode};
use url::{Host, Url};

pub use super::resolver::{Resolver, static_resolver, system_resolver};
use crate::http::api_error;

/// Covers waiting for a lookup slot as well as the lookup itself.
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(5);

fn forbidden_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    a == 0
        || a == 10
        || (a == 100 && (64..128).contains(&b))
        || a == 127
        || (a == 169 && b == 254)
        || (a == 172 && (16..32).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 168)
        || (a == 198 && (b == 18 || b == 19))
        || a >= 224 // multicast, reserved, broadcast
}

fn forbidden_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4() {
        // Mapped (::ffff:a.b.c.d) and compatible (::a.b.c.d) forms.
        return forbidden_v4(v4);
    }
    let s = ip.segments();
    ip.is_unspecified()
        || ip.is_loopback()
        || s[0] & 0xfe00 == 0xfc00 // fc00::/7 unique local
        || s[0] & 0xffc0 == 0xfe80 // fe80::/10 link-local
        || s[0] & 0xff00 == 0xff00 // ff00::/8 multicast
        || (s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0; 4]) // NAT64
}

/// True for addresses the server must never fetch from.
pub fn is_forbidden(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => forbidden_v4(v4),
        IpAddr::V6(v6) => forbidden_v6(v6),
    }
}

fn is_localhost(name: &str) -> bool {
    let n = name.trim_end_matches('.').to_ascii_lowercase();
    n == "localhost" || n.ends_with(".localhost")
}

fn refused() -> ApiError {
    api_error(ErrorCode::ForbiddenTarget, "This address is not allowed")
}

fn unresolvable() -> ApiError {
    api_error(ErrorCode::InvalidRequest, "Cannot resolve this link's host")
}

/// Refuse `url` when its host is, or resolves to, a forbidden address.
pub async fn check_target(url: &str, resolver: &Resolver) -> Result<(), ApiError> {
    let parsed = Url::parse(url).map_err(|_| unresolvable())?;
    let port = parsed.port_or_known_default().unwrap_or(80);
    let check = |ip: IpAddr| {
        if is_forbidden(ip) {
            Err(refused())
        } else {
            Ok(())
        }
    };
    match parsed.host().ok_or_else(unresolvable)? {
        Host::Ipv4(ip) => check(IpAddr::V4(ip)),
        Host::Ipv6(ip) => check(IpAddr::V6(ip)),
        Host::Domain(name) => {
            if is_localhost(name) {
                return Err(refused());
            }
            if let Ok(ip) = name.parse::<IpAddr>() {
                return check(ip);
            }
            let lookup = resolver(name.to_string(), port);
            let ips = match tokio::time::timeout(RESOLVE_TIMEOUT, lookup).await {
                Ok(Ok(ips)) if !ips.is_empty() => ips,
                _ => return Err(unresolvable()),
            };
            // One bad address is enough: the client may pick any of them.
            ips.into_iter().try_for_each(check)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn forbidden_ranges() {
        let bad = [
            "0.0.0.0",
            "0.255.255.255",
            "10.0.0.1",
            "10.255.255.255",
            "100.64.0.1",
            "100.127.255.255",
            "127.0.0.1",
            "127.255.255.254",
            "169.254.169.254",
            "172.16.0.1",
            "172.31.255.255",
            "192.0.0.1",
            "192.168.1.1",
            "198.18.0.1",
            "198.19.255.255",
            "224.0.0.1",
            "239.255.255.255",
            "240.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "fc00::1",
            "fdff::1",
            "fe80::1",
            "febf::1",
            "ff02::1",
            "64:ff9b::1.2.3.4",
            "64:ff9b::808:808",
        ];
        for s in bad {
            assert!(is_forbidden(ip(s)), "{s} should be forbidden");
        }
        let good = [
            "93.184.216.34",
            "2606:4700::1",
            "100.63.255.255",
            "100.128.0.1",
            "172.15.255.255",
            "172.32.0.1",
            "192.0.1.1",
            "198.17.0.1",
            "198.20.0.1",
            "223.255.255.255",
            "1.1.1.1",
        ];
        for s in good {
            assert!(!is_forbidden(ip(s)), "{s} should be allowed");
        }
    }

    #[test]
    fn mapped_loopback_forbidden() {
        assert!(is_forbidden(ip("::ffff:127.0.0.1")));
        assert!(is_forbidden(ip("::ffff:10.0.0.1")));
        assert!(!is_forbidden(ip("::ffff:93.184.216.34")));
    }

    fn public() -> Resolver {
        static_resolver(ip("93.184.216.34"))
    }

    async fn code(url: &str, r: &Resolver) -> Option<ErrorCode> {
        check_target(url, r).await.err().map(|e| e.code)
    }

    #[tokio::test]
    async fn localhost_names() {
        let r = public();
        for u in [
            "http://localhost/",
            "http://LOCALHOST:8080/",
            "http://localhost./",
            "http://foo.localhost/",
            "http://a.b.Localhost./",
            "http://[::1]/",
            "http://127.0.0.1:9/",
            "http://169.254.169.254/latest",
            "http://[::ffff:7f00:1]/",
            "http://2130706433/",
            "http://0x7f.1/",
        ] {
            assert_eq!(code(u, &r).await, Some(ErrorCode::ForbiddenTarget), "{u}");
        }
        assert_eq!(code("https://notlocalhost.example/", &r).await, None);
    }

    #[tokio::test]
    async fn one_bad_address_is_enough() {
        let mixed: Resolver =
            Arc::new(|_, _| Box::pin(async { Ok(vec![ip("93.184.216.34"), ip("10.0.0.1")]) }));
        assert_eq!(
            code("https://example.com/", &mixed).await,
            Some(ErrorCode::ForbiddenTarget)
        );
    }

    #[tokio::test]
    async fn resolution_failures() {
        let empty: Resolver = Arc::new(|_, _| Box::pin(async { Ok(vec![]) }));
        let failing: Resolver =
            Arc::new(|_, _| Box::pin(async { Err(std::io::Error::other("nxdomain")) }));
        for r in [empty, failing] {
            let e = check_target("https://nowhere.example/", &r)
                .await
                .unwrap_err();
            assert_eq!(e.code, ErrorCode::InvalidRequest);
            assert_eq!(e.message, "Cannot resolve this link's host");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn slow_resolution_times_out() {
        let slow: Resolver = Arc::new(|_, _| {
            Box::pin(async {
                tokio::time::sleep(Duration::from_secs(60)).await;
                Ok(vec![ip("93.184.216.34")])
            })
        });
        assert_eq!(
            code("https://slow.example/", &slow).await,
            Some(ErrorCode::InvalidRequest)
        );
    }

    #[tokio::test]
    async fn default_port_follows_scheme() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let r: Resolver = Arc::new(move |_, port| {
            s2.lock().unwrap().push(port);
            Box::pin(async { Ok(vec![ip("93.184.216.34")]) })
        });
        for u in [
            "http://a.example/",
            "https://a.example/",
            "http://a.example:81/",
        ] {
            check_target(u, &r).await.unwrap();
        }
        assert_eq!(*seen.lock().unwrap(), vec![80, 443, 81]);
    }
}
