//! Pure parsing of yt-dlp JSON output.

use std::collections::HashMap;

use reclip_shared::{ApiError, ErrorCode, FormatOption, PlaylistEntry, PlaylistInfo, VideoInfo};
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

/// Codec family of a yt-dlp `vcodec`: lower-case prefix before the first dot.
fn codec_family(vcodec: Option<&str>) -> Option<String> {
    Some(vcodec?.split('.').next()?.to_ascii_lowercase())
}

fn is_h264(vcodec: Option<&str>) -> bool {
    codec_family(vcodec).is_some_and(|c| matches!(c.as_str(), "avc1" | "avc3" | "h264"))
}

/// Short codec name of a yt-dlp `vcodec` (`None` for H.264 or unknown).
fn codec_label(vcodec: Option<&str>) -> Option<String> {
    let prefix = codec_family(vcodec)?;
    match prefix.as_str() {
        "" | "none" => None,
        _ if is_h264(vcodec) => None,
        "vp9" | "vp09" => Some("VP9".into()),
        "av01" | "av1" => Some("AV1".into()),
        "hev1" | "hvc1" | "hevc" | "h265" => Some("HEVC".into()),
        "vp8" | "vp08" => Some("VP8".into()),
        other => Some(other.to_ascii_uppercase()),
    }
}

/// Rank within one height: H.264 first, then the highest bitrate.
fn rank(f: &Value) -> (bool, f64) {
    (
        is_h264(f["vcodec"].as_str()),
        f["tbr"].as_f64().unwrap_or(0.0),
    )
}

/// Build the quality options: one format per height (H.264 preferred, else the
/// best bitrate, labelled with its codec), video only, tallest first. A format
/// with a height but no `vcodec` at all counts as video (Bilibili).
pub fn video_info(v: &Value) -> VideoInfo {
    let text = |k: &str| v[k].as_str().map(str::to_string);
    let mut best: HashMap<u32, &Value> = HashMap::new();
    let formats = v["formats"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    for f in formats {
        let height = f["height"].as_u64().and_then(|h| u32::try_from(h).ok());
        let has_video = f["vcodec"].as_str().is_none_or(|c| c != "none");
        let (Some(height), true) = (height.filter(|h| *h > 0), has_video) else {
            continue;
        };
        if best.get(&height).is_none_or(|b| rank(f) > rank(b)) {
            best.insert(height, f);
        }
    }
    let mut options: Vec<FormatOption> = best
        .into_iter()
        .filter_map(|(height, f)| {
            let codec = codec_label(f["vcodec"].as_str());
            Some(FormatOption {
                id: f["format_id"].as_str()?.to_string(),
                label: match &codec {
                    Some(c) => format!("{height}p {c}"),
                    None => format!("{height}p"),
                },
                height,
                codec,
            })
        })
        .collect();
    options.sort_by_key(|o| std::cmp::Reverse(o.height));
    VideoInfo {
        title: text("title").unwrap_or_default(),
        thumbnail: text("thumbnail"),
        duration: v["duration"].as_f64(),
        uploader: text("uploader"),
        formats: options,
    }
}

/// A post with several videos (`-j` printed one JSON object per video): one
/// entry per video, all pointing at the post URL with their 1-based position.
/// `None` when the output holds fewer than two objects.
pub fn x_post_info(stdout: &str, url: &str) -> Option<PlaylistInfo> {
    let items: Vec<Value> = stdout
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('{'))
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    if items.len() < 2 {
        return None;
    }
    let entries = items
        .iter()
        .zip(1u32..)
        .map(|(v, n)| PlaylistEntry {
            url: url.to_string(),
            title: v["title"].as_str().map(str::to_string),
            playlist_item: Some(n),
        })
        .collect();
    let title = items[0]["playlist_title"]
        .as_str()
        .or_else(|| items[0]["title"].as_str())
        .unwrap_or_default()
        .to_string();
    Some(PlaylistInfo { title, entries })
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
            .filter_map(|e| {
                let url = e["url"].as_str().filter(|u| !u.is_empty())?;
                Some(PlaylistEntry {
                    url: url.to_string(),
                    title: e["title"].as_str().map(str::to_string),
                    playlist_item: None,
                })
            })
            .collect(),
    }
}

/// Download file name: forbidden characters removed, each run of line breaks
/// or tabs turned into one space, at most 100 chars of title.
pub fn safe_filename(title: &str, ext: &str, fallback: &str) -> String {
    let mut cleaned = String::new();
    let mut after_break = false;
    for c in title.chars() {
        let is_break = c.is_control() && c.is_whitespace();
        if is_break {
            if !after_break {
                cleaned.push(' ');
            }
        } else if !c.is_control() && !r#"\/:*?"<>|"#.contains(c) {
            cleaned.push(c);
        }
        after_break = is_break;
    }
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
    fn playlist_entries_keep_non_empty_urls() {
        let v = json!({"title": "P", "entries": [{"url": "u1", "title": "T1"}, {"url": ""}, {"id": "x"}, {"url": "u2"}]});
        let p = playlist_info(&v);
        let got: Vec<_> = p
            .entries
            .iter()
            .map(|e| (e.url.as_str(), e.title.as_deref(), e.playlist_item))
            .collect();
        assert_eq!(p.title, "P");
        assert_eq!(got, [("u1", Some("T1"), None), ("u2", None, None)]);
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
    }
}
