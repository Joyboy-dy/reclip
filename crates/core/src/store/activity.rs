//! Activity (who, from where, how much), blocks, and the admin's figures.

use std::collections::HashMap;

use anyhow::Result;
use reclip_shared::{BlockInfo, BlockKind, SeenIp};
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use super::{Store, from_wire, now, to_wire};

/// Addresses kept per identity in the report.
const IPS_PER_CLIENT: usize = 10;

/// One identity as stored, with its job counts (see [`Store::clients`]).
#[derive(Debug, Clone, PartialEq)]
pub struct ClientRow {
    pub owner: String,
    pub device: Option<String>,
    pub tg_username: Option<String>,
    pub tg_name: Option<String>,
    pub first_seen: String,
    pub last_seen: String,
    pub last_ip: Option<String>,
    pub ips_day: u32,
    pub ips_total: u32,
    pub downloads_day: u32,
    pub downloads_week: u32,
    pub downloads_total: u32,
    pub bytes_total: u64,
}

/// Averages of the finished downloads (see [`Store::download_costs`]).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DownloadCosts {
    pub done: u32,
    pub avg_duration_ms: Option<f64>,
    pub avg_size: Option<f64>,
}

fn row_to_block(row: &SqliteRow) -> Result<BlockInfo> {
    let kind: String = row.try_get("kind")?;
    Ok(BlockInfo {
        id: row.try_get("id")?,
        kind: from_wire(&kind)?,
        target: row.try_get("target")?,
        reason: row.try_get("reason")?,
        created_at: row.try_get("created_at")?,
        created_by: row.try_get("created_by")?,
    })
}

fn u32_of(row: &SqliteRow, col: &str) -> Result<u32> {
    Ok(row.try_get::<Option<i64>, _>(col)?.unwrap_or(0).max(0) as u32)
}

