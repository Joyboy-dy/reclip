//! yt-dlp arguments of one download: format, codec, clip, metadata, site fixes.

use std::path::Path;

use reclip_shared::{Clip, MediaFormat};

use crate::config::Config;

/// One parsable line per progress update, read by `progress::parse_line`.
const PROGRESS_TEMPLATE: &str = "download:[reclip] %(progress.downloaded_bytes)s \
    %(progress.total_bytes)s %(progress.total_bytes_estimate)s %(progress.speed)s \
    %(progress.eta)s";

/// Hosts that need `--impersonate` to be served.
const IMPERSONATE_HOSTS: [&str; 2] = ["instagram.com", "tiktok.com"];
const BILIBILI_HOSTS: [&str; 2] = ["bilibili.com", "b23.tv"];
const X_HOSTS: [&str; 2] = ["x.com", "twitter.com"];

/// Lower-case host of `url`, without credentials or port.
fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or("");
    host.split(':').next().unwrap_or("").to_ascii_lowercase()
}

/// True when the host of `url` is one of `domains` or a subdomain of one.
fn on_domain(url: &str, domains: &[&str]) -> bool {
    let host = host_of(url);
    domains
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

/// True for a post on X / Twitter (which may carry several videos).
pub(super) fn is_x_post(url: &str) -> bool {
    on_domain(url, &X_HOSTS)
}

/// The `-f` selector of a download; `None` for audio (extraction).
pub fn format_selector(format: MediaFormat, format_id: Option<&str>) -> Option<String> {
    match (format.is_audio(), format_id) {
        (true, _) => None,
        (false, Some(id)) => Some(format!("{id}+bestaudio[ext=m4a]/{id}+bestaudio/{id}/best")),
        (false, None) => Some("bv*[vcodec^=avc1]+ba[ext=m4a]/bv*+ba/b".into()),
    }
}

/// What a download produces, as chosen by the visitor.
#[derive(Debug, Clone)]
pub struct DownloadOptions {
    pub format: MediaFormat,
    pub format_id: Option<String>,
    pub clip: Option<Clip>,
    pub strip_metadata: bool,
    /// 1-based item of a multi-video post.
    pub playlist_item: Option<u32>,
}

impl DownloadOptions {
    /// Options with nothing but a format.
    pub fn new(format: MediaFormat) -> DownloadOptions {
        DownloadOptions {
            format,
            format_id: None,
            clip: None,
            strip_metadata: false,
            playlist_item: None,
        }
    }
}

/// File extension / yt-dlp container or audio format name of `format`.
pub fn format_name(format: MediaFormat) -> &'static str {
    match format {
        MediaFormat::Mp4 => "mp4",
        MediaFormat::Mkv => "mkv",
        MediaFormat::Mov => "mov",
        MediaFormat::Mp3 => "mp3",
        MediaFormat::M4a => "m4a",
        MediaFormat::Opus => "opus",
        MediaFormat::Flac => "flac",
        MediaFormat::Wav => "wav",
    }
}

/// `*<start>-<end|inf>` in seconds, as `--download-sections` expects.
fn section(clip: &Clip) -> String {
    match clip.end {
        Some(end) => format!("*{}-{end}", clip.start),
        None => format!("*{}-inf", clip.start),
    }
}

/// Arguments of one download into `out_dir` (`media.<ext>`, `thumb.jpg`).
pub fn download_args(
    out_dir: &Path,
    opts: &DownloadOptions,
    cfg: &Config,
    url: &str,
) -> Vec<String> {
    let template = out_dir.join("media.%(ext)s");
    let thumb = out_dir.join("thumb.%(ext)s");
    let mut a: Vec<String> = Vec::new();
    match opts.playlist_item {
        Some(n) => a.extend(["--playlist-items".into(), n.to_string()]),
        None => a.push("--no-playlist".into()),
    }
    a.extend(
        [
            "--newline",
            "-o",
            &template.to_string_lossy(),
            "--progress-template",
            PROGRESS_TEMPLATE,
            "-N",
            &cfg.concurrent_fragments.to_string(),
            "--write-thumbnail",
            "--convert-thumbnails",
            "jpg",
            "-o",
            &format!("thumbnail:{}", thumb.to_string_lossy()),
            "--postprocessor-args",
            "ThumbnailsConvertor+ffmpeg_o:-vf scale=160:-2",
        ]
        .map(String::from),
    );
    let name = format_name(opts.format);
    // Faststart and metadata removal are not done here: yt-dlp skips its
    // remux/extract postprocessors for files already in the target format.
    // `finalize` runs them as an explicit ffmpeg pass after the download.
    // The remux still brings a single-file source (nothing to merge) to the
    // requested container when yt-dlp can.
    match format_selector(opts.format, opts.format_id.as_deref()) {
        None => a.extend(["-x", "--audio-format", name].map(String::from)),
        Some(selector) => a.extend([
            "-f".into(),
            selector,
            "--merge-output-format".into(),
            name.into(),
            "--remux-video".into(),
            name.into(),
        ]),
    }
    if let Some(clip) = &opts.clip {
        a.extend(["--download-sections".into(), section(clip)]);
    }
    if let Some(target) = cfg
        .impersonate
        .as_deref()
        .filter(|_| on_domain(url, &IMPERSONATE_HOSTS))
    {
        a.extend(["--impersonate".into(), target.into()]);
    }
    if on_domain(url, &BILIBILI_HOSTS) {
        for h in [
            "Origin:https://www.bilibili.com",
            "Referer:https://www.bilibili.com/",
        ] {
            a.extend(["--add-headers".into(), h.into()]);
        }
    }
    a.extend([
        "--max-filesize".into(),
        cfg.max_filesize.clone(),
        "--match-filter".into(),
        format!("!is_live & duration <=? {}", cfg.max_duration),
        "--".into(),
        url.into(),
    ]);
    a
}
