//! Sends a finished job: the file itself up to the upload limit, a signed
//! link above it or when sending fails (the job is then kept on the core).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use reclip_shared::Job;

use crate::app::{App, MAX_CAPTION, clip};
use crate::core_client::Core;
use crate::files::file_name;
use crate::i18n::{self, Lang};
use crate::link::{LinkReason, send_link};
use crate::state::{Tracked, valid_job_id};
use crate::telegram::{Telegram, TgError};
use crate::tracker::NOT_FOUND;

/// Longest wait honoured after an upload 429.
const MAX_RETRY_WAIT: u64 = 60;

/// How a delivery ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Sent(u64),
    Link(LinkReason),
    /// The file is gone from the core.
    Gone,
    /// Cancel pressed during the delivery.
    Aborted,
    /// The bot is stopping: the state keeps the job for the next start.
    Shutdown,
}

pub async fn deliver<T: Telegram + 'static, C: Core + 'static>(
    app: &Arc<App<T, C>>,
    t: &Tracked,
    job: &Job,
) {
    let l = Lang::from_stored(&t.lang);
    if !valid_job_id(&t.job_id) {
        app.state.remove(&t.job_id);
        return;
    }
    if let Some(size) = job.size.filter(|s| *s > app.settings.max_upload) {
        tracing::info!(job = %t.job_id, size, "too big to upload, sending a link");
        return send_link(app, t, LinkReason::TooBig(Some(size))).await;
    }
    if let Err(e) = app
        .tg
        .edit_message(t.chat_id, t.message_id, &i18n::done(l, &t.title), None)
        .await
    {
        tracing::warn!(job = %t.job_id, error = %e, "progress edit failed");
    }
    let running = app.deliveries.start(&t.job_id);
    let outcome = tokio::select! {
        _ = running.token.cancelled() => Outcome::Aborted,
        _ = app.shutdown.cancelled() => Outcome::Shutdown,
        o = transfer(app, t, job) => o,
    };
    drop(running);
    tracing::info!(job = %t.job_id, ?outcome, "delivery finished");
    match outcome {
        Outcome::Sent(_) => {
            // First: a crash from here on must not send the file twice.
            app.state.remove(&t.job_id);
            if let Err(e) = app.core.delete_job(t.user_id, &t.job_id).await {
                tracing::warn!(job = %t.job_id, error = %e, "cannot delete the job");
            }
            app.delete(t.chat_id, t.message_id).await;
        }
        Outcome::Link(reason) => send_link(app, t, reason).await,
        Outcome::Gone => {
            app.replace(t.chat_id, Some(t.message_id), &i18n::file_expired(l), None)
                .await;
            app.state.remove(&t.job_id);
        }
        Outcome::Aborted => {
            app.state.remove(&t.job_id);
            app.replace(t.chat_id, Some(t.message_id), &i18n::cancelled(l), None)
                .await;
            if let Err(e) = app.core.delete_job(t.user_id, &t.job_id).await {
                tracing::warn!(job = %t.job_id, error = %e, "cannot delete the job");
            }
        }
        Outcome::Shutdown => {}
    }
}

/// Download from the core (capped at the upload limit), then upload. Waits
/// for a delivery slot first. The local folder is removed whatever happens,
/// also when this future is dropped (Cancel, shutdown).
async fn transfer<T: Telegram + 'static, C: Core + 'static>(
    app: &Arc<App<T, C>>,
    t: &Tracked,
    job: &Job,
) -> Outcome {
    let _slot = app.deliveries.slot().await;
    let dir = app.settings.tmp_dir.join(&t.job_id);
    let _cleanup = DirGuard(dir.clone());
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        tracing::warn!(job = %t.job_id, error = %e, "cannot create the download folder");
        return Outcome::Link(LinkReason::SendFailed);
    }
    let path = dir.join(file_name(job, &t.title));
    let max = app.settings.max_upload;
    let size = match app
        .core
        .download_file_capped(t.user_id, &t.job_id, &path, max)
        .await
    {
        Ok(Some(n)) => n,
        Ok(None) => return Outcome::Link(LinkReason::TooBig(None)),
        Err(e) if e.status == NOT_FOUND => return Outcome::Gone,
        Err(e) => {
            tracing::warn!(job = %t.job_id, error = %e, "download from the core failed");
            return Outcome::Link(LinkReason::SendFailed);
        }
    };
    let attempts = app.settings.upload_attempts.max(1);
    for attempt in 1..=attempts {
        match upload(app, t, &path).await {
            Ok(()) => return Outcome::Sent(size),
            // Telegram's per-chat budget is shared with progress edits:
            // wait as asked rather than give up on a small file.
            Err(e) if e.code == Some(429) && attempt < attempts => {
                let wait = e.retry_after.unwrap_or(1).min(MAX_RETRY_WAIT);
                tracing::warn!(job = %t.job_id, attempt, wait, "upload rate-limited");
                tokio::time::sleep(Duration::from_secs(wait)).await;
            }
            Err(e) => {
                tracing::warn!(job = %t.job_id, error = %e, "upload failed");
                break;
            }
        }
    }
    Outcome::Link(LinkReason::SendFailed)
}

async fn upload<T: Telegram + 'static, C: Core + 'static>(
    app: &Arc<App<T, C>>,
    t: &Tracked,
    path: &Path,
) -> Result<(), TgError> {
    if t.format.is_audio() {
        app.tg
            .send_audio(
                t.chat_id,
                path,
                Some(&t.title),
                t.uploader.as_deref(),
                t.duration,
            )
            .await
    } else {
        let caption = clip(&t.title, MAX_CAPTION);
        app.tg
            .send_video(t.chat_id, path, &caption, t.duration, None, None)
            .await
    }
}

/// Removes the job's download folder, whatever happens.
struct DirGuard(PathBuf);

impl Drop for DirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
