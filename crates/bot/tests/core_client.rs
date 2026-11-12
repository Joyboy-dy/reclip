use reclip_bot::core_client::{Core, HttpCore};
use reclip_shared::{CreateJobRequest, ErrorCode, JobState, MediaFormat, ProbeResponse};
use serde_json::{Value, json};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "rk_test_key";

async fn setup() -> (MockServer, HttpCore) {
    let server = MockServer::start().await;
    let core = HttpCore::new(&server.uri(), KEY).unwrap();
    (server, core)
}

fn job_json(state: &str) -> Value {
    json!({
        "id": "j1", "url": "https://v/1", "format": "mp4", "format_id": null, "title": "T",
        "thumbnail": null, "state": state, "filename": null, "size": null, "error": null,
        "progress": 0.5, "speed": null, "eta": null, "created_at": "2026-01-01T00:00:00Z",
        "expires_at": null, "clip": null, "strip_metadata": false, "resumable": false,
        "playlist_item": null
    })
}

#[tokio::test]
async fn config_uses_bearer_only() {
    let (server, core) = setup().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/config"))
        .and(header("authorization", "Bearer rk_test_key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "version": "1", "telegram_bot_username": null, "playlist_limit": 10,
            "public_web": true,
            "limits": {"probes_per_minute": 1, "jobs_per_hour": 2, "max_active": 3},
            "max_duration": 0, "file_ttl_secs": 60, "history_days": 7
        })))
        .expect(1)
        .mount(&server)
        .await;
    let cfg = core.config().await.unwrap();
    assert_eq!(cfg.playlist_limit, 10);
    let reqs = server.received_requests().await.unwrap();
    assert!(reqs[0].headers.get("x-reclip-on-behalf-of").is_none());
}

#[tokio::test]
async fn probe_and_create_send_json_and_delegate_header() {
    let (server, core) = setup().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/probe"))
        .and(header("authorization", "Bearer rk_test_key"))
        .and(header("x-reclip-on-behalf-of", "tg:123456"))
        .and(body_json(json!({"url": "https://v/1"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "kind": "video", "title": "T", "thumbnail": null, "duration": 10.0,
            "uploader": null, "formats": [{"id": "137", "label": "1080p", "height": 1080, "codec": null}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/jobs"))
        .and(header("x-reclip-on-behalf-of", "tg:123456"))
        .and(body_json(json!({
            "url": "https://v/1", "format": "mp4", "format_id": "137", "title": "T",
            "thumbnail": null, "clip": null, "strip_metadata": false, "playlist_item": null
        })))
        .respond_with(ResponseTemplate::new(202).set_body_json(job_json("queued")))
        .expect(1)
        .mount(&server)
        .await;

    match core.probe(123456, "https://v/1").await.unwrap() {
        ProbeResponse::Video(v) => assert_eq!(v.formats[0].height, 1080),
        other => panic!("{other:?}"),
    }
    let req = CreateJobRequest {
        url: "https://v/1".into(),
        format: MediaFormat::Mp4,
        format_id: Some("137".into()),
        title: Some("T".into()),
        thumbnail: None,
        clip: None,
        strip_metadata: false,
        playlist_item: None,
    };
    let job = core.create_job(123456, &req).await.unwrap();
    assert_eq!(job.state, JobState::Queued);
}

#[tokio::test]
async fn job_endpoints_use_the_right_routes() {
    let (server, core) = setup().await;
    let on_behalf = || header("x-reclip-on-behalf-of", "tg:9");
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/j1"))
        .and(on_behalf())
        .respond_with(ResponseTemplate::new(200).set_body_json(job_json("downloading")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs"))
        .and(on_behalf())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"jobs": [job_json("done")]})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/jobs/j1/cancel"))
        .and(on_behalf())
        .respond_with(ResponseTemplate::new(200).set_body_json(job_json("cancelled")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/jobs/j1"))
        .and(on_behalf())
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v1/jobs/j1/link"))
        .and(on_behalf())
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"url": "https://r.example/d/tok", "expires_at": "2026-01-01T01:00:00Z"}),
        ))
        .expect(1)
        .mount(&server)
        .await;

    assert_eq!(
        core.get_job(9, "j1").await.unwrap().state,
        JobState::Downloading
    );
    assert_eq!(core.list_jobs(9).await.unwrap().len(), 1);
    core.cancel_job(9, "j1").await.unwrap();
    core.delete_job(9, "j1").await.unwrap();
    assert_eq!(
        core.signed_link(9, "j1").await.unwrap().url,
        "https://r.example/d/tok"
    );
}

#[tokio::test]
async fn errors_carry_code_and_retry_after() {
    let (server, core) = setup().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/probe"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "17")
                .set_body_json(json!({"code": "rate_limited", "message": "slow down"})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/nope"))
        .respond_with(
            ResponseTemplate::new(404)
                .set_body_json(json!({"code": "invalid_request", "message": "job not found"})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/weird"))
        .respond_with(ResponseTemplate::new(502).set_body_string("bad gateway"))
        .mount(&server)
        .await;

    let e = core.probe(1, "https://v").await.unwrap_err();
    assert_eq!(
        (e.status, e.code, e.retry_after),
        (429, ErrorCode::RateLimited, Some(17))
    );
    assert_eq!(e.message, "slow down");

    let e = core.get_job(1, "nope").await.unwrap_err();
    assert_eq!((e.status, e.retry_after), (404, None));
    assert_eq!(e.message, "job not found");

    let e = core.get_job(1, "weird").await.unwrap_err();
    assert_eq!((e.status, e.code), (502, ErrorCode::Internal));
}

#[tokio::test]
async fn download_file_streams_to_disk() {
    let (server, core) = setup().await;
    let payload = vec![7u8; 300_000];
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/j1/file"))
        .and(header("x-reclip-on-behalf-of", "tg:9"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.clone()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/jobs/gone/file"))
        .respond_with(
            ResponseTemplate::new(404)
                .set_body_json(json!({"code": "unavailable", "message": "expired"})),
        )
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("out.bin");
    let n = core.download_file(9, "j1", &dest).await.unwrap();
    assert_eq!(n, 300_000);
    assert_eq!(std::fs::read(&dest).unwrap(), payload);

    let e = core
        .download_file(9, "gone", &dir.path().join("x"))
        .await
        .unwrap_err();
    assert_eq!((e.status, e.code), (404, ErrorCode::Unavailable));
}

#[tokio::test]
async fn transport_error_hides_url() {
    let core = HttpCore::new("http://127.0.0.1:1", KEY).unwrap();
    let e = core.config().await.unwrap_err();
    assert_eq!(e.status, 0);
    assert!(!e.message.contains("127.0.0.1"));
}
