use super::*;

#[test]
fn language_detection() {
    assert_eq!(Lang::from_code(Some("fr")), Lang::Fr);
    assert_eq!(Lang::from_code(Some("fr-CA")), Lang::Fr);
    assert_eq!(Lang::from_code(Some("FR")), Lang::Fr);
    assert_eq!(Lang::from_code(Some("en")), Lang::En);
    assert_eq!(Lang::from_code(Some("de")), Lang::En);
    assert_eq!(Lang::from_code(None), Lang::En);
    assert_eq!(Lang::from_stored(Lang::Fr.code()), Lang::Fr);
}

#[test]
fn every_error_code_has_both_languages() {
    use ErrorCode::*;
    for c in [
        BotCheck,
        UnsupportedUrl,
        Unavailable,
        Private,
        GeoBlocked,
        TooLarge,
        TooLong,
        Timeout,
        Interrupted,
        Cancelled,
        RateLimited,
        ForbiddenTarget,
        Unauthorized,
        Forbidden,
        InvalidRequest,
        Internal,
    ] {
        let (fr, en) = (error(Lang::Fr, c, None), error(Lang::En, c, None));
        assert!(!fr.is_empty() && !en.is_empty() && fr != en, "{c:?}");
    }
    assert!(error(Lang::En, RateLimited, Some(12)).contains("12"));
}

#[test]
fn messages_carry_their_data() {
    assert!(playlist_refused(Lang::Fr, Some("https://r.example")).contains("https://r.example"));
    let no_site = playlist_refused(Lang::En, None);
    assert!(no_site.contains("website") && !no_site.contains("http"));
    assert_eq!(btn_video(Lang::Fr), "🎬 MP4");
    assert_eq!(btn_audio(Lang::En), "🎵 MP3");
    assert!(jobs_list(Lang::En, &["A".into(), "B".into()]).contains("• B"));
    assert!(cancelled_count(Lang::Fr, 2).contains('2'));
    assert!(file_too_big(Lang::En, 3 * 1024 * 1024, "https://x/d/t").contains("https://x/d/t"));
    assert!(file_too_big(Lang::En, 3 * 1024 * 1024, "l").contains("3.0 MB"));
    let p = progress(Lang::En, "T", Some(42.4), Some(2048), Some(9));
    assert!(p.contains("42 %") && p.contains("2.0 KB/s") && p.contains("9 s left"));
    assert!(progress(Lang::Fr, "T", None, None, None).contains("Téléchargement"));
}
