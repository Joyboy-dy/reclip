//! Local files: names of downloaded files and the temporary folder.

use std::path::Path;

use reclip_shared::{Job, MediaFormat};

/// Empties the temporary folder at startup: removes the job folders a
/// crash left behind. Only directories named like a core job id (32
/// lowercase hex digits) are touched, so a misconfigured `RECLIP_BOT_TMP`
/// never loses unrelated files.
pub fn wipe_tmp(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name();
        let is_dir = std::fs::symlink_metadata(e.path()).is_ok_and(|m| m.is_dir());
        if is_dir && name.to_str().is_some_and(is_core_job_id) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// The core's job ids: `Uuid::now_v7().simple()`, 32 lowercase hex digits.
fn is_core_job_id(name: &str) -> bool {
    name.len() == 32 && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Name of the local file, which Telegram shows and uses for the MIME
/// type: the core's file name if any, else the title, with a real extension.
/// ASCII only and at most [`MAX_STEM`] bytes before the extension, so the
/// path fits any file system; the full title goes in the caption.
pub fn file_name(job: &Job, title: &str) -> String {
    let from_core = job
        .filename
        .as_deref()
        .and_then(|f| f.rsplit(['/', '\\']).next())
        .filter(|f| !f.is_empty());
    let (stem, ext) = match from_core.and_then(|f| f.rsplit_once('.')) {
        Some((stem, ext)) if is_ext(ext) => (stem.to_string(), ext.to_ascii_lowercase()),
        _ => (
            from_core.unwrap_or(title).to_string(),
            format_ext(job.format).to_string(),
        ),
    };
    let stem = safe_part(&stem, MAX_STEM);
    let stem = if stem.chars().all(|c| c == '_') {
        "reclip".into()
    } else {
        stem
    };
    format!("{stem}.{ext}")
}

/// Bytes of the name before the extension (Linux allows 255 in all, and
/// the download adds `.part`).
const MAX_STEM: usize = 200;

fn is_ext(e: &str) -> bool {
    (1..=5).contains(&e.len()) && e.chars().all(|c| c.is_ascii_alphanumeric())
}

pub fn format_ext(f: MediaFormat) -> &'static str {
    match f {
        MediaFormat::Mp4 => "mp4",
        MediaFormat::Mkv => "mkv",
        MediaFormat::Mov => "mov",
        MediaFormat::Mp3 => "mp3",
        MediaFormat::M4a => "m4a",
        MediaFormat::Opus => "opus",
        MediaFormat::Flac => "flac",
        MediaFormat::Wav => "wav",
    }
}

/// A path component of printable ASCII without separators or reserved
/// characters: anything else becomes `_` (one for a run), at most `max`
/// bytes.
fn safe_part(s: &str, max: usize) -> String {
    let mut out = String::new();
    for c in s.chars() {
        let c = match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if !c.is_ascii() || c.is_ascii_control() => '_',
            c => c,
        };
        if c == '_' && out.ends_with('_') {
            continue;
        }
        if out.len() == max {
            break;
        }
        out.push(c);
    }
    out.trim_matches(['.', ' ']).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reclip_shared::JobState;

    fn job(filename: Option<&str>, format: MediaFormat) -> Job {
        Job {
            id: "j".into(),
            url: "u".into(),
            format,
            format_id: None,
            title: None,
            thumbnail: None,
            state: JobState::Done,
            filename: filename.map(Into::into),
            size: None,
            error: None,
            progress: None,
            speed: None,
            eta: None,
            created_at: String::new(),
            expires_at: None,
            clip: None,
            strip_metadata: false,
            resumable: false,
            playlist_item: None,
        }
    }

    #[test]
    fn file_names_keep_a_real_extension() {
        let f = |name, fmt| file_name(&job(name, fmt), "My: Title?");
        assert_eq!(f(Some("Clip.MP4"), MediaFormat::Mp4), "Clip.mp4");
        assert_eq!(f(Some("../../etc/x.mp3"), MediaFormat::Mp3), "x.mp3");
        assert_eq!(f(None, MediaFormat::Mp3), "My_ Title_.mp3");
        assert_eq!(f(Some("noext"), MediaFormat::Mp4), "noext.mp4");
        assert_eq!(f(Some(".."), MediaFormat::Mp4), "reclip.mp4");
    }

    #[test]
    fn file_names_are_short_ascii() {
        let name = |title: &str, core: Option<&str>| file_name(&job(core, MediaFormat::Mp3), title);
        let cjk = "日".repeat(100);
        assert_eq!(name(&cjk, None), "reclip.mp3");
        assert_eq!(name("x", Some(&format!("{cjk}.mp3"))), "reclip.mp3");
        assert_eq!(name("Été — 日本 live", None), "_t_ _ _ live.mp3");
        let long = name(&"ab".repeat(300), None);
        assert!(long.is_ascii());
        assert_eq!(long.len(), 200 + ".mp3".len());
        let emoji = name(&format!("{}x", "😀".repeat(70)), None);
        assert_eq!(emoji, "_x.mp3");
    }

    #[test]
    fn wipe_removes_only_job_folders() {
        let dir = tempfile::tempdir().unwrap();
        let id = "0193a5c47e2b7c3d9f00123456789abc";
        std::fs::create_dir_all(dir.path().join(id).join("x")).unwrap();
        std::fs::write(dir.path().join(id).join("x").join("f"), "1").unwrap();
        // Names a job id cannot have, and a file with a job id name.
        for keep in ["job-1", "bin", "ABCDEF0123456789ABCDEF0123456789", "abc"] {
            std::fs::create_dir_all(dir.path().join(keep)).unwrap();
        }
        let file_id = "ffffffffffffffffffffffffffffffff";
        std::fs::write(dir.path().join(file_id), "1").unwrap();
        std::fs::write(dir.path().join("keep.txt"), "1").unwrap();
        wipe_tmp(dir.path());
        assert!(!dir.path().join(id).exists());
        for keep in ["job-1", "bin", "ABCDEF0123456789ABCDEF0123456789", "abc"] {
            assert!(dir.path().join(keep).exists(), "{keep}");
        }
        assert!(dir.path().join(file_id).exists());
        assert!(dir.path().join("keep.txt").exists());
        wipe_tmp(&dir.path().join("missing"));
    }
}
