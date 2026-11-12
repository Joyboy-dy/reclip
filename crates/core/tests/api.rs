mod common;

use std::time::Duration;

use common::{TestServer, Visitor};
use reqwest::Response;
use serde_json::{Value, json};

async fn start() -> TestServer {
    TestServer::start(|c| c.job_timeout = Duration::from_secs(5)).await
}

#[tokio::test]
async fn rejects_bad_urls() {
    let s = start().await;
    for u in ["", "  ", "ftp://x/y", "javascript:alert(1)", "hello"] {
        let r = s.post("/api/v1/probe", json!({"url": u})).await;
        assert_eq!(r.status(), 400, "{u}");
        let b: Value = r.json().await.unwrap();
        assert_eq!(b["code"], "invalid_request");
        assert_eq!(b["message"], "Please paste a valid http(s) link");
    }
}

#[tokio::test]
async fn probe_trims_and_returns_video() {
    let s = start().await;
    let r = s
        .post("/api/v1/probe", json!({"url": "  https://ok  "}))
        .await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.json::<Value>().await.unwrap()["kind"], "video");
}

#[tokio::test]
async fn bot_check_is_422_with_code() {
    let s = start().await;
    let r = s
        .post("/api/v1/probe", json!({"url": "https://error-bot"}))
        .await;
    assert_eq!(r.status(), 422);
    assert_eq!(r.json::<Value>().await.unwrap()["code"], "bot_check");
}

#[tokio::test]
async fn job_lifecycle_and_unicode_filename() {
    let s = start().await;
    let id = s.create("https://ok").await;
    let j = s.wait_job(&id, "done").await;
    assert_eq!(j["filename"], "Fake ✨  video.mp4");
    let r = s.get(&format!("/api/v1/jobs/{id}/file")).await;
    assert_eq!(r.status(), 200);
    let cd = r.headers()["content-disposition"]
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        cd.starts_with("attachment; filename=\"Fake _  video.mp4\""),
        "{cd}"
    );
    assert!(
        cd.contains("filename*=UTF-8''Fake%20%E2%9C%A8%20%20video.mp4"),
        "{cd}"
    );
    assert_eq!(r.text().await.unwrap(), "fake media");
}

#[tokio::test]
async fn file_supports_range() {
    let s = start().await;
    let id = s.create("https://ok").await;
    s.wait_job(&id, "done").await;
    let r = s
        .client
        .get(format!("{}/api/v1/jobs/{id}/file", s.base))
        .header("Range", "bytes=0-3")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 206);
    assert!(r.headers().contains_key("content-disposition"));
    assert_eq!(r.text().await.unwrap(), "fake");
}

#[tokio::test]
async fn failed_job_reports_code_and_cleans_dir() {
    let s = start().await;
    let id = s.create("https://error-bot").await;
    let j = s.wait_job(&id, "failed").await;
    assert_eq!(j["error"]["code"], "bot_check");
    assert!(!s.downloads.join(&id).exists());
    let r = s.get(&format!("/api/v1/jobs/{id}/file")).await;
    assert_eq!(r.status(), 404);
}

#[tokio::test]
async fn unknown_job_is_404() {
    let s = start().await;
    let r = s.get("/api/v1/jobs/nope").await;
    assert_eq!(r.status(), 404);
    assert_eq!(r.json::<Value>().await.unwrap()["code"], "invalid_request");
    assert_eq!(s.get("/api/v1/jobs/nope/file").await.status(), 404);
}

#[tokio::test]
async fn serves_index_config_and_health() {
    let s = start().await;
    assert!(s.get("/").await.text().await.unwrap().contains("ReClip"));
    assert!(
        s.get("/some/spa/route")
            .await
            .text()
            .await
            .unwrap()
            .contains("ReClip")
    );
    assert_eq!(s.get("/healthz").await.text().await.unwrap(), "ok");
    let c: Value = s.get("/api/v1/config").await.json().await.unwrap();
    assert_eq!(c["playlist_limit"], 50);
}

#[tokio::test]
async fn unknown_api_path_is_json_404() {
    let s = start().await;
    let r = s.get("/api/v1/nothing").await;
    assert_eq!(r.status(), 404);
    assert_eq!(r.json::<Value>().await.unwrap()["code"], "invalid_request");
    assert_eq!(s.get("/api/other").await.status(), 404);
}

fn set_cookie(r: &Response) -> String {
    r.headers()["set-cookie"].to_str().unwrap().to_string()
}

#[tokio::test]
async fn first_request_sets_client_cookie() {
    let s = start().await;
    let r = reqwest::get(format!("{}/api/v1/jobs", s.base))
        .await
        .unwrap();
    let c = set_cookie(&r);
    let value = c
        .strip_prefix("reclip_client=")
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    assert_eq!(value.len(), 32);
    assert!(
        value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    );
    for attr in ["HttpOnly", "SameSite=Lax", "Path=/", "Max-Age=31536000"] {
        assert!(c.contains(attr), "{attr} in {c}");
    }
    assert!(!c.contains("Secure"), "{c}");
    // Static files and health do not get an identity.
    let h = reqwest::get(format!("{}/healthz", s.base)).await.unwrap();
    assert!(!h.headers().contains_key("set-cookie"));
}

