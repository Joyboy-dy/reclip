use serde_json::json;

use super::parse::{playlist_info, safe_filename, video_info, x_post_info};

fn opts(v: &serde_json::Value) -> Vec<(String, String, Option<String>)> {
    video_info(v)
        .formats
        .into_iter()
        .map(|f| (f.id, f.label, f.codec))
        .collect()
}

#[test]
fn probe_prefers_avc_and_labels_others() {
    let v = json!({"formats": [
        {"format_id": "vp9-1080", "height": 1080, "vcodec": "vp09.00.40.08", "tbr": 5000.0},
        {"format_id": "avc-1080", "height": 1080, "vcodec": "avc1.640028", "tbr": 2000.0},
        {"format_id": "vp9-2160", "height": 2160, "vcodec": "vp09.00.50.08", "tbr": 9000.0},
        {"format_id": "av1-2160", "height": 2160, "vcodec": "av01.0.12M.08", "tbr": 4000.0},
        {"format_id": "hevc-1440", "height": 1440, "vcodec": "hev1.1.6.L120", "tbr": 3000.0},
        {"format_id": "avc-720", "height": 720, "vcodec": "avc1.4d401f", "tbr": 800.0},
        {"format_id": "avc-720b", "height": 720, "vcodec": "avc1.4d401f", "tbr": 900.0}
    ]});
    let s = |a: &str, b: &str, c: Option<&str>| (a.to_string(), b.to_string(), c.map(String::from));
    assert_eq!(
        opts(&v),
        [
            s("vp9-2160", "2160p VP9", Some("VP9")),
            s("hevc-1440", "1440p HEVC", Some("HEVC")),
            s("avc-1080", "1080p", None),
            s("avc-720b", "720p", None),
        ]
    );
}

#[test]
fn av1_label_when_best() {
    let v = json!({"formats": [
        {"format_id": "a", "height": 2160, "vcodec": "av01.0.12M.08", "tbr": 4000.0}
    ]});
    assert_eq!(
        opts(&v),
        [("a".into(), "2160p AV1".into(), Some("AV1".into()))]
    );
}

#[test]
fn format_without_vcodec_but_with_height_is_video() {
    // Bilibili reports no vcodec on its DASH video formats.
    let v = json!({"formats": [
        {"format_id": "b1", "height": 1080, "tbr": 1500.0},
        {"format_id": "b2", "height": null, "tbr": 100.0, "acodec": "mp4a"},
        {"format_id": "b3", "height": 480, "vcodec": "none", "tbr": 100.0}
    ]});
    assert_eq!(opts(&v), [("b1".into(), "1080p".into(), None)]);
}

fn line(id: &str, extra: serde_json::Value) -> String {
    let mut v =
        json!({"id": id, "title": format!("T{id}"), "webpage_url": "https://x.com/u/status/1"});
    v.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    format!("{v}\n")
}

#[test]
fn x_post_entries_carry_playlist_item() {
    let url = "https://x.com/u/status/1";
    let out = format!(
        "WARNING: noise\n{}{}",
        line("a", json!({})),
        line("b", json!({}))
    );
    let p = x_post_info(&out, url).unwrap();
    let got: Vec<_> = p
        .entries
        .iter()
        .map(|e| (e.url.as_str(), e.title.as_deref(), e.playlist_item))
        .collect();
    assert_eq!(
        got,
        [(url, Some("Ta"), Some(1)), (url, Some("Tb"), Some(2))]
    );
    assert!(x_post_info(&line("a", json!({})), url).is_none());
}

#[test]
fn playlist_entries_have_no_item_number() {
    let p = playlist_info(&json!({"title": "P", "entries": [{"url": "u1"}]}));
    assert_eq!(p.entries[0].playlist_item, None);
}

#[test]
fn safe_filename_handles_multiline_titles() {
    assert_eq!(
        safe_filename("Line one\nLine two\r\n", "mp4", "x"),
        "Line one Line two.mp4"
    );
    assert_eq!(safe_filename("a\tb", "mp4", "x"), "a b.mp4");
    assert_eq!(safe_filename("a\u{0}b", "mp4", "x"), "ab.mp4");
}

#[test]
fn final_name_is_the_title() {
    assert_eq!(
        safe_filename("My Video: Part 1", "mp4", "job1"),
        "My Video Part 1.mp4"
    );
}

#[test]
fn avc3_and_h264_count_as_h264() {
    let v = json!({"formats": [
        {"format_id": "vp9", "height": 720, "vcodec": "vp09.00.40.08", "tbr": 5000.0},
        {"format_id": "avc3", "height": 720, "vcodec": "avc3.64001f", "tbr": 100.0},
        {"format_id": "h264", "height": 480, "vcodec": "h264", "tbr": 100.0}
    ]});
    assert_eq!(
        opts(&v),
        [
            ("avc3".into(), "720p".into(), None),
            ("h264".into(), "480p".into(), None)
        ]
    );
}
