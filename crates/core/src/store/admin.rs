//! Admin accounts and cookie sessions. Only the SHA-256 of a session token
//! is stored.

use anyhow::Result;
use sqlx::Row;

use super::{Store, now};

/// An admin account row.
#[derive(Debug, Clone)]
pub struct AdminAccount {
    pub email: String,
    /// `None` until the first sign-in sets a password.
    pub password_hash: Option<String>,
    pub must_change: bool,
}

impl Store {
    pub async fn admin_account(&self, email: &str) -> Result<Option<AdminAccount>> {
        let row = sqlx::query(
            "SELECT email, password_hash, must_change FROM admin_accounts WHERE email = ?",
        )
        .bind(email)
        .fetch_optional(&self.0)
        .await?;
        row.map(|r| {
            Ok(AdminAccount {
                email: r.try_get("email")?,
                password_hash: r.try_get("password_hash")?,
                must_change: r.try_get::<i64, _>("must_change")? != 0,
            })
        })
        .transpose()
    }

    /// First sign-in: store `hash` for an account that has no password yet
    /// (creating the account), with a pending password change. Returns
    /// whether this call set it; a concurrent first sign-in loses.
    pub async fn init_admin_password(&self, email: &str, hash: &str) -> Result<bool> {
        let at = now();
        sqlx::query(
            "INSERT OR IGNORE INTO admin_accounts \
             (email, password_hash, must_change, created_at, updated_at) VALUES (?, NULL, 1, ?, ?)",
        )
        .bind(email)
        .bind(&at)
        .bind(&at)
        .execute(&self.0)
        .await?;
        let res = sqlx::query(
            "UPDATE admin_accounts SET password_hash = ?, must_change = 1, updated_at = ? \
             WHERE email = ? AND password_hash IS NULL",
        )
        .bind(hash)
        .bind(&at)
        .bind(email)
        .execute(&self.0)
        .await?;
        Ok(res.rows_affected() == 1)
    }

    /// Replace the password and clear the pending change.
    pub async fn set_admin_password(&self, email: &str, hash: &str) -> Result<()> {
        sqlx::query(
            "UPDATE admin_accounts SET password_hash = ?, must_change = 0, updated_at = ? \
             WHERE email = ?",
        )
        .bind(hash)
        .bind(now())
        .bind(email)
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// Store a session and drop the expired ones.
    pub async fn insert_admin_session(
        &self,
        token_sha256: &str,
        email: &str,
        expires_at: &str,
    ) -> Result<()> {
        let at = now();
        sqlx::query("DELETE FROM admin_sessions WHERE expires_at <= ?")
            .bind(&at)
            .execute(&self.0)
            .await?;
        sqlx::query(
            "INSERT INTO admin_sessions (token_sha256, email, created_at, expires_at) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(token_sha256)
        .bind(email)
        .bind(&at)
        .bind(expires_at)
        .execute(&self.0)
        .await?;
        Ok(())
    }

    /// The account owning an unexpired session.
    pub async fn admin_session(&self, token_sha256: &str) -> Result<Option<AdminAccount>> {
        let row = sqlx::query(
            "SELECT a.email, a.password_hash, a.must_change FROM admin_sessions s \
             JOIN admin_accounts a ON a.email = s.email \
             WHERE s.token_sha256 = ? AND s.expires_at > ?",
        )
        .bind(token_sha256)
        .bind(now())
        .fetch_optional(&self.0)
        .await?;
        row.map(|r| {
            Ok(AdminAccount {
                email: r.try_get("email")?,
                password_hash: r.try_get("password_hash")?,
                must_change: r.try_get::<i64, _>("must_change")? != 0,
            })
        })
        .transpose()
    }

    pub async fn delete_admin_session(&self, token_sha256: &str) -> Result<()> {
        sqlx::query("DELETE FROM admin_sessions WHERE token_sha256 = ?")
            .bind(token_sha256)
            .execute(&self.0)
            .await?;
        Ok(())
    }

    /// End every session of `email` except `keep`.
    pub async fn delete_other_admin_sessions(&self, email: &str, keep: &str) -> Result<()> {
        sqlx::query("DELETE FROM admin_sessions WHERE email = ? AND token_sha256 <> ?")
            .bind(email)
            .bind(keep)
            .execute(&self.0)
            .await?;
        Ok(())
    }
}
