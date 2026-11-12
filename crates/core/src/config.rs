//! Runtime configuration read from environment variables.

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
    fn downloads_dir_follows_data_dir() {
        let c = Config::from_env(&|k| (k == "RECLIP_DATA_DIR").then(|| "/srv/r".into()));
        assert_eq!(c.downloads_dir, PathBuf::from("/srv/r").join("downloads"));
    }
}
