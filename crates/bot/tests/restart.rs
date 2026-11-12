//! Restart persistence and the update loop.

mod common;

use common::*;
use reclip_bot::app::App;
use reclip_bot::i18n::{self, Lang};
use reclip_bot::state::{StateStore, Tracked};
use reclip_bot::telegram::TgError;
use reclip_shared::{JobState, MediaFormat};

fn done(size: Option<u64>) -> reclip_shared::Job {
    job("x", JobState::Done, Some(100.0), size)
}

fn tracked(job_id: &str) -> Tracked {
    Tracked {
        job_id: job_id.into(),
        user_id: 5,
        chat_id: 5,
        message_id: 77,
        lang: "fr".into(),
        format: MediaFormat::Mp4,
        title: "Reprise".into(),
        uploader: None,
        duration: Some(9),
    }
}

/// A fresh bot process on the state file of `dir`.
fn restarted(dir: &std::path::Path, core: FakeCore) -> TestApp {
    let state = StateStore::load(&dir.join("state.json")).unwrap();
    App::new(FakeTg::default(), core, settings(dir), state)
}

#[tokio::test]
async fn restart_resumes_tracking() {
    let dir = tempfile::tempdir().unwrap();
    StateStore::empty(&dir.path().join("state.json")).add(tracked("job7"));
    let core = FakeCore::default();
    core.add_job(
        5,
        vec![
            job("job7", JobState::Downloading, Some(60.0), None),
            done(Some(10)),
        ],
    );
    *core.file_bytes.lock().unwrap() = 10;
    let app = restarted(dir.path(), core);
    assert_eq!(app.resume(), 1);
    settle(&app).await;
    let calls = app.tg.calls();
    assert!(calls.contains(&Call::Edit {
        chat: 5,
        msg: 77,
        text: i18n::progress(Lang::Fr, "Reprise", Some(60.0), None, None),
        kb: Some(reclip_bot::app::cancel_keyboard(Lang::Fr, "job7")),
    }));
    assert!(calls.iter().any(|c| matches!(
        c,
        Call::Video {
            chat: 5,
            bytes: 10,
            ..
        }
    )));
    let on_disk = StateStore::load(&dir.path().join("state.json")).unwrap();
    assert!(on_disk.snapshot().is_empty());
}

#[tokio::test]
async fn vanished_job_is_dropped() {
    let dir = tempfile::tempdir().unwrap();
    StateStore::empty(&dir.path().join("state.json")).add(tracked("gone"));
    let app = restarted(dir.path(), FakeCore::default());
    app.resume();
    settle(&app).await;
    // Dropped without an error; the stale progress loses its Cancel button.
    assert_eq!(
        app.tg.calls(),
        vec![Call::Edit {
            chat: 5,
            msg: 77,
            text: i18n::job_gone(Lang::Fr),
            kb: None
        }]
    );
    let on_disk = StateStore::load(&dir.path().join("state.json")).unwrap();
    assert!(on_disk.snapshot().is_empty());
}

#[tokio::test]
async fn poll_loop_dispatches_and_survives_errors() {
    let e = env(FakeCore::default());
    let limited = TgError {
        code: Some(429),
        description: "slow".into(),
        retry_after: Some(0),
    };
    let transport = TgError {
        code: None,
        description: "connection reset".into(),
        retry_after: None,
    };
    {
        let mut u = e.app.tg.updates.lock().unwrap();
        u.push_back(Ok(vec![text_update(41, 5, "en", "/start")]));
        u.push_back(Err(limited));
        u.push_back(Err(transport));
        u.push_back(Ok(vec![text_update(42, 5, "en", "/help")]));
    }
    let run = tokio::spawn(e.app.clone().run());
    for _ in 0..500 {
        if e.app.tg.offsets.lock().unwrap().len() == 5 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    e.app.shutdown.cancel();
    tokio::time::timeout(std::time::Duration::from_secs(5), run)
        .await
        .unwrap()
        .unwrap();
    settle(&e.app).await;
    assert_eq!(*e.app.tg.offsets.lock().unwrap(), vec![0, 42, 42, 42, 43]);
    assert_eq!(
        e.app.tg.texts(),
        vec![i18n::welcome(Lang::En), i18n::help(Lang::En)]
    );
}
