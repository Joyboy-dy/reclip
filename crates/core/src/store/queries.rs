//! Owner-scoped and lifecycle queries.
//!
//! All timestamps passed in or stored must come from [`super::now`]
//! (UTC, millisecond RFC 3339 with `Z`), since SQL compares them as strings.

use std::path::PathBuf;

use anyhow::Result;
use reclip_shared::api::Job;
use reclip_shared::errors::ErrorCode;
use sqlx::Row;

use super::{COLUMNS, Store, now, row_to_job, to_wire};

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

    pub async fn delete(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM jobs WHERE id = ?")
            .bind(id)
            .execute(&self.0)
            .await?;
        Ok(())
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

    /// Fail every `downloading`/`processing` job (server restart recovery).
    /// Returns the affected ids.
    pub async fn mark_interrupted(&self) -> Result<Vec<String>> {
        let ids = sqlx::query_scalar(
            "UPDATE jobs SET state = 'failed', error_code = ?, error_message = ?, \
             finished_at = ? WHERE state IN ('downloading', 'processing') RETURNING id",
        )
        .bind(to_wire(&ErrorCode::Interrupted)?)
        .bind("Interrupted by a server restart")
        .bind(now())
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

    /// Move `done` jobs with `expires_at <= now` to `expired`, clearing their
    /// file path. Returns each id with its previous path.
    pub async fn expire_due(&self, now_ts: &str) -> Result<Vec<(String, Option<PathBuf>)>> {
        let mut tx = self.0.begin().await?;
        let due: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT id, file_path FROM jobs WHERE state = 'done' \
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

    /// Delete `expired` rows whose `finished_at` is before `before`.
    pub async fn purge_expired(&self, before: &str) -> Result<u64> {
        let res = sqlx::query("DELETE FROM jobs WHERE state = 'expired' AND finished_at < ?")
            .bind(before)
            .execute(&self.0)
            .await?;
        Ok(res.rows_affected())
    }
}
