//! Tests of the environment configuration.

use super::*;

#[test]
fn config_defaults_and_overrides() {
    let c = Config::from_env(&|_| None);
    assert_eq!(
        (c.port, c.playlist_limit, c.job_timeout.as_secs()),
        (8899, 50, 1800)
    );
    assert_eq!((c.max_filesize.as_str(), c.max_duration), ("2G", 10800));
    assert_eq!(c.downloads_dir, PathBuf::from("./data").join("downloads"));
    let c = Config::from_env(&|k| (k == "RECLIP_PORT").then(|| "9000".into()));
    assert_eq!(c.port, 9000);
    let c = Config::from_env(&|k| (k == "PORT").then(|| "9100".into()));
    assert_eq!(c.port, 9100);
    let c = Config::from_env(&|k| (k == "RECLIP_PROXY").then(|| "".into()));
    assert_eq!(c.proxy, None);
}

#[test]
fn queue_and_lifecycle_settings() {
    let c = Config::from_env(&|_| None);
    assert_eq!((c.max_concurrent, c.max_per_site), (4, 2));
    assert_eq!(c.file_ttl, Duration::from_secs(21600));
    assert_eq!(c.cleanup_interval, Duration::from_secs(600));
    let env = |k: &str| match k {
        "RECLIP_MAX_CONCURRENT" => Some("8".into()),
        "RECLIP_MAX_PER_SITE" => Some("3".into()),
        "RECLIP_FILE_TTL" => Some("60".into()),
        "RECLIP_CLEANUP_INTERVAL" => Some("5".into()),
        _ => None,
    };
    let c = Config::from_env(&env);
    assert_eq!((c.max_concurrent, c.max_per_site), (8, 3));
    assert_eq!(c.file_ttl, Duration::from_secs(60));
    assert_eq!(c.cleanup_interval, Duration::from_secs(5));
    // Zero would stall the queue forever: at least one slot each.
    let c = Config::from_env(&|k| k.starts_with("RECLIP_MAX_").then(|| "0".into()));
    assert_eq!((c.max_concurrent, c.max_per_site), (1, 1));
}

#[test]
fn phase3b_defaults() {
    let c = Config::from_env(&|_| None);
    assert_eq!(c.concurrent_fragments, 4);
    assert_eq!(c.impersonate.as_deref(), Some("chrome"));
    assert_eq!(c.max_disk, 5 * 1024 * 1024 * 1024);
    assert_eq!(c.history_days, 30);
    assert!(c.admin_emails.is_empty() && c.admin_default_password.is_none());
    assert_eq!(c.admin_session_ttl, Duration::from_secs(12 * 3600));
}

#[test]
fn phase3b_overrides() {
    let env = |k: &str| match k {
        "RECLIP_CONCURRENT_FRAGMENTS" => Some("8".into()),
        "RECLIP_IMPERSONATE" => Some("safari".into()),
        "RECLIP_MAX_DISK" => Some("5G".into()),
        "RECLIP_HISTORY_DAYS" => Some("7".into()),
        "RECLIP_ADMIN_EMAILS" => Some(" Ann@Example.com, bob@example.com ,,".into()),
        "RECLIP_ADMIN_DEFAULT_PASSWORD" => Some("first-login-pw".into()),
        _ => None,
    };
    let c = Config::from_env(&env);
    assert_eq!(c.concurrent_fragments, 8);
    assert_eq!(c.impersonate.as_deref(), Some("safari"));
    assert_eq!(c.max_disk, 5_368_709_120);
    assert_eq!(c.history_days, 7);
    assert_eq!(c.admin_emails, ["ann@example.com", "bob@example.com"]);
    let pw = c.admin_default_password.as_ref().unwrap();
    assert_eq!(pw.expose(), "first-login-pw");
    assert_eq!(format!("{pw:?}"), "<redacted>");
}

#[test]
fn empty_impersonate_disables_it() {
    let c = Config::from_env(&|k| (k == "RECLIP_IMPERSONATE").then(|| "".into()));
    assert_eq!(c.impersonate, None);
}

#[test]
fn size_suffixes() {
    for (raw, want) in [
        ("1024", Some(1024)),
        ("2K", Some(2048)),
        ("3m", Some(3 << 20)),
        ("5G", Some(5 << 30)),
        ("x", None),
        ("", None),
    ] {
        assert_eq!(parse_size(raw), want, "{raw}");
    }
}

#[test]
fn downloads_dir_follows_data_dir() {
    let c = Config::from_env(&|k| (k == "RECLIP_DATA_DIR").then(|| "/srv/r".into()));
    assert_eq!(c.downloads_dir, PathBuf::from("/srv/r").join("downloads"));
}

