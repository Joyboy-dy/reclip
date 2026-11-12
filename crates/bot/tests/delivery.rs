//! Progress tracking and delivery.

mod common;

use std::sync::atomic::Ordering;

use common::*;
use reclip_bot::i18n::{self, Lang};
use reclip_shared::{ApiError, ErrorCode, JobState};

const URL: &str = "https://v.example/1";

fn core(states: Vec<reclip_shared::Job>, file_bytes: u64) -> FakeCore {
    let core = FakeCore::default();
    core.probe(URL, Ok(video("My clip", None, &[720])));
    core.script(states);
    *core.file_bytes.lock().unwrap() = file_bytes;
    core
}

fn done(size: Option<u64>, filename: Option<&str>) -> reclip_shared::Job {
    let mut j = job("x", JobState::Done, Some(100.0), size);
    j.filename = filename.map(Into::into);
    j
}

/// `/mp3 <link>` from user 5: the analyzing message (100) becomes the progress message.
async fn start_mp3(e: &Env) {
    send(&e.app, text_update(1, 5, "en", &format!("/mp3 {URL}"))).await;
}

fn edits_of(e: &Env, msg: i64) -> Vec<String> {
    e.app
        .tg
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Edit { msg: m, text, .. } if m == msg => Some(text),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn progress_edits_only_on_change() {
    let dl = |p| job("x", JobState::Downloading, Some(p), None);
    let e = env(core(
        vec![
            dl(10.0),
            dl(10.0),
            dl(10.0),
            dl(50.0),
            dl(50.0),
            done(Some(10), None),
        ],
        10,
    ));
    start_mp3(&e).await;
    let progress: Vec<String> = edits_of(&e, 100)
        .into_iter()
        .filter(|t| t.contains('%'))
        .collect();
    assert_eq!(
        progress,
        vec![
            i18n::progress(Lang::En, "My clip", Some(10.0), None, None),
            i18n::progress(Lang::En, "My clip", Some(50.0), None, None)
        ]
    );
}

#[tokio::test]
async fn cancel_button_cancels() {
    let e = env(core(
        vec![job("x", JobState::Downloading, Some(5.0), None)],
        0,
    ));
    let app = e.app.clone();
    reclip_bot::router::handle_update(&app, text_update(1, 5, "en", &format!("/mp3 {URL}"))).await;
    reclip_bot::router::handle_update(&app, click(5, 5, 100, Some("progress"), "x:job1")).await;
    settle(&e.app).await;
    assert_eq!(
        *e.app.core.cancelled.lock().unwrap(),
        vec![(5, "job1".to_string())]
    );
    // Answered at once; the tracker then shows the result.
    assert_eq!(
        e.app.tg.answers().last().cloned().flatten(),
        Some(i18n::cancelling(Lang::En))
    );
    assert_eq!(edits_of(&e, 100).last(), Some(&i18n::cancelled(Lang::En)));
    assert!(e.app.state.snapshot().is_empty());
}

#[tokio::test]
async fn small_file_is_sent_then_deleted() {
    let e = env(core(
        vec![
            job("x", JobState::Processing, None, None),
            done(Some(10), Some("My clip.MP3")),
        ],
        10,
    ));
    start_mp3(&e).await;
    let calls = e.app.tg.calls();
    let audio = calls.iter().find_map(|c| match c {
        Call::Audio {
            name,
            bytes,
            title,
            performer,
            ..
        } => Some((name.clone(), *bytes, title.clone(), performer.clone())),
        _ => None,
    });
    assert_eq!(
        audio,
        Some((
            "My clip.mp3".into(),
            10,
            Some("My clip".into()),
            Some("Uploader".into())
        ))
    );
    assert_eq!(
        *e.app.core.deleted.lock().unwrap(),
        vec![(5, "job1".to_string())]
    );
    assert_eq!(
        calls.last(),
        Some(&Call::Delete { chat: 5, msg: 100 }),
        "progress message removed"
    );
    assert!(e.app.state.snapshot().is_empty());
    let leftovers = std::fs::read_dir(e.dir.path().join("tmp"))
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(leftovers, 0, "local file removed");
}

#[tokio::test]
async fn large_file_gets_signed_link() {
    let e = env(core(vec![done(Some(5000), None)], 5000));
    start_mp3(&e).await;
    assert!(
        e.app.core.downloads.lock().unwrap().is_empty(),
        "no download when the size is known"
    );
    assert_eq!(*e.app.core.links.lock().unwrap(), vec!["job1".to_string()]);
    assert!(e.app.core.deleted.lock().unwrap().is_empty(), "job kept");
    let calls = e.app.tg.calls();
    assert!(
        !calls
            .iter()
            .any(|c| matches!(c, Call::Audio { .. } | Call::Video { .. }))
    );
    let last = edits_of(&e, 100).last().cloned().unwrap();
    assert_eq!(
        last,
        i18n::file_too_big(Lang::En, Some(5000), "https://reclip.example/d/job1?sig=s")
    );
}

#[tokio::test]
async fn size_cap_during_download() {
    // Size unknown: the download stops at the upload limit, then a link.
    let e = env(core(vec![done(None, None)], 5000));
    start_mp3(&e).await;
    assert_eq!(e.app.core.downloads.lock().unwrap().len(), 1);
    assert_eq!(e.app.core.links.lock().unwrap().len(), 1);
    assert!(e.app.state.snapshot().is_empty());
    assert!(e.app.core.deleted.lock().unwrap().is_empty(), "job kept");
    let last = edits_of(&e, 100).last().cloned().unwrap();
    assert_eq!(
        last,
        i18n::file_too_big(Lang::En, None, "https://reclip.example/d/job1?sig=s")
    );
    assert!(!last.contains("0 B"));
}

#[tokio::test]
async fn video_is_sent_with_caption_and_duration() {
    let e = env(core(vec![done(Some(10), None)], 10));
    send(&e.app, text_update(1, 5, "en", &format!("/mp4 {URL}"))).await;
    send(
        &e.app,
        click(5, 5, 100, Some("quality"), &buttons_of_last_edit(&e)[0]),
    )
    .await;
    let video = e
        .app
        .tg
        .calls()
        .into_iter()
        .find(|c| matches!(c, Call::Video { .. }));
    assert_eq!(
        video,
        Some(Call::Video {
            chat: 5,
            name: "My clip.mp4".into(),
            bytes: 10,
            caption: "My clip".into(),
            duration: Some(185)
        })
    );
}

fn buttons_of_last_edit(e: &Env) -> Vec<String> {
    let kb = e.app.tg.calls().into_iter().rev().find_map(|c| match c {
        Call::Edit { kb: Some(kb), .. } => Some(kb),
        _ => None,
    });
    buttons(&kb).into_iter().map(|(_, d)| d).collect()
}

#[tokio::test]
async fn failed_job_shows_error_and_failed_edits_are_ignored() {
    let mut failed = job("x", JobState::Failed, None, None);
    failed.error = Some(ApiError {
        code: ErrorCode::Private,
        message: "private".into(),
    });
    let dl = |p| job("x", JobState::Downloading, Some(p), None);
    let e = env(core(vec![dl(1.0), dl(2.0), dl(3.0), failed], 0));
    e.app.tg.fail_edits.store(true, Ordering::SeqCst);
    start_mp3(&e).await;
    let texts = e.app.tg.texts();
    // Every edit failed (429): each step was tried once, then the error was sent anew.
    assert_eq!(texts.iter().filter(|t| t.contains('%')).count(), 3);
    assert_eq!(
        texts.last(),
        Some(&i18n::error(Lang::En, ErrorCode::Private, None))
    );
    assert!(e.app.state.snapshot().is_empty());
}
