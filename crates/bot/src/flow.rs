//! The conversation: commands, links, job creation and cancel.

use std::sync::Arc;
use std::time::Instant;

use reclip_shared::{CreateJobRequest, ErrorCode, JobState, MediaFormat, ProbeResponse};

use crate::app::{App, MAX_CAPTION, MAX_TITLE, cancel_keyboard, clip, core_error_text};
use crate::choice;
use crate::core_client::{Core, CoreError};
use crate::i18n::{self, Lang};
use crate::pending::{Choice, qualities};
use crate::router::{Command, find_link, parse_command};
use crate::state::{Tracked, valid_job_id};
use crate::telegram::{
    CallbackQuery, InlineKeyboardButton, InlineKeyboardMarkup, Message, MessageId, Telegram, User,
};

/// What to do once the link is analysed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Ask,
    Mp4,
    Mp3,
}

pub(crate) fn lang(u: &User) -> Lang {
    Lang::from_code(u.language_code.as_deref())
}

pub async fn on_message<T: Telegram + 'static, C: Core + 'static>(
    app: &Arc<App<T, C>>,
    msg: Message,
) {
    let Some(user) = msg.from else { return };
    let (l, chat) = (lang(&user), msg.chat.id);
    let text = msg.text.unwrap_or_default();
    let cmd = parse_command(&text);
    tracing::info!(user = user.id, command = ?cmd.as_ref().map(cmd_name), "message");
    let link = match cmd {
        Some(Command::Start) => Err(i18n::welcome(l)),
        Some(Command::Help | Command::Unknown) => Err(i18n::help(l)),
        Some(Command::Cancel) => return cancel_all(app, user.id, chat, l).await,
        Some(Command::Jobs) => {
            let titles: Vec<String> = app
                .state
                .for_user(user.id)
                .into_iter()
                .map(|t| t.title)
                .collect();
            Err(i18n::jobs_list(l, &titles))
        }
        Some(Command::Mp4(link)) => link.map(|u| (u, Mode::Mp4)).ok_or(i18n::not_a_link(l)),
        Some(Command::Mp3(link)) => link.map(|u| (u, Mode::Mp3)).ok_or(i18n::not_a_link(l)),
        None => find_link(&text)
            .map(|u| (u, Mode::Ask))
            .ok_or(i18n::not_a_link(l)),
    };
    // Err: a plain answer.
    match link {
        Ok((url, mode)) => on_link(app, user.id, chat, l, url, mode).await,
        Err(answer) => {
            app.say(chat, &answer, None).await;
        }
    }
}

fn cmd_name(c: &Command) -> &'static str {
    match c {
        Command::Start => "start",
        Command::Help => "help",
        Command::Mp4(_) => "mp4",
        Command::Mp3(_) => "mp3",
        Command::Cancel => "cancel",
        Command::Jobs => "jobs",
        Command::Unknown => "unknown",
    }
}

async fn on_link<T: Telegram + 'static, C: Core + 'static>(
    app: &Arc<App<T, C>>,
    user: i64,
    chat: i64,
    l: Lang,
    url: String,
    mode: Mode,
) {
    let status = app.say(chat, &i18n::analyzing(l), None).await;
    let info = match app.core.probe(user, &url).await {
        Ok(ProbeResponse::Video(v)) => v,
        Ok(ProbeResponse::Playlist(_)) => {
            tracing::info!(user, "playlist refused");
            let site = app.settings.public_url.as_deref();
            app.replace(chat, status, &i18n::playlist_refused(l, site), None)
                .await;
            return;
        }
        Err(e) => {
            tracing::info!(user, status = e.status, code = ?e.code, "probe failed");
            app.replace(chat, status, &core_error_text(l, &e), None)
                .await;
            return;
        }
    };
    let choice = Choice {
        user_id: user,
        url,
        lang: l,
        title: clip(&info.title, MAX_TITLE),
        thumbnail: info.thumbnail.clone(),
        uploader: info.uploader.as_deref().map(|u| clip(u, MAX_TITLE)),
        duration: info
            .duration
            .filter(|d| *d >= 0.0)
            .map(|d| d.round() as u32),
        qualities: qualities(&info.formats),
    };
    if mode == Mode::Mp3 {
        return start_job(app, chat, status, &choice, MediaFormat::Mp3, None).await;
    }
    let id = app.pending().insert(choice.clone(), Instant::now());
    if mode == Mode::Mp4 {
        let (text, kb) = choice::quality_prompt(&choice, &id);
        app.replace(chat, status, &text, Some(&kb)).await;
        return;
    }
    let kb = InlineKeyboardMarkup {
        inline_keyboard: vec![vec![
            InlineKeyboardButton::new(i18n::btn_video(l), format!("f:{id}:v")),
            InlineKeyboardButton::new(i18n::btn_audio(l), format!("f:{id}:a")),
        ]],
    };
    let text = i18n::choose_format(l, &title_line(&choice));
    if let Some(thumb) = choice.thumbnail.as_deref().filter(|u| web_url(u)) {
        let caption = clip(&text, MAX_CAPTION);
        match app.photo(chat, thumb, &caption, Some(&kb)).await {
            Ok(_) => {
                if let Some(s) = status {
                    app.delete(chat, s).await;
                }
                return;
            }
            Err(e) => tracing::warn!(chat, error = %e, "sendPhoto failed, sending text"),
        }
    }
    app.replace(chat, status, &text, Some(&kb)).await;
}

