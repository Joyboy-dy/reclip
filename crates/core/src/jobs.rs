//! Job creation and background execution.

use std::path::Path;

use reclip_shared::{ApiError, CreateJobRequest, Job, JobState};

use crate::app::AppState;
use crate::http::internal;
use crate::ytdlp::parse::safe_filename;
use crate::ytdlp::runner;

/// Insert a queued job and run it in the background.
pub async fn spawn_job(
    state: AppState,
    req: CreateJobRequest,
    owner: &str,
) -> Result<Job, ApiError> {
    let job = Job {
        id: uuid::Uuid::now_v7().simple().to_string(),
        url: req.url,
        format: req.format,
        format_id: req.format_id.clone(),
        title: req.title,
        thumbnail: req.thumbnail,
        state: JobState::Queued,
        filename: None,
        size: None,
        error: None,
        progress: None,
        speed: None,
        eta: None,
        created_at: crate::store::now(),
        expires_at: None,
    };
    state
        .store
        .insert(&job, owner)
        .await
        .map_err(|e| internal(e).0)?;
    tokio::spawn(run_job(state, job.clone(), req.format_id));
    Ok(job)
}

async fn run_job(state: AppState, job: Job, format_id: Option<String>) {
    let store = &state.store;
    if let Err(e) = store
        .set_state(&job.id, JobState::Downloading, None, None, None)
        .await
    {
        tracing::error!(job = %job.id, error = %e, "cannot start job");
        return;
    }
    let dir = state.cfg.downloads_dir.join(&job.id);
    let result = match download(&state, &job, &dir, format_id.as_deref()).await {
        Ok((filename, size)) => {
            store
                .set_state(&job.id, JobState::Done, Some(&filename), Some(size), None)
                .await
        }
        Err(err) => {
            let _ = tokio::fs::remove_dir_all(&dir).await;
            store
                .set_state(&job.id, JobState::Failed, None, None, Some(&err))
                .await
        }
    };
    if let Err(e) = result {
        tracing::error!(job = %job.id, error = %e, "cannot record job result");
    }
}

async fn download(
    state: &AppState,
    job: &Job,
    dir: &Path,
    format_id: Option<&str>,
) -> Result<(String, u64), ApiError> {
    let io_err = |e: std::io::Error| internal(e).0;
    tokio::fs::create_dir_all(dir).await.map_err(io_err)?;
    // Progress and cancellation are wired by the job manager (next task).
    let path = runner::download(
        &state.cfg,
        dir,
        job.format,
        format_id,
        &job.url,
        |_| {},
        tokio_util::sync::CancellationToken::new(),
    )
    .await?;
    let size = tokio::fs::metadata(&path).await.map_err(io_err)?.len();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("bin");
    let filename = safe_filename(job.title.as_deref().unwrap_or(""), ext, &job.id);
    state
        .store
        .set_file_path(&job.id, &path)
        .await
        .map_err(|e| internal(e).0)?;
    Ok((filename, size))
}
