//! `POST /api/v1/transcript`: subtitles as plain text.

mod common;

use common::TestServer;
use reclip_core::ytdlp::subs::vtt_to_text;
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
    let bad = s
        .post(
            TRANSCRIPT,
            json!({"url": "https://subs-manual.example/v", "language": "en.*;rm"}),
        )
        .await;
    assert_eq!(bad.status(), 400);
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
