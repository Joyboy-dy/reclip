use std::path::Path;
use std::time::{Duration, Instant};

use reclip_core::config::Config;
use reclip_core::ytdlp::progress::ProgressEvent;
use reclip_core::ytdlp::runner::{self, probe};
use reclip_shared::{ApiError, ErrorCode, MediaFormat, ProbeResponse};
use tokio_util::sync::CancellationToken;

fn cfg(bin: &str) -> Config {
    let mut c = Config::from_env(&|_| None);
    c.ytdlp_bin = bin.to_string();
    c.job_timeout = Duration::from_secs(2);
    c
}

fn fake() -> Config {
    cfg(env!("CARGO_BIN_EXE_fake-ytdlp"))
}

/// Download without progress reporting or cancellation.
async fn download(
    cfg: &Config,
    dir: &Path,
    format: MediaFormat,
    format_id: Option<&str>,
    url: &str,
) -> Result<std::path::PathBuf, ApiError> {
    runner::download(
        cfg,
        dir,
        format,
        format_id,
        url,
        |_| {},
        CancellationToken::new(),
    )
    .await
}

fn count(dir: &Path) -> usize {
    std::fs::read_dir(dir).unwrap().count()
}

#[tokio::test]
async fn probe_video() {
    let ProbeResponse::Video(v) = probe(&fake(), "https://ok").await.unwrap() else {
        panic!("expected video")
    };
    assert_eq!(v.title, "Fake ✨ / video");
    assert_eq!(
        v.formats.iter().map(|f| f.height).collect::<Vec<_>>(),
        [1080, 720]
    );
}

#[tokio::test]
async fn probe_playlist() {
    let ProbeResponse::Playlist(p) = probe(&fake(), "https://ok?list=PL1").await.unwrap() else {
        panic!("expected playlist")
    };
    assert_eq!(p.entries.len(), 3);
}

#[tokio::test]
async fn probe_bot_check() {
    let e = probe(&fake(), "https://error-bot").await.unwrap_err();
    assert_eq!(e.code, ErrorCode::BotCheck);
}

#[tokio::test]
async fn download_keeps_only_mp4() {
    let d = tempfile::tempdir().unwrap();
    let p = download(
        &fake(),
        d.path(),
        MediaFormat::Mp4,
        None,
        "https://multi-files",
    )
    .await
    .unwrap();
    assert_eq!(p.extension().unwrap(), "mp4");
    assert_eq!(count(d.path()), 1);
}

#[tokio::test]
async fn download_mp4_plain() {
    let d = tempfile::tempdir().unwrap();
    let p = download(
        &fake(),
        d.path(),
        MediaFormat::Mp4,
        Some("137"),
        "https://ok",
    )
    .await
    .unwrap();
    assert_eq!(p.extension().unwrap(), "mp4");
    assert_eq!(std::fs::read_to_string(p).unwrap(), "fake media");
}

#[tokio::test]
async fn download_mp3() {
    let d = tempfile::tempdir().unwrap();
    let p = download(&fake(), d.path(), MediaFormat::Mp3, None, "https://ok")
        .await
        .unwrap();
    assert_eq!(p.extension().unwrap(), "mp3");
}

#[tokio::test]
async fn timeout_still_reported() {
    let d = tempfile::tempdir().unwrap();
    let t = Instant::now();
    let e = download(&fake(), d.path(), MediaFormat::Mp4, None, "https://slow")
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Timeout);
    assert_eq!(e.message, "yt-dlp timed out");
    assert!(t.elapsed() < Duration::from_secs(15));
}

