//! Runtime configuration read from environment variables.

use ipnet::IpNet;
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub data_dir: PathBuf,
    pub downloads_dir: PathBuf,
    pub web_dir: PathBuf,
    pub ytdlp_bin: String,
    pub cookies: Option<PathBuf>,
    pub proxy: Option<String>,
    pub js_runtime: Option<String>,
    pub job_timeout: Duration,
    pub playlist_limit: u32,
    pub max_filesize: String,
    pub max_duration: u64,
    pub telegram_bot_username: Option<String>,
    /// Downloads running at once, all sites together (at least 1).
    pub max_concurrent: usize,
    /// Downloads running at once for one site (at least 1).
    pub max_per_site: usize,
    /// How long a finished file stays downloadable.
    pub file_ttl: Duration,
    /// Period of the expiry sweep.
    pub cleanup_interval: Duration,
    /// Peers allowed to set `X-Forwarded-*` headers.
    pub trusted_proxies: Vec<IpNet>,
    /// Public base URL, without trailing slash.
    pub public_url: Option<String>,
    pub admin_token: Option<String>,
    /// Whether the web UI is open without an API key.
    pub public_web: bool,
    /// How long a signed download link stays valid.
    pub link_ttl: Duration,
    pub limits: TierLimits,
}

/// Rate limits for one access tier. 0 means unlimited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub probes_per_minute: u32,
    pub jobs_per_hour: u32,
    pub max_active: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TierLimits {
    pub web: Limits,
    pub key: Limits,
    pub tg: Limits,
}

const DEFAULT_PROXIES: &str =
    "127.0.0.0/8,::1/128,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16,fc00::/7";

/// Parse a comma-separated list of CIDRs or bare addresses; bad entries are skipped.
fn parse_proxies(list: &str) -> Vec<IpNet> {
    list.split(',')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .filter_map(|e| {
            let net = e
                .parse::<IpNet>()
                .ok()
                .or_else(|| e.parse::<IpAddr>().ok().map(IpNet::from));
            if net.is_none() {
                tracing::warn!(entry = e, "ignoring invalid RECLIP_TRUSTED_PROXIES entry");
            }
            net
        })
        .collect()
}

