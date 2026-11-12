//! API key storage and per-quota-key job counts.
//!
//! Only the SHA-256 of a key secret is stored, never the secret itself.

use anyhow::Result;
use reclip_shared::admin::{ApiKeyInfo, KeyLimits, Scope};
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use super::{Store, from_wire, now, to_wire};

const KEY_COLUMNS: &str = "id, name, scopes, unlimited, probes_per_minute, jobs_per_hour, \
                           max_active, created_at, expires_at, revoked_at, last_used_at, \
                           request_count";

fn opt_u32(row: &SqliteRow, col: &str) -> Result<Option<u32>> {
    Ok(row.try_get::<Option<i64>, _>(col)?.map(|n| n as u32))
}

fn row_to_key(row: &SqliteRow) -> Result<ApiKeyInfo> {
    let scopes: String = row.try_get("scopes")?;
    Ok(ApiKeyInfo {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        scopes: scopes
            .split(',')
            .filter(|s| !s.is_empty())
            .map(from_wire)
            .collect::<Result<Vec<Scope>>>()?,
        unlimited: row.try_get::<i64, _>("unlimited")? != 0,
        limits: KeyLimits {
            probes_per_minute: opt_u32(row, "probes_per_minute")?,
            jobs_per_hour: opt_u32(row, "jobs_per_hour")?,
            max_active: opt_u32(row, "max_active")?,
        },
        created_at: row.try_get("created_at")?,
        expires_at: row.try_get("expires_at")?,
        revoked_at: row.try_get("revoked_at")?,
        last_used_at: row.try_get("last_used_at")?,
        request_count: row.try_get::<i64, _>("request_count")? as u64,
    })
}

impl Store {
    /// Jobs of `quota_key` that are `queued`, `downloading` or `processing`.
    pub async fn count_active(&self, quota_key: &str) -> Result<u32> {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM jobs WHERE quota_key = ? \
             AND state IN ('queued', 'downloading', 'processing')",
        )
        .bind(quota_key)
        .fetch_one(&self.0)
        .await?;
        Ok(n as u32)
    }

    pub async fn insert_key(&self, info: &ApiKeyInfo, secret_sha256: &str) -> Result<()> {
        let scopes = info
            .scopes
            .iter()
            .map(to_wire)
            .collect::<Result<Vec<_>>>()?
            .join(",");
        sqlx::query(
            "INSERT INTO api_keys (id, name, secret_sha256, scopes, unlimited, \
             probes_per_minute, jobs_per_hour, max_active, created_at, expires_at, \
             revoked_at, last_used_at, request_count) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&info.id)
        .bind(&info.name)
        .bind(secret_sha256)
        .bind(scopes)
        .bind(info.unlimited)
        .bind(info.limits.probes_per_minute.map(i64::from))
        .bind(info.limits.jobs_per_hour.map(i64::from))
        .bind(info.limits.max_active.map(i64::from))
        .bind(&info.created_at)
        .bind(&info.expires_at)
        .bind(&info.revoked_at)
        .bind(&info.last_used_at)
        .bind(info.request_count as i64)
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// A key and the stored SHA-256 of its secret.
    pub async fn key_with_hash(&self, id: &str) -> Result<Option<(ApiKeyInfo, String)>> {
        let row = sqlx::query(&format!(
            "SELECT {KEY_COLUMNS}, secret_sha256 FROM api_keys WHERE id = ?"
        ))
        .bind(id)
        .fetch_optional(&self.0)
        .await?;
        row.as_ref()
            .map(|r| Ok((row_to_key(r)?, r.try_get("secret_sha256")?)))
            .transpose()
    }

    /// Every key, newest first.
    pub async fn list_keys(&self) -> Result<Vec<ApiKeyInfo>> {
        let rows = sqlx::query(&format!(
            "SELECT {KEY_COLUMNS} FROM api_keys ORDER BY created_at DESC, id DESC"
        ))
        .fetch_all(&self.0)
        .await?;
        rows.iter().map(row_to_key).collect()
    }

    /// Revoke a key. Returns `false` for an unknown id; revoking twice keeps
    /// the first `revoked_at`.
    pub async fn revoke_key(&self, id: &str) -> Result<bool> {
        let res =
            sqlx::query("UPDATE api_keys SET revoked_at = COALESCE(revoked_at, ?) WHERE id = ?")
                .bind(now())
                .bind(id)
                .execute(&self.0)
                .await?;
        Ok(res.rows_affected() == 1)
    }

    /// Record a use: `last_used_at = now`, `request_count + 1`.
    pub async fn touch_key(&self, id: &str) -> Result<()> {
        sqlx::query(
            "UPDATE api_keys SET last_used_at = ?, request_count = request_count + 1 \
             WHERE id = ?",
        )
        .bind(now())
        .bind(id)
        .execute(&self.0)
        .await?;
        Ok(())
    }
}
