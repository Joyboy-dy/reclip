//! Per-visitor download history: the owner's finished jobs, hidden on request.
//!
//! Timestamps passed in must come from [`super::now`] / [`super::now_minus`].

use std::path::{Path, PathBuf};

use anyhow::Result;
use reclip_shared::{HistoryEntry, JobState};
use sqlx::Row;

use super::{Store, from_wire, now, playlist_item};

/// Most entries the history returns.
const HISTORY_LIMIT: i64 = 500;

/// Rows the history shows, bound as (owner, since): the owner's `done` jobs
/// and those that expired after finishing (`filename` is only set by a
/// `done` outcome and survives expiry), not hidden, created at or after
/// `since`.
const VISIBLE: &str = "owner = ? AND state IN ('done', 'expired') AND filename IS NOT NULL \
                       AND hidden_at IS NULL AND created_at >= ?";

impl Store {
    /// The owner's history since `since`, newest first, each entry with its
    /// file path: `file_available` is only set for a `done` job with a path,
    /// the caller still has to check that the file exists.
    pub async fn history(
        &self,
        owner: &str,
        since: &str,
    ) -> Result<Vec<(HistoryEntry, Option<PathBuf>)>> {
        let rows = sqlx::query(&format!(
            "SELECT id, url, title, format, format_id, size, state, created_at, expires_at, \
             file_path, thumb_path, playlist_item FROM jobs WHERE {VISIBLE} \
             ORDER BY created_at DESC, id DESC LIMIT ?"
        ))
        .bind(owner)
        .bind(since)
        .bind(HISTORY_LIMIT)
        .fetch_all(&self.0)
        .await?;
        rows.iter()
            .map(|row| {
                let state: JobState = from_wire(&row.try_get::<String, _>("state")?)?;
                let path: Option<String> = row.try_get("file_path")?;
                let thumb: Option<String> = row.try_get("thumb_path")?;
                let entry = HistoryEntry {
                    id: row.try_get("id")?,
                    url: row.try_get("url")?,
                    title: row.try_get("title")?,
                    format: from_wire(&row.try_get::<String, _>("format")?)?,
                    format_id: row.try_get("format_id")?,
                    size: row.try_get::<Option<i64>, _>("size")?.map(|n| n as u64),
                    has_thumbnail: thumb.is_some(),
                    file_available: state == JobState::Done && path.is_some(),
                    created_at: row.try_get("created_at")?,
                    expires_at: row.try_get("expires_at")?,
                    playlist_item: playlist_item(row)?,
                };
                Ok((entry, path.map(PathBuf::from)))
            })
            .collect()
    }

    /// Hide one history entry. `false` if the owner has no such visible entry.
    pub async fn hide(&self, owner: &str, since: &str, id: &str) -> Result<bool> {
        let res = sqlx::query(&format!(
            "UPDATE jobs SET hidden_at = ? WHERE {VISIBLE} AND id = ?"
        ))
        .bind(now())
        .bind(owner)
        .bind(since)
        .bind(id)
        .execute(&self.0)
        .await?;
        Ok(res.rows_affected() == 1)
    }

    /// Hide the owner's visible entries: only `expired` ones if
    /// `expired_only`, only those created before `created_before` if set.
    /// Returns how many were hidden.
    pub async fn hide_many(
        &self,
        owner: &str,
        since: &str,
        expired_only: bool,
        created_before: Option<&str>,
    ) -> Result<u64> {
        let res = sqlx::query(&format!(
            "UPDATE jobs SET hidden_at = ? WHERE {VISIBLE} \
             AND (? = 0 OR state = 'expired') AND (? IS NULL OR created_at < ?)"
        ))
        .bind(now())
        .bind(owner)
        .bind(since)
        .bind(expired_only)
        .bind(created_before)
        .bind(created_before)
        .execute(&self.0)
        .await?;
        Ok(res.rows_affected())
    }

    /// The thumbnail of one of the owner's jobs, if it has one.
    pub async fn thumb_path(&self, owner: &str, id: &str) -> Result<Option<PathBuf>> {
        let path: Option<Option<String>> =
            sqlx::query_scalar("SELECT thumb_path FROM jobs WHERE id = ? AND owner = ?")
                .bind(id)
                .bind(owner)
                .fetch_optional(&self.0)
                .await?;
        Ok(path.flatten().map(PathBuf::from))
    }

    pub async fn set_thumb_path(&self, id: &str, path: &Path) -> Result<()> {
        sqlx::query("UPDATE jobs SET thumb_path = ? WHERE id = ?")
            .bind(path.to_string_lossy().into_owned())
            .bind(id)
            .execute(&self.0)
            .await?;
        Ok(())
    }
}
