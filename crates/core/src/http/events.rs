//! `GET /api/v1/jobs/{id}/events`: live job updates as server-sent events.
//!
//! Each event is `event: job` with the `Job` JSON as data. The first one is
//! the current snapshot; the stream ends after a terminal state, or when the
//! server shuts down (graceful shutdown waits for every open response).

use std::convert::Infallible;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::{HeaderName, HeaderValue};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{AppendHeaders, IntoResponse};
use futures_util::stream::{self, Stream, StreamExt};
use reclip_shared::{Job, JobState, Scope};
use tokio::sync::broadcast::{Receiver, error::RecvError};

use crate::access::caller::Caller;
use crate::app::AppState;
use crate::jobs::{JobManager, ManagerError};

fn is_terminal(job: &Job) -> bool {
    matches!(
        job.state,
        JobState::Done | JobState::Failed | JobState::Cancelled | JobState::Expired
    )
}

fn job_event(job: &Job) -> Result<Event, Infallible> {
    Ok(Event::default()
        .event("job")
        .json_data(job)
        .unwrap_or_else(|_| Event::default().event("job").data("{}")))
}

enum Phase {
    Snapshot(Box<Job>, Option<Receiver<Job>>),
    Updates(Receiver<Job>),
    Done,
}

struct Ctx {
    jobs: JobManager,
    owner: String,
    id: String,
}

fn updates(ctx: Ctx, first: Phase) -> impl Stream<Item = Result<Event, Infallible>> {
    stream::unfold((ctx, first), |(ctx, phase)| async move {
        match phase {
            Phase::Done => None,
            Phase::Snapshot(job, rx) => {
                let next = match rx {
                    Some(rx) if !is_terminal(&job) => Phase::Updates(rx),
                    _ => Phase::Done,
                };
                Some((job_event(&job), (ctx, next)))
            }
            Phase::Updates(mut rx) => {
                let job = match rx.recv().await {
                    Ok(job) => job,
                    Err(RecvError::Closed) => return None,
                    Err(RecvError::Lagged(_)) => ctx.jobs.get(&ctx.owner, &ctx.id).await.ok()?,
                };
                let next = if is_terminal(&job) {
                    Phase::Done
                } else {
                    Phase::Updates(rx)
                };
                Some((job_event(&job), (ctx, next)))
            }
        }
    })
}

pub async fn events(
    State(s): State<AppState>,
    caller: Caller,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ManagerError> {
    caller.require(Scope::JobsRead)?;
    let sub = s.jobs.subscribe(&caller.owner, &id).await?;
    let stop = s.jobs.shutdown_token().cancelled_owned();
    let ctx = Ctx {
        jobs: s.jobs.clone(),
        owner: caller.owner,
        id,
    };
    let body = updates(ctx, Phase::Snapshot(Box::new(sub.snapshot), sub.updates)).take_until(stop);
    let sse = Sse::new(body).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)));
    Ok((
        AppendHeaders([(
            HeaderName::from_static("x-accel-buffering"),
            HeaderValue::from_static("no"),
        )]),
        sse,
    ))
}