#[test]
fn access_defaults() {
    let c = Config::from_env(&|_| None);
    assert_eq!(c.trusted_proxies.len(), 6);
    assert!(c.public_web && c.public_url.is_none() && c.admin_token.is_none());
    assert_eq!(c.link_ttl, Duration::from_secs(21600));
    let l = |a, b, c| Limits {
        probes_per_minute: a,
        jobs_per_hour: b,
        max_active: c,
    };
    assert_eq!(c.limits.web, l(120, 300, 200));
    assert_eq!(c.limits.key, l(120, 200, 100));
    assert_eq!(c.limits.tg, l(30, 40, 10));
}

#[test]
fn limits_override_and_zero() {
    let env = |k: &str| match k {
        "RECLIP_LIMIT_TG_ACTIVE" => Some("0".into()),
        "RECLIP_LIMIT_WEB_PROBES_PER_MIN" => Some("7".into()),
        "RECLIP_LIMIT_KEY_JOBS_PER_HOUR" => Some("abc".into()),
        "RECLIP_PUBLIC_URL" => Some("https://r.example/".into()),
        _ => None,
    };
    let c = Config::from_env(&env);
    assert_eq!(c.limits.tg.max_active, 0);
    assert_eq!(c.limits.web.probes_per_minute, 7);
    assert_eq!(c.limits.key.jobs_per_hour, 200);
    assert_eq!(c.public_url.as_deref(), Some("https://r.example"));
    let c = Config::from_env(&|k| (k == "RECLIP_PUBLIC_URL").then(|| "//".into()));
    assert_eq!(c.public_url, None);
}

#[test]
fn bad_proxy_entry_skipped() {
    let c = Config::from_env(&|k| {
        (k == "RECLIP_TRUSTED_PROXIES").then(|| "10.0.0.0/8, nope, 203.0.113.5, ::1".into())
    });
    let want: Vec<IpNet> = ["10.0.0.0/8", "203.0.113.5/32", "::1/128"]
        .iter()
        .map(|s| s.parse().unwrap())
        .collect();
    assert_eq!(c.trusted_proxies, want);
}

#[test]
fn admin_token_redacted_in_debug() {
    let c = Config::from_env(&|k| (k == "RECLIP_ADMIN_TOKEN").then(|| "s3cr3t-token".into()));
    assert_eq!(
        c.admin_token.as_ref().map(Secret::expose),
        Some("s3cr3t-token")
    );
    let dbg = format!("{c:?}");
    assert!(!dbg.contains("s3cr3t-token"), "{dbg}");
    assert!(dbg.contains("<redacted>"), "{dbg}");
}

#[test]
fn public_web_false_values() {
    for v in ["false", "0", "no", "FALSE"] {
        let c = Config::from_env(&|k| (k == "RECLIP_PUBLIC_WEB").then(|| v.into()));
        assert!(!c.public_web, "{v}");
    }
    let c = Config::from_env(&|k| (k == "RECLIP_PUBLIC_WEB").then(|| "true".into()));
    assert!(c.public_web);
}

#[test]
fn public_url_must_be_a_bare_origin() {
    let public = |v: &str| {
        let c = Config::from_env(&|k| (k == "RECLIP_PUBLIC_URL").then(|| v.to_string()));
        c.public_url
    };
    for (raw, want) in [
        ("https://r.example", "https://r.example"),
        ("https://r.example/", "https://r.example"),
        ("HTTPS://R.Example:443/", "https://r.example"),
        ("http://r.example:8080", "http://r.example:8080"),
    ] {
        assert_eq!(public(raw).as_deref(), Some(want), "{raw}");
    }
    for bad in [
        "//",
        "r.example",
        "ftp://r.example",
        "https://r.example/app",
        "https://r.example/?q=1",
        "https://r.example/#top",
        "https://user:pw@r.example",
        "https://",
        "file:///etc/passwd",
    ] {
        assert_eq!(public(bad), None, "{bad}");
    }
}

#[test]
fn webmcp_trial_token_is_optional() {
    let token = |v: Option<&str>| {
        Config::from_env(&|k| {
            (k == "RECLIP_WEBMCP_TRIAL_TOKEN")
                .then(|| v.map(str::to_string))
                .flatten()
        })
        .webmcp_trial_token
    };
    assert_eq!(token(None), None);
    assert_eq!(token(Some("  ")), None);
    assert_eq!(token(Some(" abc= ")).as_deref(), Some("abc="));
}
