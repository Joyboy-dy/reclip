//! `/api/v1/history`: the visitor's finished downloads of the last
//! `history_days` days. Removing an entry only hides it.

use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use reclip_shared::{
    ErrorCode, HistoryClean, HistoryCleanMode, HistoryCleanResult, HistoryList, Scope,
};

use super::{ApiJson, HttpError, api_error, internal, not_found};
use crate::access::caller::Caller;
use crate::app::AppState;
use crate::store::now_minus;

/// Browsers may keep a thumbnail this long (it never changes).
const THUMB_CACHE: &str = "private, max-age=86400";

fn days_ago(days: u32) -> String {
    now_minus(Duration::from_secs(u64::from(days) * 24 * 3600))
}

/// Start of the history window.
fn since(s: &AppState) -> String {
    days_ago(s.jobs.config().history_days)
}

pub async fn list(
    State(s): State<AppState>,
    caller: Caller,
) -> Result<Json<HistoryList>, HttpError> {
    caller.require(Scope::JobsRead)?;
    let rows = s
        .store
        .history(&caller.owner, &since(&s))
        .await
        .map_err(internal)?;
    let mut entries = Vec::with_capacity(rows.len());
    for (mut entry, path) in rows {
        if let (true, Some(path)) = (entry.file_available, path) {
            let meta = tokio::fs::metadata(&path).await;
            entry.file_available = meta.is_ok_and(|m| m.is_file());
        }
        entries.push(entry);
    }
    Ok(Json(HistoryList { entries }))
}

pub async fn remove(State(s): State<AppState>, caller: Caller, Path(id): Path<String>) -> Response {
    if let Err(e) = caller.require(Scope::JobsCancel) {
        return e.into_response();
    }
    match s.store.hide(&caller.owner, &since(&s), &id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => not_found("History entry not found"),
        Err(e) => internal(e).into_response(),
    }
}

pub async fn clean(
    State(s): State<AppState>,
    caller: Caller,
    ApiJson(req): ApiJson<HistoryClean>,
) -> Result<Json<HistoryCleanResult>, HttpError> {
    caller.require(Scope::JobsCancel)?;
    let (expired_only, before) = match req.mode {
        HistoryCleanMode::Expired => (true, None),
        HistoryCleanMode::OlderThan => match req.days {
            Some(days) => (false, Some(days_ago(days))),
            None => {
                let msg = "days is required for older_than";
                return Err(HttpError(api_error(ErrorCode::InvalidRequest, msg)));
            }
        },
        HistoryCleanMode::All => (false, None),
    };
    let removed = s
        .store
        .hide_many(&caller.owner, &since(&s), expired_only, before.as_deref())
        .await
        .map_err(internal)?;
    Ok(Json(HistoryCleanResult {
        removed: u32::try_from(removed).unwrap_or(u32::MAX),
    }))
}

pub async fn thumb(State(s): State<AppState>, caller: Caller, Path(id): Path<String>) -> Response {
    if let Err(e) = caller.require(Scope::JobsRead) {
        return e.into_response();
    }
    let path = match s.store.thumb_path(&caller.owner, &id).await {
        Ok(Some(p)) => p,
        Ok(None) => return not_found("Thumbnail not available"),
        Err(e) => return internal(e).into_response(),
    };
    let bytes = match tokio::fs::read(&path).await {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return not_found("Thumbnail not available");
        }
        Err(e) => return internal(e).into_response(),
    };
    let headers = [
        (header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg")),
        (header::CACHE_CONTROL, HeaderValue::from_static(THUMB_CACHE)),
    ];
    (headers, bytes).into_response()
}
