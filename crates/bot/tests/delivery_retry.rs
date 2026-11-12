//! Delivery robustness: transient core errors, Cancel around the "done"
//! edit, truthful `/cancel` counts and file names from long titles.

mod common;

use std::time::Duration;

use common::*;
use reclip_bot::i18n::{self, Lang};
use reclip_bot::router::handle_update;
use reclip_bot::state::{StateStore, Tracked};
use reclip_shared::{Job, JobState, MediaFormat};

const URL: &str = "https://v.example/1";
const LINK: &str = "https://reclip.example/d/job1?sig=s";

fn core(title: &str, states: Vec<Job>, file_bytes: u64) -> FakeCore {
    let core = FakeCore::default();
    core.probe(URL, Ok(video(title, None, &[720])));
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

fn audio_names(e: &Env) -> Vec<(String, Option<String>)> {
    let calls = e.app.tg.calls();
    calls
        .into_iter()
        .filter_map(|c| match c {
            Call::Audio { name, title, .. } => Some((name, title)),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn core_down_during_download_retries_then_sends_the_file() {
    let e = env(core("My clip", vec![done(Some(10))], 10));
    let errs = [unreachable(), unreachable()];
    e.app.core.download_errors.lock().unwrap().extend(errs);
    send(&e.app, mp3(1)).await;
    assert_eq!(e.app.core.downloads.lock().unwrap().len(), 3);
    assert_eq!(audio_names(&e).len(), 1, "the file itself is sent");
    assert!(e.app.core.links.lock().unwrap().is_empty(), "no link");
    assert!(e.app.state.snapshot().is_empty());
}

#[tokio::test]
async fn link_failure_keeps_tracking_until_the_link_is_sent() {
    let e = env(core("My clip", vec![done(Some(5000))], 0));
    let errs = [unreachable(), unreachable()];
    e.app.core.link_errors.lock().unwrap().extend(errs);
    send(&e.app, mp3(1)).await;
    assert_eq!(*e.app.core.links.lock().unwrap(), vec!["job1".to_string()]);
    let want = i18n::file_too_big(Lang::En, Some(5000), LINK);
    assert_eq!(e.app.tg.texts().last(), Some(&want));
    assert!(e.app.state.snapshot().is_empty());
}

#[tokio::test]
async fn after_the_download_retries_only_the_link_is_retried() {
    let e = env(core("My clip", vec![done(Some(10))], 10));
    let downloads = std::iter::repeat_with(unreachable).take(6);
    e.app.core.download_errors.lock().unwrap().extend(downloads);
    let links = [unreachable(), unreachable()];
    e.app.core.link_errors.lock().unwrap().extend(links);
    send(&e.app, mp3(1)).await;
    // Five failed downloads, then never again: no re-download per retry.
    assert_eq!(e.app.core.downloads.lock().unwrap().len(), 5);
    assert!(audio_names(&e).is_empty(), "no file after giving up");
    assert_eq!(e.app.core.links.lock().unwrap().len(), 1, "one link sent");
    assert!(e.app.state.snapshot().is_empty());
}

#[tokio::test]
async fn link_404_drops_the_job_as_expired() {
    let e = env(core("My clip", vec![done(Some(5000))], 0));
    e.app
        .core
        .link_errors
        .lock()
        .unwrap()
        .push_back(not_found());
    send(&e.app, mp3(1)).await;
    assert_eq!(e.app.tg.texts().last(), Some(&i18n::file_expired(Lang::En)));
    assert!(e.app.state.snapshot().is_empty());
}

#[tokio::test]
async fn cancel_during_the_done_edit_stops_the_delivery() {
    let e = env(core("My clip", vec![done(Some(10))], 10));
    *e.app.tg.hold_edit.lock().unwrap() = Some(i18n::done(Lang::En, "My clip"));
    handle_update(&e.app, mp3(1)).await;
    let held = Call::Edit {
        chat: 5,
        msg: 100,
        text: i18n::done(Lang::En, "My clip"),
        kb: None,
    };
    for _ in 0..1000 {
        if e.app.tg.calls().contains(&held) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert!(e.app.tg.calls().contains(&held), "the done edit is running");
    handle_update(&e.app, text_update(2, 5, "en", "/cancel")).await;
    e.app.tg.release.notify_one();
    settle(&e.app).await;
    assert!(audio_names(&e).is_empty(), "no file sent");
    let texts = e.app.tg.texts();
    assert!(
        texts.contains(&i18n::cancelled_count(Lang::En, 1)),
        "{texts:?}"
    );
    assert!(e.app.state.snapshot().is_empty());
}

#[tokio::test]
async fn cancel_counts_only_jobs_really_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let state = StateStore::load(&dir.path().join("state.json")).unwrap();
    state.add(Tracked {
        job_id: "job7".into(),
        user_id: 5,
        chat_id: 5,
        message_id: 77,
        lang: "en".into(),
        format: MediaFormat::Mp3,
        title: "T".into(),
        uploader: None,
        duration: None,
    });
    let core = FakeCore::default();
    core.add_job(5, vec![job("job7", JobState::Done, Some(100.0), Some(1))]);
    let app = reclip_bot::app::App::new(FakeTg::default(), core, settings(dir.path()), state);
    // No tracker runs: the job is finished on the core, nothing to cancel.
    send(&app, text_update(1, 5, "en", "/cancel")).await;
    assert_eq!(
        app.tg.texts().last(),
        Some(&i18n::nothing_to_cancel(Lang::En))
    );
}

#[tokio::test]
async fn long_non_latin_titles_give_short_ascii_file_names() {
    let title = "日".repeat(100);
    let mut d = done(Some(10));
    d.filename = Some(format!("{title}.mp3"));
    let e = env(core(&title, vec![d], 10));
    send(&e.app, mp3(1)).await;
    let sent = audio_names(&e);
    assert_eq!(sent.len(), 1, "the file is sent");
    let (name, shown) = &sent[0];
    assert!(name.is_ascii() && name.len() <= 210, "{name}");
    assert!(name.ends_with(".mp3"));
    assert_eq!(shown.as_deref(), Some(title.as_str()), "title kept");
    assert!(e.app.core.links.lock().unwrap().is_empty());
}
