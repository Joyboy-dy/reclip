//! Pure parsing of yt-dlp JSON output.

use std::collections::HashMap;

use reclip_shared::{ApiError, ErrorCode, FormatOption, PlaylistInfo, VideoInfo};
use serde_json::Value;

/// First JSON object line of `-j` output; noise and later objects are ignored.
pub fn parse_first_json(stdout: &str) -> Result<Value, ApiError> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('{'))
        .find_map(|l| serde_json::from_str::<Value>(l).ok())
        .ok_or_else(|| ApiError {
            code: ErrorCode::Internal,
            message: "yt-dlp returned no data".into(),
        })
}

/// Build the quality options: best format (by `tbr`) per height, video only, tallest first.
pub fn video_info(v: &Value) -> VideoInfo {
    let text = |k: &str| v[k].as_str().map(str::to_string);
    let mut best: HashMap<u32, (&Value, f64)> = HashMap::new();
    let formats = v["formats"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    for f in formats {
        let height = f["height"].as_u64().and_then(|h| u32::try_from(h).ok());
        let has_video = f["vcodec"].as_str().is_some_and(|c| c != "none");
        let (Some(height), true) = (height.filter(|h| *h > 0), has_video) else {
            continue;
        };
        let tbr = f["tbr"].as_f64().unwrap_or(0.0);
        if best.get(&height).is_none_or(|(_, t)| tbr > *t) {
            best.insert(height, (f, tbr));
        }
    }
    let mut options: Vec<FormatOption> = best
        .into_iter()
        .filter_map(|(height, (f, _))| {
            Some(FormatOption {
                id: f["format_id"].as_str()?.to_string(),
                label: format!("{height}p"),
                height,
            })
        })
        .collect();
    options.sort_by(|a, b| b.height.cmp(&a.height));
    VideoInfo {
        title: text("title").unwrap_or_default(),
        thumbnail: text("thumbnail"),
        duration: v["duration"].as_f64(),
        uploader: text("uploader"),
        formats: options,
    }
}

/// Non-empty `entries[*].url` of a flat playlist dump.
pub fn playlist_info(v: &Value) -> PlaylistInfo {
    let entries = v["entries"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    PlaylistInfo {
        title: v["title"].as_str().unwrap_or_default().to_string(),
        entries: entries
            .iter()
            .filter_map(|e| e["url"].as_str())
            .filter(|u| !u.is_empty())
            .map(str::to_string)
            .collect(),
    }
}

/// Download file name: forbidden characters removed, at most 100 chars of title.
pub fn safe_filename(title: &str, ext: &str, fallback: &str) -> String {
    let cleaned: String = title
        .chars()
        .filter(|c| !c.is_control() && !r#"\/:*?"<>|"#.contains(*c))
        .collect();
    let cut: String = cleaned.trim().chars().take(100).collect();
    let cut = cut.trim();
    let stem = if cut.is_empty() { fallback } else { cut };
    format!("{stem}.{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn first_json_skips_noise_and_extra_objects() {
        let v = parse_first_json("WARNING: x\n\n{\"id\":\"a\"}\n{\"id\":\"b\"}\n").unwrap();
        assert_eq!(v["id"], "a");
    }

    #[test]
    fn no_json_is_internal_error() {
        let e = parse_first_json("WARNING: x\n  \n").unwrap_err();
        assert_eq!(e.code, reclip_shared::ErrorCode::Internal);
        assert_eq!(e.message, "yt-dlp returned no data");
    }

    #[test]
    fn best_format_per_height() {
        let v = json!({
            "title": "T", "thumbnail": "http://t", "duration": 12.5, "uploader": "U",
            "formats": [
                {"format_id": "a", "height": 720, "vcodec": "avc1", "tbr": 1000.0},
                {"format_id": "b", "height": 720, "vcodec": "avc1", "tbr": 2500.0},
                {"format_id": "c", "height": 1080, "vcodec": "avc1"},
                {"format_id": "d", "height": null, "vcodec": "none", "tbr": 128.0},
                {"format_id": "e", "height": 480, "vcodec": "none", "tbr": 9999.0}
            ]
        });
        let i = video_info(&v);
        assert_eq!((i.title.as_str(), i.duration), ("T", Some(12.5)));
        assert_eq!(i.uploader.as_deref(), Some("U"));
        let got: Vec<_> = i
            .formats
            .iter()
            .map(|f| (f.id.as_str(), f.label.as_str(), f.height))
            .collect();
        assert_eq!(got, [("c", "1080p", 1080), ("b", "720p", 720)]);
    }

    #[test]
    fn playlist_entries_keep_non_empty_urls() {
        let v = json!({"title": "P", "entries": [{"url": "u1"}, {"url": ""}, {"id": "x"}, {"url": "u2"}]});
        let p = playlist_info(&v);
        assert_eq!(
            (p.title.as_str(), p.entries),
            ("P", vec!["u1".to_string(), "u2".to_string()])
        );
    }

    #[test]
    fn safe_filename_strips_and_truncates() {
        assert_eq!(
            safe_filename("a/b\\c:d*e?\"f<g>h|i", "mp4", "x"),
            "abcdefghi.mp4"
        );
        assert_eq!(
            safe_filename(&"é".repeat(300), "mp3", "x").chars().count(),
            100 + 4
        );
        assert_eq!(safe_filename("  ", "mp4", "job123"), "job123.mp4");
        assert_eq!(safe_filename("🎬 clip", "mp4", "x"), "🎬 clip.mp4");
        assert_eq!(safe_filename("a\u{0}b\nc", "mp4", "x"), "abc.mp4");
    }
}
