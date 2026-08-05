//! End-to-end X/Twitter extraction against a wiremock server serving the
//! hand-written fixtures.

use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use ytdown::error::UnavailableReason;
use ytdown::extractor::twitter::TwitterExtractor;
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

const TWEET_URL: &str = "https://x.com/jack/status/1668680561921038336";

#[tokio::test]
async fn extracts_tweet_video_variants() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/tweet-result"))
        .and(query_param("id", "1668680561921038336"))
        // The computed access token must be sent.
        .and(query_param("token", "41mbbppzkg"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture("twitter/tweet_result.json"), "application/json"),
        )
        .mount(&server)
        .await;

    let extractor = TwitterExtractor::with_base_url(server.uri());
    let url = url::Url::parse(TWEET_URL).unwrap();
    let info = extractor.extract(&ctx(), &url).await.unwrap();
    let video = match info {
        MediaInfo::Single(v) => v,
        other => panic!("expected single video, got {other:?}"),
    };

    assert_eq!(video.id, "1668680561921038336");
    assert_eq!(video.title, "Check out this awesome clip 🎬");
    assert_eq!(video.uploader.as_deref(), Some("Jack"));
    assert_eq!(video.uploader_id.as_deref(), Some("jack"));
    assert_eq!(video.channel_id.as_deref(), Some("12"));
    assert_eq!(
        video.duration,
        Some(std::time::Duration::from_millis(21000))
    );
    assert_eq!(video.upload_date.as_deref(), Some("20230613"));
    assert_eq!(video.view_count, Some(4242));
    assert_eq!(
        video.webpage_url,
        "https://x.com/jack/status/1668680561921038336"
    );
    assert_eq!(video.thumbnails.len(), 1);
    assert_eq!(
        video.thumbnails[0].url,
        "https://pbs.twimg.example/poster.jpg"
    );

    // Only the two progressive MP4 variants; the HLS playlist is skipped.
    assert_eq!(video.formats.len(), 2);
    let low = &video.formats[0];
    assert_eq!(
        low.url,
        "https://video.twimg.example/ext_tw_video/1/vid/avc1/320x568/low.mp4"
    );
    assert_eq!(low.kind(), FormatKind::Progressive);
    assert_eq!(low.bitrate, Some(632_000));
    // Dimensions are recovered from the variant URL path.
    let lv = low.video.as_ref().unwrap();
    assert_eq!(lv.width, Some(320));
    assert_eq!(lv.height, Some(568));
    let high = &video.formats[1];
    assert_eq!(high.bitrate, Some(2_176_000));
    assert_eq!(high.video.as_ref().unwrap().height, Some(1280));
}

#[tokio::test]
async fn tweet_without_video_errors_clearly() {
    let server = MockServer::start().await;
    let body = serde_json::json!({
        "__typename": "Tweet",
        "id_str": "1668680561921038336",
        "text": "just words",
        "user": { "id_str": "12", "name": "Jack", "screen_name": "jack" },
        "photos": [],
        "mediaDetails": []
    });
    Mock::given(method("GET"))
        .and(path("/tweet-result"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let extractor = TwitterExtractor::with_base_url(server.uri());
    let url = url::Url::parse(TWEET_URL).unwrap();
    let err = extractor.extract(&ctx(), &url).await.unwrap_err();
    match err {
        Error::Extraction { message, .. } => {
            assert!(message.contains("no video"), "message: {message}")
        }
        other => panic!("expected Extraction error, got {other:?}"),
    }
}

#[tokio::test]
async fn tombstone_maps_to_unavailable() {
    let server = MockServer::start().await;
    let body = serde_json::json!({
        "__typename": "TweetTombstone",
        "tombstone": { "text": {
            "text": "Age-restricted adult content. This content might not be appropriate for people under 18 years old. To view this media, you’ll need to log in to X. Learn more"
        } }
    });
    Mock::given(method("GET"))
        .and(path("/tweet-result"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let extractor = TwitterExtractor::with_base_url(server.uri());
    let url = url::Url::parse(TWEET_URL).unwrap();
    let err = extractor.extract(&ctx(), &url).await.unwrap_err();
    match err {
        Error::Unavailable {
            reason: UnavailableReason::AgeRestricted,
            message,
        } => assert!(message.contains("Age-restricted"), "message: {message}"),
        other => panic!("expected AgeRestricted, got {other:?}"),
    }
}

#[tokio::test]
async fn deleted_tweet_maps_to_gone() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/tweet-result"))
        .respond_with(ResponseTemplate::new(404).set_body_string("Not found"))
        .mount(&server)
        .await;

    let extractor = TwitterExtractor::with_base_url(server.uri());
    let url = url::Url::parse(TWEET_URL).unwrap();
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
