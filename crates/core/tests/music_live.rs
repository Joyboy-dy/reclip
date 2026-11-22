//! Real downloads from the music sites, with the real yt-dlp and ffmpeg.
//! Ignored by default (network, minutes); run with:
//! `RECLIP_YTDLP_BIN=... cargo test -p reclip-core --test music_live -- --ignored --nocapture`

use std::path::Path;
use std::process::Command;

use reclip_core::config::Config;
use reclip_core::ytdlp::args::DownloadOptions;
use reclip_core::ytdlp::runner::{download_with, probe};
use reclip_shared::{MediaFormat, ProbeResponse};
use tokio_util::sync::CancellationToken;

fn cfg(dir: &Path) -> Config {
    let mut c = Config::from_env(&|k| std::env::var(k).ok());
    c.data_dir = dir.to_path_buf();
    c
}

/// `key=value` tags and the stream list of a file, by ffprobe.
fn tags(path: &Path) -> String {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries"])
        // Ogg files keep their tags on the stream.
        .arg("format_tags=title,artist,album,track,date:stream=codec_name,codec_type:stream_tags=title,artist,album")
        .args(["-of", "compact"])
        .arg(path)
        .output()
        .expect("ffprobe");
    String::from_utf8_lossy(&out.stdout).to_lowercase()
}

async fn fetch(url: &str, format: MediaFormat) -> (String, String) {
    let dir = tempfile::tempdir().unwrap();
    let c = cfg(dir.path());
    let job = dir.path().join("job");
    std::fs::create_dir_all(&job).unwrap();
    let path = download_with(
        &c,
        &job,
        &DownloadOptions::new(format),
        url,
        |_| {},
        CancellationToken::new(),
    )
    .await
    .unwrap_or_else(|e| panic!("{url}: {e:?}"));
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    (name, tags(&path))
}

#[tokio::test]
#[ignore = "network"]
async fn probes() {
    let dir = tempfile::tempdir().unwrap();
    let c = cfg(dir.path());
    for url in [
        "https://soundcloud.com/forss/flickermood",
        "https://soundcloud.com/forss/sets/soulhack",
        "https://blazo.bandcamp.com/album/jazz-format-mixtape-vol-1",
        "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT",
        "https://open.spotify.com/album/4aawyAB9vmqN3uQ7FjRGTy",
        "https://www.deezer.com/fr/track/3135556",
        "https://www.deezer.com/album/302127",
        "https://music.apple.com/us/album/1/1440833098?i=1440833334",
        "https://music.apple.com/us/album/1/1440833098",
    ] {
        match probe(&c, url).await {
            Ok(ProbeResponse::Video(v)) => println!(
                "{url}\n  track: {} | {:?} | {:?}s | audio_only={} via={:?}",
                v.title, v.uploader, v.duration, v.audio_only, v.matched_from
            ),
            Ok(ProbeResponse::Playlist(p)) => println!(
                "{url}\n  list: {} ({} entries) first={:?}",
                p.title,
                p.entries.len(),
                p.entries.first()
            ),
            Err(e) => panic!("{url}: {e:?}"),
        }
    }
}

#[tokio::test]
#[ignore = "network"]
async fn downloads_with_tags_and_cover() {
    for (url, format) in [
        ("https://soundcloud.com/forss/flickermood", MediaFormat::Mp3),
        (
            "https://soundcloud.com/forss/flickermood",
            MediaFormat::Original,
        ),
        ("https://blazo.bandcamp.com/track/intro", MediaFormat::Mp3),
        ("https://www.deezer.com/track/3135556", MediaFormat::Mp3),
        (
            "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT",
            MediaFormat::Opus,
        ),
        (
            "https://music.apple.com/us/album/1/1440833098?i=1440833334",
            MediaFormat::M4a,
        ),
    ] {
        let (name, tags) = fetch(url, format).await;
        println!(
            "{url} {format:?}\n  {name}\n  {}",
            tags.replace('\n', "\n  ")
        );
        assert!(tags.contains("tag:title="), "{url}: no title tag");
        assert!(tags.contains("codec_type=video"), "{url}: no cover");
    }
}
