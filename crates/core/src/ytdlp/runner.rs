//! Launches yt-dlp for probes and downloads, with timeouts and cancellation.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use reclip_shared::{ApiError, ErrorCode, MediaFormat, ProbeResponse, Transcript};
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;

use super::args::{
    DownloadOptions, SessionArgs, cookies_available, download_args, format_name, meta_args,
    probe_args, session_args, subs_args,
};
use super::download_args::is_x_post;
use super::errors::classify;
use super::finalize::finalize;
use super::parse::{parse_first_json, playlist_info, video_info, x_post_info};
use super::process::{Captured, capture, spawn_group, terminate};
use super::progress::{ProgressEvent, parse_line};
use super::subs::{TMP_PREFIX, available, choose, parse_tracks, read_track};
use crate::config::Config;

const PROBE_TIMEOUT: Duration = Duration::from_secs(60);
/// Time yt-dlp gets to exit after SIGTERM before the group is SIGKILLed.
const KILL_GRACE: Duration = Duration::from_secs(5);
/// stdout lines kept from a download for failure classification.
const DOWNLOAD_TAIL: usize = 200;
/// Thumbnail written next to the media; moved by the job worker.
const THUMB_NAME: &str = "thumb.jpg";
/// Temp output of the ffmpeg pass (see `finalize`).
const FINALIZE_PREFIX: &str = "media.finalize.";

fn err(code: ErrorCode, message: impl Into<String>) -> ApiError {
    ApiError {
        code,
        message: message.into(),
    }
}

fn io_err(e: std::io::Error) -> ApiError {
    match e.kind() {
        ErrorKind::NotFound => err(ErrorCode::Internal, "yt-dlp not found"),
        _ => err(ErrorCode::Internal, e.to_string()),
    }
}

/// Spawn yt-dlp in its own process group with both outputs piped. The
/// returned session args must outlive the child (they own the cookies copy).
fn spawn(cfg: &Config, args: &[String]) -> Result<(Child, SessionArgs), ApiError> {
    let session = session_args(cfg).map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
    let mut cmd = Command::new(&cfg.ytdlp_bin);
    cmd.args(&session.args)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Last resort if the future is dropped: kills the leader only.
        .kill_on_drop(true);
    let child = spawn_group(&mut cmd).map_err(io_err)?;
    Ok((child, session))
}

/// Run yt-dlp to completion, feeding each stdout line to `on_line`. Stops the
/// whole process group on timeout or cancellation.
async fn run(
    cfg: &Config,
    args: &[String],
    timeout: Duration,
    keep: usize,
    mut on_line: impl FnMut(&str) + Send,
    cancel: &CancellationToken,
) -> Result<Captured, ApiError> {
    let (mut child, _session) = spawn(cfg, args)?;
    let stopped = tokio::select! {
        biased;
        () = cancel.cancelled() => err(ErrorCode::Cancelled, "Download cancelled"),
        () = tokio::time::sleep(timeout) => err(ErrorCode::Timeout, "yt-dlp timed out"),
        out = capture(&mut child, keep, &mut on_line) => {
            if cancel.is_cancelled() {
                err(ErrorCode::Cancelled, "Download cancelled")
            } else {
                return out.map_err(io_err);
            }
        }
    };
    terminate(&mut child, KILL_GRACE).await;
    Err(stopped)
}

fn failure(cfg: &Config, out: &Captured) -> ApiError {
    classify(&out.stderr, cookies_available(cfg))
}

pub async fn probe(cfg: &Config, url: &str) -> Result<ProbeResponse, ApiError> {
    let args = probe_args(url, cfg.playlist_limit);
    let never = CancellationToken::new();
    let out = run(cfg, &args, PROBE_TIMEOUT, usize::MAX, |_| {}, &never).await?;
    if !out.status.success() {
        return Err(failure(cfg, &out));
    }
    if is_x_post(url)
        && let Some(post) = x_post_info(&out.stdout, url)
    {
        return Ok(ProbeResponse::Playlist(post));
    }
    let json = parse_first_json(&out.stdout)?;
    if url.contains("list=") {
        Ok(ProbeResponse::Playlist(playlist_info(&json)))
    } else {
        Ok(ProbeResponse::Video(video_info(&json)))
    }
}

