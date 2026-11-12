//! Unit tests of `subs`.

use std::path::Path;

use super::subs::*;

fn tracks(lang: Option<&str>, manual: &[&str], auto: &[&str]) -> Tracks {
    let own = |v: &[&str]| v.iter().map(|s| s.to_string()).collect();
    Tracks {
        language: lang.map(String::from),
        manual: own(manual),
        automatic: own(auto),
    }
}
fn pick(t: &Tracks, req: Option<&str>) -> Option<(String, bool)> {
    choose(t, req).map(|t| (t.key, t.automatic))
}

#[test]
fn choice_order() {
    let t = tracks(Some("fr"), &["en", "fr-FR"], &["fr-orig", "fr", "en"]);
    assert_eq!(pick(&t, None), Some(("fr-FR".into(), false)));
    assert_eq!(pick(&t, Some("en")), Some(("en".into(), false)));
    assert_eq!(pick(&t, Some("fr")), Some(("fr-FR".into(), false)));
    assert_eq!(pick(&t, Some("de")), None);
    let t = tracks(Some("ja"), &["de", "en-US"], &[]);
    assert_eq!(pick(&t, None), Some(("en-US".into(), false)));
    let t = tracks(Some("ja"), &[], &["en", "ja", "ja-orig"]);
    assert_eq!(pick(&t, None), Some(("ja-orig".into(), true)));
    let t = tracks(None, &[], &["en"]);
    assert_eq!(pick(&t, None), Some(("en".into(), true)));
    assert_eq!(pick(&tracks(None, &[], &[]), None), None);
    let t = tracks(Some("fr"), &[], &["de-orig", "fr", "en"]);
    assert_eq!(pick(&t, None), Some(("fr".into(), true)));
    let t = tracks(None, &[], &["de-orig", "en"]);
    assert_eq!(pick(&t, None), Some(("de-orig".into(), true)));
}

#[test]
fn unsafe_keys_are_dropped_and_listing_is_capped() {
    let json = r#"reclip-meta:{"subtitles":{"../x":[],"a/b":[],"en":[]},"automatic_captions":{}}"#;
    assert_eq!(parse_tracks(json).manual, ["en"]);
    let many: Vec<String> = (0..30).map(|i| format!("l{i}")).collect();
    let t = Tracks {
        language: None,
        manual: many.clone(),
        automatic: many,
    };
    assert_eq!(available(&t).len(), 20);
    assert!(read_track(Path::new("."), "../x").is_none());
}

#[test]
fn selectors_are_escaped_and_validated() {
    assert_eq!(escape_selector("en-US"), "en\\-US");
    for bad in ["all", "ALL", "-en", "en.*", ""] {
        assert!(validate_language(bad).is_err(), "{bad}");
    }
    assert!(validate_language("pt-BR").is_ok());
}

#[test]
fn parser_edge_cases() {
    let vtt = "\u{feff}WEBVTT\r\r1\r00:00:00.000 --> 00:00:01.000\rone &#39;a&#x42; &lrm;x\rtwo <b\r\rnext\r00:00:01.000 --> 00:00:02.000\rthree\r";
    assert_eq!(vtt_to_text(vtt), "one 'aB x\ntwo <b\nthree");
}

#[test]
fn danmaku_and_live_chat_are_not_tracks() {
    let json = r#"reclip-meta:{"subtitles":{"danmaku":[],"live_chat":[],"zh-CN":[]},"automatic_captions":{}}"#;
    assert_eq!(parse_tracks(json).manual, ["zh-CN"]);
}

#[test]
fn srt_is_read_when_there_is_no_vtt() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("sub.zh.srt"),
        "1\n00:00:00,000 --> 00:00:01,000\nni hao\n\n",
    )
    .unwrap();
    assert_eq!(read_track(dir.path(), "zh").as_deref(), Some("ni hao"));
    std::fs::write(
        dir.path().join("sub.zh.vtt"),
        "WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nvtt wins\n",
    )
    .unwrap();
    assert_eq!(read_track(dir.path(), "zh").as_deref(), Some("vtt wins"));
    assert!(read_track(dir.path(), "xx").is_none());
}

/// Shape of a real YouTube auto caption: a cue's first text line is one
/// space, then the tagged line; the next cue repeats the plain line.
#[test]
fn youtube_auto_caption_cues_keep_their_tagged_line() {
    let vtt = "WEBVTT\nKind: captions\nLanguage: en\n\n\
00:00:00.160 --> 00:00:02.310 align:start position:0%\n \nwelcome<00:00:00.640><c> to</c><00:00:00.800><c> the</c><00:00:00.960><c> show</c>\n\n\
00:00:02.310 --> 00:00:02.320 align:start position:0%\nwelcome to the show\n \n\n\
00:00:02.320 --> 00:00:04.230 align:start position:0%\nwelcome to the show\ntoday<00:00:02.800><c> we</c><00:00:03.000><c> cook</c>\n\n\
00:00:04.230 --> 00:00:04.240 align:start position:0%\ntoday we cook\n \n";
    assert_eq!(vtt_to_text(vtt), "welcome to the show\ntoday we cook");
}
