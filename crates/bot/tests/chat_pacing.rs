//! Per-chat pacing of every send and edit, 429 retries, and which
//! thumbnails may go to sendPhoto.

mod common;

use std::time::Duration;

use common::*;
use reclip_bot::i18n::{self, Lang};
use reclip_bot::router::handle_update;

fn link(i: usize) -> String {
    format!("https://v.example/{i}")
}

#[tokio::test]
async fn a_burst_of_links_is_paced_and_survives_429() {
    const LINKS: usize = 12;
    let gap = Duration::from_millis(30);
    let core = FakeCore::default();
    for i in 0..LINKS {
        let thumb = Some("https://i.example/t.jpg");
        core.probe(&link(i), Ok(video("Clip", thumb, &[720])));
    }
    let e = env_with(core, |s| s.edit_gap = gap);
    // Telegram refuses the first 6 calls in the chat.
    e.app.tg.throttle.lock().unwrap().insert(5, 6);
    for i in 0..LINKS {
        let app = e.app.clone();
        let u = text_update(i as i64 + 1, 5, "en", &link(i));
        e.app
            .tasks
            .spawn(async move { handle_update(&app, u).await });
    }
    settle(&e.app).await;
    let calls = e.app.tg.calls();
    let analyzing = calls
        .iter()
        .filter(|c| matches!(c, Call::Send { text, .. } if *text == i18n::analyzing(Lang::En)))
        .count();
    assert_eq!(analyzing, LINKS, "every status message was sent");
    let choices = calls
        .iter()
        .filter(|c| matches!(c, Call::Photo { kb: Some(_), .. }))
        .count();
    assert_eq!(choices, LINKS, "every link got its buttons");
    let mut log: Vec<_> = e.app.tg.chat_log.lock().unwrap().clone();
    log.sort_by_key(|(_, at)| *at);
    assert!(log.len() >= 2 * LINKS + 6, "{} attempts", log.len());
    for w in log.windows(2) {
        let spacing = w[1].1 - w[0].1;
        assert!(spacing >= gap * 3 / 4, "two calls {spacing:?} apart");
    }
}

#[tokio::test]
async fn a_send_gives_up_after_three_429() {
    let e = env(FakeCore::default());
    e.app.tg.throttle.lock().unwrap().insert(5, 3);
    send(&e.app, text_update(1, 5, "en", "/help")).await;
    assert!(e.app.tg.texts().is_empty(), "three attempts, all refused");
    assert_eq!(e.app.tg.chat_log.lock().unwrap().len(), 3);
    send(&e.app, text_update(2, 5, "en", "/help")).await;
    assert_eq!(e.app.tg.texts(), vec![i18n::help(Lang::En)]);
}

#[tokio::test]
async fn only_web_thumbnails_go_to_send_photo() {
    for (thumb, photo) in [
        ("file:///var/lib/telegram-bot-api/x.jpg", false),
        ("attach://x", false),
        ("/etc/passwd", false),
        ("HTTP://i.example/t.jpg", true),
        ("https://i.example/t.jpg", true),
    ] {
        let core = FakeCore::default();
        core.probe(&link(1), Ok(video("Clip", Some(thumb), &[720])));
        let e = env(core);
        send(&e.app, text_update(1, 5, "en", &link(1))).await;
        let calls = e.app.tg.calls();
        let photos = calls.iter().any(|c| matches!(c, Call::Photo { .. }));
        assert_eq!(photos, photo, "{thumb}");
        let with_buttons = calls.iter().any(|c| match c {
            Call::Photo { kb, .. } | Call::Edit { kb, .. } | Call::Send { kb, .. } => {
                buttons(kb).iter().any(|(_, d)| d.starts_with("f:"))
            }
            _ => false,
        });
        assert!(with_buttons, "{thumb}: the choice is shown");
    }
}
