use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::errors::ApiError;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, TS)]
#[serde(rename_all = "lowercase")]
pub enum MediaFormat {
    Mp4,
    Mp3,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Downloading,
    Processing,
    Done,
    Cancelled,
    Failed,
    Expired,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct FormatOption {
    pub id: String,
    pub label: String,
    pub height: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct VideoInfo {
    pub title: String,
    pub thumbnail: Option<String>,
    pub duration: Option<f64>,
    pub uploader: Option<String>,
    pub formats: Vec<FormatOption>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct PlaylistInfo {
    pub title: String,
    /// Entry URLs.
    pub entries: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ProbeResponse {
    Video(VideoInfo),
    Playlist(PlaylistInfo),
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct ProbeRequest {
    pub url: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct CreateJobRequest {
    pub url: String,
    pub format: MediaFormat,
    #[serde(default)]
    #[ts(optional)]
    pub format_id: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub title: Option<String>,
    #[serde(default)]
    #[ts(optional)]
    pub thumbnail: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct Job {
    pub id: String,
    pub url: String,
    pub format: MediaFormat,
    pub format_id: Option<String>,
    pub title: Option<String>,
    pub thumbnail: Option<String>,
    pub state: JobState,
    pub filename: Option<String>,
    #[ts(type = "number | null")]
    pub size: Option<u64>,
    pub error: Option<ApiError>,
    /// Live progress, 0 to 100. Never stored.
    pub progress: Option<f32>,
    /// Live speed in bytes per second. Never stored.
    #[ts(type = "number | null")]
    pub speed: Option<u64>,
    /// Live ETA in seconds. Never stored.
    #[ts(type = "number | null")]
    pub eta: Option<u64>,
    /// RFC 3339 timestamp.
    pub created_at: String,
    /// RFC 3339 timestamp.
    pub expires_at: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct JobList {
    pub jobs: Vec<Job>,
}

/// Limits of the visitor (`web`) tier. 0 means unlimited.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, TS)]
pub struct PublicLimits {
    pub probes_per_minute: u32,
    pub jobs_per_hour: u32,
    pub max_active: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct PublicConfig {
    pub version: String,
    pub telegram_bot_username: Option<String>,
    pub playlist_limit: u32,
    /// Whether the site works without an API key.
    pub public_web: bool,
    pub limits: PublicLimits,
    /// Longest accepted media, in seconds.
    #[ts(type = "number")]
    pub max_duration: u64,
}

/// A signed, time-limited download link.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct LinkResponse {
    pub url: String,
    /// RFC 3339 timestamp.
    pub expires_at: String,
}
