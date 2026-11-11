//! Process-group handling for yt-dlp and everything it spawns.
//!
//! The shipped yt-dlp is a PyInstaller one-file launcher that starts Python,
//! which starts ffmpeg. Killing only the launcher would orphan the others, so
//! on unix yt-dlp gets its own process group and the whole group is signalled.

use std::collections::VecDeque;
use std::io;
use std::process::ExitStatus;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader};
use tokio::process::{Child, Command};

/// Spawn `cmd` as the leader of a new process group (unix only; elsewhere a
/// plain spawn).
pub fn spawn_group(cmd: &mut Command) -> io::Result<Child> {
    #[cfg(unix)]
    cmd.process_group(0);
    cmd.spawn()
}

/// Stop the child and everything it spawned, then reap it.
///
/// Unix: SIGTERM to the group, wait up to `grace` for the leader to exit, then
/// SIGKILL to the group (sweeping members that outlived the leader). Elsewhere
/// the child alone is killed.
pub async fn terminate(child: &mut Child, grace: Duration) {
    #[cfg(unix)]
    if let Some(pgid) = child.id().and_then(|id| libc::pid_t::try_from(id).ok()) {
        // A pgid of 0 or 1 would signal ourselves or everything: never do that.
        if pgid > 1 {
            signal_group(pgid, libc::SIGTERM);
            let _ = tokio::time::timeout(grace, child.wait()).await;
            signal_group(pgid, libc::SIGKILL);
            let _ = child.wait().await;
            return;
        }
    }
    let _ = grace;
    let _ = child.start_kill();
    let _ = child.wait().await;
}

#[cfg(unix)]
fn signal_group(pgid: libc::pid_t, signal: libc::c_int) {
    // SAFETY: killpg has no memory-safety preconditions.
    if unsafe { libc::killpg(pgid, signal) } != 0 {
        let e = io::Error::last_os_error();
        // ESRCH: the group is already gone, which is what we want.
        if e.raw_os_error() != Some(libc::ESRCH) {
            tracing::warn!(pgid, signal, error = %e, "cannot signal yt-dlp process group");
        }
    }
}

/// What a finished child printed.
pub struct Captured {
    pub status: ExitStatus,
    /// The last `keep` stdout lines.
    pub stdout: String,
    pub stderr: String,
}

/// Drain stdout (line by line, through `on_line`) and stderr concurrently so
/// a chatty child never blocks on a full pipe, then wait for the exit status.
/// The child must have been spawned with both streams piped.
pub async fn capture(
    child: &mut Child,
    keep: usize,
    on_line: &mut (impl FnMut(&str) + Send),
) -> io::Result<Captured> {
    let stdout = child.stdout.take().ok_or_else(missing_pipe)?;
    let mut stderr = child.stderr.take().ok_or_else(missing_pipe)?;
    let err = async {
        let mut buf = Vec::new();
        stderr.read_to_end(&mut buf).await.map(|_| buf)
    };
    let (out, err) = tokio::join!(read_lines(stdout, keep, on_line), err);
    let status = child.wait().await?;
    Ok(Captured {
        status,
        stdout: out?,
        stderr: String::from_utf8_lossy(&err?).into_owned(),
    })
}

async fn read_lines(
    stream: impl AsyncRead + Unpin,
    keep: usize,
    on_line: &mut (impl FnMut(&str) + Send),
) -> io::Result<String> {
    let mut reader = BufReader::new(stream);
    let mut tail = VecDeque::new();
    let mut buf = Vec::new();
    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf).await? == 0 {
            break;
        }
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim_end_matches(['\r', '\n']);
        on_line(line);
        if tail.len() == keep {
            tail.pop_front();
        }
        tail.push_back(line.to_owned());
    }
    Ok(Vec::from(tail).join("\n"))
}

fn missing_pipe() -> io::Error {
    io::Error::other("child stdout/stderr not piped")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::Instant;

    #[tokio::test]
    async fn sigterm_is_enough_for_a_polite_child() {
        let mut child = spawn_group(Command::new("sleep").arg("30")).unwrap();
        let t = Instant::now();
        terminate(&mut child, Duration::from_secs(5)).await;
        assert!(t.elapsed() < Duration::from_secs(2));
        assert!(child.try_wait().unwrap().is_some());
    }

    #[tokio::test]
    async fn sigkill_after_grace_when_sigterm_is_ignored() {
        let mut child =
            spawn_group(Command::new("sh").args(["-c", "trap '' TERM; sleep 30"])).unwrap();
        // Let the shell install its trap before signalling.
        tokio::time::sleep(Duration::from_millis(200)).await;
        let t = Instant::now();
        terminate(&mut child, Duration::from_millis(300)).await;
        let took = t.elapsed();
        assert!(took >= Duration::from_millis(300), "{took:?}");
        assert!(took < Duration::from_secs(3), "{took:?}");
        assert!(child.try_wait().unwrap().is_some());
    }

    #[tokio::test]
    async fn terminating_an_exited_child_is_fine() {
        let mut child = spawn_group(&mut Command::new("true")).unwrap();
        child.wait().await.unwrap();
        terminate(&mut child, Duration::from_millis(100)).await;
    }
}
