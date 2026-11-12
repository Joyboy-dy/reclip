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

/// `--write-subs`: writes `<template>.<lang>.vtt` files like yt-dlp and prints
/// the `subtitles` field. `subs-manual` has uploaded subtitles in the
/// requested language (English by default), `subs-auto` only automatic
/// captions, `subs-both` an automatic English and a manual French track; any
/// other URL has none.
fn subtitles(args: &[String], url: &str, template: String) {
    let langs = args
        .windows(2)
        .find(|w| w[0] == "--sub-langs")
        .map_or("", |w| w[1].as_str());
    // The default request is a regex list; a named language is used as is.
    let named = (!langs.contains('*')).then_some(langs);
    let write = |lang: &str, vtt: &str| {
        std::fs::write(format!("{template}.{lang}.vtt"), vtt).unwrap();
    };
    let mut manual = serde_json::Map::new();
    if url.contains("subs-manual") {
        let lang = named.unwrap_or("en");
        write(lang, MANUAL_VTT);
        manual.insert(lang.into(), serde_json::json!([]));
    } else if url.contains("subs-auto") {
        write("en-orig", AUTO_VTT);
    } else if url.contains("subs-both") {
        write("en-orig", AUTO_VTT);
        write("fr", MANUAL_VTT);
        manual.insert("fr".into(), serde_json::json!([]));
    }
    println!("reclip-subs:{}", serde_json::Value::Object(manual));
}