impl Config {
    /// Build the config from an environment lookup. Empty values count as unset.
    pub fn from_env(env: &dyn Fn(&str) -> Option<String>) -> Config {
        let get = |k: &str| {
            env(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let num = |k: &str| get(k).and_then(|v| v.parse::<u64>().ok());
        let slots = |k: &str, default: usize| {
            get(k)
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(default)
                .max(1)
        };

        let limit = |tier: &str, d: (u32, u32, u32)| {
            let n = |what: &str, default: u32| {
                get(&format!("RECLIP_LIMIT_{tier}_{what}"))
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(default)
            };
            Limits {
                probes_per_minute: n("PROBES_PER_MIN", d.0),
                jobs_per_hour: n("JOBS_PER_HOUR", d.1),
                max_active: n("ACTIVE", d.2),
            }
        };
        let public_web = get("RECLIP_PUBLIC_WEB")
            .map(|v| !matches!(v.to_ascii_lowercase().as_str(), "false" | "0" | "no"))
            .unwrap_or(true);

        let data_dir = get("RECLIP_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("./data"));
        let downloads_dir = get("RECLIP_DOWNLOADS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| data_dir.join("downloads"));

        Config {
            host: get("RECLIP_HOST").unwrap_or_else(|| "0.0.0.0".into()),
            port: get("RECLIP_PORT")
                .or_else(|| get("PORT"))
                .and_then(|v| v.parse().ok())
                .unwrap_or(8899),
            downloads_dir,
            data_dir,
            web_dir: get("RECLIP_WEB_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("./web/dist")),
            ytdlp_bin: get("RECLIP_YTDLP_BIN").unwrap_or_else(|| "yt-dlp".into()),
            cookies: get("RECLIP_COOKIES").map(PathBuf::from),
            proxy: get("RECLIP_PROXY"),
            js_runtime: get("RECLIP_JS_RUNTIME"),
            job_timeout: Duration::from_secs(num("RECLIP_JOB_TIMEOUT").unwrap_or(1800)),
            playlist_limit: num("RECLIP_PLAYLIST_LIMIT")
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(50),
            max_filesize: get("RECLIP_MAX_FILESIZE").unwrap_or_else(|| "2G".into()),
            max_duration: num("RECLIP_MAX_DURATION").unwrap_or(10800),
            telegram_bot_username: get("RECLIP_TELEGRAM_BOT_USERNAME"),
            max_concurrent: slots("RECLIP_MAX_CONCURRENT", 4),
            max_per_site: slots("RECLIP_MAX_PER_SITE", 2),
            file_ttl: Duration::from_secs(num("RECLIP_FILE_TTL").unwrap_or(86400)),
            cleanup_interval: Duration::from_secs(num("RECLIP_CLEANUP_INTERVAL").unwrap_or(600)),
            trusted_proxies: parse_proxies(
                &get("RECLIP_TRUSTED_PROXIES").unwrap_or_else(|| DEFAULT_PROXIES.into()),
            ),
            public_url: get("RECLIP_PUBLIC_URL")
                .map(|u| u.trim_end_matches('/').to_string())
                .filter(|u| !u.is_empty()),
            admin_token: get("RECLIP_ADMIN_TOKEN"),
            public_web,
            link_ttl: Duration::from_secs(num("RECLIP_LINK_TTL").unwrap_or(21600)),
            limits: TierLimits {
                web: limit("WEB", (120, 100, 80)),
                key: limit("KEY", (120, 200, 100)),
                tg: limit("TG", (30, 40, 10)),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_and_overrides() {
        let c = Config::from_env(&|_| None);
        assert_eq!(
            (c.port, c.playlist_limit, c.job_timeout.as_secs()),
            (8899, 50, 1800)
        );
        assert_eq!((c.max_filesize.as_str(), c.max_duration), ("2G", 10800));
        assert_eq!(c.downloads_dir, PathBuf::from("./data").join("downloads"));
        let c = Config::from_env(&|k| (k == "RECLIP_PORT").then(|| "9000".into()));
        assert_eq!(c.port, 9000);
        let c = Config::from_env(&|k| (k == "PORT").then(|| "9100".into()));
        assert_eq!(c.port, 9100);
        let c = Config::from_env(&|k| (k == "RECLIP_PROXY").then(|| "".into()));
        assert_eq!(c.proxy, None);
    }

    #[test]
    fn queue_and_lifecycle_settings() {
        let c = Config::from_env(&|_| None);
        assert_eq!((c.max_concurrent, c.max_per_site), (4, 2));
        assert_eq!(c.file_ttl, Duration::from_secs(86400));
        assert_eq!(c.cleanup_interval, Duration::from_secs(600));
        let env = |k: &str| match k {
            "RECLIP_MAX_CONCURRENT" => Some("8".into()),
            "RECLIP_MAX_PER_SITE" => Some("3".into()),
            "RECLIP_FILE_TTL" => Some("60".into()),
            "RECLIP_CLEANUP_INTERVAL" => Some("5".into()),
            _ => None,
        };
        let c = Config::from_env(&env);
        assert_eq!((c.max_concurrent, c.max_per_site), (8, 3));
        assert_eq!(c.file_ttl, Duration::from_secs(60));
        assert_eq!(c.cleanup_interval, Duration::from_secs(5));
        // Zero would stall the queue forever: at least one slot each.
        let c = Config::from_env(&|k| k.starts_with("RECLIP_MAX_").then(|| "0".into()));
        assert_eq!((c.max_concurrent, c.max_per_site), (1, 1));
    }

    #[test]
    fn downloads_dir_follows_data_dir() {
        let c = Config::from_env(&|k| (k == "RECLIP_DATA_DIR").then(|| "/srv/r".into()));
        assert_eq!(c.downloads_dir, PathBuf::from("/srv/r").join("downloads"));
    }

    #[test]
    fn access_defaults() {
        let c = Config::from_env(&|_| None);
        assert_eq!(c.trusted_proxies.len(), 6);
        assert!(c.public_web && c.public_url.is_none() && c.admin_token.is_none());
        assert_eq!(c.link_ttl, Duration::from_secs(21600));
        let l = |a, b, c| Limits {
            probes_per_minute: a,
            jobs_per_hour: b,
            max_active: c,
        };
        assert_eq!(c.limits.web, l(120, 100, 80));
        assert_eq!(c.limits.key, l(120, 200, 100));
        assert_eq!(c.limits.tg, l(30, 40, 10));
    }

    #[test]
    fn limits_override_and_zero() {
        let env = |k: &str| match k {
            "RECLIP_LIMIT_TG_ACTIVE" => Some("0".into()),
            "RECLIP_LIMIT_WEB_PROBES_PER_MIN" => Some("7".into()),
            "RECLIP_LIMIT_KEY_JOBS_PER_HOUR" => Some("abc".into()),
            "RECLIP_PUBLIC_URL" => Some("https://r.example/".into()),
            _ => None,
        };
        let c = Config::from_env(&env);
        assert_eq!(c.limits.tg.max_active, 0);
        assert_eq!(c.limits.web.probes_per_minute, 7);
        assert_eq!(c.limits.key.jobs_per_hour, 200);
        assert_eq!(c.public_url.as_deref(), Some("https://r.example"));
        let c = Config::from_env(&|k| (k == "RECLIP_PUBLIC_URL").then(|| "//".into()));
        assert_eq!(c.public_url, None);
    }

    #[test]
    fn bad_proxy_entry_skipped() {
        let c = Config::from_env(&|k| {
            (k == "RECLIP_TRUSTED_PROXIES").then(|| "10.0.0.0/8, nope, 203.0.113.5, ::1".into())
        });
        let want: Vec<IpNet> = ["10.0.0.0/8", "203.0.113.5/32", "::1/128"]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(c.trusted_proxies, want);
    }

    #[test]
    fn public_web_false_values() {
        for v in ["false", "0", "no", "FALSE"] {
            let c = Config::from_env(&|k| (k == "RECLIP_PUBLIC_WEB").then(|| v.into()));
            assert!(!c.public_web, "{v}");
        }
        let c = Config::from_env(&|k| (k == "RECLIP_PUBLIC_WEB").then(|| "true".into()));
        assert!(c.public_web);
    }
}
