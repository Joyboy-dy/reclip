use std::path::Path;

use reclip_shared::{Clip, MediaFormat};

use super::args::{DownloadOptions, download_args, probe_args};
use crate::config::Config;

fn cfg() -> Config {
    Config::from_env(&|_| None)
}

fn run(opts: &DownloadOptions, url: &str) -> Vec<String> {
    download_args(Path::new("/d"), opts, &cfg(), url)
}

fn has(a: &[String], pair: [&str; 2]) -> bool {
    a.windows(2).any(|w| w == pair)
}

fn mp4() -> DownloadOptions {
    DownloadOptions::new(MediaFormat::Mp4)
}

#[test]
fn video_with_id_selector() {
    let o = DownloadOptions {
        format_id: Some("137".into()),
        ..mp4()
    };
    let a = run(&o, "u");
    assert!(has(
        &a,
        ["-f", "137+bestaudio[ext=m4a]/137+bestaudio/137/best"]
    ));
    assert!(has(&a, ["--merge-output-format", "mp4"]));
}

#[test]
fn video_default_prefers_avc() {
    let a = run(&mp4(), "u");
    assert!(has(&a, ["-f", "bv*[vcodec^=avc1]+ba[ext=m4a]/bv*+ba/b"]));
}

#[test]
fn containers() {
    for (f, n) in [(MediaFormat::Mkv, "mkv"), (MediaFormat::Mov, "mov")] {
        assert!(has(
            &run(&DownloadOptions::new(f), "u"),
            ["--merge-output-format", n]
        ));
    }
}

#[test]
fn audio_formats_extract() {
    for (f, n) in [
        (MediaFormat::Mp3, "mp3"),
        (MediaFormat::M4a, "m4a"),
        (MediaFormat::Opus, "opus"),
        (MediaFormat::Flac, "flac"),
        (MediaFormat::Wav, "wav"),
    ] {
        let a = run(&DownloadOptions::new(f), "u");
        assert!(a.contains(&"-x".into()) && has(&a, ["--audio-format", n]));
        assert!(!a.contains(&"-f".into()));
    }
}

#[test]
fn clip_sections() {
    let c = |start, end| DownloadOptions {
        clip: Some(Clip { start, end }),
        ..mp4()
    };
    assert!(has(
        &run(&c(10.0, Some(25.5)), "u"),
        ["--download-sections", "*10-25.5"]
    ));
    assert!(has(
        &run(&c(90.0, None), "u"),
        ["--download-sections", "*90-inf"]
    ));
    assert!(!run(&mp4(), "u").contains(&"--download-sections".into()));
}

#[test]
fn video_is_remuxed_to_the_container_and_ffmpeg_args_are_left_to_finalize() {
    // A single-file source (e.g. webm) is not merged, so only the remux
    // brings it to the requested container. Faststart and metadata removal
    // are not passed here: yt-dlp skips its postprocessors for files
    // already in the target format, `finalize` runs them instead.
    for (f, remux) in [
        (MediaFormat::Mp4, Some("mp4")),
        (MediaFormat::Mov, Some("mov")),
        (MediaFormat::Mkv, Some("mkv")),
        (MediaFormat::Mp3, None),
    ] {
        let o = DownloadOptions {
            strip_metadata: true,
            ..DownloadOptions::new(f)
        };
        let a = run(&o, "u");
        match remux {
            Some(n) => assert!(has(&a, ["--remux-video", n]), "{f:?}"),
            None => assert!(!a.contains(&"--remux-video".into()), "{f:?}"),
        }
        let ppa: Vec<_> = a.iter().filter(|x| x.contains("ffmpeg_o:")).collect();
        assert_eq!(
            ppa,
            ["ThumbnailsConvertor+ffmpeg_o:-vf scale=160:-2"],
            "{f:?}"
        );
    }
}

#[test]
fn concurrent_fragments() {
    assert!(has(&run(&mp4(), "u"), ["-N", "4"]));
}

#[test]
fn impersonate_only_on_instagram_and_tiktok() {
    for u in [
        "https://www.instagram.com/reel/x",
        "https://vm.tiktok.com/abc",
        "https://tiktok.com/@a/video/1",
    ] {
        assert!(has(&run(&mp4(), u), ["--impersonate", "chrome"]), "{u}");
    }
    for u in [
        "https://youtube.com/watch?v=a",
        "https://notinstagram.com/x",
        "https://evil.example/?u=instagram.com",
    ] {
        assert!(!run(&mp4(), u).contains(&"--impersonate".into()), "{u}");
    }
    let mut c = cfg();
    c.impersonate = None;
    let a = download_args(Path::new("/d"), &mp4(), &c, "https://tiktok.com/a");
    assert!(!a.contains(&"--impersonate".into()));
}

#[test]
fn bilibili_headers() {
    for u in ["https://www.bilibili.com/video/BV1", "https://b23.tv/abc"] {
        let a = run(&mp4(), u);
        assert!(
            has(&a, ["--add-headers", "Origin:https://www.bilibili.com"]),
            "{u}"
        );
        assert!(
            has(&a, ["--add-headers", "Referer:https://www.bilibili.com/"]),
            "{u}"
        );
    }
    assert!(!run(&mp4(), "https://youtube.com/watch?v=a").contains(&"--add-headers".into()));
}

#[test]
fn playlist_item_replaces_no_playlist() {
    let o = DownloadOptions {
        playlist_item: Some(2),
        ..mp4()
    };
    let a = run(&o, "https://x.com/u/status/1");
    assert!(has(&a, ["--playlist-items", "2"]));
    assert!(!a.contains(&"--no-playlist".into()));
    assert!(run(&mp4(), "u").contains(&"--no-playlist".into()));
}

#[test]
fn thumbnail_args() {
    let a = run(&mp4(), "u");
    assert!(a.contains(&"--write-thumbnail".into()));
    assert!(has(&a, ["--convert-thumbnails", "jpg"]));
    assert!(
        a.iter()
            .any(|x| x.starts_with("thumbnail:/d") && x.ends_with("thumb.%(ext)s"))
    );
    assert!(has(
        &a,
        [
            "--postprocessor-args",
            "ThumbnailsConvertor+ffmpeg_o:-vf scale=160:-2"
        ]
    ));
}

#[test]
fn x_probe_lists_every_video() {
    assert_eq!(
        probe_args("https://x.com/u/status/1", 50),
        ["-j", "--", "https://x.com/u/status/1"]
    );
}
