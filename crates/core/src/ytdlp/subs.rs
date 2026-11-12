//! Subtitles: choosing a track from the video's metadata, reading the file
//! yt-dlp wrote and turning WebVTT into text.

use std::io::Read;
use std::path::Path;

use reclip_shared::{ApiError, ErrorCode};
use serde_json::Value;

/// Longest transcript returned, in bytes of text.
pub const MAX_TEXT_BYTES: usize = 1024 * 1024;
/// Largest subtitle file read; anything beyond is ignored.
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// Prefix of the stdout line carrying the metadata of pass 1.
pub const META_PREFIX: &str = "reclip-meta:";
/// Base name of the subtitle files (`sub.<lang>.vtt`).
pub const FILE_STEM: &str = "sub";
/// Prefix of the per-request temp directories under `data_dir/tmp`.
pub const TMP_PREFIX: &str = "subs-";

/// Accept a BCP-47-looking code only: it becomes a yt-dlp language selector.
pub fn validate_language(raw: &str) -> Result<String, ApiError> {
    let lang = raw.trim();
    let ok = (1..=35).contains(&lang.len())
        && !lang.starts_with('-')
        && !lang.eq_ignore_ascii_case("all")
        && lang.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if ok {
        Ok(lang.to_string())
    } else {
        Err(ApiError {
            code: ErrorCode::InvalidRequest,
            message: "Invalid language code".into(),
        })
    }
}

/// What pass 1 learned about a video.
#[derive(Debug, Default, PartialEq)]
pub struct Tracks {
    /// The video's own language, when the site says it.
    pub language: Option<String>,
    pub manual: Vec<String>,
    pub automatic: Vec<String>,
}

/// A track to fetch in pass 2.
#[derive(Debug, PartialEq)]
pub struct Track {
    pub key: String,
    pub automatic: bool,
}

/// Read the `reclip-meta:` JSON line (`language` and the `subtitles` /
/// `automatic_captions` objects, of which only the keys matter).
pub fn parse_tracks(stdout: &str) -> Tracks {
    let json = stdout
        .lines()
        .filter_map(|l| l.strip_prefix(META_PREFIX))
        .find_map(|j| serde_json::from_str::<Value>(j).ok());
    let Some(json) = json else {
        return Tracks::default();
    };
    let keys = |field: &str| -> Vec<String> {
        let mut k: Vec<String> = json
            .get(field)
            .and_then(Value::as_object)
            .map(|o| {
                o.keys()
                    .filter(|k| *k != "live_chat" && *k != "danmaku" && is_safe_key(k))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        k.sort();
        k
    };
    Tracks {
        language: json
            .get("language")
            .and_then(Value::as_str)
            .filter(|l| !l.is_empty())
            .map(String::from),
        manual: keys("subtitles"),
        automatic: keys("automatic_captions"),
    }
}

/// A track key that is safe to put in a file name and a yt-dlp selector.
pub fn is_safe_key(key: &str) -> bool {
    (1..=64).contains(&key.len())
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Track keys to tell the caller about: uploaded first, at most 20.
pub fn available(t: &Tracks) -> Vec<&str> {
    let mut keys: Vec<&str> = Vec::new();
    for k in t.manual.iter().chain(&t.automatic) {
        if !keys.contains(&k.as_str()) {
            keys.push(k);
        }
    }
    keys.truncate(20);
    keys
}

/// `key` is `want` or a regional variant of it (`fr-FR` for `fr`).
fn is_variant(key: &str, want: &str) -> bool {
    let (k, w) = (key.to_ascii_lowercase(), want.to_ascii_lowercase());
    k == w || k.strip_prefix(&w).is_some_and(|r| r.starts_with('-'))
}

fn exact<'a>(keys: &'a [String], want: &str) -> Option<&'a String> {
    keys.iter().find(|k| k.eq_ignore_ascii_case(want))
}

fn variant<'a>(keys: &'a [String], want: &str) -> Option<&'a String> {
    exact(keys, want).or_else(|| keys.iter().find(|k| is_variant(k, want)))
}

