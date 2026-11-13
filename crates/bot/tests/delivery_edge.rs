//! Delivery edge cases: upload failures, Cancel during delivery, the
//! per-chat edit gate, internal links and invalid job ids.

mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use common::*;
use reclip_bot::i18n::{self, Lang};
use reclip_bot::router::handle_update;
use reclip_bot::telegram::TgError;
use reclip_shared::{ErrorCode, Job, JobState};

const URL: &str = "https://v.example/1";

fn core(states: Vec<Job>, file_bytes: u64) -> FakeCore {
    let core = FakeCore::default();
    core.probe(URL, Ok(video("My clip", None, &[720])));
    core.script(states);
    *core.file_bytes.lock().unwrap() = file_bytes;
    core
}

fn done(size: Option<u64>) -> Job {
    job("x", JobState::Done, Some(100.0), size)
}

fn mp3(update_id: i64) -> reclip_bot::telegram::Update {
    text_update(update_id, 5, "en", &format!("/mp3 {URL}"))
}

fn uploads(e: &Env) -> usize {
    let calls = e.app.tg.calls();
    calls
        .iter()
        .filter(|c| matches!(c, Call::Audio { .. } | Call::Video { .. }))
        .count()
}

fn tmp_is_empty(e: &Env) -> bool {
    std::fs::read_dir(e.dir.path().join("tmp")).map_or(true, |mut d| d.next().is_none())
}

#[tokio::test]
async fn upload_failure_sends_a_link_that_says_so() {
    let e = env(core(vec![done(Some(10))], 10));
    let broken = TgError {
        code: Some(500),
        description: "Internal".into(),
        retry_after: None,
    };
    e.app.tg.upload_errors.lock().unwrap().push_back(broken);
    send(&e.app, mp3(1)).await;
    assert_eq!(uploads(&e), 1, "a non-429 error is not retried");
    let link = "https://reclip.example/d/job1?sig=s";
    assert_eq!(
        e.app.tg.texts().last(),
        Some(&i18n::send_failed_link(Lang::En, link))
    );
    assert!(e.app.core.deleted.lock().unwrap().is_empty(), "job kept");
    assert!(e.app.state.snapshot().is_empty());
    assert!(tmp_is_empty(&e), "local file removed on the error path");
}

#[tokio::test]
async fn upload_429_waits_and_retries() {
    let e = env(core(vec![done(Some(10))], 10));
    let errs = [too_many(0), too_many(0)];
    e.app.tg.upload_errors.lock().unwrap().extend(errs);
    send(&e.app, mp3(1)).await;
    assert_eq!(uploads(&e), 3);
    assert!(e.app.core.links.lock().unwrap().is_empty(), "no fallback");
    assert_eq!(
        *e.app.core.deleted.lock().unwrap(),
        vec![(5, "job1".to_string())]
    );

    // Three 429 in a row: then the link.
    let e = env(core(vec![done(Some(10))], 10));
    let errs = [too_many(0), too_many(0), too_many(0)];
    e.app.tg.upload_errors.lock().unwrap().extend(errs);
    send(&e.app, mp3(1)).await;
    assert_eq!(uploads(&e), 3);
    assert_eq!(e.app.core.links.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn cancel_during_delivery_stops_it() {
    let e = env(core(vec![done(Some(10))], 10));
    e.app.core.hold_downloads.store(true, Ordering::SeqCst);
    handle_update(&e.app, mp3(1)).await;
    for _ in 0..1000 {
        if !e.app.core.downloads.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert!(!tmp_is_empty(&e), "the file is being downloaded");
    send(&e.app, click(5, 5, 100, Some("done"), "x:job1")).await;
    assert_eq!(uploads(&e), 0, "no file sent");
    assert!(tmp_is_empty(&e), "local file removed");
    assert!(
        e.app.core.cancelled.lock().unwrap().is_empty(),
        "nothing to cancel on the core"
    );
    assert_eq!(
        *e.app.core.deleted.lock().unwrap(),
        vec![(5, "job1".to_string())]
    );
    assert_eq!(e.app.tg.texts().last(), Some(&i18n::cancelled(Lang::En)));
    assert!(e.app.state.snapshot().is_empty());
}

#[tokio::test]
async fn several_jobs_in_one_chat_respect_the_edit_gap() {
    let gap = Duration::from_millis(40);
    let mut states: Vec<Job> = (1..=60)
        .map(|p| job("x", JobState::Downloading, Some(p as f32), None))
        .collect();
    states.push(done(Some(10)));
    // 60 polls of 10 ms (~600 ms of progress per job) leave room for several
    // gated edits after the three first messages took their turns (3 x 40 ms);
    // with the 2 ms default a fast machine could finish before any edit.
    let e = env_with(core(states, 10), |s| {
        s.edit_gap = gap;
        s.poll_interval = Duration::from_millis(10);
    });
    for id in 1..=3 {
        handle_update(&e.app, mp3(id)).await;
    }
    settle(&e.app).await;
    let mut times: Vec<_> = e.app.tg.edit_log.lock().unwrap().clone();
    times.retain(|(chat, _, text)| *chat == 5 && text.contains('%'));
    times.sort_by_key(|(_, at, _)| *at);
    assert!(times.len() >= 2, "progress was shown: {}", times.len());
    for w in times.windows(2) {
        let spacing = w[1].1 - w[0].1;
        assert!(spacing >= gap * 3 / 4, "edits {spacing:?} apart");
    }
    assert_eq!(uploads(&e), 3, "every job still delivered");
}

#[tokio::test]
async fn internal_links_are_rebuilt_or_withheld() {
    // The core answers with its internal address.
    let internal = || {
        let c = core(vec![done(Some(5000))], 0);
        *c.link_base.lock().unwrap() = Some("http://core:8899".into());
        c
    };
    let e = env(internal());
    send(&e.app, mp3(1)).await;
    let want = i18n::file_too_big(Lang::En, Some(5000), "https://reclip.example/d/job1?sig=s");
    assert_eq!(e.app.tg.texts().last(), Some(&want));

    let e = env_with(internal(), |s| s.public_url = None);
    send(&e.app, mp3(1)).await;
    let last = e.app.tg.texts().last().cloned().unwrap();
    assert_eq!(last, i18n::link_unavailable(Lang::En, true));
    assert!(!last.contains("core:8899"));
}

#[tokio::test]
async fn invalid_job_id_is_not_followed() {
    let e = env(core(
        vec![job("x", JobState::Downloading, Some(1.0), None)],
        0,
    ));
    *e.app.core.next_job_id.lock().unwrap() = Some("../evil".into());
    send(&e.app, mp3(1)).await;
    assert!(e.app.state.snapshot().is_empty());
    assert_eq!(
        e.app.tg.texts().last(),
        Some(&i18n::error(Lang::En, ErrorCode::Internal, None))
    );
    let calls = e.app.tg.calls();
    assert!(calls.iter().all(|c| match c {
        Call::Edit { kb, .. } | Call::Send { kb, .. } => buttons(kb).is_empty(),
        _ => true,
    }));
    // A forged Cancel button with such an id is ignored.
    send(&e.app, click(5, 5, 100, Some("x"), "x:../evil")).await;
    assert!(e.app.core.cancelled.lock().unwrap().is_empty());
    assert_eq!(e.app.tg.answers().last(), Some(&None));
}
