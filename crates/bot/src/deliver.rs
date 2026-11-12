//! Sends a finished job: the file itself up to the upload limit, a signed
//! link above it (the job is then kept on the core).

use std::path::PathBuf;
use std::sync::Arc;

use reclip_shared::{Job, MediaFormat};

use crate::app::{App, MAX_CAPTION, clip, core_error_text};
use crate::core_client::Core;
use crate::i18n::{self, Lang};
use crate::state::Tracked;
use crate::telegram::Telegram;
use crate::tracker::NOT_FOUND;

pub async fn deliver<T: Telegram + 'static, C: Core + 'static>(
    app: &Arc<App<T, C>>,
    t: &Tracked,
    job: &Job,
) {
    let l = Lang::from_stored(&t.lang);
    let max = app.settings.max_upload;
    if let Some(size) = job.size.filter(|s| *s > max) {
        tracing::info!(job = %t.job_id, size, "too big to upload, sending a link");
        return send_link(app, t, size).await;
    }
    if let Err(e) = app
        .tg
        .edit_message(t.chat_id, t.message_id, &i18n::done(l, &t.title), None)
        .await
    {
        tracing::warn!(job = %t.job_id, error = %e, "progress edit failed");
    }
    let dir = app.settings.tmp_dir.join(safe_part(&t.job_id, 64));
    let _cleanup = DirGuard(dir.clone());
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        tracing::warn!(job = %t.job_id, error = %e, "cannot create the download folder");
        return send_link(app, t, job.size.unwrap_or(0)).await;
    }
    let path = dir.join(file_name(job, &t.title));
    let size = match app.core.download_file(t.user_id, &t.job_id, &path).await {
        Ok(n) => n,
        Err(e) if e.status == NOT_FOUND => {
            tracing::info!(job = %t.job_id, "file gone from the core");
            app.replace(t.chat_id, Some(t.message_id), &i18n::file_expired(l), None)
                .await;
            app.state.remove(&t.job_id);
            return;
        }
        Err(e) => {
            tracing::warn!(job = %t.job_id, error = %e, "download from the core failed");
            return send_link(app, t, job.size.unwrap_or(0)).await;
        }
    };
    if size > max {
        tracing::info!(job = %t.job_id, size, "too big to upload, sending a link");
        return send_link(app, t, size).await;
    }
    let sent = if t.format.is_audio() {
        app.tg
            .send_audio(
                t.chat_id,
                &path,
                Some(&t.title),
                t.uploader.as_deref(),
                t.duration,
            )
            .await
    } else {
        let caption = clip(&t.title, MAX_CAPTION);
        app.tg
            .send_video(t.chat_id, &path, &caption, t.duration, None, None)
            .await
    };
    match sent {
        Ok(()) => {
            tracing::info!(job = %t.job_id, size, "file sent");
            if let Err(e) = app.core.delete_job(t.user_id, &t.job_id).await {
                tracing::warn!(job = %t.job_id, error = %e, "cannot delete the job");
            }
            app.delete(t.chat_id, t.message_id).await;
            app.state.remove(&t.job_id);
        }
        Err(e) => {
            tracing::warn!(job = %t.job_id, error = %e, "upload failed, sending a link");
            send_link(app, t, size).await;
        }
    }
}

/// Replaces the progress message by a signed link; the job stays on the
/// core until it expires.
async fn send_link<T: Telegram + 'static, C: Core + 'static>(
    app: &Arc<App<T, C>>,
    t: &Tracked,
    size: u64,
) {
    let l = Lang::from_stored(&t.lang);
    let text = match app.core.signed_link(t.user_id, &t.job_id).await {
        Ok(link) => i18n::file_too_big(l, size, &link.url),
        Err(e) => {
            tracing::warn!(job = %t.job_id, error = %e, "cannot get a signed link");
            core_error_text(l, &e)
        }
    };
    app.replace(t.chat_id, Some(t.message_id), &text, None)
        .await;
    app.state.remove(&t.job_id);
}

/// Removes the job's download folder, whatever happens.
struct DirGuard(PathBuf);

impl Drop for DirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
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
}
