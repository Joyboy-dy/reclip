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