#[tokio::test]
async fn download_reports_progress_then_processing() {
    for (format, ext) in [(MediaFormat::Mp4, "mp4"), (MediaFormat::Mp3, "mp3")] {
        let d = tempfile::tempdir().unwrap();
        let mut events = Vec::new();
        let p = runner::download(
            &fake(),
            d.path(),
            format,
            None,
            "https://ok",
            |e| events.push(e),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(p.extension().unwrap(), ext);
        let percents: Vec<f32> = events
            .iter()
            .filter_map(|e| match e {
                ProgressEvent::Download { percent, .. } => *percent,
                ProgressEvent::Processing => None,
            })
            .collect();
        assert_eq!(percents, [20.0, 40.0, 60.0, 80.0, 100.0], "{events:?}");
        assert_eq!(events.last(), Some(&ProgressEvent::Processing));
    }
}

#[tokio::test]
async fn cancel_stops_download() {
    let d = tempfile::tempdir().unwrap();
    let mut c = fake();
    c.job_timeout = Duration::from_secs(60);
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        trigger.cancel();
    });
    let mut events = 0;
    let t = Instant::now();
    let e = runner::download(
        &c,
        d.path(),
        MediaFormat::Mp4,
        None,
        "https://slow",
        |_| events += 1,
        cancel,
    )
    .await
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Cancelled);
    assert_eq!(e.message, "Download cancelled");
    assert!(t.elapsed() < Duration::from_secs(7));
    assert!(events > 0, "slow mode reports progress before the cancel");
    assert_eq!(count(d.path()), 0);
}

/// True once `pid` no longer exists (or is a zombie waiting for init to reap it).
#[cfg(unix)]
fn gone(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    if unsafe { libc::kill(pid, 0) } != 0 {
        return true;
    }
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
        s.rsplit(')')
            .next()
            .is_some_and(|r| r.trim_start().starts_with('Z'))
    })
}

#[cfg(unix)]
#[tokio::test]
async fn cancel_kills_whole_process_group() {
    let d = tempfile::tempdir().unwrap();
    let mut c = fake();
    c.job_timeout = Duration::from_secs(60);
    let cancel = CancellationToken::new();
    let pid_file = d.path().join("child.pid");
    let watcher = async {
        for _ in 0..100 {
            if let Some(pid) = std::fs::read_to_string(&pid_file)
                .ok()
                .and_then(|s| s.trim().parse::<i32>().ok())
            {
                cancel.cancel();
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("the fake never wrote child.pid");
    };
    let run = runner::download(
        &c,
        d.path(),
        MediaFormat::Mp4,
        None,
        "https://spawns-child",
        |_| {},
        cancel.clone(),
    );
    let (res, pid) = tokio::join!(run, watcher);
    assert_eq!(res.unwrap_err().code, ErrorCode::Cancelled);
    let mut dead = false;
    for _ in 0..60 {
        if gone(pid) {
            dead = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(dead, "grandchild {pid} survived the cancellation");
}

#[tokio::test]
async fn download_error_is_classified() {
    let d = tempfile::tempdir().unwrap();
    let e = download(
        &fake(),
        d.path(),
        MediaFormat::Mp4,
        None,
        "https://error-bot",
    )
    .await
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::BotCheck);
}

#[tokio::test]
async fn download_too_large() {
    let d = tempfile::tempdir().unwrap();
    let e = download(
        &fake(),
        d.path(),
        MediaFormat::Mp4,
        None,
        "https://too-large",
    )
    .await
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::TooLarge);
    assert!(e.message.contains("larger than max-filesize"));
}

#[tokio::test]
async fn download_too_long() {
    let d = tempfile::tempdir().unwrap();
    let e = download(
        &fake(),
        d.path(),
        MediaFormat::Mp4,
        None,
        "https://too-long",
    )
    .await
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::TooLong);
}

#[tokio::test]
async fn missing_binary() {
    let c = cfg("definitely-not-yt-dlp");
    let e = probe(&c, "https://ok").await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Internal);
    assert_eq!(e.message, "yt-dlp not found");
    let d = tempfile::tempdir().unwrap();
    let e = download(&c, d.path(), MediaFormat::Mp4, None, "https://ok")
        .await
        .unwrap_err();
    assert_eq!(e.message, "yt-dlp not found");
}
