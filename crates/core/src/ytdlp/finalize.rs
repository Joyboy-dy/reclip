//! Post-download ffmpeg pass: metadata removal and web-friendly mp4/mov.
//!
//! yt-dlp skips its own remux / audio-extraction postprocessors when the
//! file is already in the target format, so `-movflags +faststart` and
//! `-map_metadata -1` would silently never run there. This pass always runs
//! when either is wanted: a stream copy into a temp file, then an atomic
//! rename over the original.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use reclip_shared::{ApiError, ErrorCode, MediaFormat};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use super::download_args::DownloadOptions;
use super::process::{spawn_group, terminate};
use crate::config::Config;

const KILL_GRACE: Duration = Duration::from_secs(5);

fn err(code: ErrorCode, message: &str) -> ApiError {
    ApiError {
        code,
        message: message.into(),
    }
}

/// ffmpeg arguments for the pass, or `None` when the file needs no rewrite
/// (no metadata removal asked and the container is not mp4/mov).
pub fn ffmpeg_args(
    input: &Path,
    output: &Path,
    format: MediaFormat,
    strip_metadata: bool,
) -> Option<Vec<OsString>> {
    let faststart = matches!(format, MediaFormat::Mp4 | MediaFormat::Mov);
    if !strip_metadata && !faststart {
        return None;
    }
    let mut a: Vec<OsString> = ["-nostdin", "-y", "-i"].map(OsString::from).to_vec();
    a.push(input.into());
    a.extend(["-map", "0", "-c", "copy"].map(OsString::from));
    if strip_metadata {
        a.extend(["-map_metadata", "-1"].map(OsString::from));
    }
    if faststart {
        a.extend(["-movflags", "+faststart"].map(OsString::from));
    }
    a.push(output.into());
    Some(a)
}

fn temp_path(path: &Path) -> PathBuf {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("bin");
    path.with_file_name(format!("media.finalize.{ext}"))
}

/// Run ffmpeg to completion; `true` when it succeeded. Kills the whole
/// process group on cancellation or timeout.
async fn run_ffmpeg(
    cfg: &Config,
    args: &[OsString],
    cancel: &CancellationToken,
) -> Result<bool, ApiError> {
    let mut cmd = Command::new(&cfg.ffmpeg_bin);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let Ok(mut child) = spawn_group(&mut cmd) else {
        return Ok(false);
    };
    tokio::select! {
        biased;
        () = cancel.cancelled() => {
            terminate(&mut child, KILL_GRACE).await;
            Err(err(ErrorCode::Cancelled, "Download cancelled"))
        }
        () = tokio::time::sleep(cfg.job_timeout) => {
            terminate(&mut child, KILL_GRACE).await;
            Ok(false)
        }
        status = child.wait() => Ok(status.is_ok_and(|s| s.success())),
    }
}

/// Rewrite `path` in place as requested by `opts`. An ffmpeg failure keeps
/// the original file, unless metadata removal was asked for: then it fails.
pub async fn finalize(
    cfg: &Config,
    path: &Path,
    opts: &DownloadOptions,
    cancel: &CancellationToken,
) -> Result<(), ApiError> {
    let tmp = temp_path(path);
    let Some(args) = ffmpeg_args(path, &tmp, opts.format, opts.strip_metadata) else {
        return Ok(());
    };
    let outcome = run_ffmpeg(cfg, &args, cancel).await;
    let renamed = matches!(outcome, Ok(true)) && std::fs::rename(&tmp, path).is_ok();
    if !renamed {
        let _ = std::fs::remove_file(&tmp);
    }
    outcome?;
    if renamed {
        return Ok(());
    }
    if opts.strip_metadata {
        return Err(err(ErrorCode::Internal, "Could not remove metadata"));
    }
    tracing::warn!(file = %path.display(), "ffmpeg faststart pass failed; keeping the original");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(format: MediaFormat, strip: bool) -> Option<Vec<String>> {
        ffmpeg_args(Path::new("in.x"), Path::new("out.x"), format, strip)
            .map(|v| v.iter().map(|s| s.to_string_lossy().into_owned()).collect())
    }
    fn expected(extra: &[&str]) -> Vec<String> {
        let mut v: Vec<&str> = vec!["-nostdin", "-y", "-i", "in.x", "-map", "0", "-c", "copy"];
        v.extend(extra);
        v.push("out.x");
        v.into_iter().map(String::from).collect()
    }

    #[test]
    fn mp4_with_strip() {
        assert_eq!(
            argv(MediaFormat::Mp4, true).unwrap(),
            expected(&["-map_metadata", "-1", "-movflags", "+faststart"])
        );
    }
    #[test]
    fn mov_without_strip() {
        assert_eq!(
            argv(MediaFormat::Mov, false).unwrap(),
            expected(&["-movflags", "+faststart"])
        );
    }
    #[test]
    fn mkv_with_strip() {
        assert_eq!(
            argv(MediaFormat::Mkv, true).unwrap(),
            expected(&["-map_metadata", "-1"])
        );
    }
    #[test]
    fn m4a_with_strip() {
        assert_eq!(
            argv(MediaFormat::M4a, true).unwrap(),
            expected(&["-map_metadata", "-1"])
        );
    }
    #[test]
    fn mkv_without_strip_needs_no_pass() {
        assert!(argv(MediaFormat::Mkv, false).is_none());
        assert!(argv(MediaFormat::Mp3, false).is_none());
    }
}
