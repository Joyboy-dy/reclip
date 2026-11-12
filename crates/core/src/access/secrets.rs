//! Server secrets and the public address: the admin token, the link-signing
//! key, and the site's public URL (configured, stored or learned).
//!
//! A learned URL is only used to build absolute links; it is never trusted
//! as an `Origin` (a misrouted first request could have planted it).

use std::sync::{Arc, RwLock};

use anyhow::{Context, bail};

use super::ip::Forwarded;
use super::keys::{ct_eq, random_base62, sha256_hex};
use super::origin::bare_origin;
use crate::config::Config;
use crate::store::Store;

/// Settings keys.
const ADMIN_HASH: &str = "admin_token_sha256";
const LINK_KEY: &str = "link_key";
/// Settings key of the learned public URL (`reclip-core url reset` clears it).
pub const PUBLIC_URL: &str = "public_url";
const ADMIN_PREFIX: &str = "rca_";
const ADMIN_SECRET_LEN: usize = 43;
/// Shorter `RECLIP_ADMIN_TOKEN` values get a startup warning.
const MIN_ADMIN_TOKEN_CHARS: usize = 32;

/// Instance secrets and public address. Cheap to clone; no `Debug` on
/// purpose (it holds the link key).
#[derive(Clone)]
pub struct Site {
    public_url: Arc<RwLock<Option<String>>>,
    /// Hex SHA-256 of the admin token.
    admin_hash: Arc<str>,
    /// HMAC key for signed download links.
    pub link_key: [u8; 32],
}

impl Site {
    /// Read or create the secrets, and the public URL: `RECLIP_PUBLIC_URL`,
    /// else the stored one, else none until a proxy reveals it.
    pub async fn load(cfg: &Config, store: &Store) -> anyhow::Result<Site> {
        let admin_hash = match &cfg.admin_token {
            Some(token) => {
                if is_short(token.expose()) {
                    tracing::warn!(
                        "RECLIP_ADMIN_TOKEN is short; use at least 32 random characters"
                    );
                }
                sha256_hex(token.expose())
            }
            None => stored_admin_hash(store).await?,
        };
        let public_url = match &cfg.public_url {
            Some(u) => Some(u.clone()),
            None => store.setting(PUBLIC_URL).await?,
        };
        Ok(Site {
            public_url: Arc::new(RwLock::new(public_url)),
            admin_hash: admin_hash.into(),
            link_key: link_key(store).await?,
        })
    }

