//! Owner-scoped and lifecycle queries.
//!
//! All timestamps passed in or stored must come from [`super::now`]
//! (UTC, millisecond RFC 3339 with `Z`), since SQL compares them as strings.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use reclip_shared::api::Job;
use reclip_shared::errors::ErrorCode;
use sqlx::Row;

use super::{COLUMNS, Store, finished_and_expiry, now, row_to_job, to_wire};

impl Store {
    /// A job, only if it belongs to `owner`.
    pub async fn get_owned(&self, id: &str, owner: &str) -> Result<Option<Job>> {
        let row = sqlx::query(&format!(
            "SELECT {COLUMNS} FROM jobs WHERE id = ? AND owner = ?"
        ))
        .bind(id)
        .bind(owner)
        .fetch_optional(&self.0)
        .await?;
        row.as_ref().map(row_to_job).transpose()
    }

    /// Most recent jobs of `owner`, newest first, excluding `expired`.
    pub async fn list_by_owner(&self, owner: &str, limit: u32) -> Result<Vec<Job>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLUMNS} FROM jobs WHERE owner = ? AND state != 'expired' \
             ORDER BY created_at DESC, id DESC LIMIT ?"
        ))
        .bind(owner)
        .bind(i64::from(limit))
        .fetch_all(&self.0)
        .await?;
        rows.iter().map(row_to_job).collect()
    }

    pub async fn owner_of(&self, id: &str) -> Result<Option<String>> {
        Ok(sqlx::query_scalar("SELECT owner FROM jobs WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.0)
            .await?)
    }

    /// Delete a job row. Returns its thumbnail, for the caller to remove.
    pub async fn delete(&self, id: &str) -> Result<Option<PathBuf>> {
        let thumb: Option<Option<String>> =
            sqlx::query_scalar("DELETE FROM jobs WHERE id = ? RETURNING thumb_path")
                .bind(id)
                .fetch_optional(&self.0)
                .await?;
        Ok(thumb.flatten().map(PathBuf::from))
    }

    /// Queued jobs with their owner, in creation order.
    pub async fn queued(&self) -> Result<Vec<(Job, String)>> {
        let rows = sqlx::query(&format!(
            "SELECT {COLUMNS}, owner FROM jobs WHERE state = 'queued' \
             ORDER BY created_at, id"
        ))
        .fetch_all(&self.0)
        .await?;
        rows.iter()
            .map(|r| Ok((row_to_job(r)?, r.try_get("owner")?)))
            .collect()
    }

    /// Ids of every job whose files are kept: `done` jobs and stopped
    /// downloads that can be resumed (`cancelled`, `failed` by interruption
    /// or timeout). Expiry is left to the sweep.
    pub async fn kept_ids(&self) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT id FROM jobs WHERE state IN ('done', 'cancelled') \
             OR (state = 'failed' AND error_code IN (?, ?))",
        )
        .bind(to_wire(&ErrorCode::Interrupted)?)
        .bind(to_wire(&ErrorCode::Timeout)?)
        .fetch_all(&self.0)
        .await?)
    }

    /// Fail every `downloading`/`processing` job (server restart recovery),
    /// expiring `ttl` after now like any other finished job. Returns the
    /// affected ids.
    pub async fn mark_interrupted(&self, ttl: Duration) -> Result<Vec<String>> {
        let (finished_at, expires_at) = finished_and_expiry(ttl);
        let ids = sqlx::query_scalar(
            "UPDATE jobs SET state = 'failed', error_code = ?, error_message = ?, \
             finished_at = ?, expires_at = ? \
             WHERE state IN ('downloading', 'processing') RETURNING id",
        )
        .bind(to_wire(&ErrorCode::Interrupted)?)
        .bind("Interrupted by a server restart")
        .bind(finished_at)
        .bind(expires_at)
        .fetch_all(&self.0)
        .await?;
        Ok(ids)
    }

    pub async fn set_expires_at(&self, id: &str, at: &str) -> Result<()> {
        sqlx::query("UPDATE jobs SET expires_at = ? WHERE id = ?")
            .bind(at)
            .bind(id)
            .execute(&self.0)
            .await?;
        Ok(())
    }

    /// Move `done`, `failed` and `cancelled` jobs with `expires_at <= now` to
    /// `expired`, clearing their file path. Returns each id with its previous
    /// path (only a `done` job has one: the others' directories are removed
    /// when they finish).
    pub async fn expire_due(&self, now_ts: &str) -> Result<Vec<(String, Option<PathBuf>)>> {
        let mut tx = self.0.begin().await?;
        let due: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT id, file_path FROM jobs WHERE state IN ('done', 'failed', 'cancelled') \
             AND expires_at IS NOT NULL AND expires_at <= ?",
        )
        .bind(now_ts)
        .fetch_all(&mut *tx)
        .await?;
        for (id, _) in &due {
            sqlx::query(
                "UPDATE jobs SET state = 'expired', file_path = NULL, finished_at = ? \
                 WHERE id = ?",
            )
            .bind(now())
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(due
            .into_iter()
            .map(|(id, p)| (id, p.map(PathBuf::from)))
            .collect())
    }

    /// Delete `expired` rows whose `finished_at` is before `before`. Returns
    /// one entry per deleted row: its thumbnail, for the caller to remove.
    pub async fn purge_expired(&self, before: &str) -> Result<Vec<Option<PathBuf>>> {
        let thumbs: Vec<Option<String>> = sqlx::query_scalar(
            "DELETE FROM jobs WHERE state = 'expired' AND finished_at < ? RETURNING thumb_path",
        )
        .bind(before)
        .fetch_all(&self.0)
        .await?;
        Ok(thumbs.into_iter().map(|t| t.map(PathBuf::from)).collect())
    }

    /// Put a resumable job (see [`super::is_resumable`]) back to `queued`,
    /// clearing its outcome. `false` if it is not resumable (any more).
    pub async fn requeue(&self, id: &str) -> Result<bool> {
        let res = sqlx::query(
            "UPDATE jobs SET state = 'queued', error_code = NULL, error_message = NULL, \
             filename = NULL, size = NULL, file_path = NULL, finished_at = NULL, \
             expires_at = NULL \
             WHERE id = ? AND (state = 'cancelled' OR (state = 'failed' AND error_code IN (?, ?))) \
             AND (expires_at IS NULL OR expires_at > ?)",
        )
        .bind(id)
        .bind(to_wire(&ErrorCode::Interrupted)?)
        .bind(to_wire(&ErrorCode::Timeout)?)
        .bind(now())
        .execute(&self.0)
        .await?;
        Ok(res.rows_affected() == 1)
    }

    /// Finished jobs that may still hold files, in eviction order: stopped
    /// downloads (`cancelled`, `failed`) first, then `done`, oldest first.
    pub async fn eviction_candidates(&self) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(
            "SELECT id FROM jobs WHERE state IN ('cancelled', 'failed', 'done') \
             ORDER BY state = 'done', finished_at, id",
        )
        .fetch_all(&self.0)
        .await?)
    }

    /// Expire one finished job now, like [`Store::expire_due`] would.
    /// `false` if it is no longer `done`, `failed` or `cancelled`.
    pub async fn expire_now(&self, id: &str) -> Result<bool> {
        let res = sqlx::query(
            "UPDATE jobs SET state = 'expired', file_path = NULL, finished_at = ? \
             WHERE id = ? AND state IN ('done', 'failed', 'cancelled')",
        )
        .bind(now())
        .bind(id)
        .execute(&self.0)
        .await?;
        Ok(res.rows_affected() == 1)
    }
}
