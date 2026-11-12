//! Classification of yt-dlp stderr into API errors.

use reclip_shared::{ApiError, ErrorCode};

/// Map yt-dlp's stderr to an `ApiError`. `cookies_configured` means a cookies
/// file is configured and exists (see `args::cookies_available`).
pub fn classify(stderr: &str, cookies_configured: bool) -> ApiError {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let line = lines
        .iter()
        .rev()
        .find(|l| l.starts_with("ERROR:"))
        .or(lines.last());
    let Some(line) = line.copied() else {
        return err(
            ErrorCode::Internal,
            "yt-dlp failed without an error message",
        );
    };

    let low = line.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| low.contains(n));
    if has(&["not a bot"]) {
        let hint = if cookies_configured {
            " (the cookies in RECLIP_COOKIES were rejected or have expired: export fresh ones from a logged-in browser)"
        } else {
            " (YouTube is blocking this server's IP: set RECLIP_COOKIES to a cookies.txt exported from a logged-in browser)"
        };
        return err(ErrorCode::BotCheck, format!("{line}{hint}"));
    }
    let code = if has(&["unsupported url"]) {
        ErrorCode::UnsupportedUrl
    } else if has(&["private video", "login required", "requires authentication"]) {
        ErrorCode::Private
    } else if has(&["not available in your country"]) || (has(&["geo"]) && has(&["restrict"])) {
        ErrorCode::GeoBlocked
    } else if has(&["video unavailable", "has been removed", "404"]) {
        ErrorCode::Unavailable
    } else if has(&["larger than max-filesize", "file is larger than"]) {
        ErrorCode::TooLarge
    } else if has(&["does not pass filter"]) && has(&["duration"]) {
        ErrorCode::TooLong
    } else {
        ErrorCode::Internal
    };
    err(code, line)
}

fn err(code: ErrorCode, message: impl Into<String>) -> ApiError {
    ApiError {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reclip_shared::ErrorCode;

    #[test]
    fn classifies_bot_check_with_hint() {
        let e = classify(
            "ERROR: [youtube] a: Sign in to confirm you’re not a bot.",
            false,
        );
        assert_eq!(e.code, ErrorCode::BotCheck);
        assert!(e.message.contains("RECLIP_COOKIES"));
        assert!(e.message.starts_with("ERROR: [youtube] a: Sign in"));
        assert!(
            classify("ERROR: … not a bot …", true)
                .message
                .contains("expired")
        );
    }

    #[test]
    fn classifies_common_errors() {
        let code = |s: &str| classify(s, false).code;
        assert_eq!(
            code("ERROR: Unsupported URL: https://x"),
            ErrorCode::UnsupportedUrl
        );
        assert_eq!(
            code("ERROR: [youtube] a: Video unavailable"),
            ErrorCode::Unavailable
        );
        assert_eq!(
            code("ERROR: HTTP Error 404: Not Found"),
            ErrorCode::Unavailable
        );
        assert_eq!(code("ERROR: x: Private video. Sign in"), ErrorCode::Private);
        assert_eq!(code("ERROR: Login required"), ErrorCode::Private);
        assert_eq!(
            code("ERROR: This video requires authentication"),
            ErrorCode::Private
        );
        assert_eq!(
            code("ERROR: not available in your country"),
            ErrorCode::GeoBlocked
        );
        assert_eq!(
            code("ERROR: geo restriction applies"),
            ErrorCode::GeoBlocked
        );
        assert_eq!(
            code("ERROR: File is larger than max-filesize"),
            ErrorCode::TooLarge
        );
        assert_eq!(
            code("x does not pass filter (duration <=? 5), skipping"),
            ErrorCode::TooLong
        );
        assert_eq!(code("ERROR: weird thing"), ErrorCode::Internal);
    }

    #[test]
    fn uses_last_error_line_else_last_line() {
        let s = "ERROR: Unsupported URL: x\nWARNING: noise\n";
        assert_eq!(classify(s, false).code, ErrorCode::UnsupportedUrl);
        let s = "WARNING: a\nERROR: boom\nsome trailing text\n";
        assert_eq!(classify(s, false).message, "ERROR: boom");
        assert_eq!(classify("first\nlast line\n", false).message, "last line");
    }

    #[test]
    fn empty_stderr_has_default_message() {
        let e = classify("", false);
        assert_eq!(e.code, ErrorCode::Internal);
        assert_eq!(e.message, "yt-dlp failed without an error message");
        assert_eq!(classify(" \n \n", false).message, e.message);
    }
}
