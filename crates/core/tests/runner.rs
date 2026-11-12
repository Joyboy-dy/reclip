use std::path::Path;
use std::time::{Duration, Instant};

use reclip_core::config::Config;
use reclip_core::ytdlp::runner::{download, probe};
use reclip_shared::{ErrorCode, MediaFormat, ProbeResponse};

fn cfg(bin: &str) -> Config {
    let mut c = Config::from_env(&|_| None);
    c.ytdlp_bin = bin.to_string();
    c.job_timeout = Duration::from_secs(2);
    c
}

fn fake() -> Config {
    cfg(env!("CARGO_BIN_EXE_fake-ytdlp"))
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
async fn download_times_out() {
    let d = tempfile::tempdir().unwrap();
    let t = Instant::now();
    let e = download(&fake(), d.path(), MediaFormat::Mp4, None, "https://slow")
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Timeout);
    assert!(t.elapsed() < Duration::from_secs(15));
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
