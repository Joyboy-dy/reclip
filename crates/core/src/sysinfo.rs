//! Load of the container, read from cgroup v2 and `/proc` (Linux). Every
//! value is `None` elsewhere. Read only when the admin's server view asks:
//! nothing samples in the background.

use std::path::Path;

/// Linux reports process times in USER_HZ ticks, 100 per second on every
/// mainstream architecture.
const TICKS_PER_SEC: u64 = 100;

fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(Path::new(path)).ok()
}

/// `usage_usec` of a cgroup v2 `cpu.stat`.
pub fn parse_cpu_stat(text: &str) -> Option<u64> {
    text.lines()
        .find_map(|l| l.strip_prefix("usage_usec "))
        .and_then(|v| v.trim().parse().ok())
}

/// utime + stime + cutime + cstime of a `/proc/<pid>/stat` line, in ticks:
/// this process and the tools it ran and waited for (yt-dlp, ffmpeg).
pub fn parse_proc_ticks(text: &str) -> Option<u64> {
    // The command name may hold spaces: fields start after the last ')'.
    let rest = &text[text.rfind(')')? + 1..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    // rest[0] is the state (field 3): utime..cstime are fields 14 to 17.
    let tick = |i: usize| f.get(i)?.parse::<i64>().ok();
    let sum = tick(11)? + tick(12)? + tick(13)? + tick(14)?;
    u64::try_from(sum).ok()
}

/// Received and sent bytes of every interface but loopback, from `/proc/net/dev`.
pub fn parse_net_dev(text: &str) -> Option<(u64, u64)> {
    let mut total = None;
    for line in text.lines().skip(2) {
        let Some((name, data)) = line.split_once(':') else {
            continue;
        };
        if name.trim() == "lo" {
            continue;
        }
        let f: Vec<u64> = data
            .split_whitespace()
            .filter_map(|v| v.parse().ok())
            .collect();
        if let (Some(rx), Some(tx)) = (f.first(), f.get(8)) {
            let (r, t) = total.unwrap_or((0, 0));
            total = Some((r + rx, t + tx));
        }
    }
    total
}

/// `VmRSS` of a `/proc/<pid>/status`, in bytes.
pub fn parse_rss(text: &str) -> Option<u64> {
    let kb: u64 = text
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    Some(kb * 1024)
}

/// CPU time used so far, in microseconds: the whole container when it has
/// a cgroup, else this process and its finished children.
pub fn cpu_usec() -> Option<u64> {
    read("/sys/fs/cgroup/cpu.stat")
        .as_deref()
        .and_then(parse_cpu_stat)
        .or_else(|| process_ticks().map(|t| t * 1_000_000 / TICKS_PER_SEC))
}

/// CPU seconds of this process and the tools it ran, since it started.
pub fn process_cpu_secs() -> Option<f64> {
    process_ticks().map(|t| t as f64 / TICKS_PER_SEC as f64)
}

fn process_ticks() -> Option<u64> {
    read("/proc/self/stat")
        .as_deref()
        .and_then(parse_proc_ticks)
}

/// Memory in use and the container's limit (`None` when unlimited).
pub fn memory() -> (Option<u64>, Option<u64>) {
    let num = |p: &str| read(p).and_then(|v| v.trim().parse::<u64>().ok());
    match num("/sys/fs/cgroup/memory.current") {
        Some(used) => (Some(used), num("/sys/fs/cgroup/memory.max")),
        None => (
            read("/proc/self/status").as_deref().and_then(parse_rss),
            None,
        ),
    }
}

/// Bytes received and sent by the container's interfaces so far.
pub fn net_bytes() -> Option<(u64, u64)> {
    read("/proc/net/dev").as_deref().and_then(parse_net_dev)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsers() {
        assert_eq!(
            parse_cpu_stat("usage_usec 123456\nuser_usec 1\n"),
            Some(123_456)
        );
        let stat =
            "42 (reclip core) S 1 42 42 0 -1 4194560 100 0 0 0 250 50 30 20 20 0 8 0 100 0 0";
        assert_eq!(parse_proc_ticks(stat), Some(250 + 50 + 30 + 20));
        let dev = "Inter-|   Receive  |  Transmit\n face |bytes packets|bytes\n    lo: 999 1 0 0 0 0 0 0 999 1 0 0 0 0 0 0\n  eth0: 1000 10 0 0 0 0 0 0 4000 20 0 0 0 0 0 0\n  eth1: 5 1 0 0 0 0 0 0 7 1 0 0 0 0 0 0\n";
        assert_eq!(parse_net_dev(dev), Some((1005, 4007)));
        assert_eq!(
            parse_rss("Name:\tx\nVmRSS:\t   2048 kB\n"),
            Some(2 * 1024 * 1024)
        );
    }
}
