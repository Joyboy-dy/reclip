//! Follows one job: edits the progress message when its text changes,
//! then delivers the file or shows the failure.

use std::sync::Arc;
use std::time::{Duration, Instant};

use reclip_shared::{ErrorCode, Job, JobState};

use crate::app::{App, MAX_TEXT, cancel_keyboard, clip};
use crate::core_client::Core;
use crate::deliver::{self, Delivery};
use crate::i18n::{self, Lang};
use crate::state::Tracked;
use crate::telegram::Telegram;

/// The core answers 404 for a job that is gone or belongs to someone else.
pub const NOT_FOUND: u16 = 404;
const MAX_BACKOFF: Duration = Duration::from_secs(60);

pub async fn track<T: Telegram + 'static, C: Core + 'static>(
    app: Arc<App<T, C>>,
    t: Tracked,
    mut shown: String,
) {
    let l = Lang::from_stored(&t.lang);
    let kb = cancel_keyboard(l, &t.job_id);
    let mut failures = 0u32;
    // Deliveries that ended in `Retry` (core unreachable).
    let mut retries = 0u32;
    loop {
        let mut wait = app.settings.poll_interval;
        let read = app.core.get_job(t.user_id, &t.job_id).await;
        if read.is_ok() {
            failures = 0;
        }
        match read {
            Err(e) if e.status == NOT_FOUND => {
                tracing::info!(job = %t.job_id, "job gone from the core, dropped");
                app.state.remove(&t.job_id);
                // No live Cancel button on a job that no longer exists.
                let gone = i18n::job_gone(l);
                if let Err(e) = app.edit(t.chat_id, t.message_id, &gone, None).await {
                    tracing::warn!(job = %t.job_id, error = %e, "edit failed");
                }
                return;
            }
            Err(e) => {
                tracing::warn!(job = %t.job_id, status = e.status, error = %e, "cannot read the job");
                // Back off up to a minute while the core misbehaves.
                failures = failures.saturating_add(1);
                wait = backoff(wait, failures);
                if let Some(s) = e.retry_after {
                    wait = wait.max(Duration::from_secs(s));
                }
            }
            Ok(job) => match job.state {
                JobState::Queued | JobState::Downloading | JobState::Processing => {
                    let text = clip(&running_text(l, &t.title, &job), MAX_TEXT);
                    // Skipped when the chat was edited less than `edit_gap`
                    // ago: the next poll shows the latest text.
                    if text != shown && app.gate.allow(t.chat_id, Instant::now()) {
                        // A failed edit (429 twice, message deleted) only
                        // costs one progress step.
                        if let Err(e) = app
                            .tg
                            .edit_message(t.chat_id, t.message_id, &text, Some(&kb))
                            .await
                        {
                            tracing::warn!(job = %t.job_id, error = %e, "progress edit failed");
                        }
                        shown = text;
                    }
                }
                JobState::Done => match deliver::deliver(&app, &t, &job, retries).await {
                    Delivery::Over => return,
                    Delivery::Retry => {
                        retries = retries.saturating_add(1);
                        wait = backoff(wait, retries);
                    }
                },
                JobState::Failed => {
                    let code = job.error.as_ref().map_or(ErrorCode::Internal, |e| e.code);
                    tracing::info!(job = %t.job_id, ?code, "job failed");
                    return finish(&app, &t, &i18n::error(l, code, None)).await;
                }
                JobState::Cancelled => {
                    tracing::info!(job = %t.job_id, "job cancelled");
                    return finish(&app, &t, &i18n::cancelled(l)).await;
                }
                JobState::Expired => {
                    tracing::info!(job = %t.job_id, "job expired before delivery");
                    return finish(&app, &t, &i18n::file_expired(l)).await;
                }
            },
        }
        if !app.pause(wait).await {
            return; // shutting down: the state keeps the job for the next start
        }
    }
}

/// Shows `text` in place of the progress and stops following the job.
async fn finish<T: Telegram + 'static, C: Core + 'static>(
    app: &Arc<App<T, C>>,
    t: &Tracked,
    text: &str,
) {
    app.replace(t.chat_id, Some(t.message_id), text, None).await;
    app.state.remove(&t.job_id);
}

/// `wait` doubled `n` times (at most 5), capped at a minute.
fn backoff(wait: Duration, n: u32) -> Duration {
    (wait * 2u32.saturating_pow(n.min(5))).min(MAX_BACKOFF)
}

fn running_text(l: Lang, title: &str, job: &Job) -> String {
    match job.state {
        JobState::Queued => i18n::queued(l, title),
        JobState::Processing => i18n::processing(l, title),
        _ => i18n::progress(l, title, job.progress, job.speed, job.eta),
    }
}
