//! Test double for yt-dlp, driven by the URL (last argument).

use std::process::exit;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let url = args.last().cloned().unwrap_or_default();
    let flag = |f: &str| args.iter().any(|a| a == f);
    let value = |f: &str| {
        args.iter()
            .position(|a| a == f)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };

    if url.contains("error-bot") {
        eprintln!("ERROR: [youtube] x: Sign in to confirm you're not a bot.");
        exit(1);
    }
    if url.contains("slow") {
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    if url.contains("too-large") {
        println!(
            "[download] File is larger than max-filesize (3000000000 bytes > 2147483648 bytes). Aborting."
        );
        return;
    }
    if url.contains("too-long") {
        println!("[download] Fake does not pass filter (duration <=? 10800), skipping ..");
        return;
    }
    if url.contains("multi-files") {
        let template = value("-o").unwrap_or_else(|| "media.%(ext)s".into());
        for ext in ["webm", "m4a", "mp4"] {
            std::fs::write(template.replace("%(ext)s", ext), "fake media").unwrap();
        }
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
    if let Some(template) = value("-o") {
        let ext = if flag("-x") { "mp3" } else { "mp4" };
        std::fs::write(template.replace("%(ext)s", ext), "fake media").unwrap();
    }
}
