use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::errors::ApiError;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, TS)]
#[serde(rename_all = "lowercase")]
pub enum MediaFormat {
    Mp4,
    Mkv,
    Mov,
    Mp3,
    M4a,
    Opus,
    Flac,
    Wav,
}

impl MediaFormat {
    /// Whether the format is audio only.
    pub fn is_audio(&self) -> bool {
        matches!(
            self,
            MediaFormat::Mp3
                | MediaFormat::M4a
                | MediaFormat::Opus
                | MediaFormat::Flac
                | MediaFormat::Wav
        )
    }
}

/// A time range to keep, in seconds.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, TS)]
pub struct Clip {
    pub start: f64,
    /// `None` runs to the end of the media.
    #[serde(default)]
    #[ts(optional)]
    pub end: Option<f64>,
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
    /// Video codec when it is not H.264 (e.g. `VP9`); `None` means H.264.
    pub codec: Option<String>,
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
    pub entries: Vec<PlaylistEntry>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct PlaylistEntry {
    pub url: String,
    pub title: Option<String>,
    /// 1-based position when the entry is one video of a multi-video post.
    pub playlist_item: Option<u32>,
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
    #[serde(default)]
    #[ts(optional)]
    pub clip: Option<Clip>,
    /// Drop container metadata from the output.
    #[serde(default)]
    pub strip_metadata: bool,
    /// 1-based item of a multi-video post.
    #[serde(default)]
    #[ts(optional)]
    pub playlist_item: Option<u32>,
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
    pub clip: Option<Clip>,
    pub strip_metadata: bool,
    /// Whether `POST /jobs/{id}/resume` can continue this job.
    pub resumable: bool,
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
    /// How long a finished file stays downloadable, in seconds.
    #[ts(type = "number")]
    pub file_ttl_secs: u64,
    /// Days the download history is kept.
    pub history_days: u32,
}

/// A signed, time-limited download link.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, TS)]
pub struct LinkResponse {
    pub url: String,
    /// RFC 3339 timestamp.
    pub expires_at: String,
}