    pub fn public_url(&self) -> Option<String> {
        self.public_url
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Whether `token` is the admin token (constant-time on the digests).
    pub fn is_admin(&self, token: &str) -> bool {
        ct_eq(sha256_hex(token).as_bytes(), self.admin_hash.as_bytes())
    }

    /// Remember the public origin a trusted proxy forwards, the first time
    /// one does and only when no address is known. A read lock otherwise.
    /// Call it only for requests whose caller was accepted.
    pub async fn learn(&self, store: &Store, fwd: &Forwarded) {
        let known = || {
            let url = self.public_url.read().unwrap_or_else(|e| e.into_inner());
            url.is_some()
        };
        if !fwd.trusted_peer || !fwd.forwarded_origin || known() {
            return;
        }
        let scheme = if fwd.https { "https" } else { "http" };
        let Some(url) = fwd
            .host
            .as_deref()
            .and_then(|h| bare_origin(&format!("{scheme}://{h}")))
        else {
            return;
        };
        {
            let mut known = self.public_url.write().unwrap_or_else(|e| e.into_inner());
            if known.is_some() {
                return;
            }
            *known = Some(url.clone());
        }
        tracing::warn!("learned public URL {url}; set RECLIP_PUBLIC_URL to pin it");
        if let Err(e) = store.set_setting_if_absent(PUBLIC_URL, &url).await {
            tracing::warn!(error = %e, "failed to store the public url");
        }
    }
}

/// The stored admin token digest, or a new token: logged once, here only.
async fn stored_admin_hash(store: &Store) -> anyhow::Result<String> {
    if let Some(h) = store.setting(ADMIN_HASH).await? {
        return Ok(h);
    }
    let token = format!("{ADMIN_PREFIX}{}", random_base62(ADMIN_SECRET_LEN)?);
    let hash = sha256_hex(&token);
    if store.set_setting_if_absent(ADMIN_HASH, &hash).await? {
        tracing::warn!("admin token (shown once, store it now): {token}");
        // Also on stderr, so a silenced log (RUST_LOG=off) cannot lose it.
        eprintln!("admin token (shown once, store it now): {token}");
        return Ok(hash);
    }
    // Another process stored one first: use it, never show ours.
    store
        .setting(ADMIN_HASH)
        .await?
        .context("admin token digest vanished")
}

/// The stored link-signing key, or a new random one.
async fn link_key(store: &Store) -> anyhow::Result<[u8; 32]> {
    if store.setting(LINK_KEY).await?.is_none() {
        let mut key = [0u8; 32];
        getrandom::fill(&mut key).map_err(|e| anyhow::anyhow!("getrandom: {e}"))?;
        store.set_setting_if_absent(LINK_KEY, &hex(&key)).await?;
    }
    let stored = store.setting(LINK_KEY).await?.context("link key missing")?;
    match unhex(&stored) {
        Some(key) => Ok(key),
        None => bail!("setting {LINK_KEY} is not 64 hex characters"),
    }
}

fn is_short(token: &str) -> bool {
    token.chars().count() < MIN_ADMIN_TOKEN_CHARS
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Secret;
    use std::net::IpAddr;

    fn fwd(trusted: bool, origin: bool) -> Forwarded {
        Forwarded {
            client_ip: IpAddr::from([203, 0, 113, 7]),
            trusted_peer: trusted,
            https: true,
            host: Some("r.example".into()),
            forwarded_origin: origin,
        }
    }

    #[tokio::test]
    async fn admin_token_generated_once() {
        let (cfg, store) = (Config::from_env(&|_| None), Store::memory().await);
        let a = Site::load(&cfg, &store).await.unwrap();
        let b = Site::load(&cfg, &store).await.unwrap();
        assert_eq!(a.admin_hash, b.admin_hash);
        let stored = store.setting(ADMIN_HASH).await.unwrap().unwrap();
        assert_eq!(*a.admin_hash, stored);
        assert_eq!(stored.len(), 64);
        assert!(!a.is_admin("rca_wrong"));
    }

    #[tokio::test]
    async fn env_admin_token_not_stored() {
        let mut cfg = Config::from_env(&|_| None);
        cfg.admin_token = Some(Secret::new("rca_from_env"));
        let store = Store::memory().await;
        let site = Site::load(&cfg, &store).await.unwrap();
        assert!(site.is_admin("rca_from_env"));
        assert!(!site.is_admin("rca_from_envx"));
        assert_eq!(store.setting(ADMIN_HASH).await.unwrap(), None);
    }

    #[test]
    fn short_admin_token_detected() {
        assert!(is_short("rca_test-admin-token"));
        assert!(!is_short(&"x".repeat(32)));
        assert!(is_short(&"é".repeat(31)));
    }

    #[tokio::test]
    async fn link_key_persisted() {
        let (cfg, store) = (Config::from_env(&|_| None), Store::memory().await);
        let a = Site::load(&cfg, &store).await.unwrap();
        let b = Site::load(&cfg, &store).await.unwrap();
        assert_eq!(a.link_key, b.link_key);
        assert_ne!(a.link_key, [0u8; 32]);
        let stored = store.setting(LINK_KEY).await.unwrap().unwrap();
        assert_eq!(stored, hex(&a.link_key));
        store.set_setting(LINK_KEY, "not hex").await.unwrap();
        assert!(Site::load(&cfg, &store).await.is_err());
    }

    #[tokio::test]
    async fn public_url_learned_from_trusted_proxy_only() {
        let (cfg, store) = (Config::from_env(&|_| None), Store::memory().await);
        let site = Site::load(&cfg, &store).await.unwrap();
        assert_eq!(site.public_url(), None);
        site.learn(&store, &fwd(false, false)).await;
        // A trusted peer that did not forward both proto and host.
        site.learn(&store, &fwd(true, false)).await;
        assert_eq!(site.public_url(), None);
        assert_eq!(store.setting(PUBLIC_URL).await.unwrap(), None);
        let mut odd = fwd(true, true);
        odd.host = Some("evil.example@r.example".into());
        site.learn(&store, &odd).await;
        assert_eq!(site.public_url(), None);

        site.learn(&store, &fwd(true, true)).await;
        let want = Some("https://r.example".to_string());
        assert_eq!(site.public_url(), want);
        assert_eq!(store.setting(PUBLIC_URL).await.unwrap(), want);
        // Known: never replaced, and a fresh load reads it back.
        let mut other = fwd(true, true);
        other.host = Some("other.example".into());
        site.learn(&store, &other).await;
        assert_eq!(site.public_url(), want);
        let again = Site::load(&cfg, &store).await.unwrap();
        assert_eq!(again.public_url(), want);
    }

    #[tokio::test]
    async fn configured_public_url_wins() {
        let mut cfg = Config::from_env(&|_| None);
        cfg.public_url = Some("https://conf.example".into());
        let store = Store::memory().await;
        store
            .set_setting(PUBLIC_URL, "https://old.example")
            .await
            .unwrap();
        let site = Site::load(&cfg, &store).await.unwrap();
        assert_eq!(site.public_url().as_deref(), Some("https://conf.example"));
    }
}
