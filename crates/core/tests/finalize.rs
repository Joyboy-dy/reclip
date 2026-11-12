//! The post-download ffmpeg pass, with fake yt-dlp and fake ffmpeg binaries.

use std::path::Path;
use std::time::Duration;

use reclip_core::config::Config;
use reclip_core::ytdlp::args::DownloadOptions;
use reclip_core::ytdlp::progress::ProgressEvent;
use reclip_core::ytdlp::runner::download_with;
use reclip_shared::{ApiError, ErrorCode, MediaFormat};
use tokio_util::sync::CancellationToken;

fn cfg(ffmpeg: &str) -> Config {
    let mut c = Config::from_env(&|_| None);
    c.ytdlp_bin = env!("CARGO_BIN_EXE_fake-ytdlp").to_string();
    c.ffmpeg_bin = ffmpeg.to_string();
    c.job_timeout = Duration::from_secs(5);
    c
}

fn fake_ffmpeg() -> Config {
    cfg(env!("CARGO_BIN_EXE_fake-ffmpeg"))
}

async fn run(
    c: &Config,
    dir: &Path,
    format: MediaFormat,
    strip: bool,
) -> Result<std::path::PathBuf, ApiError> {
    run_url(c, dir, format, strip, "https://ok").await
}

async fn run_url(
    c: &Config,
    dir: &Path,
    format: MediaFormat,
    strip: bool,
    url: &str,
) -> Result<std::path::PathBuf, ApiError> {
    let opts = DownloadOptions {
        strip_metadata: strip,
        ..DownloadOptions::new(format)
    };
    download_with(c, dir, &opts, url, |_| {}, CancellationToken::new()).await
}

fn argv_record(dir: &Path) -> Option<String> {
    let name = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|n| n.ends_with(".argv.txt"))?;
    std::fs::read_to_string(dir.join(name)).ok()
}

#[tokio::test]
async fn mp4_gets_faststart_and_strip() {
    let dir = tempfile::tempdir().unwrap();
    let p = run(&fake_ffmpeg(), dir.path(), MediaFormat::Mp4, true)
        .await
        .unwrap();
    assert_eq!(p.file_name().unwrap(), "media.mp4");
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "fake media");
    let argv = argv_record(dir.path()).expect("ffmpeg ran");
    assert!(
        argv.contains(
            "-map_metadata\n-1\n-map_metadata:s\n-1\n-map_chapters\n-1\n-movflags\n+faststart"
        ),
        "{argv}"
    );
    assert!(!dir.path().join("media.finalize.mp4").exists());
}

#[tokio::test]
async fn mkv_without_strip_skips_ffmpeg() {
    let dir = tempfile::tempdir().unwrap();
    run(&fake_ffmpeg(), dir.path(), MediaFormat::Mkv, false)
        .await
        .unwrap();
    assert!(argv_record(dir.path()).is_none());
}

#[tokio::test]
async fn faststart_failure_keeps_original() {
    let dir = tempfile::tempdir().unwrap();
    let p = run(
        &cfg("/definitely/not/ffmpeg"),
        dir.path(),
        MediaFormat::Mp4,
        false,
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read_to_string(p).unwrap(), "fake media");
}

#[tokio::test]
async fn strip_failure_fails_the_job() {
    let dir = tempfile::tempdir().unwrap();
    let e = run(
        &cfg("/definitely/not/ffmpeg"),
        dir.path(),
        MediaFormat::Mp3,
        true,
    )
    .await
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Internal);
    assert_eq!(e.message, "Could not remove metadata");
}

#[tokio::test]
async fn fallback_extension_gets_no_faststart() {
    // mov was asked for, but the source stayed webm: `-movflags` would make
    // ffmpeg fail on it, so the pass decides from the real extension.
    let dir = tempfile::tempdir().unwrap();
    let p = run_url(
        &fake_ffmpeg(),
        dir.path(),
        MediaFormat::Mov,
        true,
        "https://webm-only",
    )
    .await
    .unwrap();
    assert_eq!(p.file_name().unwrap(), "media.webm");
    let argv = argv_record(dir.path()).expect("ffmpeg ran");
    assert!(!argv.contains("-movflags"), "{argv}");
    assert!(
        argv.contains(
            "-map_metadata
-1"
        ),
        "{argv}"
    );
    assert!(
        argv.contains(
            "
-dn
"
        ),
        "{argv}"
    );
    assert!(!dir.path().join("media.finalize.webm").exists());
}

#[tokio::test]
async fn processing_is_reported_before_the_ffmpeg_pass() {
    // No yt-dlp postprocessor line for this source: only the runner can say
    // the job moved on to processing, and it must do so before ffmpeg runs.
    let dir = tempfile::tempdir().unwrap();
    let opts = DownloadOptions {
        strip_metadata: true,
        ..DownloadOptions::new(MediaFormat::Mp4)
    };
    let mut seen = Vec::new();
    let path = dir.path().to_path_buf();
    download_with(
        &fake_ffmpeg(),
        dir.path(),
        &opts,
        "https://webm-only",
        |e| {
            if e == ProgressEvent::Processing {
                seen.push(argv_record(&path).is_none());
            }
        },
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(seen, [true]);
    assert!(argv_record(dir.path()).is_some());
}
