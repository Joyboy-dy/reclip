//! Local files: names of downloaded files and the temporary folder.

use std::path::Path;

use reclip_shared::{Job, MediaFormat};

use crate::state::valid_job_id;

/// Empties the temporary folder at startup: removes the job folders a
/// crash left behind. Only entries named like a job id are touched, so a
/// misconfigured `RECLIP_BOT_TMP` never loses unrelated files.
pub fn wipe_tmp(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name();
        if name.to_str().is_some_and(valid_job_id) {
            let p = e.path();
            let _ = if p.is_dir() {
                std::fs::remove_dir_all(&p)
            } else {
                std::fs::remove_file(&p)
            };
        }
    }
}

/// Name of the local file, which Telegram shows and uses for the MIME
/// type: the core's file name if any, else the title, with a real extension.
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
    let stem = safe_part(&stem, 100);
    let stem = if stem.is_empty() {
        "reclip".into()
    } else {
        stem
    };
    format!("{stem}.{ext}")
}

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

/// A path component without separators, reserved or control characters,
/// at most `max` characters.
fn safe_part(s: &str, max: usize) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .take(max)
        .collect();
    cleaned.trim_matches(['.', ' ']).to_string()
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
        let long = file_name(&job(None, MediaFormat::Mp4), &"é".repeat(300));
        assert_eq!(long.chars().count(), 104);
    }

    #[test]
    fn wipe_removes_only_job_folders() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("job-1").join("x")).unwrap();
        std::fs::write(dir.path().join("job-1").join("x").join("f"), "1").unwrap();
        std::fs::write(dir.path().join("keep.txt"), "1").unwrap();
        wipe_tmp(dir.path());
        assert!(!dir.path().join("job-1").exists());
        assert!(dir.path().join("keep.txt").exists());
        wipe_tmp(&dir.path().join("missing"));
    }
}
