//! Subtitles: choosing the file yt-dlp wrote and turning WebVTT into text.

use std::io::Read;
use std::path::{Path, PathBuf};

use reclip_shared::{ApiError, ErrorCode, Transcript};

/// Longest transcript returned, in bytes of text.
pub const MAX_TEXT_BYTES: usize = 1024 * 1024;
/// Largest subtitle file read; anything beyond is ignored.
const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// `--sub-langs` when the caller names none: the original language (YouTube
/// labels its auto captions `<lang>-orig`), then English.
pub const DEFAULT_LANGS: &str = ".*-orig,en";
/// Prefix of the stdout line carrying the uploaded subtitle languages.
pub const MANUAL_PREFIX: &str = "reclip-subs:";
/// Base name of the subtitle files (`sub.<lang>.vtt`).
pub const FILE_STEM: &str = "sub";

/// Accept a BCP-47-looking code only: it ends up in a yt-dlp regex.
pub fn validate_language(raw: &str) -> Result<String, ApiError> {
    let lang = raw.trim();
    let ok = (1..=35).contains(&lang.len())
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

/// Languages with uploaded (manual) subtitles, from the `--print` line of
/// yt-dlp's `subtitles` field (a JSON object keyed by language).
pub fn manual_languages(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .filter_map(|l| l.strip_prefix(MANUAL_PREFIX))
        .filter_map(|j| serde_json::from_str::<serde_json::Value>(j).ok())
        .filter_map(|v| v.as_object().map(|o| o.keys().cloned().collect()))
        .next()
        .unwrap_or_default()
}

/// The best `sub.<lang>.vtt` in `dir`: manual before automatic, then the
/// original language (`-orig`), then the language code order.
pub fn find_transcript(dir: &Path, manual: &[String]) -> Option<Transcript> {
    let mut found: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let lang = name
                .strip_prefix(FILE_STEM)?
                .strip_prefix('.')?
                .strip_suffix(".vtt")?;
            Some((lang.to_string(), e.path()))
        })
        .collect();
    found.sort_by_key(|(lang, _)| {
        (
            !manual.contains(lang),
            !lang.ends_with("-orig"),
            lang.clone(),
        )
    });
    // The first one with some text wins.
    found.into_iter().find_map(|(language, path)| {
        let text = vtt_to_text(&read_capped(&path)?);
        (!text.is_empty()).then(|| Transcript {
            automatic: !manual.contains(&language),
            language,
            text,
        })
    })
}

fn read_capped(path: &Path) -> Option<String> {
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_FILE_BYTES)
        .read_to_end(&mut buf)
        .ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Plain text of a WebVTT file: no header, notes, cue ids, timings or tags;
/// one caption line per line, consecutive repeats merged (auto captions
/// repeat each line while they scroll). At most [`MAX_TEXT_BYTES`].
pub fn vtt_to_text(vtt: &str) -> String {
    let vtt = vtt.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut lines: Vec<String> = Vec::new();
    for block in vtt.split("\n\n") {
        let mut it = block.lines();
        // Cue identifier lines may precede the timing line.
        if !it.by_ref().any(|l| l.contains("-->")) {
            continue;
        }
        for raw in it {
            let line = decode(&strip_tags(raw));
            let line = line.trim();
            if !line.is_empty() && lines.last().is_none_or(|p| p != line) {
                lines.push(line.to_string());
            }
        }
    }
    cap(lines.join("\n"))
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

fn decode(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
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
