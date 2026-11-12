//! Types of the per-visitor download history.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::api::MediaFormat;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct HistoryEntry {
    pub id: String,
    pub url: String,
    pub title: Option<String>,
    pub format: MediaFormat,
    pub format_id: Option<String>,
    #[ts(type = "number | null")]
    pub size: Option<u64>,
    /// Whether a small local thumbnail can be fetched.
    pub has_thumbnail: bool,
    /// Whether the media file can still be downloaded.
    pub file_available: bool,
    /// RFC 3339 timestamp.
    pub created_at: String,
    /// RFC 3339 timestamp of the file expiry.
    pub expires_at: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct HistoryList {
    pub entries: Vec<HistoryEntry>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
pub enum HistoryCleanMode {
    /// Remove the files that have expired.
    Expired,
    /// Remove entries older than `days`.
    OlderThan,
    /// Remove the whole history.
    All,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct HistoryClean {
    pub mode: HistoryCleanMode,
    /// Required by `older_than`.
    #[serde(default)]
    #[ts(optional)]
    pub days: Option<u32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct HistoryCleanResult {
    pub removed: u32,
}
