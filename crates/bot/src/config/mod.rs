//! Bot configuration, read from environment variables.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

pub const OFFICIAL_API: &str = "https://api.telegram.org";
/// Telegram documents its limits in decimal megabytes.
const OFFICIAL_MAX_UPLOAD: u64 = 50_000_000;
const LOCAL_MAX_UPLOAD: u64 = 2_000_000_000;
const DEFAULT_MAX_DELIVERIES: usize = 3;

#[derive(Clone)]
pub struct Config {
    pub token: String,
    pub api_url: String,
    pub core_url: String,
    pub key: String,
    /// The file the key was read from (`RECLIP_BOT_KEY_FILE`), if any.
    pub key_file: Option<PathBuf>,
    pub state_path: PathBuf,
    /// Largest file the bot sends itself, in bytes.
    pub max_upload: u64,
    /// Public address of the ReClip website (`RECLIP_PUBLIC_URL`), if known.
    pub public_url: Option<String>,
    /// Where files wait between the core and Telegram (`RECLIP_BOT_TMP`).
    pub tmp_dir: PathBuf,
    /// Downloads + uploads running at once (`RECLIP_BOT_MAX_DELIVERIES`).
    pub max_deliveries: usize,
}

// The token and key are secrets: never print them.
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("api_url", &self.api_url)
            .field("core_url", &self.core_url)
            .field("key_file", &self.key_file)
            .field("state_path", &self.state_path)
            .field("max_upload", &self.max_upload)
            .field("public_url", &self.public_url)
            .field("tmp_dir", &self.tmp_dir)
            .field("max_deliveries", &self.max_deliveries)
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
        let (key, key_file) = match val("RECLIP_BOT_KEY") {
            Some(k) => (k, None),
            None => {
                let path =
                    val("RECLIP_BOT_KEY_FILE").unwrap_or_else(|| "/run/reclip/bot.key".into());
                let raw = std::fs::read_to_string(&path)
                    .with_context(|| format!("cannot read the bot key file {path}"))?;
                let k = raw.trim().to_string();
                if k.is_empty() {
                    bail!("the bot key file {path} is empty");
                }
                (k, Some(PathBuf::from(path)))
            }
        };
        let state_path: PathBuf = val("RECLIP_BOT_STATE")
            .unwrap_or_else(|| "/var/lib/reclip-bot/state.json".into())
            .into();
        let max_upload = match val("RECLIP_BOT_MAX_UPLOAD") {
            Some(v) => parse_size(&v).context("invalid RECLIP_BOT_MAX_UPLOAD")?,
            None if api_url == OFFICIAL_API => OFFICIAL_MAX_UPLOAD,
            None => LOCAL_MAX_UPLOAD,
        };
        let public_url = val("RECLIP_PUBLIC_URL").and_then(|u| public_origin(&u));
        // Next to the state file by default: on the bot's volume.
        let tmp_dir = match val("RECLIP_BOT_TMP") {
            Some(d) => PathBuf::from(d),
            None => state_path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."))
                .join("tmp"),
        };
        let max_deliveries = match val("RECLIP_BOT_MAX_DELIVERIES") {
            Some(v) => v
                .parse::<usize>()
                .ok()
                .filter(|n| *n >= 1)
                .context("RECLIP_BOT_MAX_DELIVERIES must be a number of at least 1")?,
            None => DEFAULT_MAX_DELIVERIES,
        };
        Ok(Config {
            token,
            api_url,
            core_url,
            key,
            key_file,
            state_path,
            max_upload,
            public_url,
            tmp_dir,
            max_deliveries,
        })
    }
}

fn trim_url(u: &str) -> String {
    u.trim_end_matches('/').to_string()
}

/// `RECLIP_PUBLIC_URL` as the core reads it: a normalized origin
/// (`https://host[:port]`). A path, query, fragment, credentials or another
/// scheme make it ignored, with a warning, as the core does.
fn public_origin(raw: &str) -> Option<String> {
    let origin = bare_origin(raw);
    if origin.is_none() {
        tracing::warn!(
            value = raw,
            "RECLIP_PUBLIC_URL ignored: must be like https://reclip.example.com"
        );
    }
    origin
}

/// Same rules as the core's `access::origin::bare_origin`.
fn bare_origin(raw: &str) -> Option<String> {
    let u = reqwest::Url::parse(raw).ok()?;
    let bare = u.username().is_empty()
        && u.password().is_none()
        && u.path() == "/"
        && u.query().is_none()
        && u.fragment().is_none();
    if !bare || !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    let host = u.host_str()?;
    Some(match u.port() {
        Some(p) => format!("{}://{host}:{p}", u.scheme()),
        None => format!("{}://{host}", u.scheme()),
    })
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
mod tests;
