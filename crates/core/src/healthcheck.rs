//! Minimal local health probe, used by `reclip-core healthcheck`.
//! A plain HTTP/1.0 GET over TCP keeps the image free of curl and TLS clients.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub const TIMEOUT: Duration = Duration::from_secs(3);

/// GET `/healthz` on 127.0.0.1:`port`; true only on a 200 status.
pub async fn probe(port: u16, timeout: Duration) -> bool {
    tokio::time::timeout(timeout, async {
        let mut s = TcpStream::connect(("127.0.0.1", port)).await.ok()?;
        s.write_all(b"GET /healthz HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n")
            .await
            .ok()?;
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).await.ok()?;
        let head = String::from_utf8_lossy(&buf);
        let status = head.lines().next()?.split_whitespace().nth(1)?.to_string();
        Some(status == "200")
    })
    .await
    .ok()
    .flatten()
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    async fn serve_once(reply: &'static [u8]) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut b = [0u8; 512];
            let _ = s.read(&mut b).await;
            s.write_all(reply).await.unwrap();
        });
        port
    }

    #[tokio::test]
    async fn ok_only_on_200() {
        let p = serve_once(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok").await;
        assert!(probe(p, TIMEOUT).await);
        let p = serve_once(b"HTTP/1.1 503 Service Unavailable\r\n\r\n").await;
        assert!(!probe(p, TIMEOUT).await);
    }

    #[tokio::test]
    async fn false_when_nothing_listens() {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let p = l.local_addr().unwrap().port();
        drop(l);
        assert!(!probe(p, Duration::from_millis(500)).await);
    }
}