/// Only `http(s)://` addresses go to sendPhoto: the thumbnail comes from
/// the probed page, and a local Bot API server would also read `file://`.
fn web_url(u: &str) -> bool {
    reqwest::Url::parse(u)
        .is_ok_and(|u| matches!(u.scheme(), "http" | "https") && u.host_str().is_some())
}

/// "Title (3:05)".
pub(crate) fn title_line(c: &Choice) -> String {
    match c.duration {
        Some(d) if d >= 3600 => format!(
            "{} ({}:{:02}:{:02})",
            c.title,
            d / 3600,
            d / 60 % 60,
            d % 60
        ),
        Some(d) => format!("{} ({}:{:02})", c.title, d / 60, d % 60),
        None => c.title.clone(),
    }
}

/// Creates the job, turns `msg` (or a new message) into the progress
/// message and starts tracking.
pub(crate) async fn start_job<T: Telegram + 'static, C: Core + 'static>(
    app: &Arc<App<T, C>>,
    chat: i64,
    msg: Option<MessageId>,
    c: &Choice,
    format: MediaFormat,
    format_id: Option<String>,
) {
    let req = CreateJobRequest {
        url: c.url.clone(),
        format,
        format_id,
        title: Some(c.title.clone()),
        thumbnail: c.thumbnail.clone(),
        clip: None,
        strip_metadata: false,
        playlist_item: None,
    };
    let job = match app.core.create_job(c.user_id, &req).await {
        Ok(j) => j,
        Err(e) => {
            tracing::info!(user = c.user_id, status = e.status, code = ?e.code, "job refused");
            app.replace(chat, msg, &core_error_text(c.lang, &e), None)
                .await;
            return;
        }
    };
    if !valid_job_id(&job.id) {
        // It would end up in paths and button data: refuse to follow it.
        tracing::warn!(user = c.user_id, "the core returned an invalid job id");
        let text = i18n::error(c.lang, ErrorCode::Internal, None);
        app.replace(chat, msg, &text, None).await;
        return;
    }
    tracing::info!(user = c.user_id, job = %job.id, ?format, "job created");
    let text = i18n::queued(c.lang, &c.title);
    let kb = cancel_keyboard(c.lang, &job.id);
    // Without any message (Telegram down), the file is still delivered.
    let message_id = app.replace(chat, msg, &text, Some(&kb)).await.unwrap_or(0);
    let t = Tracked {
        job_id: job.id,
        user_id: c.user_id,
        chat_id: chat,
        message_id,
        lang: c.lang.code().into(),
        format,
        title: c.title.clone(),
        uploader: c.uploader.clone(),
        duration: c.duration,
    };
    app.state.add(t.clone());
    app.spawn_tracker(t, text);
}

pub async fn on_cancel_button<T: Telegram + 'static, C: Core + 'static>(
    app: &Arc<App<T, C>>,
    cb: &CallbackQuery,
    job: &str,
) {
    let l = lang(&cb.from);
    let t = match app.state.get(job) {
        None => return app.answer(&cb.id, Some(&i18n::nothing_to_cancel(l))).await,
        Some(t) if t.user_id != cb.from.id => {
            tracing::info!(user = cb.from.id, job, "foreign cancel click refused");
            return app.answer(&cb.id, Some(&i18n::not_yours(l))).await;
        }
        Some(t) => t,
    };
    // Answered first: the core may take a while to stop the download, and
    // Telegram drops answers to old clicks. The tracker shows the result.
    app.answer(&cb.id, Some(&i18n::cancelling(l))).await;
    if let Err(e) = cancel_one(app, &t).await {
        app.say(t.chat_id, &core_error_text(l, &e), None).await;
    }
}

/// Stops a delivery in progress, or asks the core to cancel the job.
/// True when something was really stopped (a job that had already ended
/// on the core is left as it is).
async fn cancel_one<T: Telegram + 'static, C: Core + 'static>(
    app: &Arc<App<T, C>>,
    t: &Tracked,
) -> Result<bool, CoreError> {
    if app.deliveries.abort(&t.job_id) {
        tracing::info!(user = t.user_id, job = %t.job_id, "delivery aborted");
        return Ok(true);
    }
    let r = app.core.cancel_job(t.user_id, &t.job_id).await;
    match &r {
        Ok(job) => {
            tracing::info!(user = t.user_id, job = %t.job_id, state = ?job.state, "cancel requested")
        }
        Err(e) => tracing::info!(job = %t.job_id, status = e.status, "cancel failed"),
    }
    r.map(|job| job.state == JobState::Cancelled)
}

/// `/cancel`: cancels every job of the user that the bot follows.
async fn cancel_all<T: Telegram + 'static, C: Core + 'static>(
    app: &Arc<App<T, C>>,
    user: i64,
    chat: i64,
    l: Lang,
) {
    let mut n = 0;
    for t in app.state.for_user(user) {
        if cancel_one(app, &t).await == Ok(true) {
            n += 1;
        }
    }
    let text = if n == 0 {
        i18n::nothing_to_cancel(l)
    } else {
        i18n::cancelled_count(l, n)
    };
    app.say(chat, &text, None).await;
}
