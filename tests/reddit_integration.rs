//! End-to-end Reddit extraction against a wiremock server serving the
//! hand-written fixtures.

use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use ytdown::error::UnavailableReason;
use ytdown::extractor::reddit::RedditExtractor;
use ytdown::extractor::{Extractor, ExtractorContext};
use ytdown::{Error, FormatKind, MediaInfo};

fn fixture(rel: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel);
    std::fs::read_to_string(p).expect("fixture present")
}

fn ctx() -> ExtractorContext {
    ExtractorContext::new(std::sync::Arc::new(ytdown::ReqwestClient::new(
        reqwest::Client::new(),
    )))
}

/// Mount the post JSON with all v.redd.it URLs rewritten to the mock server.
async fn mount_comments(server: &MockServer) {
    let body = fixture("reddit/comments_video.json").replace("https://v.redd.it", &server.uri());
    Mock::given(method("GET"))
        .and(path("/comments/1abc23.json"))
        .and(query_param("raw_json", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
        .mount(server)
        .await;
}

async fn mount_mpd(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/media123/DASHPlaylist.mpd"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw(fixture("reddit/dash.mpd"), "application/xml"),
        )
        .mount(server)
        .await;
}

#[tokio::test]
async fn extracts_video_post_with_dash_formats() {
    let server = MockServer::start().await;
    mount_comments(&server).await;
    mount_mpd(&server).await;

    let extractor = RedditExtractor::with_base_url(server.uri());
    let url = url::Url::parse("https://www.reddit.com/r/videos/comments/1abc23/cat/").unwrap();
    let info = extractor.extract(&ctx(), &url).await.unwrap();
    let video = match info {
        MediaInfo::Single(v) => v,
        other => panic!("expected single video, got {other:?}"),
    };

    assert_eq!(video.id, "1abc23");
    assert_eq!(video.title, "Cat does a backflip");
    assert_eq!(video.description, None, "empty selftext maps to None");
    assert_eq!(video.uploader.as_deref(), Some("someuser"));
    assert_eq!(video.channel_id.as_deref(), Some("videos"));
    assert_eq!(video.duration, Some(std::time::Duration::from_secs(14)));
    assert_eq!(video.upload_date.as_deref(), Some("20211201"));
    assert_eq!(
        video.webpage_url,
        "https://www.reddit.com/r/videos/comments/1abc23/cat_does_a_backflip/"
    );
    assert!(!video.is_live);

    // Post thumbnail + preview resolution + preview source (largest last).
    assert_eq!(video.thumbnails.len(), 3);
    assert_eq!(video.thumbnails[2].width, Some(1280));

    // The three DASH representations, rehosted on the mock server.
    assert_eq!(video.formats.len(), 3);
    assert_eq!(
        video.formats[0].url,
        format!("{}/media123/DASH_720.mp4", server.uri())
    );
    assert_eq!(video.formats[0].kind(), FormatKind::VideoOnly);
    assert_eq!(
        video.formats[1].url,
        format!("{}/media123/DASH_480.mp4", server.uri())
    );
    let audio = &video.formats[2];
    assert_eq!(
        audio.url,
        format!("{}/media123/DASH_AUDIO_128.mp4", server.uri())
    );
    assert_eq!(audio.kind(), FormatKind::AudioOnly);
}

#[tokio::test]
async fn falls_back_to_fallback_url_when_mpd_unavailable() {
    let server = MockServer::start().await;
    mount_comments(&server).await;
    // No MPD mock: the manifest request 404s and extraction falls back.

    let extractor = RedditExtractor::with_base_url(server.uri());
    let url = url::Url::parse("https://redd.it/1abc23").unwrap();
    let info = extractor.extract(&ctx(), &url).await.unwrap();
    let video = match info {
        MediaInfo::Single(v) => v,
        other => panic!("expected single video, got {other:?}"),
    };

    assert_eq!(video.formats.len(), 1);
    let f = &video.formats[0];
    assert_eq!(
        f.url,
        format!("{}/media123/DASH_720.mp4?source=fallback", server.uri())
    );
    assert_eq!(f.kind(), FormatKind::VideoOnly);
    let v = f.video.as_ref().unwrap();
    assert_eq!(v.width, Some(1280));
    assert_eq!(v.height, Some(720));
    // bitrate_kbps is scaled to bits/s.
    assert_eq!(f.bitrate, Some(2_400_000));
}

#[tokio::test]
async fn post_without_video_reports_external_url() {
    let server = MockServer::start().await;
    let body = serde_json::json!([
        {
            "kind": "Listing",
            "data": { "children": [ { "kind": "t3", "data": {
                "id": "1abc23",
                "title": "Link post",
                "author": "someuser",
                "subreddit": "videos",
                "permalink": "/r/videos/comments/1abc23/link_post/",
                "url_overridden_by_dest": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                "secure_media": null,
                "media": null
            } } ] }
        },
        { "kind": "Listing", "data": { "children": [] } }
    ]);
    Mock::given(method("GET"))
        .and(path("/comments/1abc23.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let extractor = RedditExtractor::with_base_url(server.uri());
    let url = url::Url::parse("https://redd.it/1abc23").unwrap();
    let err = extractor.extract(&ctx(), &url).await.unwrap_err();
    match err {
        Error::Extraction { stage, message } => {
            assert_eq!(stage, "reddit");
            assert!(
                message.contains("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
                "message should point at the external link: {message}"
            );
        }
        other => panic!("expected Extraction error, got {other:?}"),
    }
}

#[tokio::test]
async fn deleted_post_maps_to_unavailable_gone() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/comments/1abc23.json"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "message": "Not Found", "error": 404
        })))
        .mount(&server)
        .await;

    let extractor = RedditExtractor::with_base_url(server.uri());
    let url = url::Url::parse("https://redd.it/1abc23").unwrap();
    let err = extractor.extract(&ctx(), &url).await.unwrap_err();
    assert!(
        matches!(
            err,
            Error::Unavailable {
                reason: UnavailableReason::Gone,
                ..
            }
        ),
        "got {err:?}"
    );
}

#[tokio::test]
async fn blocked_request_maps_to_bot_check() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/comments/1abc23.json"))
        .respond_with(ResponseTemplate::new(403).set_body_string("Blocked"))
        .mount(&server)
        .await;

    let extractor = RedditExtractor::with_base_url(server.uri());
    let url = url::Url::parse("https://redd.it/1abc23").unwrap();
    let err = extractor.extract(&ctx(), &url).await.unwrap_err();
    assert!(
        matches!(
            err,
            Error::Unavailable {
                reason: UnavailableReason::BotCheck,
                ..
            }
        ),
        "got {err:?}"
    );
}

#[tokio::test]
async fn requests_carry_a_browser_user_agent() {
    let server = MockServer::start().await;
    let body = fixture("reddit/comments_video.json").replace("https://v.redd.it", &server.uri());
    Mock::given(method("GET"))
        .and(path("/comments/1abc23.json"))
        .and(|req: &wiremock::Request| {
            req.headers
                .get("user-agent")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|ua| ua.contains("Mozilla"))
        })
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
        .mount(&server)
        .await;
    mount_mpd(&server).await;

    let extractor = RedditExtractor::with_base_url(server.uri());
    let url = url::Url::parse("https://redd.it/1abc23").unwrap();
    // The mock only answers when the UA header is browser-like; without it the
    // request 404s and extraction fails.
    assert!(extractor.extract(&ctx(), &url).await.is_ok());
}
