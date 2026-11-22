//! `/api/v1/admin/server`: load, files, jobs and the average cost of a
//! download. Rates are measured between two requests of the page: nothing
//! samples while nobody looks.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use axum::Json;
use axum::extract::State;
use reclip_shared::ServerStats;

use super::{HttpError, internal};
use crate::app::AppState;
use crate::store::{now_minus, timestamp_of};
use crate::sysinfo;

#[derive(Clone, Copy)]
struct Sample {
    at: Instant,
    cpu_usec: Option<u64>,
    net: Option<(u64, u64)>,
}

/// The previous sample, and when the process started.
#[derive(Clone)]
pub struct LoadSampler {
    started: Instant,
    started_at: SystemTime,
    prev: Arc<Mutex<Option<Sample>>>,
}

impl Default for LoadSampler {
    fn default() -> Self {
        LoadSampler {
            started: Instant::now(),
            started_at: SystemTime::now(),
            prev: Arc::default(),
        }
    }
}

/// `(now - before) / seconds`, when both are known and time passed.
fn rate(now: Option<u64>, before: Option<u64>, secs: f64) -> Option<u64> {
    let d = now?.checked_sub(before?)?;
    (secs > 0.0).then(|| (d as f64 / secs).round() as u64)
}

pub async fn server(State(s): State<AppState>) -> Result<Json<ServerStats>, HttpError> {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get() as u32);
    let now = Sample {
        at: Instant::now(),
        cpu_usec: sysinfo::cpu_usec(),
        net: sysinfo::net_bytes(),
    };
    let prev = s
        .load
        .prev
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .replace(now);
    let (mut cpu_percent, mut rx, mut tx) = (None, None, None);
    if let Some(p) = prev {
        let secs = now.at.duration_since(p.at).as_secs_f64();
        cpu_percent = rate(now.cpu_usec, p.cpu_usec, secs)
            .map(|usec_per_sec| (usec_per_sec as f64 / 1e6 / f64::from(cores) * 100.0) as f32);
        rx = rate(now.net.map(|n| n.0), p.net.map(|n| n.0), secs);
        tx = rate(now.net.map(|n| n.1), p.net.map(|n| n.1), secs);
    }
    let (memory_bytes, memory_limit) = sysinfo::memory();
    let counts = s.jobs.counts();
    let window = Duration::from_secs(u64::from(s.cfg.history_days) * 86_400);
    let costs = s
        .store
        .download_costs(&now_minus(window))
        .await
        .map_err(internal)?;
    // CPU per download: this process and its tools, over what it finished.
    let since_start = s
        .store
        .download_costs(&timestamp_of(s.load.started_at))
        .await
        .map_err(internal)?;
    let avg_cpu_secs = sysinfo::process_cpu_secs()
        .filter(|_| since_start.done > 0)
        .map(|cpu| cpu / f64::from(since_start.done));
    Ok(Json(ServerStats {
        uptime_secs: s.load.started.elapsed().as_secs(),
        cpu_percent,
        cpu_cores: cores,
        memory_bytes,
        memory_limit,
        files_bytes: s.jobs.disk_bytes(),
        files_limit: s.cfg.max_disk,
        jobs_running: counts.running as u32,
        jobs_queued: counts.queued as u32,
        net_rx_rate: rx,
        net_tx_rate: tx,
        downloads_done: costs.done,
        avg_duration_secs: costs.avg_duration_ms.map(|ms| ms / 1000.0),
        avg_size_bytes: costs.avg_size.map(|b| b.round() as u64),
        avg_cpu_secs,
    }))
}

#[cfg(test)]
mod tests {
    use super::rate;

    #[test]
    fn rates() {
        assert_eq!(rate(Some(3000), Some(1000), 2.0), Some(1000));
        assert_eq!(rate(Some(1000), Some(3000), 2.0), None);
        assert_eq!(rate(None, Some(1), 1.0), None);
        assert_eq!(rate(Some(5), Some(1), 0.0), None);
    }
}
