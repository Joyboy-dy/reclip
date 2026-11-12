use super::*;
use std::collections::HashMap;

fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let m: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |k| m.get(k).cloned()
}

#[test]
fn defaults_with_key_env() {
    let c = Config::from_env(&env(&[
        ("TELEGRAM_BOT_TOKEN", "t"),
        ("RECLIP_BOT_KEY", "k"),
    ]))
    .unwrap();
    assert_eq!(c.api_url, "https://api.telegram.org");
    assert_eq!(c.core_url, "http://core:8899");
    assert_eq!(c.key, "k");
    assert_eq!(c.key_file, None);
    assert_eq!(c.max_upload, 50_000_000);
    assert_eq!(
        c.state_path,
        PathBuf::from("/var/lib/reclip-bot/state.json")
    );
    assert_eq!(c.public_url, None);
    assert_eq!(c.tmp_dir, PathBuf::from("/var/lib/reclip-bot/tmp"));
    assert_eq!(c.max_deliveries, 3);
}

#[test]
fn public_url_tmp_and_deliveries() {
    let c = Config::from_env(&env(&[
        ("TELEGRAM_BOT_TOKEN", "t"),
        ("RECLIP_BOT_KEY", "k"),
        ("RECLIP_PUBLIC_URL", "https://reclip.example/"),
        ("RECLIP_BOT_STATE", "/data/s.json"),
        ("RECLIP_BOT_MAX_DELIVERIES", "5"),
    ]))
    .unwrap();
    assert_eq!(c.public_url.as_deref(), Some("https://reclip.example"));
    assert_eq!(c.tmp_dir, PathBuf::from("/data/tmp"));
    assert_eq!(c.max_deliveries, 5);
    let c = Config::from_env(&env(&[
        ("TELEGRAM_BOT_TOKEN", "t"),
        ("RECLIP_BOT_KEY", "k"),
        ("RECLIP_BOT_TMP", "/scratch"),
    ]))
    .unwrap();
    assert_eq!(c.tmp_dir, PathBuf::from("/scratch"));
    for bad in ["0", "x"] {
        let e = Config::from_env(&env(&[
            ("TELEGRAM_BOT_TOKEN", "t"),
            ("RECLIP_BOT_KEY", "k"),
            ("RECLIP_BOT_MAX_DELIVERIES", bad),
        ]));
        assert!(e.is_err(), "{bad}");
    }
}

#[test]
fn local_api_raises_limit_and_trims_slash() {
    let c = Config::from_env(&env(&[
        ("TELEGRAM_BOT_TOKEN", "t"),
        ("RECLIP_BOT_KEY", "k"),
        ("TELEGRAM_API_URL", "http://tg:8081/"),
    ]))
    .unwrap();
    assert_eq!(c.api_url, "http://tg:8081");
    assert_eq!(c.max_upload, 2_000_000_000);
}

#[test]
fn explicit_limit_and_key_priority() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("bot.key");
    std::fs::write(&f, "from-file\n").unwrap();
    let file = f.to_str().unwrap();
    let c = Config::from_env(&env(&[
        ("TELEGRAM_BOT_TOKEN", "t"),
        ("RECLIP_BOT_KEY_FILE", file),
        ("RECLIP_BOT_MAX_UPLOAD", "10M"),
    ]))
    .unwrap();
    assert_eq!(c.key, "from-file");
    assert_eq!(c.key_file, Some(f.clone()));
    assert_eq!(c.max_upload, 10 * 1024 * 1024);
    let c = Config::from_env(&env(&[
        ("TELEGRAM_BOT_TOKEN", "t"),
        ("RECLIP_BOT_KEY_FILE", file),
        ("RECLIP_BOT_KEY", "direct"),
    ]))
    .unwrap();
    assert_eq!(c.key, "direct");
    assert_eq!(c.key_file, None);
}

#[test]
fn missing_token_or_key_fails_and_debug_hides_secrets() {
    assert!(Config::from_env(&env(&[("RECLIP_BOT_KEY", "k")])).is_err());
    assert!(
        Config::from_env(&env(&[
            ("TELEGRAM_BOT_TOKEN", "t"),
            ("RECLIP_BOT_KEY_FILE", "/nonexistent/bot.key")
        ]))
        .is_err()
    );
    let c = Config::from_env(&env(&[
        ("TELEGRAM_BOT_TOKEN", "SECRET1"),
        ("RECLIP_BOT_KEY", "SECRET2"),
    ]))
    .unwrap();
    let dbg = format!("{c:?}");
    assert!(!dbg.contains("SECRET1") && !dbg.contains("SECRET2"));
}

#[test]
fn empty_or_blank_values_behave_like_unset() {
    // docker compose passes unset variables as empty strings.
    let c = Config::from_env(&env(&[
        ("TELEGRAM_BOT_TOKEN", "t"),
        ("RECLIP_BOT_KEY", "k"),
        ("TELEGRAM_API_URL", ""),
        ("RECLIP_CORE_URL", "  "),
        ("RECLIP_BOT_MAX_UPLOAD", ""),
        ("RECLIP_BOT_STATE", ""),
        ("RECLIP_BOT_KEY_FILE", ""),
    ]))
    .unwrap();
    assert_eq!(c.api_url, OFFICIAL_API);
    assert_eq!(c.core_url, "http://core:8899");
    assert_eq!(c.max_upload, 50_000_000);
    assert_eq!(
        c.state_path,
        PathBuf::from("/var/lib/reclip-bot/state.json")
    );
    // Empty token and empty key (no file at the default path) are the missing errors.
    assert!(
        Config::from_env(&env(&[
            ("TELEGRAM_BOT_TOKEN", " "),
            ("RECLIP_BOT_KEY", "k")
        ]))
        .is_err()
    );
    let e = Config::from_env(&env(&[("TELEGRAM_BOT_TOKEN", "t"), ("RECLIP_BOT_KEY", "")]));
    assert!(e.is_err());
}

#[test]
fn public_url_is_reduced_to_an_origin_like_the_core() {
    let public = |v: &str| {
        Config::from_env(&env(&[
            ("TELEGRAM_BOT_TOKEN", "t"),
            ("RECLIP_BOT_KEY", "k"),
            ("RECLIP_PUBLIC_URL", v),
        ]))
        .unwrap()
        .public_url
    };
    assert_eq!(
        public("https://R.example/").as_deref(),
        Some("https://r.example")
    );
    assert_eq!(
        public("https://r.example:443").as_deref(),
        Some("https://r.example")
    );
    assert_eq!(
        public("http://r.example:8080").as_deref(),
        Some("http://r.example:8080")
    );
    for ignored in [
        "https://r.example/base",
        "https://r.example/?x=1",
        "https://u:p@r.example",
        "ftp://r.example",
        "r.example",
    ] {
        assert_eq!(public(ignored), None, "{ignored}");
    }
}

#[test]
fn sizes() {
    assert_eq!(parse_size("512K").unwrap(), 512 * 1024);
    assert_eq!(parse_size("2G").unwrap(), 2 * 1024 * 1024 * 1024);
    assert_eq!(parse_size("100").unwrap(), 100);
    assert!(parse_size("abc").is_err());
}