/// Pick the track: the requested language (exact, then regional variant;
/// uploaded before automatic); else uploaded subtitles in the video's
/// language, any uploaded ones (English first), the automatic original-language
/// captions, automatic captions in the video's language, automatic English.
pub fn choose(t: &Tracks, requested: Option<&str>) -> Option<Track> {
    let manual = |k: Option<&String>| {
        k.map(|key| Track {
            key: key.clone(),
            automatic: false,
        })
    };
    let auto = |k: Option<&String>| {
        k.map(|key| Track {
            key: key.clone(),
            automatic: true,
        })
    };
    if let Some(want) = requested {
        return manual(exact(&t.manual, want))
            .or_else(|| manual(variant(&t.manual, want)))
            .or_else(|| auto(exact(&t.automatic, want)))
            .or_else(|| auto(variant(&t.automatic, want)));
    }
    let lang = t.language.as_deref();
    let orig = lang.map(|l| format!("{l}-orig"));
    manual(lang.and_then(|l| variant(&t.manual, l)))
        .or_else(|| manual(variant(&t.manual, "en")))
        .or_else(|| manual(t.manual.first()))
        .or_else(|| auto(orig.as_deref().and_then(|o| exact(&t.automatic, o))))
        .or_else(|| {
            // Without a known video language, any original-language captions.
            let any_orig = lang
                .is_none()
                .then(|| t.automatic.iter().find(|k| k.ends_with("-orig")));
            auto(any_orig.flatten())
        })
        .or_else(|| auto(lang.and_then(|l| variant(&t.automatic, l))))
        .or_else(|| auto(variant(&t.automatic, "en")))
}

/// Escape `key` for use inside a yt-dlp (Python regex) `--sub-langs` value.
pub fn escape_selector(key: &str) -> String {
    key.chars()
        .flat_map(|c| {
            let plain = c.is_ascii_alphanumeric() || c == '_';
            (!plain).then_some('\\').into_iter().chain([c])
        })
        .collect()
}

/// Text of `<dir>/sub.<key>.vtt`, else `.srt` (a site whose subtitles could
/// not be converted); `None` when neither file exists.
pub fn read_track(dir: &Path, key: &str) -> Option<String> {
    if !is_safe_key(key) {
        return None;
    }
    let open = |ext: &str| std::fs::File::open(dir.join(format!("{FILE_STEM}.{key}.{ext}"))).ok();
    let mut buf = Vec::new();
    open("vtt")
        .or_else(|| open("srt"))?
        .take(MAX_FILE_BYTES)
        .read_to_end(&mut buf)
        .ok()?;
    Some(vtt_to_text(&String::from_utf8_lossy(&buf)))
}

/// Remove the temp directories a crash left under `<data_dir>/tmp`.
pub fn sweep_leftovers(data_dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(data_dir.join("tmp")) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with(TMP_PREFIX))
        .filter(|e| std::fs::remove_dir_all(e.path()).is_ok())
        .count()
}

/// Plain text of a WebVTT file: no header, notes, cue ids, timings or tags;
/// one caption line per line, consecutive repeats merged (auto captions
/// repeat each line while they scroll). At most [`MAX_TEXT_BYTES`].
pub fn vtt_to_text(vtt: &str) -> String {
    let vtt = vtt
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut out: Vec<String> = Vec::new();
    let mut block: Vec<&str> = Vec::new();
    // A trailing empty line closes the last block.
    for line in vtt.lines().chain(std::iter::once("")) {
        // Only an empty line ends a cue; a line of spaces (YouTube starts
        // rolling cues with one) is part of it and is skipped below.
        if !line.is_empty() {
            block.push(line);
            continue;
        }
        // Cue identifier lines may precede the timing line; blocks without
        // one (header, NOTE, STYLE) carry no caption.
        if let Some(at) = block.iter().position(|l| l.contains("-->")) {
            for raw in &block[at + 1..] {
                let text = decode(&strip_tags(raw));
                let text = text.trim();
                if !text.is_empty() && out.last().is_none_or(|p| p != text) {
                    out.push(text.to_string());
                }
            }
        }
        block.clear();
    }
    cap(out.join("\n"))
}

/// Drop `<...>` tags; a `<` that is never closed keeps the rest of the line.
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        match rest[open..].find('>') {
            Some(close) => rest = &rest[open + close + 1..],
            None => return out + &rest[open..],
        }
    }
    out + rest
}

fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';').filter(|e| *e <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        match entity(&rest[1..end]) {
            Some(text) => out.push_str(&text),
            None => out.push_str(&rest[..=end]),
        }
        rest = &rest[end + 1..];
    }
    out + rest
}

fn entity(name: &str) -> Option<String> {
    let text = match name {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => " ",
        "lrm" | "rlm" => "",
        _ => {
            let num = name.strip_prefix('#')?;
            let code = match num.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => num.parse().ok()?,
            };
            return char::from_u32(code).map(String::from);
        }
    };
    Some(text.to_string())
}

fn cap(mut text: String) -> String {
    if text.len() > MAX_TEXT_BYTES {
        let mut end = MAX_TEXT_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}
