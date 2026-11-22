//! Recognising Spotify, Deezer and Apple Music links.

use crate::ytdlp::download_args::on_domain;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Spotify,
    Deezer,
    Apple,
}

impl Platform {
    pub fn name(self) -> &'static str {
        match self {
            Platform::Spotify => "Spotify",
            Platform::Deezer => "Deezer",
            Platform::Apple => "Apple Music",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Track,
    Album,
    Playlist,
}

/// A parsed music link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub platform: Platform,
    pub kind: Kind,
    pub id: String,
    /// Apple's storefront (`us`, `fr`), which the lookup needs.
    pub country: Option<String>,
}

const SPOTIFY: [&str; 2] = ["open.spotify.com", "play.spotify.com"];
const DEEZER: [&str; 1] = ["deezer.com"];
const APPLE: [&str; 2] = ["music.apple.com", "itunes.apple.com"];
/// Share links that redirect to one of the above.
pub(super) const SHORT: [&str; 4] = [
    "spotify.link",
    "link.deezer.com",
    "deezer.page.link",
    "dzr.page.link",
];

/// Whether `url` belongs to a platform handled here, short links included.
pub fn is_music_link(url: &str) -> bool {
    [&SPOTIFY[..], &DEEZER, &APPLE, &SHORT]
        .iter()
        .any(|hosts| on_domain(url, hosts))
}

fn segments(u: &url::Url) -> Vec<&str> {
    u.path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default()
}

fn kind_of(word: &str) -> Option<Kind> {
    match word {
        "track" | "song" => Some(Kind::Track),
        "album" => Some(Kind::Album),
        "playlist" => Some(Kind::Playlist),
        _ => None,
    }
}

/// The track, album or playlist that `url` names, `None` for any other page
/// (artist, podcast) or another site.
pub fn parse(url: &str) -> Option<Link> {
    let u = url::Url::parse(url).ok()?;
    let seg = segments(&u);
    let link = |platform, kind, id: &str, country: Option<&str>| {
        let ok = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.');
        ok.then(|| Link {
            platform,
            kind,
            id: id.to_string(),
            country: country.map(str::to_ascii_lowercase),
        })
    };
    if on_domain(url, &SPOTIFY) {
        // /track/<id>, /intl-fr/track/<id>, /embed/track/<id>
        let i = seg.iter().position(|s| kind_of(s).is_some())?;
        return link(Platform::Spotify, kind_of(seg[i])?, seg.get(i + 1)?, None);
    }
    if on_domain(url, &DEEZER) {
        // /track/<id>, /fr/album/<id>
        let i = seg.iter().position(|s| kind_of(s).is_some())?;
        return link(Platform::Deezer, kind_of(seg[i])?, seg.get(i + 1)?, None);
    }
    if on_domain(url, &APPLE) {
        // /<cc>/album/<slug>/<id>?i=<track>, /<cc>/song/[<slug>/]<id>
        let country = seg.first().filter(|c| c.len() == 2).copied();
        let i = seg.iter().position(|s| kind_of(s).is_some())?;
        let kind = kind_of(seg[i])?;
        if kind == Kind::Playlist {
            return None;
        }
        let track = u.query_pairs().find(|(k, _)| k == "i").map(|(_, v)| v);
        let last = seg.last().filter(|_| seg.len() > i + 1)?;
        let id = last.strip_prefix("id").unwrap_or(last);
        return match (kind, track) {
            (Kind::Album, Some(t)) => link(Platform::Apple, Kind::Track, &t, country),
            _ => link(Platform::Apple, kind, id, country),
        };
    }
    None
}

/// Whether `url` is an Apple Music playlist, which has no public lookup.
pub(super) fn is_apple_playlist(url: &str) -> bool {
    on_domain(url, &APPLE) && url.contains("/playlist/")
}
