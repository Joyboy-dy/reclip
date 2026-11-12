use reclip_shared::*;

#[test]
fn error_code_is_snake_case() {
    assert_eq!(
        serde_json::to_string(&ErrorCode::BotCheck).unwrap(),
        "\"bot_check\""
    );
}

#[test]
fn probe_response_is_tagged_by_kind() {
    let v = serde_json::to_value(ProbeResponse::Playlist(PlaylistInfo {
        title: "t".into(),
        entries: vec![],
    }))
    .unwrap();
    assert_eq!(v["kind"], "playlist");
}

#[test]
fn job_state_round_trips() {
    for s in [JobState::Queued, JobState::Done, JobState::Failed] {
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<JobState>(&j).unwrap(), s);
    }
}

#[test]
fn http_status_mapping() {
    assert_eq!(ErrorCode::InvalidRequest.http_status(), 400);
    assert_eq!(ErrorCode::RateLimited.http_status(), 429);
    assert_eq!(ErrorCode::BotCheck.http_status(), 422);
}

#[test]
fn create_job_request_accepts_missing_optionals() {
    let r: CreateJobRequest = serde_json::from_str(r#"{"url":"u","format":"mp3"}"#).unwrap();
    assert_eq!(r.format_id, None);
    assert_eq!(r.title, None);
    assert_eq!(r.thumbnail, None);
}

#[test]
fn scopes_use_colon_names() {
    assert_eq!(
        serde_json::to_string(&Scope::JobsCreate).unwrap(),
        "\"jobs:create\""
    );
    assert_eq!(
        serde_json::from_str::<Scope>("\"jobs:cancel\"").unwrap(),
        Scope::JobsCancel
    );
    assert!(!Scope::default_set().contains(&Scope::Delegate));
    assert_eq!(Scope::default_set().len(), 4);
}

#[test]
fn create_key_request_minimal() {
    let r: CreateKeyRequest = serde_json::from_str(r#"{"name":"agent"}"#).unwrap();
    assert_eq!(r.name, "agent");
    assert!(!r.unlimited);
    assert_eq!(r.scopes, None);
    assert_eq!(r.limits, None);
}

#[test]
fn key_limits_default_to_none() {
    let l: KeyLimits = serde_json::from_str("{}").unwrap();
    assert_eq!(l, KeyLimits::default());
}

#[test]
fn new_error_codes_status() {
    assert_eq!(ErrorCode::Unauthorized.http_status(), 401);
    assert_eq!(ErrorCode::Forbidden.http_status(), 403);
}

#[test]
fn media_formats_use_lowercase_wire_names() {
    for (f, s, audio) in [
        (MediaFormat::Mp4, "mp4", false),
        (MediaFormat::Mkv, "mkv", false),
        (MediaFormat::Mov, "mov", false),
        (MediaFormat::Mp3, "mp3", true),
        (MediaFormat::M4a, "m4a", true),
        (MediaFormat::Opus, "opus", true),
        (MediaFormat::Flac, "flac", true),
        (MediaFormat::Wav, "wav", true),
    ] {
        assert_eq!(serde_json::to_string(&f).unwrap(), format!("\"{s}\""));
        assert_eq!(f.is_audio(), audio, "{s}");
    }
}

#[test]
fn history_clean_mode_wire_names() {
    for (m, s) in [
        (HistoryCleanMode::Expired, "expired"),
        (HistoryCleanMode::OlderThan, "older_than"),
        (HistoryCleanMode::All, "all"),
    ] {
        assert_eq!(serde_json::to_string(&m).unwrap(), format!("\"{s}\""));
    }
    let c: HistoryClean = serde_json::from_str(r#"{"mode":"older_than","days":7}"#).unwrap();
    assert_eq!((c.mode, c.days), (HistoryCleanMode::OlderThan, Some(7)));
    let c: HistoryClean = serde_json::from_str(r#"{"mode":"all"}"#).unwrap();
    assert_eq!(c.days, None);
}

#[test]
fn create_job_request_new_options_default() {
    let r: CreateJobRequest = serde_json::from_str(r#"{"url":"u","format":"flac"}"#).unwrap();
    assert_eq!(
        (r.clip, r.strip_metadata, r.playlist_item),
        (None, false, None)
    );
    let r: CreateJobRequest = serde_json::from_str(
        r#"{"url":"u","format":"mkv","clip":{"start":1.5},"strip_metadata":true,"playlist_item":2}"#,
    )
    .unwrap();
    assert_eq!(
        r.clip,
        Some(Clip {
            start: 1.5,
            end: None
        })
    );
    assert!(r.strip_metadata);
    assert_eq!(r.playlist_item, Some(2));
}

#[test]
fn admin_types_round_trip() {
    let l: AdminLogin = serde_json::from_str(r#"{"email":"a@b.c","password":"p"}"#).unwrap();
    assert_eq!(l.email, "a@b.c");
    let p: AdminPasswordChange =
        serde_json::from_str(r#"{"current":"a","new":"b","sign_out_others":true}"#).unwrap();
    assert!(p.sign_out_others);
    let me = serde_json::to_value(AdminMe {
        email: "a@b.c".into(),
        must_change_password: true,
    })
    .unwrap();
    assert_eq!(me["must_change_password"], true);
}
