//! Signed download links, sent instead of the file. The job stays on the
//! core until it expires.

use std::sync::Arc;

use crate::app::{App, core_error_text};
use crate::core_client::Core;
use crate::i18n::{self, Lang};
use crate::state::Tracked;
use crate::telegram::Telegram;

/// Why the file goes as a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkReason {
    /// Above the upload limit; the size when known.
    TooBig(Option<u64>),
    /// Download from the core or upload to Telegram failed.
    SendFailed,
}

/// Replaces the progress message by a signed link and stops tracking.
pub async fn send_link<T: Telegram + 'static, C: Core + 'static>(
    app: &Arc<App<T, C>>,
    t: &Tracked,
    reason: LinkReason,
) {
    let l = Lang::from_stored(&t.lang);
    let text = match app.core.signed_link(t.user_id, &t.job_id).await {
        Ok(link) => match public_link(&link.url, app.settings.public_url.as_deref()) {
            Some(url) => link_text(l, reason, &url),
            None => {
                tracing::warn!(
                    job = %t.job_id,
                    "the core gave an internal link: set RECLIP_PUBLIC_URL"
                );
                i18n::link_unavailable(l, matches!(reason, LinkReason::TooBig(_)))
            }
        },
        Err(e) => {
            tracing::warn!(job = %t.job_id, error = %e, "cannot get a signed link");
            core_error_text(l, &e)
        }
    };
    app.replace(t.chat_id, Some(t.message_id), &text, None)
        .await;
    app.state.remove(&t.job_id);
}

fn link_text(l: Lang, reason: LinkReason, url: &str) -> String {
    match reason {
        LinkReason::TooBig(size) => i18n::file_too_big(l, size, url),
        LinkReason::SendFailed => i18n::send_failed_link(l, url),
    }
}

/// The link users can open. With a public address, the core's link is
/// rebuilt on it (path and query kept). Without one, a link to a host that
/// is clearly internal (`localhost`, no dot such as `core:8899`) is `None`.
pub fn public_link(raw: &str, public_url: Option<&str>) -> Option<String> {
    let (_, rest) = raw.split_once("://")?;
    let (authority, path) = match rest.find(['/', '?', '#']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    if let Some(base) = public_url {
        let path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        return Some(format!("{}{path}", base.trim_end_matches('/')));
    }
    let host = authority.rsplit('@').next().unwrap_or("");
    let host = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => host.split(':').next().unwrap_or(""),
    };
    let internal = host.is_empty() || host.eq_ignore_ascii_case("localhost") || !host.contains('.');
    (!internal).then(|| raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_address_rebuilds_the_link() {
        let p = Some("https://reclip.example");
        assert_eq!(
            public_link("http://core:8899/d/tok?sig=1", p).as_deref(),
            Some("https://reclip.example/d/tok?sig=1")
        );
        assert_eq!(
            public_link("http://core:8899/d/x", Some("https://r.example/base/")).as_deref(),
            Some("https://r.example/base/d/x")
        );
    }

    #[test]
    fn internal_hosts_are_not_sent_without_public_address() {
        for raw in [
            "http://core:8899/d/t",
            "http://localhost:8899/d/t",
            "http://LOCALHOST/d/t",
            "http://[::1]:80/d/t",
            "nonsense",
        ] {
            assert_eq!(public_link(raw, None), None, "{raw}");
        }
        assert_eq!(
            public_link("https://reclip.example/d/t", None).as_deref(),
            Some("https://reclip.example/d/t")
        );
    }
}
