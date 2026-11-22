//! `/api/v1/admin/activity` and `/api/v1/admin/blocks`: who uses ReClip,
//! signs of a shared link, and blocking.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use ipnet::IpNet;
use reclip_shared::{
    ActivityReport, BlockInfo, BlockList, ClientFlag, ClientInfo, ClientKind, CreateBlockRequest,
    ErrorCode, SharedIp,
};

use super::admin::AdminWho;
use super::{ApiJson, HttpError, api_error, internal, not_found};
use crate::access::blocks::normalize_target;
use crate::app::AppState;
use crate::store::{ClientRow, now_minus};

const HOUR: Duration = Duration::from_secs(3600);
const DAY: Duration = Duration::from_secs(86_400);
/// Identities listed, most recently seen first.
const CLIENT_LIMIT: u32 = 500;
/// A device seen from this many addresses in 24 h may be shared.
const MANY_IPS_DAY: u32 = 5;
/// An address this many web devices used in 7 days may be shared.
const SHARED_IP_DEVICES: u32 = 4;
/// New web devices in one hour that make a burst.
const BURST_PER_HOUR: u32 = 10;
const MAX_REASON: usize = 200;

/// An address as stored (`1.2.3.4`, or an IPv6 /64): the address to match.
fn stored_ip(text: &str) -> Option<IpAddr> {
    text.parse::<IpAddr>()
        .ok()
        .or_else(|| text.parse::<IpNet>().ok().map(|n| n.addr()))
}

fn kind_of(owner: &str) -> ClientKind {
    match owner.split_once(':').map(|(p, _)| p) {
        Some("tg") => ClientKind::Telegram,
        Some("key") => ClientKind::Key,
        _ => ClientKind::Web,
    }
}

/// `@username`, the Telegram name, the key's name, else the device.
fn label(row: &ClientRow, keys: &HashMap<String, String>) -> String {
    let id = row.owner.split_once(':').map_or("", |(_, id)| id);
    match kind_of(&row.owner) {
        ClientKind::Telegram => row
            .tg_username
            .as_ref()
            .map(|u| format!("@{u}"))
            .or_else(|| row.tg_name.clone())
            .unwrap_or_else(|| format!("Telegram {id}")),
        ClientKind::Key => keys
            .get(id)
            .cloned()
            .unwrap_or_else(|| format!("Key {}", id.chars().take(8).collect::<String>())),
        ClientKind::Web => row
            .device
            .clone()
            .unwrap_or_else(|| "Unknown browser".into()),
    }
}

pub async fn report(State(s): State<AppState>) -> Result<Json<ActivityReport>, HttpError> {
    let (day, week) = (now_minus(DAY), now_minus(7 * DAY));
    let rows = s
        .store
        .clients(&day, &week, CLIENT_LIMIT)
        .await
        .map_err(internal)?;
    let mut ips = s.store.client_ips().await.map_err(internal)?;
    let shared = s
        .store
        .shared_ips(&week, SHARED_IP_DEVICES)
        .await
        .map_err(internal)?;
    let keys: HashMap<String, String> = s
        .store
        .list_keys()
        .await
        .map_err(internal)?
        .into_iter()
        .map(|k| (k.id, k.name))
        .collect();
    let shared_set: HashSet<&str> = shared.iter().map(|(ip, _, _)| ip.as_str()).collect();

    let clients = rows
        .iter()
        .map(|r| {
            let kind = kind_of(&r.owner);
            let last = r.last_ip.as_deref();
            let mut flags = Vec::new();
            if r.ips_day >= MANY_IPS_DAY {
                flags.push(ClientFlag::ManyIps);
            }
            if kind == ClientKind::Web && last.is_some_and(|ip| shared_set.contains(ip)) {
                flags.push(ClientFlag::SharedIp);
            }
            ClientInfo {
                owner: r.owner.clone(),
                kind,
                label: label(r, &keys),
                device: r.device.clone(),
                tg_username: r.tg_username.clone(),
                tg_name: r.tg_name.clone(),
                first_seen: r.first_seen.clone(),
                last_seen: r.last_seen.clone(),
                last_ip: r.last_ip.clone(),
                ips_day: r.ips_day,
                ips_total: r.ips_total,
                ips: ips.remove(&r.owner).unwrap_or_default(),
                downloads_day: r.downloads_day,
                downloads_week: r.downloads_week,
                downloads_total: r.downloads_total,
                bytes_total: r.bytes_total,
                block: s.blocks.find(&r.owner, last.and_then(stored_ip)),
                flags,
            }
        })
        .collect();
    let shared_ips = shared
        .into_iter()
        .map(|(ip, devices, last_seen)| SharedIp {
            block: stored_ip(&ip).and_then(|addr| s.blocks.find("", Some(addr))),
            ip,
            devices,
            last_seen,
        })
        .collect();
    let new_devices_hour = s
        .store
        .new_devices(&now_minus(HOUR))
        .await
        .map_err(internal)?;
    let new_devices_day = s.store.new_devices(&day).await.map_err(internal)?;
    Ok(Json(ActivityReport {
        clients,
        shared_ips,
        new_devices_hour,
        new_devices_day,
        burst: new_devices_hour >= BURST_PER_HOUR,
        retention_days: s.cfg.history_days,
    }))
}

pub async fn list_blocks(State(s): State<AppState>) -> Result<Json<BlockList>, HttpError> {
    let blocks = s.store.list_blocks().await.map_err(internal)?;
    Ok(Json(BlockList { blocks }))
}

/// Block a device, Telegram account, key, address or range; blocking the
/// same target again updates its reason.
pub async fn block(
    State(s): State<AppState>,
    Extension(who): Extension<AdminWho>,
    ApiJson(req): ApiJson<CreateBlockRequest>,
) -> Result<(StatusCode, Json<BlockInfo>), HttpError> {
    let invalid = |m: &str| HttpError(api_error(ErrorCode::InvalidRequest, m));
    let target = normalize_target(req.kind, &req.target).map_err(|m| invalid(&m))?;
    let reason: String = req.reason.trim().chars().take(MAX_REASON).collect();
    let info = s
        .store
        .add_block(req.kind, &target, &reason, who.0.as_deref())
        .await
        .map_err(internal)?;
    s.blocks.reload(&s.store).await.map_err(internal)?;
    tracing::info!(id = info.id, target = %info.target, "blocked");
    Ok((StatusCode::CREATED, Json(info)))
}

pub async fn unblock(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    match s.store.remove_block(id).await {
        Ok(true) => {
            if let Err(e) = s.blocks.reload(&s.store).await {
                return internal(e).into_response();
            }
            tracing::info!(id, "unblocked");
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => not_found("Block not found"),
        Err(e) => internal(e).into_response(),
    }
}
