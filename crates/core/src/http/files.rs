use axum::extract::{Path, Request, State};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use reclip_shared::JobState;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use super::{internal, not_found};
use crate::app::AppState;

/// RFC 5987 attr-char: everything else is percent-encoded.
const ATTR_CHAR: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'!')
    .remove(b'#')
    .remove(b'$')
    .remove(b'&')
    .remove(b'+')
    .remove(b'-')
    .remove(b'.')
    .remove(b'^')
    .remove(b'_')
    .remove(b'`')
    .remove(b'|')
    .remove(b'~');

pub fn content_disposition(filename: &str) -> String {
    let ascii: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() && c != '"' && c != '\\' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded = utf8_percent_encode(filename, ATTR_CHAR);
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}")
}

pub async fn download(State(s): State<AppState>, Path(id): Path<String>, req: Request) -> Response {
    let job = match s.store.get(&id).await {
        Ok(Some(j)) if j.state == JobState::Done => j,
        Ok(_) => return not_found("File not available"),
        Err(e) => return internal(e).into_response(),
    };
    let path = match s.store.file_path(&id).await {
        Ok(Some(p)) if p.is_file() => p,
        Ok(_) => return not_found("File not available"),
        Err(e) => return internal(e).into_response(),
    };
    let name = job.filename.unwrap_or_else(|| job.id.clone());
    let mut res = match ServeFile::new(path).oneshot(req).await {
        Ok(r) => r.into_response(),
        Err(e) => match e {},
    };
    if let Ok(v) = HeaderValue::from_str(&content_disposition(&name)) {
        res.headers_mut().insert(header::CONTENT_DISPOSITION, v);
    }
    res
}

#[cfg(test)]
mod tests {
    use super::content_disposition;

    #[test]
    fn disposition_escapes() {
        assert_eq!(
            content_disposition("a\"b\\c ✨.mp4"),
            "attachment; filename=\"a_b_c _.mp4\"; filename*=UTF-8''a%22b%5Cc%20%E2%9C%A8.mp4"
        );
    }
}
