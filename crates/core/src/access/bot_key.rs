//! The Telegram bot's API key, created by the core at startup and written to
//! a file the bot container can read (`RECLIP_BOT_KEY_FILE`).

use std::io::Write;
use std::path::Path;

use anyhow::Context;
use reclip_shared::admin::{CreateKeyRequest, Scope};

use super::key_usage::KeyUsage;
use super::keys::{create_key, verify_key};
use crate::store::Store;

pub const BOT_KEY_NAME: &str = "telegram-bot";

/// Do nothing without a configured path, else [`ensure_bot_key`].
pub async fn ensure_configured(store: &Store, path: Option<&Path>) -> anyhow::Result<()> {
    match path {
        Some(path) => ensure_bot_key(store, path).await,
        None => Ok(()),
    }
}

/// Make sure `path` holds a valid, unrevoked key with the `delegate` scope;
/// create one and write the file otherwise. A stale key is left as it is.
pub async fn ensure_bot_key(store: &Store, path: &Path) -> anyhow::Result<()> {
    if current_key_is_valid(store, path).await {
        return Ok(());
    }
    let created = create_key(
        store,
        CreateKeyRequest {
            name: BOT_KEY_NAME.into(),
            scopes: Some(vec![
                Scope::Probe,
                Scope::JobsCreate,
                Scope::JobsRead,
                Scope::JobsCancel,
                Scope::Delegate,
            ]),
            unlimited: false,
            expires_in_days: None,
            limits: None,
        },
    )
    .await
    .map_err(|e| anyhow::anyhow!("create bot key: {}", e.message))?;
    write_private(path, &created.key)?;
    tracing::info!("bot key written to {}", path.display());
    Ok(())
}

async fn current_key_is_valid(store: &Store, path: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return false;
    };
    // A scratch usage tracker: this check must not count as a key use.
    let usage = KeyUsage::default();
    match verify_key(store, &usage, raw.trim()).await {
        Ok(info) => info.scopes.contains(&Scope::Delegate),
        Err(_) => false,
    }
}

/// Write `key` and a newline through a temp file in the same folder, then
/// rename it into place. The file is created private (0600 on Unix).
fn write_private(path: &Path, key: &str) -> anyhow::Result<()> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty());
    let dir = dir.unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir).context("create temp key file")?;
    writeln!(tmp, "{key}").context("write bot key")?;
    tmp.flush().context("write bot key")?;
    tmp.persist(path)
        .map_err(|e| e.error)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}
