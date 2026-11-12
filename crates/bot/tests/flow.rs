//! Conversation: commands, links, choices and buttons.

mod common;

use common::*;
use reclip_bot::core_client::CoreError;
use reclip_bot::i18n::{self, Lang};
use reclip_shared::{ErrorCode, JobState, MediaFormat, PlaylistInfo, ProbeResponse};

const URL: &str = "https://v.example/1";

fn core_with_video(thumb: Option<&str>) -> FakeCore {
    let core = FakeCore::default();
    core.probe(URL, Ok(video("My clip", thumb, &[1080, 720, 480])));
    core.script(vec![
        job("x", JobState::Queued, None, None),
        job("x", JobState::Cancelled, None, None),
    ]);
    core
}

/// The callback data of the format button `audio` from the choice message.
fn format_data(calls: &[Call], audio: bool) -> String {
    let kb = calls.iter().find_map(|c| match c {
        Call::Photo { kb, .. } | Call::Edit { kb, .. } if kb.is_some() => Some(kb.clone()),
        _ => None,
    });
    let b = buttons(&kb.expect("a choice keyboard"));
    b[usize::from(audio)].1.clone()
}

#[tokio::test]
async fn start_and_help() {
    let e = env(FakeCore::default());
    send(&e.app, text_update(1, 5, "en", "/start")).await;
    send(&e.app, text_update(2, 5, "en", "/help")).await;
    send(&e.app, text_update(3, 5, "en", "hello")).await;
    assert_eq!(
        e.app.tg.texts(),
        vec![
            i18n::welcome(Lang::En),
            i18n::help(Lang::En),
            i18n::not_a_link(Lang::En)
        ]
    );
}

#[tokio::test]
async fn link_shows_format_choice() {
    let e = env(core_with_video(Some("https://i.example/t.jpg")));
    send(&e.app, text_update(1, 5, "en", &format!("look {URL} !"))).await;
    let calls = e.app.tg.calls();
    assert!(matches!(&calls[0], Call::Send { text, .. } if *text == i18n::analyzing(Lang::En)));
    let Call::Photo {
        url, caption, kb, ..
    } = &calls[1]
    else {
        panic!("{calls:?}")
    };
    assert_eq!(url, "https://i.example/t.jpg");
    assert!(caption.contains("My clip (3:05)"));
    let b = buttons(kb);
    assert_eq!(b[0].0, "🎬 MP4");
    assert_eq!(b[1].0, "🎵 MP3");
    assert!(b[0].1.starts_with("f:") && b.iter().all(|(_, d)| d.len() <= 64));
    assert!(
        matches!(calls[2], Call::Delete { msg: 100, .. }),
        "analyzing message removed"
    );

    // Without a thumbnail the analyzing message becomes the choice.
    let e = env(core_with_video(None));
    send(&e.app, text_update(1, 5, "en", URL)).await;
    let calls = e.app.tg.calls();
    assert!(
        matches!(
            &calls[1],
            Call::Edit {
                msg: 100,
                kb: Some(_),
                ..
            }
        ),
        "{calls:?}"
    );
}

