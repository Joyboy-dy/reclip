//! Music platforms that encrypt their audio (Spotify, Deezer, Apple Music).
//!
//! Their public metadata (title, artists, duration, cover) is read without
//! an account or key; the audio is the best matching YouTube upload, and the
//! file gets the platform's tags and cover. A probe only reads metadata, so
//! an album of 50 tracks is listed quickly; the YouTube search runs with the
//! download.

mod fetch;
mod link;
mod matching;
mod sources;

#[cfg(test)]
mod tests;

use reclip_shared::{ApiError, ErrorCode, PlaylistInfo, ProbeResponse, VideoInfo};
use serde_json::{Value, json};

pub use fetch::fetch;
pub use link::{Kind, Link, Platform, is_music_link};
pub use matching::{pick, search_query};

/// One track as the platform describes it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Track {
    pub title: String,
    pub artists: Vec<String>,
    pub album: Option<String>,
    /// Seconds.
    pub duration: Option<f64>,
    pub cover: Option<String>,
    pub track_number: Option<u32>,
    /// `YYYY-MM-DD`, or a prefix of it.
    pub release_date: Option<String>,
}

/// What a music link points at.
#[derive(Debug, Clone, PartialEq)]
pub enum Fetched {
    Track(Track),
    /// An album or a playlist: one entry per track URL.
    List(PlaylistInfo),
}

fn err(code: ErrorCode, message: impl Into<String>) -> ApiError {
    ApiError {
        code,
        message: message.into(),
    }
}

/// The probe answer of a music link: the track with its cover, audio only,
/// or the list of its tracks (at most `limit`).
pub fn probe_response(platform: Platform, fetched: Fetched, limit: u32) -> ProbeResponse {
    match fetched {
        Fetched::Track(t) => ProbeResponse::Video(VideoInfo {
            title: t.title,
            thumbnail: t.cover,
            duration: t.duration,
            uploader: Some(t.artists.join(", ")).filter(|a| !a.is_empty()),
            formats: vec![],
            audio_only: true,
            matched_from: Some(platform.name().into()),
        }),
        Fetched::List(mut list) => {
            list.entries.truncate(limit as usize);
            ProbeResponse::Playlist(list)
        }
    }
}

/// A single track of `fetched`, or why a list cannot be downloaded as one file.
pub fn single_track(fetched: Fetched) -> Result<Track, ApiError> {
    match fetched {
        Fetched::Track(t) => Ok(t),
        Fetched::List(_) => Err(err(
            ErrorCode::InvalidRequest,
            "This is an album or a playlist: download its tracks one by one",
        )),
    }
}

/// Replace the YouTube metadata of `info` (a yt-dlp `-j` dump) by the
/// platform's, so `--embed-metadata` and `--embed-thumbnail` write them.
pub fn retag(info: &mut Value, t: &Track) {
    let Some(obj) = info.as_object_mut() else {
        return;
    };
    let artist = t.artists.join(", ");
    let date = t.release_date.as_deref().map(|d| d.replace('-', ""));
    let year = t
        .release_date
        .as_deref()
        .and_then(|d| d.get(..4))
        .and_then(|y| y.parse::<u32>().ok());
    for (k, v) in [
        ("title", json!(t.title)),
        ("track", json!(t.title)),
        ("artist", json!(artist)),
        ("artists", json!(t.artists)),
        ("creator", json!(artist)),
        ("uploader", json!(artist)),
        ("album", json!(t.album)),
        ("album_artist", json!(t.artists.first())),
        ("track_number", json!(t.track_number)),
        ("release_date", json!(date)),
        ("release_year", json!(year)),
        ("upload_date", json!(date)),
        // The YouTube description would end up in the comment tag.
        ("description", Value::Null),
    ] {
        obj.insert(k.into(), v);
    }
    if let Some(cover) = &t.cover {
        obj.insert("thumbnail".into(), json!(cover));
        obj.insert("thumbnails".into(), json!([{"id": "cover", "url": cover}]));
    }
}