impl Store {
    /// Record a request of `owner` now: device and Telegram profile when
    /// known (kept otherwise), and the address `ip` when there is one.
    pub async fn touch_client(
        &self,
        owner: &str,
        ip: Option<&str>,
        device: Option<&str>,
        tg_username: Option<&str>,
        tg_name: Option<&str>,
    ) -> Result<()> {
        let at = now();
        let mut tx = self.0.begin().await?;
        sqlx::query(
            "INSERT INTO clients (owner, device, tg_username, tg_name, first_seen, last_seen, last_ip) \
             VALUES (?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT (owner) DO UPDATE SET \
             device = COALESCE(excluded.device, device), \
             tg_username = COALESCE(excluded.tg_username, tg_username), \
             tg_name = COALESCE(excluded.tg_name, tg_name), \
             last_seen = excluded.last_seen, \
             last_ip = COALESCE(excluded.last_ip, last_ip)",
        )
        .bind(owner)
        .bind(device)
        .bind(tg_username)
        .bind(tg_name)
        .bind(&at)
        .bind(&at)
        .bind(ip)
        .execute(&mut *tx)
        .await?;
        if let Some(ip) = ip {
            sqlx::query(
                "INSERT INTO client_ips (owner, ip, first_seen, last_seen) VALUES (?, ?, ?, ?) \
                 ON CONFLICT (owner, ip) DO UPDATE SET last_seen = excluded.last_seen, hits = hits + 1",
            )
            .bind(owner)
            .bind(ip)
            .bind(&at)
            .bind(&at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Where a job came from.
    pub async fn set_job_origin(
        &self,
        id: &str,
        ip: Option<&str>,
        device: Option<&str>,
    ) -> Result<()> {
        sqlx::query("UPDATE jobs SET ip = ?, device = ? WHERE id = ?")
            .bind(ip)
            .bind(device)
            .bind(id)
            .execute(&self.0)
            .await?;
        Ok(())
    }

    /// How long a job ran, from the start of its download to its outcome.
    pub async fn set_job_duration(&self, id: &str, ms: u64) -> Result<()> {
        sqlx::query("UPDATE jobs SET duration_ms = ? WHERE id = ?")
            .bind(i64::try_from(ms).unwrap_or(i64::MAX))
            .bind(id)
            .execute(&self.0)
            .await?;
        Ok(())
    }

    /// Forget identities and addresses not seen since `before`.
    pub async fn purge_activity(&self, before: &str) -> Result<u64> {
        let ips = sqlx::query("DELETE FROM client_ips WHERE last_seen < ?")
            .bind(before)
            .execute(&self.0)
            .await?;
        let clients = sqlx::query("DELETE FROM clients WHERE last_seen < ?")
            .bind(before)
            .execute(&self.0)
            .await?;
        Ok(ips.rows_affected() + clients.rows_affected())
    }

    /// Identities, most recently seen first, with their job
    /// counts since `day` and `week` (and in all), at most `limit`.
    pub async fn clients(&self, day: &str, week: &str, limit: u32) -> Result<Vec<ClientRow>> {
        let rows = sqlx::query(
            "SELECT c.owner, c.device, c.tg_username, c.tg_name, c.first_seen, c.last_seen, c.last_ip, \
             (SELECT COUNT(*) FROM client_ips i WHERE i.owner = c.owner AND i.last_seen >= ?1) AS ips_day, \
             (SELECT COUNT(*) FROM client_ips i WHERE i.owner = c.owner) AS ips_total, \
             j.n_day, j.n_week, j.n_total, j.bytes \
             FROM clients c LEFT JOIN ( \
               SELECT owner, SUM(created_at >= ?1) AS n_day, SUM(created_at >= ?2) AS n_week, \
               COUNT(*) AS n_total, \
               SUM(CASE WHEN state IN ('done', 'expired') THEN COALESCE(size, 0) ELSE 0 END) AS bytes \
               FROM jobs GROUP BY owner \
             ) j ON j.owner = c.owner \
             ORDER BY c.last_seen DESC LIMIT ?3",
        )
        .bind(day)
        .bind(week)
        .bind(i64::from(limit))
        .fetch_all(&self.0)
        .await?;
        rows.iter()
            .map(|r| {
                Ok(ClientRow {
                    owner: r.try_get("owner")?,
                    device: r.try_get("device")?,
                    tg_username: r.try_get("tg_username")?,
                    tg_name: r.try_get("tg_name")?,
                    first_seen: r.try_get("first_seen")?,
                    last_seen: r.try_get("last_seen")?,
                    last_ip: r.try_get("last_ip")?,
                    ips_day: u32_of(r, "ips_day")?,
                    ips_total: u32_of(r, "ips_total")?,
                    downloads_day: u32_of(r, "n_day")?,
                    downloads_week: u32_of(r, "n_week")?,
                    downloads_total: u32_of(r, "n_total")?,
                    bytes_total: r.try_get::<Option<i64>, _>("bytes")?.unwrap_or(0).max(0) as u64,
                })
            })
            .collect()
    }

    /// The most recent addresses of each identity, at most 10 each.
    pub async fn client_ips(&self) -> Result<HashMap<String, Vec<SeenIp>>> {
        let rows = sqlx::query(
            "SELECT owner, ip, last_seen, hits FROM client_ips ORDER BY last_seen DESC",
        )
        .fetch_all(&self.0)
        .await?;
        let mut out: HashMap<String, Vec<SeenIp>> = HashMap::new();
        for r in &rows {
            let list = out.entry(r.try_get("owner")?).or_default();
            if list.len() < IPS_PER_CLIENT {
                list.push(SeenIp {
                    ip: r.try_get("ip")?,
                    last_seen: r.try_get("last_seen")?,
                    hits: r.try_get::<i64, _>("hits")?.max(0) as u64,
                });
            }
        }
        Ok(out)
    }

    /// Addresses at least `min` web devices came from since `since`, with
    /// that count and their last use, busiest first.
    pub async fn shared_ips(&self, since: &str, min: u32) -> Result<Vec<(String, u32, String)>> {
        let rows = sqlx::query(
            "SELECT ip, COUNT(*) AS devices, MAX(last_seen) AS last_seen FROM client_ips \
             WHERE owner LIKE 'web:%' AND last_seen >= ? \
             GROUP BY ip HAVING COUNT(*) >= ? ORDER BY devices DESC, last_seen DESC LIMIT 100",
        )
        .bind(since)
        .bind(i64::from(min))
        .fetch_all(&self.0)
        .await?;
        rows.iter()
            .map(|r| {
                Ok((
                    r.try_get("ip")?,
                    u32_of(r, "devices")?,
                    r.try_get("last_seen")?,
                ))
            })
            .collect()
    }

    /// Web devices seen for the first time since `since`.
    pub async fn new_devices(&self, since: &str) -> Result<u32> {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM clients WHERE owner LIKE 'web:%' AND first_seen >= ?",
        )
        .bind(since)
        .fetch_one(&self.0)
        .await?;
        Ok(n.max(0) as u32)
    }

    /// Done downloads finished since `since`: count, average run time and size.
    pub async fn download_costs(&self, since: &str) -> Result<DownloadCosts> {
        let r = sqlx::query(
            "SELECT COUNT(*) AS n, AVG(duration_ms) AS ms, AVG(size) AS size FROM jobs \
             WHERE state IN ('done', 'expired') AND size IS NOT NULL AND finished_at >= ?",
        )
        .bind(since)
        .fetch_one(&self.0)
        .await?;
        Ok(DownloadCosts {
            done: u32_of(&r, "n")?,
            avg_duration_ms: r.try_get("ms")?,
            avg_size: r.try_get("size")?,
        })
    }

    pub async fn list_blocks(&self) -> Result<Vec<BlockInfo>> {
        let rows = sqlx::query(
            "SELECT id, kind, target, reason, created_at, created_by FROM blocks ORDER BY id DESC",
        )
        .fetch_all(&self.0)
        .await?;
        rows.iter().map(row_to_block).collect()
    }

    /// Block `target`; blocking it again updates the reason.
    pub async fn add_block(
        &self,
        kind: BlockKind,
        target: &str,
        reason: &str,
        by: Option<&str>,
    ) -> Result<BlockInfo> {
        let row = sqlx::query(
            "INSERT INTO blocks (kind, target, reason, created_at, created_by) VALUES (?, ?, ?, ?, ?) \
             ON CONFLICT (kind, target) DO UPDATE SET reason = excluded.reason \
             RETURNING id, kind, target, reason, created_at, created_by",
        )
        .bind(to_wire(&kind)?)
        .bind(target)
        .bind(reason)
        .bind(now())
        .bind(by)
        .fetch_one(&self.0)
        .await?;
        row_to_block(&row)
    }

    /// Lift a block; false when there was none with this id.
    pub async fn remove_block(&self, id: i64) -> Result<bool> {
        let r = sqlx::query("DELETE FROM blocks WHERE id = ?")
            .bind(id)
            .execute(&self.0)
            .await?;
        Ok(r.rows_affected() > 0)
    }
}
