//! `POST /api/v1/transcript`: subtitles as plain text.

mod common;

use common::TestServer;
use reclip_core::ytdlp::subs::{MAX_TEXT_BYTES, sweep_leftovers, vtt_to_text};
use reclip_shared::Scope;
use serde_json::{Value, json};

const TRANSCRIPT: &str = "/api/v1/transcript";

fn body(url: &str) -> Value {
    json!({"url": url})
}

#[test]
fn vtt_to_text_strips_timing_and_tags() {
    let vtt = "WEBVTT\nKind: captions\nLanguage: en\n\nNOTE a comment\n\n\
        STYLE\n::cue { color: red }\n\n1\n00:00:01.000 --> 00:00:02.000 align:start\n\
        <c.colorE5E5E5>Hello</c> &amp; <00:00:01.500>welcome\n\n\
        00:00:02.000 --> 00:00:03.000\nsecond line\n\n";
    assert_eq!(vtt_to_text(vtt), "Hello & welcome\nsecond line");
}

#[test]
fn auto_captions_are_deduplicated() {
    let vtt = "WEBVTT\n\n00:00:00.000 --> 00:00:02.000\nhello world\n\n\
        00:00:02.000 --> 00:00:02.010\nhello world\n\n\
        00:00:02.010 --> 00:00:04.000\nhello world\nsecond part\n\n\
        00:00:04.000 --> 00:00:04.010\nsecond part\n";
    assert_eq!(vtt_to_text(vtt), "hello world\nsecond part");
}

#[tokio::test]
async fn auto_captions_over_http() {
    let s = TestServer::start(|_| {}).await;
    let r = s
        .post(TRANSCRIPT, body("https://subs-auto.example/v"))
        .await;
    assert_eq!(r.status(), 200);
    let b: Value = r.json().await.unwrap();
    assert_eq!(b["automatic"], true);
    assert_eq!(b["language"], "en-orig");
    assert_eq!(b["text"], "hello world\nsecond part");
}

#[tokio::test]
async fn prefers_manual_over_automatic() {
    let s = TestServer::start(|_| {}).await;
    let r = s
        .post(TRANSCRIPT, body("https://subs-both.example/v"))
        .await;
    assert_eq!(r.status(), 200);
    let b: Value = r.json().await.unwrap();
    assert_eq!(b["automatic"], false);
    assert_eq!(b["language"], "fr");
    assert_eq!(b["text"], "Bonjour");
}

#[tokio::test]
async fn requested_language_is_passed_on() {
    let s = TestServer::start(|_| {}).await;
    let r = s
        .post(
            TRANSCRIPT,
            json!({"url": "https://subs-manual.example/v", "language": "de"}),
        )
        .await;
    assert_eq!(r.status(), 200);
    let b: Value = r.json().await.unwrap();
    assert_eq!(
        (b["language"].as_str(), b["automatic"].as_bool()),
        (Some("de"), Some(false))
    );
}

#[tokio::test]
async fn no_subtitles_is_unavailable() {
    let s = TestServer::start(|_| {}).await;
    let r = s.post(TRANSCRIPT, body("https://nosubs.example/v")).await;
    assert_eq!(r.status(), 422);
    let b: Value = r.json().await.unwrap();
    assert_eq!(b["code"], "unavailable");
    assert_eq!(b["message"], "No subtitles available");
    // Nothing is left in the data dir's temp area.
    let tmp = s.data.join("tmp");
    let left = std::fs::read_dir(&tmp).map_or(0, Iterator::count);
    assert_eq!(left, 0);
}

#[tokio::test]
async fn transcript_counts_as_a_probe() {
    let s = TestServer::start(|c| c.limits.web.probes_per_minute = 2).await;
    let r = s
        .post(TRANSCRIPT, body("https://subs-manual.example/v"))
        .await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["ratelimit-remaining"], "1");
    let r = s.post("/api/v1/probe", json!({"url": "https://ok"})).await;
    assert_eq!(r.status(), 200);
    let r = s
        .post(TRANSCRIPT, body("https://subs-manual.example/v"))
        .await;
    assert_eq!(r.status(), 429);
}

#[tokio::test]
async fn transcript_needs_probe_scope() {
    let s = TestServer::start(|_| {}).await;
    let a = s.agent(&[Scope::JobsRead]).await;
    let r = a
        .post(TRANSCRIPT, body("https://subs-manual.example/v"))
        .await;
    assert_eq!(r.status(), 403);
    let a = s.agent(&[Scope::Probe]).await;
    let r = a
        .post(TRANSCRIPT, body("https://subs-manual.example/v"))
        .await;
    assert_eq!(r.status(), 200);
}

#[tokio::test]
async fn private_targets_are_refused() {
    let s = TestServer::start(|_| {}).await;
    let r = s.post(TRANSCRIPT, body("http://[::1]:8080/")).await;
    assert_eq!(r.status(), 403);
}

async fn fetch(s: &TestServer, url: &str, language: Option<&str>) -> Value {
    let r = s
        .post(TRANSCRIPT, json!({"url": url, "language": language}))
        .await;
    assert_eq!(r.status(), 200, "{url}");
    r.json().await.unwrap()
}

#[tokio::test]
async fn bad_language_codes_are_refused() {
    let s = TestServer::start(|_| {}).await;
    for bad in ["en.*;rm", "all", "-en"] {
        let r = s
            .post(
                TRANSCRIPT,
                json!({"url": "https://subs-manual.example/v", "language": bad}),
            )
            .await;
        assert_eq!(r.status(), 400, "{bad}");
    }
}

