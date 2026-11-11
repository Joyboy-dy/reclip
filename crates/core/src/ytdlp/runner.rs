//! Launches yt-dlp for probes and downloads, with timeouts and cancellation.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use reclip_shared::{ApiError, ErrorCode, MediaFormat, ProbeResponse};
use tokio::process::{Child, Command};
use tokio_util::sync::CancellationToken;

use super::args::{SessionArgs, cookies_available, download_args, probe_args, session_args};
use super::errors::classify;
use super::parse::{parse_first_json, playlist_info, video_info};
use super::process::{Captured, capture, spawn_group, terminate};
use super::progress::{ProgressEvent, parse_line};
use crate::config::Config;

const PROBE_TIMEOUT: Duration = Duration::from_secs(60);
/// Time yt-dlp gets to exit after SIGTERM before the group is SIGKILLed.
const KILL_GRACE: Duration = Duration::from_secs(5);
/// stdout lines kept from a download for failure classification.
const DOWNLOAD_TAIL: usize = 200;

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
    let json = parse_first_json(&out.stdout)?;
    if url.contains("list=") {
        Ok(ProbeResponse::Playlist(playlist_info(&json)))
    } else {
        Ok(ProbeResponse::Video(video_info(&json)))
    }
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
    mut on_progress: impl FnMut(ProgressEvent) + Send,
    cancel: CancellationToken,
) -> Result<PathBuf, ApiError> {
    let template = job_dir.join("media.%(ext)s");
    let args = download_args(
        &template.to_string_lossy(),
        format,
        format_id,
        &cfg.max_filesize,
        cfg.max_duration,
        url,
    );
    let on_line = |line: &str| {
        if let Some(event) = parse_line(line) {
            on_progress(event);
        }
    };
    let out = run(cfg, &args, cfg.job_timeout, DOWNLOAD_TAIL, on_line, &cancel).await?;
    if !out.status.success() {
        return Err(failure(cfg, &out));
    }
    match pick_output(job_dir, format) {
        Some(p) => Ok(p),
        None => Err(no_file_error(&out)),
    }
}

/// Keep the `.mp4` (or `.mp3`) file, else the first file; delete the rest.
fn pick_output(job_dir: &Path, format: MediaFormat) -> Option<PathBuf> {
    let want = match format {
        MediaFormat::Mp4 => "mp4",
        MediaFormat::Mp3 => "mp3",
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(job_dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        // `args.txt` is written by the test double to record its arguments.
        .filter(|p| p.is_file() && p.file_name().is_some_and(|n| n != "args.txt"))
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
