//! HTTP reads of the platforms' public pages and APIs (no key, no cookie).

use std::sync::OnceLock;
use std::time::Duration;

use reclip_shared::{ApiError, ErrorCode};
use serde_json::Value;

use super::link::{self, Kind, Link, Platform};
use super::{Fetched, err, sources};

const TIMEOUT: Duration = Duration::from_secs(15);
/// Spotify serves its embed data to browsers only.
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
    (KHTML, like Gecko) Chrome/131.0 Safari/537.36";
/// Redirects followed from a share link before giving up.
const MAX_HOPS: usize = 4;

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(TIMEOUT)
            .user_agent(USER_AGENT)
            // Share links are followed by hand, only to the platforms' hosts.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("static reqwest client")
    })
}

fn unreadable(platform: &str) -> ApiError {
    err(
        ErrorCode::Unavailable,
        format!("Could not read this {platform} link, try again later"),
    )
}

async fn get_text(url: &str, platform: &str) -> Result<String, ApiError> {
    let res = client().get(url).send().await.map_err(|e| {
        tracing::warn!(url, error = %e, "music metadata request failed");
        unreadable(platform)
    })?;
    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(err(
            ErrorCode::Unavailable,
            format!("This {platform} item does not exist or is not public"),
        ));
    }
    if !res.status().is_success() {
        tracing::warn!(url, status = %res.status(), "music metadata refused");
        return Err(unreadable(platform));
    }
    res.text().await.map_err(|_| unreadable(platform))
}

async fn get_json(url: &str, platform: &str) -> Result<Value, ApiError> {
    let body = get_text(url, platform).await?;
    let v: Value = serde_json::from_str(&body).map_err(|_| unreadable(platform))?;
    // Deezer answers 200 with `{"error": ...}` for a missing id.
    if v.get("error").is_some() {
        return Err(err(
            ErrorCode::Unavailable,
            format!("This {platform} item does not exist or is not public"),
        ));
    }
    Ok(v)
}

/// The platform link behind a share link (`spotify.link/...`), by its
/// redirects or, for a landing page, the first platform link in its HTML.
async fn expand(url: &str) -> Result<String, ApiError> {
    let mut current = url.to_string();
    for _ in 0..MAX_HOPS {
        if link::parse(&current).is_some() {
            return Ok(current);
        }
        if !link::is_music_link(&current) {
            break;
        }
        let res = client()
            .get(&current)
            .send()
            .await
            .map_err(|_| unreadable("share"))?;
        if let Some(next) = res
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|l| l.to_str().ok())
            .and_then(|l| url::Url::parse(&current).ok()?.join(l).ok())
        {
            current = next.into();
            continue;
        }
        let body = res.text().await.unwrap_or_default();
        return first_platform_link(&body).ok_or_else(unsupported);
    }
    if link::parse(&current).is_some() {
        return Ok(current);
    }
    Err(unsupported())
}

fn first_platform_link(html: &str) -> Option<String> {
    ["https://open.spotify.com/", "https://www.deezer.com/"]
        .iter()
        .filter_map(|p| html.find(p))
        .min()
        .map(|i| {
            html[i..]
                .split(['"', '\'', '<', ' ', '\\'])
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .filter(|u| link::parse(u).is_some())
}

fn unsupported() -> ApiError {
    err(
        ErrorCode::UnsupportedUrl,
        "Only music tracks, albums and playlists are supported for this site",
    )
}

/// The track, album or playlist behind a music link.
pub async fn fetch(url: &str) -> Result<(Platform, Fetched), ApiError> {
    if link::is_apple_playlist(url) {
        return Err(err(
            ErrorCode::UnsupportedUrl,
            "Apple Music playlists are not public: share a song or an album",
        ));
    }
    let url = expand(url).await?;
    let l = link::parse(&url).ok_or_else(unsupported)?;
    let name = l.platform.name();
    let bad = || unreadable(name);
    let fetched = match l {
        Link {
            platform: Platform::Spotify,
            kind,
            ref id,
            ..
        } => {
            let word = match kind {
                Kind::Track => "track",
                Kind::Album => "album",
                Kind::Playlist => "playlist",
            };
            let html =
                get_text(&format!("https://open.spotify.com/embed/{word}/{id}"), name).await?;
            sources::spotify_embed(&html).ok_or_else(bad)?
        }
        Link {
            platform: Platform::Deezer,
            kind,
            ref id,
            ..
        } => match kind {
            Kind::Track => {
                let v = get_json(&format!("https://api.deezer.com/track/{id}"), name).await?;
                Fetched::Track(sources::deezer_track(&v).ok_or_else(bad)?)
            }
            Kind::Album | Kind::Playlist => {
                let word = if kind == Kind::Album {
                    "album"
                } else {
                    "playlist"
                };
                let v = get_json(&format!("https://api.deezer.com/{word}/{id}"), name).await?;
                sources::deezer_list(&v).ok_or_else(bad)?
            }
        },
        Link {
            platform: Platform::Apple,
            kind,
            ref id,
            ref country,
        } => {
            let cc = country.as_deref().unwrap_or("us");
            let base = format!("https://itunes.apple.com/lookup?id={id}&country={cc}");
            if kind == Kind::Album {
                let v = get_json(&format!("{base}&entity=song"), name).await?;
                sources::apple_album(&v, cc).ok_or_else(bad)?
            } else {
                let v = get_json(&base, name).await?;
                Fetched::Track(sources::apple_track(&v).ok_or_else(bad)?)
            }
        }
    };
    Ok((l.platform, fetched))
}
