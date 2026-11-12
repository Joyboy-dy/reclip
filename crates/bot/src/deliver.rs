//! Sends a finished job: the file itself up to the upload limit, a signed
//! link above it or when sending fails (the job is then kept on the core).
//! The job stays tracked until one of them reached the user, the job was
//! cancelled, or the core no longer has it.

use std::path::PathBuf;
use std::sync::Arc;

use reclip_shared::Job;

use crate::app::App;
use crate::core_client::{Core, CoreError};
use crate::files::file_name;
use crate::i18n::{self, Lang};
use crate::link::{LinkReason, send_link};
use crate::state::{Tracked, valid_job_id};
use crate::telegram::Telegram;
use crate::tracker::NOT_FOUND;

/// Downloads retried after a transient core error before falling back to
/// a link.
pub const MAX_DOWNLOAD_RETRIES: u32 = 5;

/// What the tracker does after a delivery attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Delivered, linked, cancelled, gone, or the bot is stopping.
    Over,
    /// The core could not answer: try again after a pause.
    Retry,
}

/// How a transfer ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Sent(u64),
    Link(LinkReason),
    /// The file is gone from the core.
    Gone,
    /// The core did not answer (restart, overload): try again later.
    Retry,
    /// Cancel pressed during the delivery.
    Aborted,
    /// The bot is stopping: the state keeps the job for the next start.
    Shutdown,
}

/// A core error worth waiting for: no answer, overload, a key being
/// replaced, rate limit.
pub fn transient(e: &CoreError) -> bool {
    matches!(e.status, 0 | 401 | 429) || e.status >= 500
}

/// Delivers a `Done` job; `retries` counts the earlier attempts that ended
/// in [`Delivery::Retry`].
pub async fn deliver<T: Telegram + 'static, C: Core + 'static>(
    app: &Arc<App<T, C>>,
    t: &Tracked,
    job: &Job,
    retries: u32,
) -> Delivery {
    let l = Lang::from_stored(&t.lang);
    if !valid_job_id(&t.job_id) {
        app.state.remove(&t.job_id);
        return Delivery::Over;
    }
    if let Some(size) = job.size.filter(|s| *s > app.settings.max_upload) {
        tracing::info!(job = %t.job_id, size, "too big to upload, sending a link");
        return send_link(app, t, LinkReason::TooBig(Some(size))).await;
    }
    // The download already failed this many times: only the link is retried
    // from now on, instead of moving the whole file again every cycle.
    if retries >= MAX_DOWNLOAD_RETRIES {
        return send_link(app, t, LinkReason::SendFailed).await;
    }
    // Registered before the edit: a Cancel pressed meanwhile stops it.
    let running = app.deliveries.start(&t.job_id);
    if retries == 0
        && let Err(e) = app
            .edit(t.chat_id, t.message_id, &i18n::done(l, &t.title), None)
            .await
    {
        tracing::warn!(job = %t.job_id, error = %e, "progress edit failed");
    }
    let outcome = tokio::select! {
        biased;
        _ = running.token.cancelled() => Outcome::Aborted,
        _ = app.shutdown.cancelled() => Outcome::Shutdown,
        o = transfer(app, t, job, retries) => o,
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
        Outcome::Link(reason) => return send_link(app, t, reason).await,
        Outcome::Gone => {
            app.replace(t.chat_id, Some(t.message_id), &i18n::file_expired(l), None)
                .await;
            app.state.remove(&t.job_id);
        }
        Outcome::Retry => return Delivery::Retry,
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
    Delivery::Over
}

/// Download from the core (capped at the upload limit), then upload. Waits
/// for a delivery slot first. The local folder is removed whatever happens,
/// also when this future is dropped (Cancel, shutdown).
async fn transfer<T: Telegram + 'static, C: Core + 'static>(
    app: &Arc<App<T, C>>,
    t: &Tracked,
    job: &Job,
    retries: u32,
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
        Err(e) if transient(&e) && retries < MAX_DOWNLOAD_RETRIES => {
            tracing::warn!(job = %t.job_id, retries, error = %e, "download failed, will retry");
            return Outcome::Retry;
        }
        Err(e) => {
            tracing::warn!(job = %t.job_id, error = %e, "download from the core failed");
            return Outcome::Link(LinkReason::SendFailed);
        }
    };
    // Paced with the chat's other messages; a 429 is retried there.
    let sent = app
        .send_file(
            t.chat_id,
            &path,
            t.format.is_audio(),
            &t.title,
            t.uploader.as_deref(),
            t.duration,
        )
        .await;
    match sent {
        Ok(()) => Outcome::Sent(size),
        Err(e) => {
            tracing::warn!(job = %t.job_id, error = %e, "upload failed");
            Outcome::Link(LinkReason::SendFailed)
        }
    }
}

/// Removes the job's download folder, whatever happens.
struct DirGuard(PathBuf);

impl Drop for DirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
