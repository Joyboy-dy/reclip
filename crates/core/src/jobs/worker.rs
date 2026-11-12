//! Runs one job: `downloading` with live progress, `processing`, then `done`
//! or `failed`/`cancelled` (directory removed), each with an expiry.

use std::path::Path;

use reclip_shared::{ApiError, ErrorCode, Job, JobState};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::progress_tracker::ProgressTracker;
use super::{JobManager, internal};
use crate::ytdlp::args::DownloadOptions;
use crate::ytdlp::parse::safe_filename;
use crate::ytdlp::progress::ProgressEvent;
use crate::ytdlp::runner;

/// Releases the worker's slot on every exit path, panics included.
///
/// If the worker ends without recording an outcome (panic, task dropped),
/// its subscribers are closed at once (`hub.forget`) and a task records
/// `failed` ("Job crashed") and removes the job dir *before* the slot is
/// freed: `delete`, which waits for the slot, still never races a write.
struct Release {
    mgr: JobManager,
    id: String,
    recorded: bool,
}

impl Drop for Release {
    fn drop(&mut self) {
        if self.recorded {
            self.mgr.release(&self.id);
            return;
        }
        tracing::error!(job = %self.id, "job worker crashed");
        self.mgr.hub().forget(&self.id);
        let guard = Release {
            mgr: self.mgr.clone(),
            id: self.id.clone(),
            recorded: true,
        };
        match tokio::runtime::Handle::try_current() {
            // `guard` frees the slot when this task ends or is dropped.
            Ok(rt) => drop(rt.spawn(async move { record_crash(&guard.mgr, &guard.id).await })),
            Err(_) => drop(guard),
        }
    }
}

async fn record_crash(mgr: &JobManager, id: &str) {
    remove_dir(id, &mgr.config().downloads_dir.join(id)).await;
    let error = ApiError {
        code: ErrorCode::Internal,
        message: "Job crashed".into(),
    };
    let ttl = mgr.config().file_ttl;
    let res = mgr
        .store()
        .set_finished(id, JobState::Failed, None, None, Some(&error), ttl)
        .await;
    if let Err(e) = res {
        tracing::error!(job = %id, error = %e, "cannot record job crash");
    }
}

async fn remove_dir(id: &str, dir: &Path) {
    if let Err(e) = tokio::fs::remove_dir_all(dir).await
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(job = %id, error = %e, "cannot remove job dir");
    }
}

/// Test hook: a worker for this URL panics before downloading.
#[cfg(test)]
pub(super) const PANIC_URL: &str = "https://panic.test/";

pub(super) async fn run(mgr: JobManager, job: Job, cancel: CancellationToken) {
    let mut release = Release {
        mgr: mgr.clone(),
        id: job.id.clone(),
        recorded: false,
    };
    #[cfg(test)]
    if job.url == PANIC_URL {
        panic!("injected worker panic");
    }
    let dir = mgr.config().downloads_dir.join(&job.id);
    let outcome = if cancel.is_cancelled() {
        Err(cancelled())
    } else {
        download(&mgr, &job, &dir, &cancel).await
    };
    finish(&mgr, job, &dir, outcome).await;
    release.recorded = true;
}

fn cancelled() -> ApiError {
    ApiError {
        code: ErrorCode::Cancelled,
        message: "Download cancelled".into(),
    }
}

/// Download into `dir`, publishing progress. Returns the filename and size.
async fn download(
    mgr: &JobManager,
    job: &Job,
    dir: &Path,
    cancel: &CancellationToken,
) -> Result<(String, u64), ApiError> {
    let store = mgr.store();
    store
        .set_state(&job.id, JobState::Downloading, None, None, None)
        .await
        .map_err(internal)?;
    let mut live = Job {
        state: JobState::Downloading,
        progress: Some(0.0),
        ..job.clone()
    };
    mgr.hub().publish(live.clone());
    tokio::fs::create_dir_all(dir).await.map_err(internal)?;

    let mut tracker = ProgressTracker::for_download(job.format, job.format_id.as_deref());
    let (tx, mut rx) = mpsc::unbounded_channel();
    let opts = DownloadOptions {
        format: job.format,
        format_id: job.format_id.clone(),
        clip: job.clip,
        strip_metadata: job.strip_metadata,
        playlist_item: store.playlist_item(&job.id).await.map_err(internal)?,
    };
    let download = runner::download_with(
        mgr.config(),
        dir,
        &opts,
        &job.url,
        move |event| {
            let _ = tx.send(event);
        },
        cancel.clone(),
    );
    // Polled to completion, never dropped early: cancellation goes through
    // the token so the runner can stop the whole process group.
    tokio::pin!(download);
    let result = loop {
        tokio::select! {
            result = &mut download => break result,
            Some(event) = rx.recv() => apply(mgr, &mut live, &mut tracker, event).await,
        }
    };
    let path = result?;
    while let Ok(event) = rx.try_recv() {
        apply(mgr, &mut live, &mut tracker, event).await;
    }

    let size = tokio::fs::metadata(&path).await.map_err(internal)?.len();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("bin");
    let filename = safe_filename(job.title.as_deref().unwrap_or(""), ext, &job.id);
    store
        .set_file_path(&job.id, &path)
        .await
        .map_err(internal)?;
    Ok((filename, size))
}

/// Fold one progress event into the live snapshot and publish it. The hub
/// throttles progress-only updates; the switch to `processing` is persisted.
async fn apply(mgr: &JobManager, live: &mut Job, tracker: &mut ProgressTracker, ev: ProgressEvent) {
    let shown = tracker.update(ev);
    live.progress = Some(shown.progress);
    live.speed = shown.speed;
    live.eta = shown.eta;
    if tracker.is_processing() && live.state != JobState::Processing {
        live.state = JobState::Processing;
        let res = mgr
            .store()
            .set_state(&live.id, JobState::Processing, None, None, None)
            .await;
        if let Err(e) = res {
            tracing::warn!(job = %live.id, error = %e, "cannot record processing");
        }
    }
    mgr.hub().publish(live.clone());
}

/// Persist the outcome with its expiry, then publish it (the hub drops
/// finished jobs, so the store must be up to date first).
async fn finish(mgr: &JobManager, job: Job, dir: &Path, outcome: Result<(String, u64), ApiError>) {
    let mut done = Job {
        progress: None,
        speed: None,
        eta: None,
        ..job
    };
    match outcome {
        Ok((filename, size)) => {
            done.state = JobState::Done;
            done.filename = Some(filename);
            done.size = Some(size);
        }
        Err(err) => {
            remove_dir(&done.id, dir).await;
            let (state, error) = match err.code {
                ErrorCode::Cancelled if mgr.stopping() => (JobState::Failed, Some(interrupted())),
                ErrorCode::Cancelled => (JobState::Cancelled, None),
                _ => (JobState::Failed, Some(err)),
            };
            done.state = state;
            done.error = error;
        }
    }
    let (name, size) = match done.state {
        JobState::Done => (done.filename.as_deref(), done.size),
        _ => (None, None),
    };
    let ttl = mgr.config().file_ttl;
    let written = mgr
        .store()
        .set_finished(&done.id, done.state, name, size, done.error.as_ref(), ttl)
        .await;
    match written {
        Ok(expires_at) => done.expires_at = Some(expires_at),
        Err(e) => tracing::error!(job = %done.id, error = %e, "cannot record job result"),
    }
    mgr.hub().publish(done);
}

fn interrupted() -> ApiError {
    ApiError {
        code: ErrorCode::Interrupted,
        message: "Interrupted by a server shutdown".into(),
    }
}
