//! End-to-end TikTok extraction against a wiremock server serving the
//! hand-written fixtures.

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use ytdown::error::UnavailableReason;
use ytdown::extractor::tiktok::TiktokExtractor;
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

const VIDEO_PATH: &str = "/@someuser/video/7123456789012345678";

async fn mount_video_page(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path(VIDEO_PATH))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture("tiktok/video_page.html"), "text/html")
                .insert_header(
                    "set-cookie",
                    "tt_chain_token=CHAINTOK; Path=/; Domain=.tiktok.com; Secure; HttpOnly",
                )
                .append_header("set-cookie", "ttwid=WID42; Path=/; Domain=.tiktok.com"),
        )
        .mount(server)
        .await;
}

#[tokio::test]
async fn extracts_video_from_universal_data() {
    let server = MockServer::start().await;
    mount_video_page(&server).await;

    let extractor = TiktokExtractor::with_base_url(server.uri());
    let url =
        url::Url::parse("https://www.tiktok.com/@someuser/video/7123456789012345678").unwrap();
    let info = extractor.extract(&ctx(), &url).await.unwrap();
    let video = match info {
        MediaInfo::Single(v) => v,
        other => panic!("expected single video, got {other:?}"),
    };

    assert_eq!(video.id, "7123456789012345678");
    assert_eq!(video.title, "A funny cat does cat things #cat");
    assert_eq!(video.uploader.as_deref(), Some("Some User"));
    assert_eq!(video.uploader_id.as_deref(), Some("someuser"));
    assert_eq!(video.channel_id.as_deref(), Some("6789012345"));
    assert_eq!(video.duration, Some(std::time::Duration::from_secs(15)));
    // createTime 1690000000 = 2023-07-22 UTC.
    assert_eq!(video.upload_date.as_deref(), Some("20230722"));
    assert_eq!(video.view_count, Some(54321));
    assert_eq!(
        video.webpage_url,
        "https://www.tiktok.com/@someuser/video/7123456789012345678"
    );
    assert_eq!(video.thumbnails.len(), 2);

    // Both bitrateInfo renditions, as progressive (muxed) formats.
    assert_eq!(video.formats.len(), 2);
    let hd = &video.formats[0];
    assert_eq!(hd.url, "https://v16-webapp.tiktok.example/hd.mp4?tk=1");
    assert_eq!(hd.kind(), FormatKind::Progressive);
    assert_eq!(hd.bitrate, Some(1_500_000));
    assert_eq!(hd.filesize, Some(2_500_000));
    let v = hd.video.as_ref().unwrap();
    assert_eq!(v.width, Some(720));
    assert_eq!(v.height, Some(1280));
    assert_eq!(v.codec, "h264");
    let sd = &video.formats[1];
    assert_eq!(sd.url, "https://v16-webapp.tiktok.example/sd.mp4?tk=2");
    assert_eq!(sd.video.as_ref().unwrap().codec, "h265_hvc1");

    // The media CDN gates play URLs on the session cookies minted by the page
    // response (tt_chain_token), so every format must carry them plus a
    // browser-consistent Referer and User-Agent.
    for f in &video.formats {
        let header = |name: &str| {
            f.http_headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(
            header("Cookie"),
            Some("tt_chain_token=CHAINTOK; ttwid=WID42"),
            "format {}",
            f.url
        );
        assert_eq!(header("Referer"), Some("https://www.tiktok.com/"));
        assert!(header("User-Agent").is_some_and(|ua| ua.contains("Mozilla")));
    }
}

#[tokio::test]
async fn shortlink_resolves_via_redirect() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/t/ZTabc123"))
        .respond_with(ResponseTemplate::new(302).insert_header(
            "Location",
            format!("{VIDEO_PATH}?is_from_webapp=1").as_str(),
        ))
        .mount(&server)
        .await;
    mount_video_page(&server).await;

    let extractor = TiktokExtractor::with_base_url(server.uri());
    let url = url::Url::parse("https://vm.tiktok.com/ZTabc123/").unwrap();
    let info = extractor.extract(&ctx(), &url).await.unwrap();
    let video = match info {
        MediaInfo::Single(v) => v,
        other => panic!("expected single video, got {other:?}"),
    };
    assert_eq!(video.id, "7123456789012345678");
    assert_eq!(video.formats.len(), 2);
}

#[tokio::test]
async fn photo_post_errors_clearly() {
    let server = MockServer::start().await;
    let extractor = TiktokExtractor::with_base_url(server.uri());
    let url =
        url::Url::parse("https://www.tiktok.com/@someuser/photo/7123456789012345678").unwrap();
    let err = extractor.extract(&ctx(), &url).await.unwrap_err();
    match err {
        Error::Extraction { message, .. } => {
            assert!(message.contains("photo"), "message: {message}")
        }
        other => panic!("expected Extraction error, got {other:?}"),
    }
}

#[tokio::test]
async fn login_wall_maps_to_bot_check() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(VIDEO_PATH))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw("<html><body>Log in to TikTok</body></html>", "text/html"),
        )
        .mount(&server)
        .await;

    let extractor = TiktokExtractor::with_base_url(server.uri());
    let url =
        url::Url::parse("https://www.tiktok.com/@someuser/video/7123456789012345678").unwrap();
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
async fn not_found_status_maps_to_gone() {
    let server = MockServer::start().await;
    let body = fixture("tiktok/video_page.html").replace(
        r#""statusCode":0,"statusMsg":"""#,
        r#""statusCode":10204,"statusMsg":"item doesn't exist""#,
    );
    Mock::given(method("GET"))
        .and(path(VIDEO_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/html"))
        .mount(&server)
        .await;

    let extractor = TiktokExtractor::with_base_url(server.uri());
    let url =
        url::Url::parse("https://www.tiktok.com/@someuser/video/7123456789012345678").unwrap();
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
