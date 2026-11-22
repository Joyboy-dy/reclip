//! Blocked devices, Telegram accounts, keys and addresses, checked before
//! any new probe, transcript or job. The list lives in memory (reloaded
//! after each change), so a check costs no database query.

use std::net::IpAddr;
use std::sync::{Arc, RwLock};

use ipnet::IpNet;
use reclip_shared::{ApiError, BlockInfo, BlockKind, ErrorCode};

use super::caller::Caller;
use crate::http::api_error;
use crate::store::Store;

/// Narrowest ranges accepted: a mistyped /0 must not block everyone.
const MIN_V4_PREFIX: u8 = 16;
const MIN_V6_PREFIX: u8 = 32;

struct Rule {
    info: BlockInfo,
    net: Option<IpNet>,
}

#[derive(Clone, Default)]
pub struct Blocks(Arc<RwLock<Vec<Rule>>>);

impl Blocks {
    /// Read the list again from the store.
    pub async fn reload(&self, store: &Store) -> anyhow::Result<()> {
        let rules = store
            .list_blocks()
            .await?
            .into_iter()
            .map(|info| Rule {
                net: (info.kind == BlockKind::Ip)
                    .then(|| info.target.parse().ok())
                    .flatten(),
                info,
            })
            .collect();
        *self.0.write().unwrap_or_else(|e| e.into_inner()) = rules;
        Ok(())
    }

    /// The block that applies to `owner` or to `ip`, if any.
    pub fn find(&self, owner: &str, ip: Option<IpAddr>) -> Option<BlockInfo> {
        let rules = self.0.read().unwrap_or_else(|e| e.into_inner());
        rules
            .iter()
            .find(|r| match (&r.net, ip) {
                (Some(net), Some(ip)) => net.contains(&ip),
                (None, _) => r.info.kind == BlockKind::Owner && r.info.target == owner,
                _ => false,
            })
            .map(|r| r.info.clone())
    }

    /// 403 `blocked` when an admin blocked this caller or its address. The
    /// reason stays private to the admins.
    pub fn check(&self, caller: &Caller) -> Result<(), ApiError> {
        match self.find(&caller.owner, caller.origin.ip) {
            None => Ok(()),
            Some(b) => {
                tracing::info!(owner = %caller.owner, block = b.id, "blocked caller refused");
                Err(api_error(
                    ErrorCode::Blocked,
                    "Access to ReClip is blocked for this device or network",
                ))
            }
        }
    }
}

/// The stored form of a block target, or why it is refused: an owner
/// (`web:<hex>`, `tg:<digits>`, `key:<id>`) or an address / CIDR range
/// (an address alone becomes a /32 or /128).
pub fn normalize_target(kind: BlockKind, target: &str) -> Result<String, String> {
    let t = target.trim();
    match kind {
        BlockKind::Owner => {
            let (prefix, rest) = t.split_once(':').ok_or("Expected web:…, tg:… or key:…")?;
            let ok = matches!(prefix, "web" | "tg" | "key")
                && (1..=128).contains(&rest.len())
                && rest
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
            if ok {
                Ok(t.to_string())
            } else {
                Err("Expected web:…, tg:… or key:…".into())
            }
        }
        BlockKind::Ip => {
            let net = match t.parse::<IpNet>() {
                Ok(net) => net.trunc(),
                Err(_) => IpNet::from(t.parse::<IpAddr>().map_err(
                    |_| "Expected an address such as 203.0.113.7 or a range such as 203.0.113.0/24",
                )?),
            };
            let too_wide = match net {
                IpNet::V4(n) => n.prefix_len() < MIN_V4_PREFIX,
                IpNet::V6(n) => n.prefix_len() < MIN_V6_PREFIX,
            };
            if too_wide {
                return Err(format!(
                    "This range is too wide: use at least /{MIN_V4_PREFIX} (IPv4) or /{MIN_V6_PREFIX} (IPv6)"
                ));
            }
            Ok(net.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_are_checked_and_normalized() {
        let ip = |t| normalize_target(BlockKind::Ip, t);
        assert_eq!(ip("203.0.113.7").unwrap(), "203.0.113.7/32");
        assert_eq!(ip(" 203.0.113.99/24 ").unwrap(), "203.0.113.0/24");
        assert_eq!(ip("2001:db8:1:2::9/64").unwrap(), "2001:db8:1:2::/64");
        for bad in ["0.0.0.0/0", "10.0.0.0/8", "::/0", "nope", ""] {
            assert!(ip(bad).is_err(), "{bad}");
        }
        let owner = |t| normalize_target(BlockKind::Owner, t);
        assert!(
            owner("web:0123abcd").is_ok() && owner("tg:42").is_ok() && owner("key:ab12").is_ok()
        );
        for bad in ["ip:1.2.3.4", "web:", "web:a b", "42"] {
            assert!(owner(bad).is_err(), "{bad}");
        }
    }

    #[tokio::test]
    async fn owners_and_ranges_match() {
        let store = Store::memory().await;
        store
            .add_block(BlockKind::Owner, "tg:42", "spam", None)
            .await
            .unwrap();
        store
            .add_block(BlockKind::Ip, "203.0.113.0/24", "", Some("a@b.c"))
            .await
            .unwrap();
        let blocks = Blocks::default();
        blocks.reload(&store).await.unwrap();
        let ip = |s: &str| Some(s.parse::<IpAddr>().unwrap());
        assert!(blocks.find("tg:42", None).is_some());
        assert!(blocks.find("web:x", ip("203.0.113.200")).is_some());
        assert!(blocks.find("web:x", ip("203.0.114.1")).is_none());
        assert!(blocks.find("tg:43", None).is_none());
    }
}
