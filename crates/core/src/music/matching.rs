//! Picking the YouTube upload that carries a platform's track.
//!
//! shortcut: candidates come from a YouTube search (`ytsearch`), because
//! yt-dlp's YouTube Music search tab returns no entries (2026.10.26); move to
//! `music.youtube.com/search?q=...#songs` once it parses again.

use serde_json::Value;

use super::Track;

/// Results read from the search.
pub const SEARCH_SIZE: u32 = 10;
/// Longest accepted gap between the track and the upload, in seconds.
const MAX_GAP: f64 = 3.0;
/// Versions refused unless the track itself is one.
const VARIANTS: [&str; 16] = [
    "live",
    "cover",
    "remix",
    "karaoke",
    "instrumental",
    "acoustic",
    "sped up",
    "slowed",
    "nightcore",
    "8d",
    "reverb",
    "mashup",
    "bass boosted",
    "a cappella",
    "acapella",
    "extended",
];

/// Lower case, every run of non-alphanumeric characters as one space,
/// padded with spaces so `contains(" word ")` matches whole words.
fn norm(s: &str) -> String {
    let mut out = String::from(" ");
    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with(' ') {
            out.push(' ');
        }
    }
    if !out.ends_with(' ') {
        out.push(' ');
    }
    out
}

/// The title without its `(feat. ...)`, `[...]` or ` - Remastered` parts.
fn core_title(title: &str) -> String {
    let main = title.split(['(', '[']).next().unwrap_or(title);
    let main = main.split(" - ").next().unwrap_or(main).trim();
    if main.is_empty() { title.trim() } else { main }.to_string()
}

/// `ytsearch` query of a track: its first artist and its main title.
pub fn search_query(t: &Track) -> String {
    let artist = t.artists.first().map(String::as_str).unwrap_or_default();
    format!("ytsearch{SEARCH_SIZE}:{artist} {}", core_title(&t.title))
        .trim()
        .to_string()
}

/// Score of one search entry, `None` when it cannot be the track.
fn score(t: &Track, e: &Value) -> Option<(i32, f64)> {
    let title = norm(e["title"].as_str()?);
    let channel = e["channel"]
        .as_str()
        .or_else(|| e["uploader"].as_str())
        .unwrap_or_default();
    let topic = channel.ends_with(" - Topic");
    let gap = match (t.duration, e["duration"].as_f64()) {
        (Some(want), Some(got)) => (want - got).abs(),
        (Some(_), None) => return None,
        (None, _) => 0.0,
    };
    if gap > MAX_GAP {
        return None;
    }
    let source = norm(&t.title);
    for v in VARIANTS {
        let w = format!(" {v} ");
        if title.contains(&w) && !source.contains(&w) {
            return None;
        }
    }
    let has_title = title.contains(&norm(&core_title(&t.title)));
    // A Topic upload is titled with the track itself; anything else must name it.
    if !has_title && !topic {
        return None;
    }
    let artist = t.artists.first().map(|a| norm(a)).unwrap_or_default();
    let by = norm(channel);
    let by_artist = artist.trim().len() > 1
        && by.trim().len() > 1
        && (by.contains(&artist) || artist.contains(&by));
    let mut s = 0;
    if topic {
        s += 4;
    }
    if by_artist {
        s += 2;
    }
    if title.contains(" audio ") {
        s += 1;
    }
    if title.contains(" lyrics ") || title.contains(" lyric ") || title.contains(" video ") {
        s -= 1;
    }
    Some((s, gap))
}

/// Id of the best entry of a flat search dump, `None` when no entry passes.
/// Ties go to the closest duration, then to the search order.
pub fn pick(t: &Track, search: &Value) -> Option<String> {
    let entries = search["entries"].as_array()?;
    let mut best: Option<(i32, f64, &Value)> = None;
    for e in entries {
        let Some((s, gap)) = score(t, e) else {
            continue;
        };
        let better = best.is_none_or(|(bs, bg, _)| s > bs || (s == bs && gap < bg));
        if better {
            best = Some((s, gap, e));
        }
    }
    best.and_then(|(_, _, e)| e["id"].as_str().map(String::from))
}
