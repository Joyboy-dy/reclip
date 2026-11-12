//! Test double for yt-dlp, driven by the URL (last argument).

use std::io::Write;
use std::process::exit;
use std::time::Duration;

/// One line in the `--progress-template` format (`NA` for missing values).
fn progress(downloaded: &str, total: &str, estimate: &str, speed: &str, eta: &str) {
    println!("[reclip] {downloaded} {total} {estimate} {speed} {eta}");
    std::io::stdout().flush().unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let url = args.last().cloned().unwrap_or_default();
    let flag = |f: &str| args.iter().any(|a| a == f);
    // The media template (yt-dlp also takes `-o "thumbnail:<path>"`).
    let template = args
        .windows(2)
        .find(|w| w[0] == "-o" && !w[1].starts_with("thumbnail:"))
        .map(|w| w[1].clone());

    if url.contains("error-bot") {
        eprintln!("ERROR: [youtube] x: Sign in to confirm you're not a bot.");
        exit(1);
    }
    // Only the download is slow; probes (`-j` / `-J`) answer at once.
    if url.contains("slow") && !flag("-j") && !flag("-J") {
        for i in 0..300u32 {
            progress(
                &i.to_string(),
                "NA",
                "300000.0",
                "1000.0",
                &(300 - i).to_string(),
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        return;
    }
    if url.contains("partial") && !flag("-j") && !flag("-J") {
        partial(&template.expect("-o template"));
        return;
    }
    #[cfg(unix)]
    if url.contains("spawns-child") {
        let template = template.clone().expect("-o template");
        let dir = std::path::Path::new(&template).parent().unwrap().to_owned();
        let mut child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .unwrap();
        let tmp = dir.join("child.pid.tmp");
        std::fs::write(&tmp, child.id().to_string()).unwrap();
        std::fs::rename(tmp, dir.join("child.pid")).unwrap();
        let _ = child.wait();
        return;
    }
    if url.contains("too-large") {
        println!(
            "[download] File is larger than max-filesize (3000000000 bytes > 2147483648 bytes). Aborting."
        );
        return;
    }
    if url.contains("too-long") {
        println!(
            "[download] Fake does not pass filter (!is_live & duration <=? 10800), skipping .."
        );
        return;
    }
    if url.contains("multi-files") {
        let template = template.unwrap_or_else(|| "media.%(ext)s".into());
        for ext in ["webm", "m4a", "mp4"] {
            std::fs::write(template.replace("%(ext)s", ext), "fake media").unwrap();
        }
        return;
    }
    if args.iter().any(|a| a.starts_with("reclip-meta:")) {
        subtitle_meta(&url);
        return;
    }
    if flag("--write-subs") {
        subtitles(&args, &url, template.expect("-o template"));
        return;
    }
    if flag("-j") {
        println!("WARNING: noise");
        println!(
            "{}",
            serde_json::json!({
                "title": "Fake ✨ / video",
                "duration": 12,
                "uploader": "Fake",
                "thumbnail": "https://example.com/t.jpg",
                "formats": [
                    {"format_id": "136", "height": 720, "vcodec": "avc1", "tbr": 1000.0},
                    {"format_id": "247", "height": 720, "vcodec": "vp9", "tbr": 800.0},
                    {"format_id": "137", "height": 1080, "vcodec": "avc1", "tbr": 2500.0},
                    {"format_id": "140", "height": null, "vcodec": "none", "tbr": 128.0}
                ]
            })
        );
        return;
    }
    if flag("-J") {
        println!(
            "{}",
            serde_json::json!({
                "title": "Fake playlist",
                "entries": [
                    {"url": "https://ok/1"}, {"url": "https://ok/2"}, {"url": "https://ok/3"}
                ]
            })
        );
        return;
    }
    if let Some(template) = template {
        // Record the arguments so tests can assert on them.
        // Next to the job dir (`<id>.args.txt`), which the runner would clean.
        if let Some(dir) = std::path::Path::new(&template).parent() {
            std::fs::create_dir_all(dir).unwrap();
            if let (Some(root), Some(name)) = (dir.parent(), dir.file_name()) {
                let file = root.join(format!("{}.args.txt", name.to_string_lossy()));
                std::fs::write(file, args.join("\n")).unwrap();
            }
        }
        for done in [200, 400, 600, 800, 1000] {
            progress(&done.to_string(), "1000", "NA", "NA", "NA");
        }
        let (ext, post) = if flag("-x") {
            ("mp3", "[ExtractAudio] Destination: ")
        } else if url.contains("webm-only") {
            // A single-file source whose remux was not possible.
            ("webm", "[download] Destination: ")
        } else {
            // The container asked for with `--remux-video`, mp4 by default.
            let remux = args.windows(2).find(|w| w[0] == "--remux-video");
            let ext = remux.map_or("mp4", |w| w[1].as_str());
            (ext, "[Merger] Merging formats into ")
        };
        let path = template.replace("%(ext)s", ext);
        println!("{post}\"{path}\"");
        std::fs::write(&path, "fake media").unwrap();
        if url.contains("thumb") {
            let dir = std::path::Path::new(&path).parent().unwrap();
            std::fs::write(dir.join("thumb.jpg"), "fake jpeg").unwrap();
        }
    }
}

/// A download that leaves `media.mp4.part` and runs until stopped. Started
/// again with that `.part` present, it finishes from it (like yt-dlp's
/// default `--continue`); the media says how many bytes it resumed after.
fn partial(template: &str) {
    let media = template.replace("%(ext)s", "mp4");
    let part = format!("{media}.part");
    if let Ok(prior) = std::fs::read(&part) {
        progress("1000", "1000", "NA", "NA", "NA");
        std::fs::write(&media, format!("resumed after {} bytes", prior.len())).unwrap();
        std::fs::remove_file(&part).unwrap();
        println!("[Merger] Merging formats into \"{media}\"");
        return;
    }
    let dir = std::path::Path::new(&media).parent().unwrap();
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(&part, "partial").unwrap();
    for i in 1..300u32 {
        progress(&i.to_string(), "1000", "NA", "1000.0", "NA");
        std::thread::sleep(Duration::from_millis(100));
    }
}

const MANUAL_VTT: &str = "WEBVTT
Kind: captions

1
00:00:00.000 --> 00:00:02.000
    <c.colorE5E5E5>Bonjour</c>
";

/// Rolling auto captions: every line is repeated by the next cue.
const AUTO_VTT: &str = "WEBVTT
Kind: captions
Language: en

    00:00:00.000 --> 00:00:02.000 align:start position:0%
hello<00:00:00.500><c> world</c>

    00:00:02.000 --> 00:00:02.010 align:start position:0%
hello world

    00:00:02.010 --> 00:00:04.000 align:start position:0%
hello world
second part

    00:00:04.000 --> 00:00:04.010 align:start position:0%
second part
";

/// (video language, uploaded tracks, automatic tracks) by URL keyword.
fn tracks(
    url: &str,
) -> (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
) {
    match () {
        () if url.contains("subs-manual") => ("en", &["de", "en"], &[]),
        () if url.contains("subs-auto") => ("en", &[], &["en", "en-orig", "fr"]),
        () if url.contains("subs-both") => ("fr", &["fr"], &["en-orig"]),
        () if url.contains("subs-enus") => ("en", &["en-US"], &[]),
        () if url.contains("subs-frfr") => ("fr", &["fr-FR"], &[]),
        () if url.contains("subs-srt") => ("zh", &["zh"], &[]),
        () if url.contains("subs-fr") => ("fr", &["fr"], &[]),
        () if url.contains("subs-fail") => ("en", &["en"], &[]),
        () if url.contains("subs-lost") => ("en", &["en"], &[]),
        _ => ("en", &[], &[]),
    }
}

/// Pass 1: the `--print` line with the language and the track keys.
fn subtitle_meta(url: &str) {
    let (language, manual, auto) = tracks(url);
    let dict = |keys: &[&str]| -> serde_json::Value {
        keys.iter()
            .map(|k| (k.to_string(), serde_json::json!([])))
            .collect()
    };
    let meta = serde_json::json!({
        "language": language,
        "subtitles": dict(manual),
        "automatic_captions": dict(auto),
    });
    println!("reclip-meta:{meta}");
}

/// Pass 2 (`--write-subs`): writes `<template>.<key>.vtt` for the one track
/// named by `--sub-langs` (regex-escaped), like yt-dlp. `subs-lost` writes
/// nothing and warns, like a refused subtitle download.
fn subtitles(args: &[String], url: &str, template: String) {
    let langs = args
        .windows(2)
        .find(|w| w[0] == "--sub-langs")
        .map_or("", |w| w[1].as_str());
    let key = langs.replace('\\', "");
    if url.contains("subs-fail") {
        eprintln!("ERROR: [fake] subtitle download exploded");
        exit(1);
    }
    if url.contains("subs-lost") {
        eprintln!("WARNING: Unable to download video subtitles for '{key}': HTTP Error 429");
        return;
    }
    let vtt = if tracks(url).1.contains(&key.as_str()) {
        MANUAL_VTT
    } else {
        AUTO_VTT
    };
    if url.contains("subs-srt") {
        // A site without WebVTT whose conversion did not run: yt-dlp keeps
        // the original format.
        let srt = "1\n00:00:00,000 --> 00:00:01,000\nni hao\n\n";
        std::fs::write(format!("{template}.{key}.srt"), srt).unwrap();
        return;
    }
    std::fs::write(format!("{template}.{key}.vtt"), vtt).unwrap();
}
