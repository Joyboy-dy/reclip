use reclip_shared::ProbeResponse;
use serde_json::json;

use super::link::{Kind, Link, Platform, is_music_link, parse};
use super::{Fetched, Track, pick, probe_response, retag, search_query, sources};

fn link(platform: Platform, kind: Kind, id: &str, country: Option<&str>) -> Option<Link> {
    Some(Link {
        platform,
        kind,
        id: id.into(),
        country: country.map(String::from),
    })
}

#[test]
fn parses_platform_links() {
    use Kind::*;
    use Platform::*;
    for (url, want) in [
        (
            "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT?si=x",
            link(Spotify, Track, "4cOdK2wGLETKBW3PvgPWqT", None),
        ),
        (
            "https://open.spotify.com/intl-fr/album/4aawyAB9vmqN3uQ7FjRGTy",
            link(Spotify, Album, "4aawyAB9vmqN3uQ7FjRGTy", None),
        ),
        (
            "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M",
            link(Spotify, Playlist, "37i9dQZF1DXcBWIGoYBM5M", None),
        ),
        (
            "https://open.spotify.com/artist/0gxyHStUsqpMadRV0Di1Qt",
            None,
        ),
        (
            "https://www.deezer.com/fr/track/3135556",
            link(Deezer, Track, "3135556", None),
        ),
        (
            "https://deezer.com/album/302127",
            link(Deezer, Album, "302127", None),
        ),
        (
            "https://music.apple.com/us/album/love-me-do/1440833098?i=1440833334",
            link(Apple, Track, "1440833334", Some("us")),
        ),
        (
            "https://music.apple.com/FR/album/1/1440833098",
            link(Apple, Album, "1440833098", Some("fr")),
        ),
        (
            "https://music.apple.com/us/song/1440833334",
            link(Apple, Track, "1440833334", Some("us")),
        ),
        (
            "https://itunes.apple.com/us/album/one/id1440833098",
            link(Apple, Album, "1440833098", Some("us")),
        ),
        ("https://music.apple.com/us/playlist/x/pl.u-abc", None),
        ("https://open.spotify.com.evil.test/track/abc", None),
        ("https://youtube.com/watch?v=a", None),
    ] {
        assert_eq!(parse(url), want, "{url}");
    }
}

#[test]
fn music_hosts() {
    assert!(is_music_link("https://spotify.link/AbC"));
    assert!(is_music_link("https://link.deezer.com/s/xyz"));
    assert!(is_music_link("https://open.spotify.com/track/a"));
    assert!(!is_music_link("https://soundcloud.com/forss/flickermood"));
    assert!(!is_music_link("https://notspotify.link/x"));
}

const SPOTIFY_TRACK: &str = r#"{"type": "track", "name": "Never Gonna Give You Up", "artists": [{"name": "Rick Astley"}], "releaseDate": {"isoString": "1987-11-12T00:00:00Z"}, "duration": 213573, "visualIdentity": {"image": [{"url": "https://i/300", "maxWidth": 300}, {"url": "https://i/640", "maxWidth": 640}, {"url": "https://i/64", "maxWidth": 64}]}}"#;
const SPOTIFY_ALBUM: &str = r#"{"type": "album", "name": "Global Warming", "subtitle": "Pitbull", "trackList": [{"uri": "spotify:track:6OmhkSOpvYBokMKQxpIGx2", "title": "Global Warming (feat. Sensato)", "subtitle": "Pitbull, Sensato", "duration": 85400}, {"uri": "spotify:local:x", "title": "Local"}]}"#;

fn embed(entity: &str) -> String {
    format!(
        r#"<html><script id="__NEXT_DATA__" type="application/json">{{"props":{{"pageProps":{{"state":{{"data":{{"entity":{entity}}}}}}}}}}}</script></html>"#
    )
}

#[test]
fn spotify_embed_track_and_album() {
    let Some(Fetched::Track(t)) = sources::spotify_embed(&embed(SPOTIFY_TRACK)) else {
        panic!("track expected");
    };
    assert_eq!(t.title, "Never Gonna Give You Up");
    assert_eq!(t.artists, ["Rick Astley"]);
    assert_eq!(t.duration, Some(213.573));
    assert_eq!(t.cover.as_deref(), Some("https://i/640"));
    assert_eq!(t.release_date.as_deref(), Some("1987-11-12"));

    let Some(Fetched::List(l)) = sources::spotify_embed(&embed(SPOTIFY_ALBUM)) else {
        panic!("list expected");
    };
    assert_eq!(l.title, "Global Warming");
    assert_eq!(l.entries.len(), 1);
    assert_eq!(
        l.entries[0].url,
        "https://open.spotify.com/track/6OmhkSOpvYBokMKQxpIGx2"
    );
    assert_eq!(
        l.entries[0].title.as_deref(),
        Some("Pitbull, Sensato - Global Warming (feat. Sensato)")
    );
    assert!(sources::spotify_embed("<html>nothing</html>").is_none());
}