/// Subtitles of `url` as plain text, fetched without the media into a
/// temporary directory under the data dir (always removed). `language` must
/// already be validated. Pass 1 reads the video's language and tracks and
/// picks one in Rust; pass 2 fetches exactly that track.
pub async fn transcript(
    cfg: &Config,
    url: &str,
    language: Option<&str>,
) -> Result<Transcript, ApiError> {
    let never = CancellationToken::new();
    let meta = run(
        cfg,
        &meta_args(cfg, url),
        PROBE_TIMEOUT,
        usize::MAX,
        |_| {},
        &never,
    )
    .await?;
    if !meta.status.success() {
        return Err(failure(cfg, &meta));
    }
    let none = || err(ErrorCode::Unavailable, "No subtitles available");
    let tracks = parse_tracks(&meta.stdout);
    let Some(track) = choose(&tracks, language) else {
        return Err(match (language, available(&tracks)) {
            (Some(lang), keys) if !keys.is_empty() => err(
                ErrorCode::Unavailable,
                format!("No subtitles in {lang}. Available: {}", keys.join(", ")),
            ),
            _ => none(),
        });
    };
    let root = cfg.data_dir.join("tmp");
    let dir = tokio::task::spawn_blocking(move || {
        std::fs::create_dir_all(&root)?;
        tempfile::Builder::new()
            .prefix(TMP_PREFIX)
            .tempdir_in(&root)
    })
    .await
    .map_err(|e| io_err(std::io::Error::other(e)))?
    .map_err(io_err)?;
    let args = subs_args(cfg, url, &track.key, dir.path());
    let out = run(cfg, &args, PROBE_TIMEOUT, usize::MAX, |_| {}, &never).await?;
    if !out.status.success() {
        return Err(failure(cfg, &out));
    }
    let (path, key) = (dir.path().to_path_buf(), track.key.clone());
    let read = tokio::task::spawn_blocking(move || read_track(&path, &key))
        .await
        .map_err(|e| io_err(std::io::Error::other(e)))?;
    let Some(text) = read else {
        // E.g. an HTTP 429 on the subtitle file: yt-dlp warns and exits 0.
        tracing::warn!(stderr = %out.stderr.trim(), "subtitle track listed but not written");
        return Err(err(
            ErrorCode::Unavailable,
            "Subtitles could not be downloaded, try again later",
        ));
    };
    if text.is_empty() {
        return Err(none());
    }
    Ok(Transcript {
        language: track.key,
        automatic: track.automatic,
        text,
    })
}

/// Download `url` into `job_dir`, reporting progress through `on_progress`.
/// Returns `cancelled` as soon as `cancel` fires (never `Ok` afterwards);
/// partial files are left for the caller to remove.
pub async fn download(
    cfg: &Config,
    job_dir: &Path,
    format: MediaFormat,
    format_id: Option<&str>,
    url: &str,
    on_progress: impl FnMut(ProgressEvent) + Send,
    cancel: CancellationToken,
) -> Result<PathBuf, ApiError> {
    let opts = DownloadOptions {
        format_id: format_id.map(str::to_string),
        ..DownloadOptions::new(format)
    };
    download_with(cfg, job_dir, &opts, url, on_progress, cancel).await
}

/// [`download`] with the full set of options.
pub async fn download_with(
    cfg: &Config,
    job_dir: &Path,
    opts: &DownloadOptions,
    url: &str,
    mut on_progress: impl FnMut(ProgressEvent) + Send,
    cancel: CancellationToken,
) -> Result<PathBuf, ApiError> {
    let args = download_args(job_dir, opts, cfg, url);
    let format = opts.format;
    let on_line = |line: &str| {
        if let Some(event) = parse_line(line) {
            on_progress(event);
        }
    };
    let out = run(cfg, &args, cfg.job_timeout, DOWNLOAD_TAIL, on_line, &cancel).await?;
    if !out.status.success() {
        return Err(failure(cfg, &out));
    }
    let Some(path) = pick_output(job_dir, format) else {
        return Err(no_file_error(&out));
    };
    // The bytes are all in: show processing while the ffmpeg pass runs, even
    // when yt-dlp printed no postprocessor line (single-file source).
    on_progress(ProgressEvent::Processing);
    finalize(cfg, &path, opts, &cancel).await?;
    Ok(path)
}

/// The media is a file that is neither the `thumb.jpg` thumbnail (kept for
/// the job), a leftover `*.part` nor a `media.finalize.*` temp file left by
/// an interrupted ffmpeg pass.
fn is_media(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    path.is_file()
        && name != THUMB_NAME
        && !name.ends_with(".part")
        && !name.starts_with(FINALIZE_PREFIX)
}

/// Keep the `media.<ext>` file (preferring the requested extension), else the
/// first media file; delete the other media files.
fn pick_output(job_dir: &Path, format: MediaFormat) -> Option<PathBuf> {
    let want = format_name(format);
    let mut files: Vec<PathBuf> = std::fs::read_dir(job_dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| is_media(p))
        .collect();
    files.sort();
    let keep = files
        .iter()
        .position(|p| p.extension().is_some_and(|e| e == want))
        .or(if files.is_empty() { None } else { Some(0) })?;
    let keep = files.remove(keep);
    for f in files {
        let _ = std::fs::remove_file(f);
    }
    Some(keep)
}

/// yt-dlp reports size/duration rejections on stdout with exit code 0.
fn no_file_error(out: &Captured) -> ApiError {
    let text = format!("{}\n{}", out.stdout, out.stderr);
    for line in text.lines().map(str::trim) {
        if line.contains("larger than max-filesize") {
            return err(ErrorCode::TooLarge, line);
        }
        if line.contains("does not pass filter") {
            return err(ErrorCode::TooLong, "Video is too long or is a live stream");
        }
    }
    err(
        ErrorCode::Internal,
        "download finished but no file was found",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leftover_finalize_temp_never_wins() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "media.finalize.mp4",
            "media.mp4",
            "thumb.jpg",
            "media.webm.part",
        ] {
            std::fs::write(dir.path().join(name), "x").unwrap();
        }
        let picked = pick_output(dir.path(), MediaFormat::Mp4).unwrap();
        assert_eq!(picked.file_name().unwrap(), "media.mp4");
        let only_temp = tempfile::tempdir().unwrap();
        std::fs::write(only_temp.path().join("media.finalize.mkv"), "x").unwrap();
        assert!(pick_output(only_temp.path(), MediaFormat::Mkv).is_none());
    }
}