#[tokio::test]
async fn valid_cookie_is_kept_and_invalid_is_replaced() {
    let s = start().await;
    let c = reqwest::Client::new();
    let url = format!("{}/api/v1/jobs", s.base);
    let good = "0123456789abcdef0123456789abcdef";
    let r = c
        .get(&url)
        .header("Cookie", format!("reclip_client={good}"))
        .send()
        .await
        .unwrap();
    assert!(!r.headers().contains_key("set-cookie"));
    for bad in [
        "ABCDEF0123456789abcdef0123456789",
        "short",
        "zz23456789abcdef0123456789abcdef",
    ] {
        let r = c
            .get(&url)
            .header("Cookie", format!("reclip_client={bad}"))
            .send()
            .await
            .unwrap();
        let sc = set_cookie(&r);
        assert!(sc.starts_with("reclip_client="), "{bad}");
        assert!(!sc.contains(bad), "{bad}");
    }
}

#[tokio::test]
async fn secure_cookie_behind_https_proxy() {
    let s = start().await;
    let r = reqwest::Client::new()
        .get(format!("{}/api/v1/jobs", s.base))
        .header("X-Forwarded-Proto", "https")
        .send()
        .await
        .unwrap();
    assert!(set_cookie(&r).contains("Secure"));
}

#[tokio::test]
async fn jobs_are_listed_per_visitor() {
    let s = start().await;
    let b = Visitor::new(&s.base);
    s.create("https://ok").await;
    s.create("https://ok").await;
    b.post(
        "/api/v1/jobs",
        json!({"url": "https://ok", "format": "mp4"}),
    )
    .await;
    let la: Value = s.get("/api/v1/jobs").await.json().await.unwrap();
    let lb: Value = b.get("/api/v1/jobs").await.json().await.unwrap();
    assert_eq!(la["jobs"].as_array().unwrap().len(), 2);
    assert_eq!(lb["jobs"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn other_visitor_gets_404() {
    let s = start().await;
    let id = s.create("https://ok").await;
    s.wait_job(&id, "done").await;
    let b = Visitor::new(&s.base);
    for path in [
        format!("/api/v1/jobs/{id}"),
        format!("/api/v1/jobs/{id}/file"),
    ] {
        let r = b.get(&path).await;
        assert_eq!(r.status(), 404, "{path}");
        assert_eq!(r.json::<Value>().await.unwrap()["code"], "invalid_request");
    }
}

#[tokio::test]
async fn legacy_ownerless_jobs_are_unreachable() {
    let s = start().await;
    let id = s.create("https://ok").await;
    s.wait_job(&id, "done").await;
    let mut legacy = s.store.get(&id).await.unwrap().unwrap();
    legacy.id = "legacy1".into();
    s.store.insert(&legacy, "", "").await.unwrap();
    let fresh = Visitor::new(&s.base);
    assert_eq!(fresh.get("/api/v1/jobs/legacy1").await.status(), 404);
    let l: Value = fresh.get("/api/v1/jobs").await.json().await.unwrap();
    assert!(l["jobs"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn rejects_bad_clip_and_playlist_item() {
    let s = start().await;
    let cases = [
        (
            json!({"clip": {"start": -1.0}}),
            "Clip start must be a time of 0 or more",
        ),
        (
            json!({"clip": {"start": 5.0, "end": 5.0}}),
            "Clip end must come after its start",
        ),
        (
            json!({"clip": {"start": 5.0, "end": 2.0}}),
            "Clip end must come after its start",
        ),
        (
            json!({"playlist_item": 0}),
            "Playlist item must be 1 or more",
        ),
    ];
    for (extra, message) in cases {
        let mut body = json!({"url": "https://ok", "format": "mp4"});
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let r = s.post("/api/v1/jobs", body).await;
        assert_eq!(r.status(), 400, "{extra}");
        let b: Value = r.json().await.unwrap();
        assert_eq!(
            (b["code"].as_str(), b["message"].as_str()),
            (Some("invalid_request"), Some(message))
        );
    }
    let jobs: Value = s.get("/api/v1/jobs").await.json().await.unwrap();
    assert!(jobs["jobs"].as_array().unwrap().is_empty());
    // Valid bounds are accepted: a clip to the end, the first item.
    let ok =
        json!({"url": "https://ok", "format": "mp4", "clip": {"start": 0.0}, "playlist_item": 1});
    let r = s.post("/api/v1/jobs", ok).await;
    assert_eq!(r.status(), 202);
    let job: Value = r.json().await.unwrap();
    assert_eq!(job["playlist_item"], 1);
    let id = job["id"].as_str().unwrap();
    let stored: Value = s
        .get(&format!("/api/v1/jobs/{id}"))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(stored["playlist_item"], 1);
}
