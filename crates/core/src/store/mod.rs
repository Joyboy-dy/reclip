//! SQLite-backed job store.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result};
use reclip_shared::api::{Clip, Job, JobState};
use reclip_shared::errors::{ApiError, ErrorCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow};
use sqlx::{Row, SqlitePool};

mod admin;
mod history;
mod keys;
mod queries;
mod settings;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_access;
mod time;

const COLUMNS: &str = "id, url, format, format_id, title, thumbnail, state, filename, size, \
                       error_code, error_message, created_at, expires_at, clip_start, clip_end, \
                       strip_metadata";

use time::finished_and_expiry;
pub use time::{now, now_minus, now_plus};

/// Persistent job storage. Cheap to clone (wraps a connection pool).
#[derive(Clone)]
pub struct Store(SqlitePool);

/// Serialize a unit-variant enum to its serde wire string.
fn to_wire<T: Serialize>(value: &T) -> Result<String> {
    match serde_json::to_value(value)? {
        serde_json::Value::String(s) => Ok(s),
        other => anyhow::bail!("expected string wire value, got {other}"),
    }
}

fn from_wire<T: DeserializeOwned>(s: &str) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(s.to_owned()))
        .with_context(|| format!("unknown wire value {s:?}"))
}

/// `done`, `cancelled`, `failed` and `expired`: no further work happens.
pub fn is_terminal(state: JobState) -> bool {
    matches!(
        state,
        JobState::Done | JobState::Cancelled | JobState::Failed | JobState::Expired
    )
}

/// A cancelled job, or one that failed by interruption or timeout, can be
/// resumed until it expires.
fn is_resumable(state: JobState, error: Option<&ApiError>, expires_at: Option<&str>) -> bool {
    let stopped = match state {
        JobState::Cancelled => true,
        JobState::Failed => {
            error.is_some_and(|e| matches!(e.code, ErrorCode::Interrupted | ErrorCode::Timeout))
        }
        _ => false,
    };
    stopped && expires_at.is_none_or(|at| at > now().as_str())
}

fn row_to_job(row: &SqliteRow) -> Result<Job> {
    let error = match row.try_get::<Option<String>, _>("error_code")? {
        Some(code) => Some(ApiError {
            code: from_wire(&code)?,
            message: row
                .try_get::<Option<String>, _>("error_message")?
                .unwrap_or_default(),
        }),
        None => None,
    };
    let state: JobState = from_wire(&row.try_get::<String, _>("state")?)?;
    let expires_at: Option<String> = row.try_get("expires_at")?;
    let clip = row
        .try_get::<Option<f64>, _>("clip_start")?
        .map(|start| -> Result<Clip> {
            Ok(Clip {
                start,
                end: row.try_get("clip_end")?,
            })
        })
        .transpose()?;
    Ok(Job {
        id: row.try_get("id")?,
        url: row.try_get("url")?,
        format: from_wire(&row.try_get::<String, _>("format")?)?,
        format_id: row.try_get("format_id")?,
        title: row.try_get("title")?,
        thumbnail: row.try_get("thumbnail")?,
        state,
        filename: row.try_get("filename")?,
        size: row.try_get::<Option<i64>, _>("size")?.map(|n| n as u64),
        resumable: is_resumable(state, error.as_ref(), expires_at.as_deref()),
        error,
        progress: None,
        speed: None,
        eta: None,
        created_at: row.try_get("created_at")?,
        expires_at,
        clip,
        strip_metadata: row.try_get("strip_metadata")?,
    })
}