#[tokio::test]
async fn mp4_then_quality_creates_job() {
    let e = env(core_with_video(Some("https://i.example/t.jpg")));
    send(&e.app, text_update(1, 5, "en", URL)).await;
    let data = format_data(&e.app.tg.calls(), false);
    // The photo message (id 101) has no text: the quality choice is a new message.
    send(&e.app, click(5, 5, 101, None, &data)).await;
    let calls = e.app.tg.calls();
    let Some(Call::Send { text, kb, .. }) = calls.last() else {
        panic!("{calls:?}")
    };
    assert!(text.contains("My clip"));
    let q = buttons(kb);
    let labels: Vec<&str> = q.iter().map(|(l, _)| l.as_str()).collect();
    assert_eq!(
        labels,
        ["1080p", "720p", "480p", i18n::btn_best(Lang::En).as_str()]
    );
    let quality_msg = 102;
    send(&e.app, click(5, 5, quality_msg, Some("pick"), &q[1].1)).await;

    let created = e.app.core.created.lock().unwrap().clone();
    assert_eq!(created.len(), 1);
    let (user, req) = &created[0];
    assert_eq!(*user, 5);
    assert_eq!(
        (req.format, req.format_id.as_deref()),
        (MediaFormat::Mp4, Some("f720"))
    );
    assert_eq!(req.title.as_deref(), Some("My clip"));
    assert_eq!(req.thumbnail.as_deref(), Some("https://i.example/t.jpg"));
    let progress = e.app.tg.calls().into_iter().find(|c| {
        matches!(c, Call::Edit { msg, text, .. } if *msg == quality_msg && *text == i18n::queued(Lang::En, "My clip"))
    });
    let Some(Call::Edit { kb, .. }) = progress else {
        panic!("no progress message")
    };
    assert_eq!(
        buttons(&kb),
        vec![(i18n::btn_cancel(Lang::En), "x:job1".to_string())]
    );
    // A second click on the same keyboard does not create a second job.
    send(&e.app, click(5, 5, quality_msg, Some("pick"), &q[0].1)).await;
    assert_eq!(e.app.core.created.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn mp3_creates_job() {
    let e = env(core_with_video(None));
    send(&e.app, text_update(1, 5, "en", URL)).await;
    let data = format_data(&e.app.tg.calls(), true);
    send(&e.app, click(5, 5, 100, Some("choice"), &data)).await;
    // `/mp3 <link>` skips the choice; `/mp4 <link>` asks only the quality.
    send(&e.app, text_update(2, 5, "en", &format!("/mp3 {URL}"))).await;
    send(&e.app, text_update(3, 5, "en", &format!("/mp4 {URL}"))).await;
    let created = e.app.core.created.lock().unwrap().clone();
    let formats: Vec<_> = created
        .iter()
        .map(|(_, r)| (r.format, r.format_id.clone()))
        .collect();
    assert_eq!(
        formats,
        vec![(MediaFormat::Mp3, None), (MediaFormat::Mp3, None)]
    );
    let last = e.app.tg.calls().last().cloned();
    assert!(
        matches!(&last, Some(Call::Edit { text, .. }) if text.contains(&i18n::choose_quality(Lang::En, "My clip (3:05)"))),
        "{last:?}"
    );
}

#[tokio::test]
async fn other_users_cannot_press_buttons() {
    let core = core_with_video(None);
    core.script(vec![job("x", JobState::Downloading, Some(10.0), None)]);
    let e = env(core);
    send(&e.app, text_update(1, 5, "en", URL)).await;
    let data = format_data(&e.app.tg.calls(), true);
    // User 6 got the message forwarded and clicks.
    send(&e.app, click(6, 6, 100, Some("choice"), &data)).await;
    assert!(e.app.core.created.lock().unwrap().is_empty());
    assert_eq!(
        e.app.tg.answers().last().cloned().flatten(),
        Some(i18n::not_yours(Lang::En))
    );

    // The owner starts a job (it keeps downloading); 6 presses Cancel.
    let app = e.app.clone();
    reclip_bot::router::handle_update(&app, click(5, 5, 100, Some("choice"), &data)).await;
    reclip_bot::router::handle_update(&app, click(6, 6, 100, Some("progress"), "x:job1")).await;
    assert!(e.app.core.cancelled.lock().unwrap().is_empty());
    assert_eq!(
        e.app.tg.answers().last().cloned().flatten(),
        Some(i18n::not_yours(Lang::En))
    );
    assert!(e.app.state.get("job1").is_some());
    reclip_bot::router::handle_update(&app, click(5, 5, 100, Some("progress"), "x:job1")).await;
    settle(&e.app).await;
    assert_eq!(
        *e.app.core.cancelled.lock().unwrap(),
        vec![(5, "job1".to_string())]
    );
}

#[tokio::test]
async fn playlist_is_refused() {
    let core = FakeCore::default();
    let list = PlaylistInfo {
        title: "L".into(),
        entries: vec![],
    };
    core.probe(URL, Ok(ProbeResponse::Playlist(list)));
    let e = env(core);
    send(&e.app, text_update(1, 5, "fr", URL)).await;
    let calls = e.app.tg.calls();
    let want = i18n::playlist_refused(Lang::Fr, Some("https://reclip.example"));
    assert!(
        matches!(&calls[1], Call::Edit { msg: 100, text, kb: None, .. } if *text == want),
        "{calls:?}"
    );
    assert!(e.app.core.created.lock().unwrap().is_empty());
}

#[tokio::test]
async fn expired_choice() {
    let e = env_with(core_with_video(None), |s| {
        s.choice_ttl = std::time::Duration::ZERO
    });
    send(&e.app, text_update(1, 5, "en", URL)).await;
    let data = format_data(&e.app.tg.calls(), true);
    send(&e.app, click(5, 5, 100, Some("choice"), &data)).await;
    send(&e.app, click(5, 5, 100, Some("choice"), "q:unknown:b")).await;
    assert!(e.app.core.created.lock().unwrap().is_empty());
    let expired = e
        .app
        .tg
        .texts()
        .iter()
        .filter(|t| **t == i18n::link_expired(Lang::En))
        .count();
    assert_eq!(expired, 2);
}

#[tokio::test]
async fn group_messages_ignored() {
    let e = env(core_with_video(None));
    send(&e.app, group_update(5, URL)).await;
    send(&e.app, group_update(5, "/start")).await;
    assert!(e.app.tg.calls().is_empty());
    assert!(e.app.core.created.lock().unwrap().is_empty());
}

#[tokio::test]
async fn rate_limit_message_shows_wait() {
    let core = FakeCore::default();
    let limited = CoreError {
        status: 429,
        code: ErrorCode::RateLimited,
        message: "slow".into(),
        retry_after: Some(42),
    };
    core.probe(URL, Err(limited));
    let e = env(core);
    send(&e.app, text_update(1, 5, "en", URL)).await;
    let last = e.app.tg.texts().last().cloned().unwrap();
    assert_eq!(
        last,
        i18n::error(Lang::En, ErrorCode::RateLimited, Some(42))
    );
    assert!(last.contains("42"));
}

#[tokio::test]
async fn french_and_english() {
    let e = env(core_with_video(None));
    send(&e.app, text_update(1, 5, "fr-FR", "/start")).await;
    send(&e.app, text_update(2, 6, "en-GB", "/start")).await;
    send(&e.app, text_update(3, 7, "de", URL)).await;
    send(&e.app, text_update(4, 8, "fr", URL)).await;
    let texts = e.app.tg.texts();
    assert_eq!(texts[0], i18n::welcome(Lang::Fr));
    assert_eq!(texts[1], i18n::welcome(Lang::En));
    assert!(texts.contains(&i18n::choose_format(Lang::En, "My clip (3:05)")));
    assert!(texts.contains(&i18n::choose_format(Lang::Fr, "My clip (3:05)")));
    assert_ne!(i18n::welcome(Lang::Fr), i18n::welcome(Lang::En));
}

#[tokio::test]
async fn cancel_and_jobs_commands() {
    let core = core_with_video(None);
    core.script(vec![job("x", JobState::Downloading, Some(1.0), None)]);
    let e = env(core);
    let app = e.app.clone();
    reclip_bot::router::handle_update(&app, text_update(1, 5, "en", &format!("/mp3 {URL}"))).await;
    reclip_bot::router::handle_update(&app, text_update(2, 5, "en", "/jobs")).await;
    reclip_bot::router::handle_update(&app, text_update(3, 5, "en", "/cancel")).await;
    settle(&e.app).await;
    send(&e.app, text_update(4, 5, "en", "/cancel")).await;
    let texts = e.app.tg.texts();
    assert!(texts.contains(&i18n::jobs_list(Lang::En, &["My clip".into()])));
    assert!(texts.contains(&i18n::cancelled_count(Lang::En, 1)));
    assert!(
        texts.contains(&i18n::cancelled(Lang::En)),
        "tracker shows the cancellation"
    );
    assert_eq!(texts.last(), Some(&i18n::nothing_to_cancel(Lang::En)));
}
