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
/// The temp file is made before the key, so an unusable path fails without
/// leaving a key behind; a later write failure revokes the new key.
pub async fn ensure_bot_key(store: &Store, path: &Path) -> anyhow::Result<()> {
    if current_key_is_valid(store, path).await {
        make_private(path);
        return Ok(());
    }
    let tmp = temp_file_beside(path)?;
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
    if let Err(e) = write_and_rename(tmp, path, &created.key) {
        if let Err(re) = store.revoke_key(&created.info.id).await {
            tracing::warn!(error = %re, "could not revoke the unused bot key");
        }
        return Err(e);
    }
    tracing::info!("bot key written to {}", path.display());
    revoke_other_bot_keys(store, &created.info.id).await;
    Ok(())
}

/// Revoke every other active key named [`BOT_KEY_NAME`], so a replaced key
/// leaves no orphan with delegation power. Failures are logged, not fatal.
async fn revoke_other_bot_keys(store: &Store, keep_id: &str) {
    let keys = match store.list_keys().await {
        Ok(keys) => keys,
        Err(e) => {
            tracing::warn!(error = %e, "could not list keys to revoke old bot keys");
            return;
        }
    };
    for key in keys {
        if key.name != BOT_KEY_NAME || key.id == keep_id || key.revoked_at.is_some() {
            continue;
        }
        match store.revoke_key(&key.id).await {
            Ok(_) => tracing::info!(key_id = %key.id, "revoked an old bot key"),
            Err(e) => {
                tracing::warn!(key_id = %key.id, error = %e, "could not revoke an old bot key")
            }
        }
    }
}

async fn current_key_is_valid(store: &Store, path: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return false;
    };
    // A fresh tracker: verify_key records one use, which is harmless here.
    let usage = KeyUsage::default();
    match verify_key(store, &usage, raw.trim()).await {
        Ok(info) => info.scopes.contains(&Scope::Delegate),
        Err(_) => false,
    }
}

/// A private temp file in the folder of `path` (created if missing).
fn temp_file_beside(path: &Path) -> anyhow::Result<tempfile::NamedTempFile> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty());
    let dir = dir.unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    tempfile::NamedTempFile::new_in(dir)
        .with_context(|| format!("cannot write the bot key file {}", path.display()))
}

/// Write `key` and a newline, then rename the temp file onto `path`.
fn write_and_rename(
    mut tmp: tempfile::NamedTempFile,
    path: &Path,
    key: &str,
) -> anyhow::Result<()> {
    let ctx = || format!("cannot write the bot key file {}", path.display());
    writeln!(tmp, "{key}").with_context(ctx)?;
    tmp.flush().with_context(ctx)?;
    tmp.as_file().sync_all().with_context(ctx)?;
    tmp.persist(path).map_err(|e| e.error).with_context(ctx)?;
    Ok(())
}

/// Tighten a reused key file to 0600 (best effort, Unix only).
fn make_private(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path)
            && meta.permissions().mode() & 0o777 != 0o600
        {
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}
