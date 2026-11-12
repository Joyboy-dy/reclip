//! Size-capped file download from the core.

use std::path::Path;

use reclip_bot::core_client::{Core, HttpCore};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn no_files(dir: &Path) -> bool {
    std::fs::read_dir(dir).unwrap().next().is_none()
}

#[tokio::test]
async fn capped_download_checks_content_length() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/j1/file"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![1u8; 300_000]))
        .mount(&server)
        .await;
    let core = HttpCore::new(&server.uri(), "k").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("f.mp4");
    let got = core
        .download_file_capped(9, "j1", &dest, 1000)
        .await
        .unwrap();
    assert_eq!(got, None);
    assert!(no_files(dir.path()), "nothing written");
    let got = core
        .download_file_capped(9, "j1", &dest, 300_000)
        .await
        .unwrap();
    assert_eq!(got, Some(300_000));
    assert_eq!(std::fs::metadata(&dest).unwrap().len(), 300_000);
}

/// A chunked answer (no Content-Length) of three 1000-byte chunks.
async fn chunked_server() -> String {
    use tokio::io::AsyncWriteExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = tokio::io::AsyncReadExt::read(&mut sock, &mut buf).await;
                let head =
                    "HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n";
                let _ = sock.write_all(head.as_bytes()).await;
                for _ in 0..3 {
                    let _ = sock.write_all(b"3e8\r\n").await;
                    let _ = sock.write_all(&[2u8; 1000]).await;
                    let _ = sock.write_all(b"\r\n").await;
                    let _ = sock.flush().await;
                }
                let _ = sock.write_all(b"0\r\n\r\n").await;
            });
        }
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn capped_download_stops_on_bytes_received() {
    let core = HttpCore::new(&chunked_server().await, "k").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("f.mp3");
    let got = core
        .download_file_capped(9, "j1", &dest, 1500)
        .await
        .unwrap();
    assert_eq!(got, None);
    assert!(no_files(dir.path()), "partial file removed");
    let got = core
        .download_file_capped(9, "j1", &dest, 3000)
        .await
        .unwrap();
    assert_eq!(got, Some(3000));
}
