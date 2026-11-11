//! Builders for yt-dlp command-line arguments.

use std::path::Path;

use reclip_shared::MediaFormat;
use tempfile::TempPath;

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
    if url.contains("list=") {
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

/// One parsable line per progress update, read by `progress::parse_line`.
const PROGRESS_TEMPLATE: &str = "download:[reclip] %(progress.downloaded_bytes)s \
    %(progress.total_bytes)s %(progress.total_bytes_estimate)s %(progress.speed)s \
    %(progress.eta)s";

pub fn download_args(
    out_template: &str,
    format: MediaFormat,
    format_id: Option<&str>,
    max_filesize: &str,
    max_duration: u64,
    url: &str,
) -> Vec<String> {
    let mut a: Vec<String> = [
        "--no-playlist",
        "--newline",
        "-o",
        out_template,
        "--progress-template",
        PROGRESS_TEMPLATE,
    ]
    .map(String::from)
    .to_vec();
    match (format, format_id) {
        (MediaFormat::Mp3, _) => a.extend(["-x", "--audio-format", "mp3"].map(String::from)),
        (MediaFormat::Mp4, Some(id)) => a.extend([
            "-f".into(),
            format!("{id}+bestaudio/best"),
            "--merge-output-format".into(),
            "mp4".into(),
        ]),
        (MediaFormat::Mp4, None) => a.extend(
            [
                "-f",
                "bestvideo+bestaudio/best",
                "--merge-output-format",
                "mp4",
            ]
            .map(String::from),
        ),
    }
    a.extend([
        "--max-filesize".into(),
        max_filesize.into(),
        "--match-filter".into(),
        format!("!is_live & duration <=? {max_duration}"),
        "--".into(),
        url.into(),
    ]);
    a
}

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
    #[test]
    fn mp3_download_args() {
        let a = download_args("/d/x.%(ext)s", MediaFormat::Mp3, None, "2G", 10800, "u");
        assert!(a.windows(2).any(|w| w == ["--audio-format", "mp3"]) && a.contains(&"-x".into()));
        assert_eq!(a[..4], ["--no-playlist", "--newline", "-o", "/d/x.%(ext)s"]);
        assert_eq!(a.last().unwrap(), "u");
    }
    #[test]
    fn mp4_with_format_id() {
        let a = download_args(
            "/d/x.%(ext)s",
            MediaFormat::Mp4,
            Some("137"),
            "2G",
            10800,
            "u",
        );
        assert!(a.windows(2).any(|w| w == ["-f", "137+bestaudio/best"]));
        assert!(a.windows(2).any(|w| w == ["--merge-output-format", "mp4"]));
        assert!(a.windows(2).any(|w| w == ["--max-filesize", "2G"]));
        assert!(
            a.windows(2)
                .any(|w| w == ["--match-filter", "!is_live & duration <=? 10800"])
        );
        let n = a.len();
        assert_eq!(a[n - 2], "--");
        assert_eq!(a[n - 1], "u");
    }
    #[test]
    fn download_args_ask_for_parsable_progress() {
        let a = download_args("o", MediaFormat::Mp4, None, "2G", 1, "u");
        assert!(a.windows(2).any(|w| w
            == [
                "--progress-template",
                "download:[reclip] %(progress.downloaded_bytes)s %(progress.total_bytes)s \
                 %(progress.total_bytes_estimate)s %(progress.speed)s %(progress.eta)s"
            ]));
    }
    #[test]
    fn download_args_reject_live_streams() {
        let a = download_args("o", MediaFormat::Mp4, None, "2G", 10800, "u");
        let i = a.iter().position(|x| x == "--match-filter").unwrap();
        assert!(a[i + 1].contains("!is_live"));
    }
    #[test]
    fn mp4_without_format_id() {
        let a = download_args("o", MediaFormat::Mp4, None, "2G", 1, "u");
        assert!(
            a.windows(2)
                .any(|w| w == ["-f", "bestvideo+bestaudio/best"])
        );
    }
}
