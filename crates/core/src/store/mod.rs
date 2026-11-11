//! SQLite-backed job store.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result};
use reclip_shared::api::{Job, JobState};
use reclip_shared::errors::ApiError;
use serde::Serialize;
use serde::de::DeserializeOwned;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow};
use sqlx::{Row, SqlitePool};

#[cfg(test)]
mod tests;

const COLUMNS: &str = "id, url, format, title, state, filename, size, \
                       error_code, error_message, created_at";

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

fn is_terminal(state: JobState) -> bool {
    matches!(
        state,
        JobState::Done | JobState::Cancelled | JobState::Failed | JobState::Expired
    )
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339()
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
    Ok(Job {
        id: row.try_get("id")?,
        url: row.try_get("url")?,
        format: from_wire(&row.try_get::<String, _>("format")?)?,
        title: row.try_get("title")?,
        state: from_wire(&row.try_get::<String, _>("state")?)?,
        filename: row.try_get("filename")?,
        size: row.try_get::<Option<i64>, _>("size")?.map(|n| n as u64),
        error,
        created_at: row.try_get("created_at")?,
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

    pub async fn insert(&self, job: &Job) -> Result<()> {
        sqlx::query(
            "INSERT INTO jobs (id, url, format, title, state, filename, size, \
             error_code, error_message, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&job.id)
        .bind(&job.url)
        .bind(to_wire(&job.format)?)
        .bind(&job.title)
        .bind(to_wire(&job.state)?)
        .bind(&job.filename)
        .bind(job.size.map(|n| n as i64))
        .bind(job.error.as_ref().map(|e| to_wire(&e.code)).transpose()?)
        .bind(job.error.as_ref().map(|e| e.message.as_str()))
        .bind(&job.created_at)
        .execute(&self.0)
        .await?;
        Ok(())
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
    /// - Terminal states stamp `finished_at`.
    pub async fn set_state(
        &self,
        id: &str,
        state: JobState,
        filename: Option<&str>,
        size: Option<u64>,
        error: Option<&ApiError>,
    ) -> Result<()> {
        let finished_at = is_terminal(state).then(now);
        sqlx::query(
            "UPDATE jobs SET state = ?, \
             filename = COALESCE(?, filename), \
             size = COALESCE(?, size), \
             error_code = ?, \
             error_message = ?, \
             finished_at = COALESCE(?, finished_at) \
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
        Ok(())
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
