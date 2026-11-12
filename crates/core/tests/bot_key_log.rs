//! The bot key is never logged. Alone in its binary: tracing caches callsite
//! interest process-wide, so parallel tests without a subscriber would hide the log.

use std::io;
use std::sync::{Arc, Mutex};

use reclip_core::access::bot_key::ensure_bot_key;
use reclip_core::access::keys::parse_key;
use reclip_core::store::Store;
use tracing_subscriber::fmt::MakeWriter;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl io::Write for Capture {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Capture {
        self.clone()
    }
}

#[tokio::test]
async fn key_is_not_logged() {
    let logs = Capture::default();
    let sub = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(logs.clone())
        .finish();
    let _guard = tracing::subscriber::set_default(sub);
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("t.db")).await.unwrap();
    let path = dir.path().join("bot.key");
    ensure_bot_key(&store, &path).await.unwrap();
    let key = std::fs::read_to_string(&path).unwrap().trim().to_string();
    let (_, secret) = parse_key(&key).unwrap();
    let text = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(text.contains("bot key written to"));
    assert!(!text.contains(secret));
}
