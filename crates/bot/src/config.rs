//! Bot configuration, read from environment variables.

use std::path::PathBuf;

use anyhow::{Context, Result, bail};

pub const OFFICIAL_API: &str = "https://api.telegram.org";
const OFFICIAL_MAX_UPLOAD: u64 = 50 * 1024 * 1024;
const LOCAL_MAX_UPLOAD: u64 = 2000 * 1024 * 1024;

#[derive(Clone)]
pub struct Config {
    pub token: String,
    pub api_url: String,
    pub core_url: String,
    pub key: String,
    pub state_path: PathBuf,
    /// Largest file the bot sends itself, in bytes.
    pub max_upload: u64,
}

// The token and key are secrets: never print them.
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("api_url", &self.api_url)
            .field("core_url", &self.core_url)
            .field("state_path", &self.state_path)
            .field("max_upload", &self.max_upload)
            .finish_non_exhaustive()
    }
}

impl Config {
    pub fn from_env(get: &dyn Fn(&str) -> Option<String>) -> Result<Config> {
        let val = |k: &str| {
            get(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let token = val("TELEGRAM_BOT_TOKEN").context("TELEGRAM_BOT_TOKEN is required")?;
        let api_url = trim_url(&val("TELEGRAM_API_URL").unwrap_or_else(|| OFFICIAL_API.into()));
        let core_url =
            trim_url(&val("RECLIP_CORE_URL").unwrap_or_else(|| "http://core:8899".into()));
        let key = match val("RECLIP_BOT_KEY") {
            Some(k) => k,
            None => {
                let path =
                    val("RECLIP_BOT_KEY_FILE").unwrap_or_else(|| "/run/reclip/bot.key".into());
                let raw = std::fs::read_to_string(&path)
                    .with_context(|| format!("cannot read the bot key file {path}"))?;
                let k = raw.trim().to_string();
                if k.is_empty() {
                    bail!("the bot key file {path} is empty");
                }
                k
            }
        };
        let state_path = val("RECLIP_BOT_STATE")
            .unwrap_or_else(|| "/var/lib/reclip-bot/state.json".into())
            .into();
        let max_upload = match val("RECLIP_BOT_MAX_UPLOAD") {
            Some(v) => parse_size(&v).context("invalid RECLIP_BOT_MAX_UPLOAD")?,
            None if api_url == OFFICIAL_API => OFFICIAL_MAX_UPLOAD,
            None => LOCAL_MAX_UPLOAD,
        };
        Ok(Config {
            token,
            api_url,
            core_url,
            key,
            state_path,
            max_upload,
        })
    }
}

fn trim_url(u: &str) -> String {
    u.trim_end_matches('/').to_string()
}

/// Parses `50M`, `2G`, `512K` or a plain byte count.
pub fn parse_size(s: &str) -> Result<u64> {
    let s = s.trim();
    let (num, mult) = match s.chars().last() {
        Some('k' | 'K') => (&s[..s.len() - 1], 1024),
        Some('m' | 'M') => (&s[..s.len() - 1], 1024 * 1024),
        Some('g' | 'G') => (&s[..s.len() - 1], 1024 * 1024 * 1024),
        _ => (s, 1),
    };
    let n: u64 = num
        .trim()
        .parse()
        .with_context(|| format!("not a size: {s}"))?;
    n.checked_mul(mult).context("size too large")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn defaults_with_key_env() {
        let c = Config::from_env(&env(&[
            ("TELEGRAM_BOT_TOKEN", "t"),
            ("RECLIP_BOT_KEY", "k"),
        ]))
        .unwrap();
        assert_eq!(c.api_url, "https://api.telegram.org");
        assert_eq!(c.core_url, "http://core:8899");
        assert_eq!(c.key, "k");
        assert_eq!(c.max_upload, 50 * 1024 * 1024);
        assert_eq!(
            c.state_path,
            PathBuf::from("/var/lib/reclip-bot/state.json")
        );
    }

    #[test]
    fn local_api_raises_limit_and_trims_slash() {
        let c = Config::from_env(&env(&[
            ("TELEGRAM_BOT_TOKEN", "t"),
            ("RECLIP_BOT_KEY", "k"),
            ("TELEGRAM_API_URL", "http://tg:8081/"),
        ]))
        .unwrap();
        assert_eq!(c.api_url, "http://tg:8081");
        assert_eq!(c.max_upload, 2000 * 1024 * 1024);
    }

    #[test]
    fn explicit_limit_and_key_priority() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("bot.key");
        std::fs::write(&f, "from-file\n").unwrap();
        let file = f.to_str().unwrap();
        let c = Config::from_env(&env(&[
            ("TELEGRAM_BOT_TOKEN", "t"),
            ("RECLIP_BOT_KEY_FILE", file),
            ("RECLIP_BOT_MAX_UPLOAD", "10M"),
        ]))
        .unwrap();
        assert_eq!(c.key, "from-file");
        assert_eq!(c.max_upload, 10 * 1024 * 1024);
        let c = Config::from_env(&env(&[
            ("TELEGRAM_BOT_TOKEN", "t"),
            ("RECLIP_BOT_KEY_FILE", file),
            ("RECLIP_BOT_KEY", "direct"),
        ]))
        .unwrap();
        assert_eq!(c.key, "direct");
    }

    #[test]
    fn missing_token_or_key_fails_and_debug_hides_secrets() {
        assert!(Config::from_env(&env(&[("RECLIP_BOT_KEY", "k")])).is_err());
        assert!(
            Config::from_env(&env(&[
                ("TELEGRAM_BOT_TOKEN", "t"),
                ("RECLIP_BOT_KEY_FILE", "/nonexistent/bot.key")
            ]))
            .is_err()
        );
        let c = Config::from_env(&env(&[
            ("TELEGRAM_BOT_TOKEN", "SECRET1"),
            ("RECLIP_BOT_KEY", "SECRET2"),
        ]))
        .unwrap();
        let dbg = format!("{c:?}");
        assert!(!dbg.contains("SECRET1") && !dbg.contains("SECRET2"));
    }

    #[test]
    fn empty_or_blank_values_behave_like_unset() {
        // docker compose passes unset variables as empty strings.
        let c = Config::from_env(&env(&[
            ("TELEGRAM_BOT_TOKEN", "t"),
            ("RECLIP_BOT_KEY", "k"),
            ("TELEGRAM_API_URL", ""),
            ("RECLIP_CORE_URL", "  "),
            ("RECLIP_BOT_MAX_UPLOAD", ""),
            ("RECLIP_BOT_STATE", ""),
            ("RECLIP_BOT_KEY_FILE", ""),
        ]))
        .unwrap();
        assert_eq!(c.api_url, OFFICIAL_API);
        assert_eq!(c.core_url, "http://core:8899");
        assert_eq!(c.max_upload, 50 * 1024 * 1024);
        assert_eq!(
            c.state_path,
            PathBuf::from("/var/lib/reclip-bot/state.json")
        );
        // Empty token and empty key (no file at the default path) are the missing errors.
        assert!(
            Config::from_env(&env(&[
                ("TELEGRAM_BOT_TOKEN", " "),
                ("RECLIP_BOT_KEY", "k")
            ]))
            .is_err()
        );
        let e = Config::from_env(&env(&[("TELEGRAM_BOT_TOKEN", "t"), ("RECLIP_BOT_KEY", "")]));
        assert!(e.is_err());
    }

    #[test]
    fn sizes() {
        assert_eq!(parse_size("512K").unwrap(), 512 * 1024);
        assert_eq!(parse_size("2G").unwrap(), 2 * 1024 * 1024 * 1024);
        assert_eq!(parse_size("100").unwrap(), 100);
        assert!(parse_size("abc").is_err());
    }
}
