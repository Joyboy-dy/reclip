//! Builders for yt-dlp command-line arguments.

use std::path::Path;

use tempfile::TempPath;

use super::download_args::{is_x_post, site_args};
use super::subs::{FILE_STEM, META_PREFIX, escape_selector};
use crate::config::Config;

/// Arguments shared by every yt-dlp call. The cookies copy lives as long as
/// this value and is deleted when it is dropped.
pub struct SessionArgs {
    pub args: Vec<String>,
    _cookies: Option<TempPath>,
}

/// `--js-runtimes` arguments: forced value wins, deno needs no flag, else bun then node.
pub fn js_runtime_args(forced: Option<&str>, which: &dyn Fn(&str) -> bool) -> Vec<String> {
    if let Some(f) = forced.map(str::trim).filter(|f| !f.is_empty()) {
        return vec!["--js-runtimes".into(), f.into()];
    }
    if which("deno") {
        return vec![];
    }
    for rt in ["bun", "node"] {
        if which(rt) {
            return vec!["--js-runtimes".into(), rt.into()];
        }
    }
    vec![]
}

/// True when a cookies file is configured and exists.
pub fn cookies_available(cfg: &Config) -> bool {
    cfg.cookies.as_deref().is_some_and(Path::is_file)
}

pub fn session_args(cfg: &Config) -> std::io::Result<SessionArgs> {
    session_args_with(cfg, &|n| which::which(n).is_ok())
}

pub fn session_args_with(
    cfg: &Config,
    which: &dyn Fn(&str) -> bool,
) -> std::io::Result<SessionArgs> {
    let mut args = js_runtime_args(cfg.js_runtime.as_deref(), which);
    if let Some(proxy) = &cfg.proxy {
        args.extend(["--proxy".into(), proxy.clone()]);
    }
    let mut cookies = None;
    if let (true, Some(src)) = (cookies_available(cfg), &cfg.cookies) {
        let tmp = tempfile::Builder::new()
            .prefix("reclip-cookies-")
            .suffix(".txt")
            .tempfile()?
            .into_temp_path();
        std::fs::copy(src, &tmp)?;
        args.extend(["--cookies".into(), tmp.to_string_lossy().into_owned()]);
        cookies = Some(tmp);
    }
    Ok(SessionArgs {
        args,
        _cookies: cookies,
    })
}

pub fn probe_args(url: &str, playlist_limit: u32) -> Vec<String> {
    if is_x_post(url) {
        // No --no-playlist: a multi-video post then prints one JSON per video.
        ["-j", "--", url].map(String::from).to_vec()
    } else if url.contains("list=") {
        ["--flat-playlist", "-J", "--playlist-end"]
            .map(String::from)
            .into_iter()
            .chain([playlist_limit.to_string(), "--".into(), url.to_string()])
            .collect()
    } else {
        ["--no-playlist", "-j", "--", url]
            .map(String::from)
            .to_vec()
    }
}

/// Pass 1 of a transcript: print the video's language and its subtitle and
/// automatic caption tracks as one JSON line. Nothing is downloaded.
pub fn meta_args(cfg: &Config, url: &str) -> Vec<String> {
    let mut a: Vec<String> = ["--skip-download", "--no-playlist", "--print"]
        .map(String::from)
        .to_vec();
    a.push(format!(
        "{META_PREFIX}%(.{{language,subtitles,automatic_captions}})j"
    ));
    a.extend(site_args(cfg, url));
    a.extend(["--".into(), url.into()]);
    a
}

/// Pass 2: write `<dir>/sub.<key>.vtt`, exactly that one track, without
/// downloading the media.
pub fn subs_args(cfg: &Config, url: &str, key: &str, dir: &Path) -> Vec<String> {
    let mut a: Vec<String> = [
        "--skip-download",
        "--no-playlist",
        "--write-subs",
        "--write-auto-subs",
        "--sub-format",
        "vtt",
        "--sub-langs",
    ]
    .map(String::from)
    .to_vec();
    a.push(escape_selector(key));
    a.extend([
        "-o".into(),
        dir.join(FILE_STEM).to_string_lossy().into_owned(),
    ]);
    a.extend(site_args(cfg, url));
    a.extend(["--".into(), url.into()]);
    a
}

pub use super::download_args::{DownloadOptions, download_args, format_name, format_selector};

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config::from_env(&|_| None)
    }
    fn no_runtime(_: &str) -> bool {
        false
    }

    #[test]
    fn deno_needs_no_flag() {
        assert!(js_runtime_args(None, &|n| n == "deno" || n == "bun").is_empty());
    }
    #[test]
    fn bun_then_node_fallback() {
        assert_eq!(
            js_runtime_args(None, &|n| n == "bun"),
            ["--js-runtimes", "bun"]
        );
        assert_eq!(
            js_runtime_args(None, &|n| n == "node"),
            ["--js-runtimes", "node"]
        );
        assert!(js_runtime_args(None, &|_| false).is_empty());
    }
    #[test]
    fn forced_runtime_wins() {
        assert_eq!(
            js_runtime_args(Some("quickjs:/opt/qjs"), &|_| true),
            ["--js-runtimes", "quickjs:/opt/qjs"]
        );
    }
    #[test]
    fn cookies_are_copied_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let orig = dir.path().join("cookies.txt");
        std::fs::write(&orig, "# Netscape\nabc").unwrap();
        let mut c = cfg();
        c.cookies = Some(orig.clone());
        assert!(cookies_available(&c));
        let s = session_args_with(&c, &no_runtime).unwrap();
        let i = s.args.iter().position(|a| a == "--cookies").unwrap();
        let copy = std::path::PathBuf::from(&s.args[i + 1]);
        assert_ne!(copy, orig);
        assert_eq!(std::fs::read_to_string(&copy).unwrap(), "# Netscape\nabc");
        drop(s);
        assert!(!copy.exists());
        assert!(orig.exists());
    }
    #[test]
    fn missing_cookies_file_is_ignored() {
        let mut c = cfg();
        c.cookies = Some("/definitely/not/here.txt".into());
        assert!(!cookies_available(&c));
        let s = session_args_with(&c, &no_runtime).unwrap();
        assert!(!s.args.contains(&"--cookies".to_string()));
    }
    #[test]
    fn proxy_is_passed() {
        let mut c = cfg();
        c.proxy = Some("socks5://127.0.0.1:7897".into());
        let s = session_args_with(&c, &no_runtime).unwrap();
        assert!(
            s.args
                .windows(2)
                .any(|w| w == ["--proxy", "socks5://127.0.0.1:7897"])
        );
    }
    #[test]
    fn single_video_probe() {
        assert_eq!(
            probe_args("https://youtube.com/watch?v=a", 50),
            ["--no-playlist", "-j", "--", "https://youtube.com/watch?v=a"]
        );
    }
    #[test]
    fn playlist_urls_are_capped() {
        assert_eq!(
            probe_args("https://youtube.com/watch?v=a&list=RDx", 50),
            [
                "--flat-playlist",
                "-J",
                "--playlist-end",
                "50",
                "--",
                "https://youtube.com/watch?v=a&list=RDx"
            ]
        );
    }
}
