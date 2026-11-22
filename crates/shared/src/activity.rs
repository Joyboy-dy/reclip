//! Admin views of who uses ReClip, blocks, and the server's load.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, TS)]
#[serde(rename_all = "lowercase")]
pub enum ClientKind {
    /// A browser, known by its cookie.
    Web,
    /// A Telegram account, through the bot.
    Telegram,
    /// An API key used directly.
    Key,
}

/// Hints that a link is shared beyond one person.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
pub enum ClientFlag {
    /// Many addresses in the last 24 h.
    ManyIps,
    /// Its last address is used by many other devices.
    SharedIp,
}

/// An address an identity came from (an IPv6 address counts as its /64).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct SeenIp {
    pub ip: String,
    /// RFC 3339 timestamp.
    pub last_seen: String,
    #[ts(type = "number")]
    pub hits: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, TS)]
#[serde(rename_all = "lowercase")]
pub enum BlockKind {
    /// A web device, Telegram account or API key (`web:…`, `tg:…`, `key:…`).
    Owner,
    /// An address or a range in CIDR notation.
    Ip,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct BlockInfo {
    #[ts(type = "number")]
    pub id: i64,
    pub kind: BlockKind,
    pub target: String,
    pub reason: String,
    /// RFC 3339 timestamp.
    pub created_at: String,
    /// Email of the admin who blocked, `None` for the admin token.
    pub created_by: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct BlockList {
    pub blocks: Vec<BlockInfo>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct CreateBlockRequest {
    pub kind: BlockKind,
    pub target: String,
    #[serde(default)]
    pub reason: String,
}

/// One identity and what it did within the retention window.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct ClientInfo {
    /// `web:…`, `tg:…` or `key:…`: the target of an owner block.
    pub owner: String,
    pub kind: ClientKind,
    /// What to call it: `@username` or name (Telegram), key name, else the device.
    pub label: String,
    /// Browser, system and form factor of the last request.
    pub device: Option<String>,
    pub tg_username: Option<String>,
    pub tg_name: Option<String>,
    /// RFC 3339 timestamps.
    pub first_seen: String,
    pub last_seen: String,
    pub last_ip: Option<String>,
    pub ips_day: u32,
    pub ips_total: u32,
    /// Most recent first, at most 10.
    pub ips: Vec<SeenIp>,
    pub downloads_day: u32,
    pub downloads_week: u32,
    /// Over the retention window.
    pub downloads_total: u32,
    #[ts(type = "number")]
    pub bytes_total: u64,
    pub block: Option<BlockInfo>,
    pub flags: Vec<ClientFlag>,
}

/// An address several web devices came from in the last 7 days.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct SharedIp {
    pub ip: String,
    pub devices: u32,
    /// RFC 3339 timestamp.
    pub last_seen: String,
    pub block: Option<BlockInfo>,
}

/// `GET /api/v1/admin/activity`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct ActivityReport {
    /// Most recently seen first.
    pub clients: Vec<ClientInfo>,
    pub shared_ips: Vec<SharedIp>,
    /// Web devices seen for the first time in the last hour / 24 h.
    pub new_devices_hour: u32,
    pub new_devices_day: u32,
    /// Unusually many new devices in the last hour.
    pub burst: bool,
    /// Days activity is kept.
    pub retention_days: u32,
}

/// `GET /api/v1/admin/server`. Load values are `None` where the system
/// does not expose them (outside Linux).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct ServerStats {
    #[ts(type = "number")]
    pub uptime_secs: u64,
    /// Since the previous request, in % of all cores (`None` on the first).
    pub cpu_percent: Option<f32>,
    pub cpu_cores: u32,
    #[ts(type = "number | null")]
    pub memory_bytes: Option<u64>,
    /// The container's memory limit, if any.
    #[ts(type = "number | null")]
    pub memory_limit: Option<u64>,
    /// Downloaded files waiting to be saved, and their budget.
    #[ts(type = "number")]
    pub files_bytes: u64,
    #[ts(type = "number")]
    pub files_limit: u64,
    pub jobs_running: u32,
    pub jobs_queued: u32,
    /// Bytes per second since the previous request.
    #[ts(type = "number | null")]
    pub net_rx_rate: Option<u64>,
    #[ts(type = "number | null")]
    pub net_tx_rate: Option<u64>,
    /// Finished downloads within the retention window.
    pub downloads_done: u32,
    pub avg_duration_secs: Option<f64>,
    #[ts(type = "number | null")]
    pub avg_size_bytes: Option<u64>,
    /// CPU time of the core and its tools per finished download, since start.
    pub avg_cpu_secs: Option<f64>,
}
