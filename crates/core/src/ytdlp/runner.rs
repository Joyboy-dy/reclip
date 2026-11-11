//! Launches yt-dlp for probes and downloads, with timeouts.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::time::Duration;

use reclip_shared::{ApiError, ErrorCode, MediaFormat, ProbeResponse};
use tokio::process::Command;

use super::args::{cookies_available, download_args, probe_args, session_args};
use super::errors::classify;
use super::parse::{parse_first_json, playlist_info, video_info};
use crate::config::Config;

const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

fn err(code: ErrorCode, message: impl Into<String>) -> ApiError {
    ApiError {
        code,
        message: message.into(),
    }
}

/// Run yt-dlp to completion. stdout and stderr are drained concurrently by
/// `output()`; the child is killed if the timeout drops the future.
async fn run(cfg: &Config, args: &[String], timeout: Duration) -> Result<Output, ApiError> {
    let session = session_args(cfg).map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
    let mut cmd = Command::new(&cfg.ytdlp_bin);
    cmd.args(&session.args)
        .args(args)
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| err(ErrorCode::Timeout, "yt-dlp timed out"))?
        .map_err(|e| match e.kind() {
            ErrorKind::NotFound => err(ErrorCode::Internal, "yt-dlp not found"),
            _ => err(ErrorCode::Internal, e.to_string()),
        })?;
    drop(session);
    Ok(out)
}

fn failure(cfg: &Config, out: &Output) -> ApiError {
    classify(
        &String::from_utf8_lossy(&out.stderr),
        cookies_available(cfg),
    )
}

pub async fn probe(cfg: &Config, url: &str) -> Result<ProbeResponse, ApiError> {
    let args = probe_args(url, cfg.playlist_limit);
    let out = run(cfg, &args, PROBE_TIMEOUT).await?;
    if !out.status.success() {
        return Err(failure(cfg, &out));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let json = parse_first_json(&stdout)?;
    if url.contains("list=") {
        Ok(ProbeResponse::Playlist(playlist_info(&json)))
    } else {
        Ok(ProbeResponse::Video(video_info(&json)))
    }
}

pub async fn download(
    cfg: &Config,
    job_dir: &Path,
    format: MediaFormat,
    format_id: Option<&str>,
    url: &str,
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
    let out = run(cfg, &args, cfg.job_timeout).await?;
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
        .filter(|p| p.is_file())
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
fn no_file_error(out: &Output) -> ApiError {
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    for line in text.lines().map(str::trim) {
        if line.contains("larger than max-filesize") {
            return err(ErrorCode::TooLarge, line);
        }
        if line.contains("does not pass filter") {
            return err(ErrorCode::TooLong, line);
        }
    }
    err(
        ErrorCode::Internal,
        "download finished but no file was found",
    )
}
