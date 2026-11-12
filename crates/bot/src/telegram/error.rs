//! Bot API error type and response envelope.

use std::error::Error;
use std::fmt;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use super::types::MessageId;

/// A Bot API failure. Never contains the bot token (no URL is kept).
#[derive(Debug, Clone)]
pub struct TgError {
    /// Bot API `error_code`, or `None` for transport failures.
    pub code: Option<u16>,
    pub description: String,
    pub retry_after: Option<u64>,
}

impl fmt::Display for TgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.code {
            Some(c) => write!(f, "telegram error {c}: {}", self.description),
            None => write!(f, "telegram request failed: {}", self.description),
        }
    }
}

impl Error for TgError {}

impl TgError {
    pub(super) fn transport(e: reqwest::Error) -> Self {
        // The Bot API URL contains the token: drop it from the error. The
        // source chain (connect, timeout...) carries no URL.
        let e = e.without_url();
        let mut description = e.to_string();
        let mut src = e.source();
        while let Some(s) = src {
            description.push_str(": ");
            description.push_str(&s.to_string());
            src = s.source();
        }
        Self {
            code: None,
            description,
            retry_after: None,
        }
    }

    pub(super) fn local(msg: impl Into<String>) -> Self {
        Self {
            code: None,
            description: msg.into(),
            retry_after: None,
        }
    }
}

#[derive(Deserialize)]
struct Envelope<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
    error_code: Option<u16>,
    parameters: Option<Params>,
}

#[derive(Deserialize)]
struct Params {
    retry_after: Option<u64>,
}

#[derive(Deserialize)]
pub(super) struct SentMessage {
    pub(super) message_id: MessageId,
}

/// Reads a Bot API answer. Keys on `ok`, not on the HTTP status.
pub(super) async fn parse<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T, TgError> {
    let status = resp.status().as_u16();
    let env: Envelope<T> = resp.json().await.map_err(|e| {
        let mut err = TgError::transport(e);
        err.code = Some(status);
        err
    })?;
    match (env.ok, env.result) {
        (true, Some(r)) => Ok(r),
        _ => Err(TgError {
            code: env.error_code.or(Some(status)),
            description: env.description.unwrap_or_default(),
            retry_after: env.parameters.and_then(|p| p.retry_after),
        }),
    }
}
