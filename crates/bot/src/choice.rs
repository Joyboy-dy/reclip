//! Button clicks on a pending choice: format, then quality.

use std::sync::Arc;
use std::time::Instant;

use reclip_shared::MediaFormat;

use crate::app::App;
use crate::core_client::Core;
use crate::flow::{lang, start_job, title_line};
use crate::i18n;
use crate::pending::Choice;
use crate::telegram::{
    CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup, MessageId, Telegram,
};

pub(crate) fn quality_prompt(c: &Choice, id: &str) -> (String, InlineKeyboardMarkup) {
    let mut rows: Vec<Vec<InlineKeyboardButton>> = c
        .qualities
        .iter()
        .enumerate()
        .map(|(i, f)| {
            vec![InlineKeyboardButton::new(
                f.label.clone(),
                format!("q:{id}:{i}"),
            )]
        })
        .collect();
    rows.push(vec![InlineKeyboardButton::new(
        i18n::btn_best(c.lang),
        format!("q:{id}:b"),
    )]);
    let text = i18n::choose_quality(c.lang, &title_line(c));
    (
        text,
        InlineKeyboardMarkup {
            inline_keyboard: rows,
        },
    )
}

/// Where the click came from: the chat, and the message if it can be
/// edited as text (a photo message cannot).
fn origin(cb: &CallbackQuery) -> (i64, Option<MessageId>) {
    match &cb.message {
        Some(m) => (m.chat.id, m.text.is_some().then_some(m.message_id)),
        None => (cb.from.id, None),
    }
}

/// The pending choice if it exists and belongs to the clicker; answers the
/// click otherwise.
async fn own_choice<T: Telegram + 'static, C: Core + 'static>(
    app: &Arc<App<T, C>>,
    cb: &CallbackQuery,
    id: &str,
) -> Option<Choice> {
    let l = lang(&cb.from);
    let found = app.pending().get(id, Instant::now()).cloned();
    match found {
        None => {
            app.answer(&cb.id, None).await;
            let (chat, _) = origin(cb);
            app.say(chat, &i18n::link_expired(l), None).await;
            None
        }
        Some(c) if c.user_id != cb.from.id => {
            tracing::info!(user = cb.from.id, "foreign choice click refused");
            app.answer(&cb.id, Some(&i18n::not_yours(l))).await;
            None
        }
        Some(c) => {
            app.answer(&cb.id, None).await;
            Some(c)
        }
    }
}

pub async fn on_format<T: Telegram + 'static, C: Core + 'static>(
    app: &Arc<App<T, C>>,
    cb: &CallbackQuery,
    id: &str,
    audio: bool,
) {
    let Some(choice) = own_choice(app, cb, id).await else {
        return;
    };
    let (chat, msg) = origin(cb);
    if audio {
        if app.pending().take(id, Instant::now()).is_some() {
            start_job(app, chat, msg, &choice, MediaFormat::Mp3, None).await;
        }
        return;
    }
    let (text, kb) = quality_prompt(&choice, id);
    app.replace(chat, msg, &text, Some(&kb)).await;
}

pub async fn on_quality<T: Telegram + 'static, C: Core + 'static>(
    app: &Arc<App<T, C>>,
    cb: &CallbackQuery,
    id: &str,
    pick: Option<usize>,
) {
    let Some(choice) = own_choice(app, cb, id).await else {
        return;
    };
    // Taken: a second click on the same keyboard does not start a second job.
    if app.pending().take(id, Instant::now()).is_none() {
        return;
    }
    let format_id = pick
        .and_then(|i| choice.qualities.get(i))
        .map(|f| f.id.clone());
    let (chat, msg) = origin(cb);
    start_job(app, chat, msg, &choice, MediaFormat::Mp4, format_id).await;
}
