//! End-to-end Instagram extraction against a wiremock server serving the
//! hand-written fixtures.

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use ytdown::error::UnavailableReason;
use ytdown::extractor::instagram::InstagramExtractor;
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

/// Match the GraphQL shortcode query: right doc, right shortcode, and the
/// logged-out web-app id header.
fn graphql_matcher(shortcode: &'static str) -> impl Fn(&wiremock::Request) -> bool {
    move |req: &wiremock::Request| {
        let body = String::from_utf8_lossy(&req.body);
        let app_id = req
            .headers
            .get("x-ig-app-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        body.contains("doc_id=8845758582119845")
            && body.contains(shortcode)
            && app_id == "936619743392459"
    }
}

async fn mount_graphql(server: &MockServer, body: String) {
    Mock::given(method("POST"))
        .and(path("/graphql/query"))
        .and(graphql_matcher("Cxample123"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
        .mount(server)
        .await;
}

#[tokio::test]
async fn extracts_video_post_with_progressive_and_dash() {
    let server = MockServer::start().await;
    mount_graphql(&server, fixture("instagram/graphql_video.json")).await;

    let extractor = InstagramExtractor::with_base_url(server.uri());
    let url = url::Url::parse("https://www.instagram.com/reel/Cxample123/").unwrap();
    let info = extractor.extract(&ctx(), &url).await.unwrap();
    let video = match info {
        MediaInfo::Single(v) => v,
        other => panic!("expected single video, got {other:?}"),
    };

    assert_eq!(video.id, "Cxample123");
    assert_eq!(video.title, "Sunset reel #sunset");
    assert_eq!(video.uploader.as_deref(), Some("reeluser"));
    assert_eq!(video.channel_id.as_deref(), Some("555666777"));
    assert_eq!(
        video.duration,
        Some(std::time::Duration::from_secs_f64(12.345))
    );
    // taken_at_timestamp 1700000000 = 2023-11-14 UTC.
    assert_eq!(video.upload_date.as_deref(), Some("20231114"));
    assert_eq!(video.view_count, Some(98765));
    assert_eq!(video.webpage_url, "https://www.instagram.com/p/Cxample123/");
    assert_eq!(video.thumbnails.len(), 2);
    assert_eq!(video.thumbnails[1].width, Some(1080));

    // Progressive video_url first, then the two DASH representations.
    assert_eq!(video.formats.len(), 3);
    let progressive = &video.formats[0];
    assert_eq!(
        progressive.url,
        "https://scontent.cdninstagram.example/v/progressive.mp4?efg=abc&_nc_ht=x"
    );
    assert_eq!(progressive.kind(), FormatKind::Progressive);
    let pv = progressive.video.as_ref().unwrap();
    assert_eq!(pv.width, Some(1080));
    assert_eq!(pv.height, Some(1920));

    let dash_v = &video.formats[1];
    assert_eq!(
        dash_v.url,
        "https://scontent.cdninstagram.example/v/dash_hd.mp4?efg=1&oh=2"
    );
    assert_eq!(dash_v.kind(), FormatKind::VideoOnly);
    assert_eq!(dash_v.video.as_ref().unwrap().codec, "avc1.64001F");

    let dash_a = &video.formats[2];
    assert_eq!(
        dash_a.url,
        "https://scontent.cdninstagram.example/v/dash_audio.mp4?efg=3&oh=4"
    );
    assert_eq!(dash_a.kind(), FormatKind::AudioOnly);
}

#[tokio::test]
async fn share_link_resolves_via_redirect() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/share/reel/AbCd12345"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", "/reel/Cxample123/"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/reel/Cxample123/"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html></html>"))
        .mount(&server)
        .await;
    mount_graphql(&server, fixture("instagram/graphql_video.json")).await;

    let extractor = InstagramExtractor::with_base_url(server.uri());
    let url = url::Url::parse("https://www.instagram.com/share/reel/AbCd12345").unwrap();
    let info = extractor.extract(&ctx(), &url).await.unwrap();
    match info {
        MediaInfo::Single(v) => assert_eq!(v.id, "Cxample123"),
        other => panic!("expected single video, got {other:?}"),
    }
}

#[tokio::test]
async fn sidecar_resolves_to_first_video_child() {
    let server = MockServer::start().await;
    let body = serde_json::json!({
        "data": { "xdt_shortcode_media": {
            "__typename": "XDTGraphSidecar",
            "id": "999",
            "shortcode": "Cxample123",
            "is_video": false,
            "edge_media_to_caption": { "edges": [{ "node": { "text": "Mixed carousel" } }] },
            "owner": { "id": "555666777", "username": "reeluser" },
            "taken_at_timestamp": 1700000000,
            "edge_sidecar_to_children": { "edges": [
                { "node": { "__typename": "XDTGraphImage", "is_video": false,
                            "display_url": "https://scontent.cdninstagram.example/img.jpg" } },
                { "node": { "__typename": "XDTGraphVideo", "is_video": true,
                            "video_url": "https://scontent.cdninstagram.example/child.mp4?x=1",
                            "dimensions": { "height": 720, "width": 720 } } }
            ] }
        } },
        "status": "ok"
    });
    Mock::given(method("POST"))
        .and(path("/graphql/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let extractor = InstagramExtractor::with_base_url(server.uri());
    let url = url::Url::parse("https://www.instagram.com/p/Cxample123/").unwrap();
    let info = extractor.extract(&ctx(), &url).await.unwrap();
    let video = match info {
        MediaInfo::Single(v) => v,
        other => panic!("expected single video, got {other:?}"),
    };
    assert_eq!(video.formats.len(), 1);
    assert_eq!(
        video.formats[0].url,
        "https://scontent.cdninstagram.example/child.mp4?x=1"
    );
    assert_eq!(video.title, "Mixed carousel");
}

#[tokio::test]
async fn image_post_errors_clearly() {
    let server = MockServer::start().await;
    let body = serde_json::json!({
        "data": { "xdt_shortcode_media": {
            "__typename": "XDTGraphImage",
            "id": "999",
            "shortcode": "Cxample123",
            "is_video": false,
            "display_url": "https://scontent.cdninstagram.example/img.jpg",
            "owner": { "id": "1", "username": "reeluser" }
        } },
        "status": "ok"
    });
    Mock::given(method("POST"))
        .and(path("/graphql/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let extractor = InstagramExtractor::with_base_url(server.uri());
    let url = url::Url::parse("https://www.instagram.com/p/Cxample123/").unwrap();
    let err = extractor.extract(&ctx(), &url).await.unwrap_err();
    match err {
        Error::Extraction { message, .. } => {
            assert!(message.contains("no video"), "message: {message}")
        }
        other => panic!("expected Extraction error, got {other:?}"),
    }
}

#[tokio::test]
async fn login_wall_maps_to_bot_check() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": { "xdt_shortcode_media": null },
            "status": "ok"
        })))
        .mount(&server)
        .await;

    let extractor = InstagramExtractor::with_base_url(server.uri());
    let url = url::Url::parse("https://www.instagram.com/p/Cxample123/").unwrap();
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