impl Store {
    /// Open (creating if needed) the database at `path` and run migrations.
    pub async fn open(path: &Path) -> Result<Store> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .with_context(|| format!("opening {}", path.display()))?;
        sqlx::migrate!().run(&pool).await?;
        Ok(Store(pool))
    }

    /// In-memory store on a single connection (tests).
    pub async fn memory() -> Store {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .expect("valid memory url")
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(options)
            .await
            .expect("in-memory sqlite");
        sqlx::migrate!().run(&pool).await.expect("migrations apply");
        Store(pool)
    }

    /// Insert a job for `owner` (e.g. `web:<hex>`; `""` for legacy rows),
    /// counted against `quota_key` (`""` for none).
    pub async fn insert(&self, job: &Job, owner: &str, quota_key: &str) -> Result<()> {
        self.insert_item(job, owner, quota_key, None).await
    }

    /// [`Store::insert`] for one `playlist_item` (1-based) of a multi-video post.
    pub async fn insert_item(
        &self,
        job: &Job,
        owner: &str,
        quota_key: &str,
        playlist_item: Option<u32>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO jobs (id, url, format, format_id, title, thumbnail, state, \
             filename, size, error_code, error_message, created_at, expires_at, owner, quota_key, \
             clip_start, clip_end, strip_metadata, playlist_item) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&job.id)
        .bind(&job.url)
        .bind(to_wire(&job.format)?)
        .bind(&job.format_id)
        .bind(&job.title)
        .bind(&job.thumbnail)
        .bind(to_wire(&job.state)?)
        .bind(&job.filename)
        .bind(job.size.map(|n| n as i64))
        .bind(job.error.as_ref().map(|e| to_wire(&e.code)).transpose()?)
        .bind(job.error.as_ref().map(|e| e.message.as_str()))
        .bind(&job.created_at)
        .bind(&job.expires_at)
        .bind(owner)
        .bind(quota_key)
        .bind(job.clip.map(|c| c.start))
        .bind(job.clip.and_then(|c| c.end))
        .bind(job.strip_metadata)
        .bind(playlist_item.map(i64::from))
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// The 1-based playlist item a job downloads, if any.
    pub async fn playlist_item(&self, id: &str) -> Result<Option<u32>> {
        let n: Option<Option<i64>> =
            sqlx::query_scalar("SELECT playlist_item FROM jobs WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.0)
                .await?;
        Ok(n.flatten().and_then(|n| u32::try_from(n).ok()))
    }

    pub async fn get(&self, id: &str) -> Result<Option<Job>> {
        let row = sqlx::query(&format!("SELECT {COLUMNS} FROM jobs WHERE id = ?"))
            .bind(id)
            .fetch_optional(&self.0)
            .await?;
        row.as_ref().map(row_to_job).transpose()
    }

    /// Update a job's state.
    ///
    /// - `filename` / `size`: `None` keeps the stored value.
    /// - `error`: authoritative. `Some` stores it, `None` clears any stored error.
    /// - Terminal states stamp `finished_at`; non-terminal states clear it.
    /// - An unknown `id` is an error ("job not found").
    pub async fn set_state(
        &self,
        id: &str,
        state: JobState,
        filename: Option<&str>,
        size: Option<u64>,
        error: Option<&ApiError>,
    ) -> Result<()> {
        let finished_at = is_terminal(state).then(now);
        let res = sqlx::query(
            "UPDATE jobs SET state = ?, \
             filename = COALESCE(?, filename), \
             size = COALESCE(?, size), \
             error_code = ?, \
             error_message = ?, \
             finished_at = ? \
             WHERE id = ?",
        )
        .bind(to_wire(&state)?)
        .bind(filename)
        .bind(size.map(|n| n as i64))
        .bind(error.map(|e| to_wire(&e.code)).transpose()?)
        .bind(error.map(|e| e.message.as_str()))
        .bind(finished_at)
        .bind(id)
        .execute(&self.0)
        .await?;
        anyhow::ensure!(res.rows_affected() == 1, "job not found: {id}");
        Ok(())
    }

    /// Record a terminal outcome: same `filename` / `size` / `error` rules as
    /// [`Store::set_state`], with `finished_at = now` and
    /// `expires_at = finished_at + ttl` in the same update, so a finished
    /// row never lacks an expiry. Returns `expires_at`.
    pub async fn set_finished(
        &self,
        id: &str,
        state: JobState,
        filename: Option<&str>,
        size: Option<u64>,
        error: Option<&ApiError>,
        ttl: std::time::Duration,
    ) -> Result<String> {
        anyhow::ensure!(is_terminal(state), "not a terminal state");
        let (finished_at, expires_at) = finished_and_expiry(ttl);
        let res = sqlx::query(
            "UPDATE jobs SET state = ?, \
             filename = COALESCE(?, filename), \
             size = COALESCE(?, size), \
             error_code = ?, \
             error_message = ?, \
             finished_at = ?, \
             expires_at = ? \
             WHERE id = ?",
        )
        .bind(to_wire(&state)?)
        .bind(filename)
        .bind(size.map(|n| n as i64))
        .bind(error.map(|e| to_wire(&e.code)).transpose()?)
        .bind(error.map(|e| e.message.as_str()))
        .bind(finished_at)
        .bind(&expires_at)
        .bind(id)
        .execute(&self.0)
        .await?;
        anyhow::ensure!(res.rows_affected() == 1, "job not found: {id}");
        Ok(expires_at)
    }

    pub async fn file_path(&self, id: &str) -> Result<Option<PathBuf>> {
        let path: Option<Option<String>> =
            sqlx::query_scalar("SELECT file_path FROM jobs WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.0)
                .await?;
        Ok(path.flatten().map(PathBuf::from))
    }

    pub async fn set_file_path(&self, id: &str, path: &Path) -> Result<()> {
        sqlx::query("UPDATE jobs SET file_path = ? WHERE id = ?")
            .bind(path.to_string_lossy().into_owned())
            .bind(id)
            .execute(&self.0)
            .await?;
        Ok(())
    }

    #[cfg(test)]
    async fn finished_at(&self, id: &str) -> Option<String> {
        sqlx::query_scalar::<_, Option<String>>("SELECT finished_at FROM jobs WHERE id = ?")
            .bind(id)
            .fetch_one(&self.0)
            .await
            .unwrap()
    }
}
