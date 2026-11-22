//! Where a request comes from: address, a short device summary, and the
//! Telegram profile the bot passes along. Only the summary of the
//! User-Agent is kept, never the header itself.

use std::net::IpAddr;

use axum::http::HeaderMap;
use axum::http::header::USER_AGENT;
use percent_encoding::percent_decode_str;

/// Headers the bot adds for the Telegram user it acts for.
pub const TG_USERNAME: &str = "x-reclip-tg-username";
/// Percent-encoded UTF-8 (first and last name).
pub const TG_NAME: &str = "x-reclip-tg-name";
const MAX_NAME: usize = 64;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Origin {
    /// The client's address; `None` for the bot acting for a Telegram user.
    pub ip: Option<IpAddr>,
    /// "Chrome 131 · Windows · desktop".
    pub device: Option<String>,
    pub tg_username: Option<String>,
    pub tg_name: Option<String>,
}

impl Origin {
    /// A browser or an API client calling directly.
    pub fn direct(ip: IpAddr, headers: &HeaderMap) -> Origin {
        let ua = headers.get(USER_AGENT).and_then(|v| v.to_str().ok());
        Origin {
            ip: Some(ip),
            device: ua.map(summary).filter(|s| !s.is_empty()),
            ..Origin::default()
        }
    }

    /// The bot acting for a Telegram user: the profile it sent, no address.
    pub fn telegram(headers: &HeaderMap) -> Origin {
        let text = |name| headers.get(name).and_then(|v| v.to_str().ok());
        let username = text(TG_USERNAME)
            .map(str::trim)
            .filter(|u| (1..=32).contains(&u.len()))
            .filter(|u| u.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
            .map(String::from);
        let name = text(TG_NAME)
            .and_then(|n| percent_decode_str(n).decode_utf8().ok())
            .map(|n| {
                n.chars()
                    .filter(|c| !c.is_control())
                    .take(MAX_NAME)
                    .collect::<String>()
                    .trim()
                    .to_string()
            })
            .filter(|n| !n.is_empty());
        Origin {
            tg_username: username,
            tg_name: name,
            ..Origin::default()
        }
    }
}

/// The version right after `marker` (`Chrome/131.0` → `131`).
fn version_after<'a>(ua: &'a str, marker: &str) -> Option<&'a str> {
    let rest = &ua[ua.find(marker)? + marker.len()..];
    let v = rest.split(['.', ' ', ';', ')']).next()?;
    (!v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())).then_some(v)
}

fn browser(ua: &str) -> Option<String> {
    // Most specific first: Edge, Opera and Samsung also say Chrome/Safari.
    let named = [
        ("Edg/", "Edge"),
        ("EdgA/", "Edge"),
        ("OPR/", "Opera"),
        ("SamsungBrowser/", "Samsung Internet"),
        ("Firefox/", "Firefox"),
        ("FxiOS/", "Firefox"),
        ("CriOS/", "Chrome"),
        ("Chrome/", "Chrome"),
    ];
    for (marker, name) in named {
        if ua.contains(marker) {
            return Some(match version_after(ua, marker) {
                Some(v) => format!("{name} {v}"),
                None => name.to_string(),
            });
        }
    }
    if ua.contains("Safari/") {
        return Some(match version_after(ua, "Version/") {
            Some(v) => format!("Safari {v}"),
            None => "Safari".into(),
        });
    }
    let tools = [
        "curl",
        "Wget",
        "python-requests",
        "Go-http-client",
        "okhttp",
    ];
    tools
        .iter()
        .find(|t| ua.starts_with(*t))
        .map(|t| t.to_string())
}

fn system(ua: &str) -> Option<&'static str> {
    let systems = [
        ("Windows", "Windows"),
        ("iPhone", "iOS"),
        ("iPad", "iPadOS"),
        ("Android", "Android"),
        ("CrOS", "ChromeOS"),
        ("Mac OS X", "macOS"),
        ("Macintosh", "macOS"),
        ("Linux", "Linux"),
    ];
    systems
        .iter()
        .find(|(marker, _)| ua.contains(marker))
        .map(|(_, name)| *name)
}

fn form(ua: &str) -> &'static str {
    if ua.contains("iPad")
        || ua.contains("Tablet")
        || (ua.contains("Android") && !ua.contains("Mobile"))
    {
        "tablet"
    } else if ua.contains("Mobi") || ua.contains("iPhone") {
        "mobile"
    } else {
        "desktop"
    }
}

/// "Chrome 131 · Windows · desktop"; an unknown agent keeps its first word.
pub fn summary(ua: &str) -> String {
    let ua = ua.trim();
    match (browser(ua), system(ua)) {
        (None, None) => ua
            .split([' ', '/'])
            .next()
            .unwrap_or_default()
            .chars()
            .take(32)
            .collect(),
        (b, s) => [b.as_deref(), s, s.map(|_| form(ua))]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn summaries() {
        for (ua, want) in [
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
                "Chrome 131 · Windows · desktop",
            ),
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0",
                "Edge 131 · Windows · desktop",
            ),
            (
                "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Mobile/15E148 Safari/604.1",
                "Safari 18 · iOS · mobile",
            ),
            (
                "Mozilla/5.0 (Linux; Android 14; SM-S918B) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/26.0 Chrome/122.0.0.0 Mobile Safari/537.36",
                "Samsung Internet 26 · Android · mobile",
            ),
            (
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 14.5; rv:132.0) Gecko/20100101 Firefox/132.0",
                "Firefox 132 · macOS · desktop",
            ),
            (
                "Mozilla/5.0 (iPad; CPU OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/131.0 Mobile/15E148 Safari/604.1",
                "Chrome 131 · iPadOS · tablet",
            ),
            ("curl/8.5.0", "curl"),
            ("reclip-bot/0.1", "reclip-bot"),
        ] {
            assert_eq!(summary(ua), want, "{ua}");
        }
    }

    #[test]
    fn telegram_profile_is_cleaned() {
        let mut h = HeaderMap::new();
        h.insert(TG_USERNAME, HeaderValue::from_static("jean_dupont"));
        h.insert(TG_NAME, HeaderValue::from_static("Jos%C3%A9%0A%20Dupont"));
        let o = Origin::telegram(&h);
        assert_eq!(o.tg_username.as_deref(), Some("jean_dupont"));
        assert_eq!(o.tg_name.as_deref(), Some("José Dupont"));
        assert_eq!(o.ip, None);
        h.insert(TG_USERNAME, HeaderValue::from_static("bad name!"));
        assert_eq!(Origin::telegram(&h).tg_username, None);
    }
}
