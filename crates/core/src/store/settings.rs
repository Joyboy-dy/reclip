//! Key-value settings (generated secrets and other instance state).

use anyhow::Result;

use super::Store;

impl Store {
    pub async fn setting(&self, k: &str) -> Result<Option<String>> {
        Ok(
            sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
                .bind(k)
                .fetch_optional(&self.0)
                .await?,
        )
    }

    pub async fn set_setting(&self, k: &str, v: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(k)
        .bind(v)
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// Remove `k`. Returns whether it was set.
    pub async fn delete_setting(&self, k: &str) -> Result<bool> {
        let res = sqlx::query("DELETE FROM settings WHERE key = ?")
            .bind(k)
            .execute(&self.0)
            .await?;
        Ok(res.rows_affected() == 1)
    }

    /// Store `v` only when `k` is unset. Returns whether this call wrote it.
    pub async fn set_setting_if_absent(&self, k: &str, v: &str) -> Result<bool> {
        let res =
            sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT DO NOTHING")
                .bind(k)
                .bind(v)
                .execute(&self.0)
                .await?;
        Ok(res.rows_affected() == 1)
    }
}