#[tokio::test]
async fn manual_only_non_english_subtitles_are_found_without_a_language() {
    let s = TestServer::start(|_| {}).await;
    let b = fetch(&s, "https://subs-fr.example/v", None).await;
    assert_eq!(b["language"], "fr");
    assert_eq!(b["automatic"], false);
    assert_eq!(b["text"], "Bonjour");
    let b = fetch(&s, "https://subs-enus.example/v", None).await;
    assert_eq!(b["language"], "en-US");
}

#[tokio::test]
async fn a_requested_language_matches_its_regional_variant() {
    let s = TestServer::start(|_| {}).await;
    let b = fetch(&s, "https://subs-frfr.example/v", Some("fr")).await;
    assert_eq!(b["language"], "fr-FR");
    let r = s
        .post(
            TRANSCRIPT,
            json!({"url": "https://subs-frfr.example/v", "language": "de"}),
        )
        .await;
    assert_eq!(r.status(), 422);
}

#[tokio::test]
async fn a_listed_track_that_is_not_written_is_a_retryable_failure() {
    let s = TestServer::start(|_| {}).await;
    let r = s
        .post(TRANSCRIPT, body("https://subs-lost.example/v"))
        .await;
    assert_eq!(r.status(), 422);
    let b: Value = r.json().await.unwrap();
    assert_eq!(b["code"], "unavailable");
    assert_eq!(
        b["message"],
        "Subtitles could not be downloaded, try again later"
    );
    assert_eq!(std::fs::read_dir(s.data.join("tmp")).unwrap().count(), 0);
}

#[tokio::test]
async fn the_temp_dir_is_empty_after_success() {
    let s = TestServer::start(|_| {}).await;
    fetch(&s, "https://subs-manual.example/v", None).await;
    assert_eq!(std::fs::read_dir(s.data.join("tmp")).unwrap().count(), 0);
}

#[test]
fn parser_handles_multi_line_cues_crlf_and_bom() {
    let vtt =
        "\u{feff}WEBVTT\r\n\r\nid\r\n00:00:00.000 --> 00:00:01.000\r\nline one\r\nline two\r\n\r\n";
    assert_eq!(vtt_to_text(vtt), "line one\nline two");
}

#[test]
fn text_is_cut_at_one_mebibyte() {
    let mut vtt = String::from("WEBVTT\n\n00:00:00.000 --> 00:00:01.000\n");
    for i in 0..60_000 {
        vtt.push_str(&format!("a fairly long caption line number {i}\n"));
    }
    let text = vtt_to_text(&vtt);
    assert!(vtt.len() > MAX_TEXT_BYTES);
    assert!(text.len() <= MAX_TEXT_BYTES && text.len() > MAX_TEXT_BYTES - 100);
}

#[test]
fn leftover_temp_dirs_are_swept() {
    let data = tempfile::tempdir().unwrap();
    let tmp = data.path().join("tmp");
    std::fs::create_dir_all(tmp.join("subs-abc")).unwrap();
    std::fs::write(tmp.join("subs-abc/sub.en.vtt"), "x").unwrap();
    std::fs::create_dir_all(tmp.join("other")).unwrap();
    assert_eq!(sweep_leftovers(data.path()), 1);
    assert!(!tmp.join("subs-abc").exists() && tmp.join("other").exists());
}

#[tokio::test]
async fn an_unavailable_language_lists_what_exists() {
    let s = TestServer::start(|_| {}).await;
    let r = s
        .post(
            TRANSCRIPT,
            json!({"url": "https://subs-manual.example/v", "language": "ja"}),
        )
        .await;
    assert_eq!(r.status(), 422);
    let b: Value = r.json().await.unwrap();
    assert_eq!(b["code"], "unavailable");
    assert_eq!(b["message"], "No subtitles in ja. Available: de, en");
}

#[tokio::test]
async fn a_failing_second_pass_is_reported_and_cleaned_up() {
    let s = TestServer::start(|_| {}).await;
    let r = s
        .post(TRANSCRIPT, body("https://subs-fail.example/v"))
        .await;
    assert_eq!(r.status(), 500);
    assert_eq!(std::fs::read_dir(s.data.join("tmp")).unwrap().count(), 0);
}

#[tokio::test]
async fn subtitles_written_as_srt_are_read() {
    let s = TestServer::start(|_| {}).await;
    let b = fetch(&s, "https://subs-srt.example/v", None).await;
    assert_eq!(
        (b["language"].as_str(), b["text"].as_str()),
        (Some("zh"), Some("ni hao"))
    );
    assert_eq!(std::fs::read_dir(s.data.join("tmp")).unwrap().count(), 0);
}

#[tokio::test]
async fn playlist_links_are_refused() {
    let s = TestServer::start(|_| {}).await;
    for url in [
        "https://subs-manual.example/playlist?list=PL1",
        "https://subs-manual.example/channel/x/videos?list=PL1",
    ] {
        let r = s.post(TRANSCRIPT, body(url)).await;
        assert_eq!(r.status(), 400, "{url}");
        let b: Value = r.json().await.unwrap();
        assert_eq!(b["code"], "invalid_request");
        assert_eq!(b["message"], "Pass a single video link, not a playlist");
    }
    // A video that also names a playlist is a single video.
    let b = fetch(&s, "https://subs-manual.example/watch?v=a&list=PL1", None).await;
    assert_eq!(b["language"], "en");
}
