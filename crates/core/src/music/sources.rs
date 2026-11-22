//! Pure parsing of each platform's public metadata.

use reclip_shared::{PlaylistEntry, PlaylistInfo};
use serde_json::Value;

use super::{Fetched, Track};

fn text(v: &Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

fn seconds_from_ms(v: &Value) -> Option<f64> {
    v.as_f64().filter(|ms| *ms > 0.0).map(|ms| ms / 1000.0)
}

fn date(v: &Value) -> Option<String> {
    text(v).map(|d| d.chars().take(10).collect())
}

fn list(title: Option<String>, entries: Vec<PlaylistEntry>) -> Fetched {
    Fetched::List(PlaylistInfo {
        title: title.unwrap_or_default(),
        entries,
    })
}

fn entry(url: String, title: Option<String>) -> PlaylistEntry {
    PlaylistEntry {
        url,
        title,
        playlist_item: None,
    }
}

/// The `__NEXT_DATA__` JSON of an `open.spotify.com/embed/...` page.
pub fn spotify_embed(html: &str) -> Option<Fetched> {
    let start = html.find(r#"<script id="__NEXT_DATA__" type="application/json">"#)?;
    let rest = &html[start..];
    let body = &rest[rest.find('>')? + 1..];
    let data: Value = serde_json::from_str(&body[..body.find("</script>")?]).ok()?;
    let e = &data["props"]["pageProps"]["state"]["data"]["entity"];
    let title = text(&e["name"]).or_else(|| text(&e["title"]));
    if e["type"] == "track" {
        let cover = e["visualIdentity"]["image"]
            .as_array()?
            .iter()
            .max_by_key(|i| i["maxWidth"].as_u64().unwrap_or(0))
            .and_then(|i| text(&i["url"]));
        return Some(Fetched::Track(Track {
            title: title?,
            artists: e["artists"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| text(&x["name"])).collect())
                .unwrap_or_default(),
            duration: seconds_from_ms(&e["duration"]),
            cover,
            release_date: date(&e["releaseDate"]["isoString"]),
            ..Track::default()
        }));
    }
    let entries = e["trackList"]
        .as_array()?
        .iter()
        .filter_map(|t| {
            let id = t["uri"].as_str()?.strip_prefix("spotify:track:")?;
            let name = text(&t["title"]);
            // "Artist A,\u{a0}Artist B"
            let by = text(&t["subtitle"]).map(|s| s.replace('\u{a0}', " "));
            let label = match (name, by) {
                (Some(n), Some(b)) => Some(format!("{b} - {n}")),
                (n, _) => n,
            };
            Some(entry(format!("https://open.spotify.com/track/{id}"), label))
        })
        .collect();
    Some(list(title, entries))
}

/// `api.deezer.com/track/<id>`.
pub fn deezer_track(v: &Value) -> Option<Track> {
    let mut artists: Vec<String> = v["contributors"]
        .as_array()
        .map(|a| a.iter().filter_map(|c| text(&c["name"])).collect())
        .unwrap_or_default();
    if artists.is_empty() {
        artists.extend(text(&v["artist"]["name"]));
    }
    Some(Track {
        title: text(&v["title"])?,
        artists,
        album: text(&v["album"]["title"]),
        duration: v["duration"].as_f64().filter(|d| *d > 0.0),
        cover: text(&v["album"]["cover_xl"]).or_else(|| text(&v["album"]["cover_big"])),
        track_number: v["track_position"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok()),
        release_date: date(&v["release_date"]),
    })
}

/// `api.deezer.com/album/<id>` or `api.deezer.com/playlist/<id>`.
pub fn deezer_list(v: &Value) -> Option<Fetched> {
    let entries = v["tracks"]["data"]
        .as_array()?
        .iter()
        .filter_map(|t| {
            let id = t["id"].as_u64()?;
            let label = match (text(&t["artist"]["name"]), text(&t["title"])) {
                (Some(a), Some(n)) => Some(format!("{a} - {n}")),
                (_, n) => n,
            };
            Some(entry(format!("https://www.deezer.com/track/{id}"), label))
        })
        .collect();
    Some(list(text(&v["title"]), entries))
}

/// Apple's 100 px artwork URL, at 600 px.
fn apple_cover(v: &Value) -> Option<String> {
    text(&v["artworkUrl100"]).map(|u| u.replace("100x100bb", "600x600bb"))
}

/// `itunes.apple.com/lookup?id=<track>`.
pub fn apple_track(v: &Value) -> Option<Track> {
    let r = v["results"]
        .as_array()?
        .iter()
        .find(|r| r["wrapperType"] == "track")?;
    Some(Track {
        title: text(&r["trackName"])?,
        artists: text(&r["artistName"]).into_iter().collect(),
        album: text(&r["collectionName"]),
        duration: seconds_from_ms(&r["trackTimeMillis"]),
        cover: apple_cover(r),
        track_number: r["trackNumber"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok()),
        release_date: date(&r["releaseDate"]),
    })
}

/// `itunes.apple.com/lookup?id=<album>&entity=song`.
pub fn apple_album(v: &Value, country: &str) -> Option<Fetched> {
    let results = v["results"].as_array()?;
    let album = results.iter().find(|r| r["wrapperType"] == "collection")?;
    let entries = results
        .iter()
        .filter(|r| r["wrapperType"] == "track")
        .filter_map(|r| {
            let id = r["trackId"].as_u64()?;
            let label = match (text(&r["artistName"]), text(&r["trackName"])) {
                (Some(a), Some(n)) => Some(format!("{a} - {n}")),
                (_, n) => n,
            };
            Some(entry(
                format!("https://music.apple.com/{country}/song/{id}"),
                label,
            ))
        })
        .collect();
    Some(list(text(&album["collectionName"]), entries))
}