#[test]
fn deezer_and_apple_tracks() {
    let d = json!({"title": "Harder, Better, Faster, Stronger", "duration": 226, "track_position": 4,
        "release_date": "2001-03-12", "artist": {"name": "Daft Punk"},
        "contributors": [{"name": "Daft Punk"}],
        "album": {"title": "Discovery", "cover_xl": "https://c/1000.jpg"}});
    let t = sources::deezer_track(&d).unwrap();
    assert_eq!(
        (t.album.as_deref(), t.track_number, t.duration),
        (Some("Discovery"), Some(4), Some(226.0))
    );
    assert_eq!(t.cover.as_deref(), Some("https://c/1000.jpg"));

    let a = json!({"results": [{"wrapperType": "track", "trackName": "Love Me Do",
        "artistName": "The Beatles", "collectionName": "1", "trackTimeMillis": 140173,
        "trackNumber": 1, "releaseDate": "1962-10-05T12:00:00Z",
        "artworkUrl100": "https://a/x/100x100bb.jpg"}]});
    let t = sources::apple_track(&a).unwrap();
    assert_eq!(t.cover.as_deref(), Some("https://a/x/600x600bb.jpg"));
    assert_eq!(t.release_date.as_deref(), Some("1962-10-05"));

    let album = json!({"results": [
        {"wrapperType": "collection", "collectionName": "1"},
        {"wrapperType": "track", "trackId": 7, "trackName": "Love Me Do", "artistName": "The Beatles"}]});
    let Some(Fetched::List(l)) = sources::apple_album(&album, "fr") else {
        panic!("list expected");
    };
    assert_eq!(l.entries[0].url, "https://music.apple.com/fr/song/7");
    assert_eq!(parse(&l.entries[0].url).unwrap().id, "7");
}

fn daft() -> Track {
    Track {
        title: "Harder, Better, Faster, Stronger".into(),
        artists: vec!["Daft Punk".into()],
        duration: Some(226.0),
        ..Track::default()
    }
}

fn entries(list: &[(&str, &str, f64, &str)]) -> serde_json::Value {
    let e: Vec<_> = list
        .iter()
        .map(|(id, title, d, ch)| json!({"id": id, "title": title, "duration": d, "channel": ch}))
        .collect();
    json!({ "entries": e })
}

#[test]
fn picks_the_official_audio_over_videos_and_remixes() {
    // A real search for this track, in its order.
    let search = entries(&[
        (
            "video",
            "Daft Punk - Harder, Better, Faster, Stronger (Official Video)",
            223.0,
            "Daft Punk",
        ),
        (
            "audio",
            "Daft Punk - Harder, Better, Faster, Stronger (Official Audio)",
            225.0,
            "Daft Punk",
        ),
        (
            "lyrics",
            "Daft Punk - Harder Better Faster Stronger (Lyrics)",
            229.0,
            "Cassiopeia",
        ),
        (
            "remix",
            "Daft Punk - Harder, Better, Faster, Stronger (Far Out Remix)",
            226.0,
            "Trap Nation",
        ),
        (
            "hq",
            "Daft Punk - Harder Better Faster Stronger HQ",
            226.0,
            "Daft Punk",
        ),
    ]);
    assert_eq!(pick(&daft(), &search).as_deref(), Some("audio"));
}

#[test]
fn topic_uploads_win_and_bad_matches_fail() {
    let search = entries(&[
        (
            "audio",
            "Daft Punk - Harder, Better, Faster, Stronger (Official Audio)",
            225.0,
            "Daft Punk",
        ),
        (
            "topic",
            "Harder, Better, Faster, Stronger",
            227.0,
            "Daft Punk - Topic",
        ),
    ]);
    assert_eq!(pick(&daft(), &search).as_deref(), Some("topic"));
    let wrong = entries(&[
        (
            "long",
            "Daft Punk - Harder, Better, Faster, Stronger (Extended)",
            400.0,
            "Daft Punk",
        ),
        (
            "live",
            "Daft Punk - Harder Better Faster Stronger Live",
            226.0,
            "Daft Punk",
        ),
        ("other", "Daft Punk - One More Time", 226.0, "Daft Punk"),
    ]);
    assert_eq!(pick(&daft(), &wrong), None);
    // A remix is kept when the track is one.
    let remix = Track {
        title: "Harder, Better, Faster, Stronger (Far Out Remix)".into(),
        ..daft()
    };
    let search = entries(&[(
        "remix",
        "Daft Punk - Harder, Better, Faster, Stronger (Far Out Remix)",
        226.0,
        "Trap Nation",
    )]);
    assert_eq!(pick(&remix, &search).as_deref(), Some("remix"));
}

#[test]
fn search_query_drops_features() {
    let t = Track {
        title: "Global Warming (feat. Sensato)".into(),
        artists: vec!["Pitbull".into(), "Sensato".into()],
        ..Track::default()
    };
    assert_eq!(search_query(&t), "ytsearch10:Pitbull Global Warming");
}

#[test]
fn retag_replaces_youtube_metadata() {
    let mut info = json!({"title": "X (Official Video)", "description": "promo",
        "upload_date": "20090101", "thumbnails": [{"url": "https://yt/t.jpg"}]});
    let t = Track {
        album: Some("Discovery".into()),
        cover: Some("https://c/1000.jpg".into()),
        track_number: Some(4),
        release_date: Some("2001-03-12".into()),
        artists: vec!["Daft Punk".into(), "Guest".into()],
        ..daft()
    };
    retag(&mut info, &t);
    assert_eq!(info["title"], "Harder, Better, Faster, Stronger");
    assert_eq!(info["artist"], "Daft Punk, Guest");
    assert_eq!(info["album_artist"], "Daft Punk");
    assert_eq!(info["release_year"], 2001);
    assert_eq!(info["upload_date"], "20010312");
    assert!(info["description"].is_null());
    assert_eq!(
        info["thumbnails"],
        json!([{"id": "cover", "url": "https://c/1000.jpg"}])
    );
}

#[test]
fn probe_answers() {
    let ProbeResponse::Video(v) = probe_response(Platform::Deezer, Fetched::Track(daft()), 50)
    else {
        panic!("video expected");
    };
    assert!(v.audio_only && v.formats.is_empty());
    assert_eq!(v.matched_from.as_deref(), Some("Deezer"));
    assert_eq!(v.uploader.as_deref(), Some("Daft Punk"));
}
