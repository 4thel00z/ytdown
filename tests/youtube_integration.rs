//! End-to-end YouTube extraction against a wiremock server serving the
//! hand-written fixtures.

use futures::StreamExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use ytdown::extractor::youtube::YoutubeExtractor;
use ytdown::extractor::{Extractor, ExtractorContext};
use ytdown::MediaInfo;

fn fixture(rel: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel);
    std::fs::read_to_string(p).expect("fixture present")
}

async fn mount_player(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/youtubei/v1/player"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture("innertube/player_android.json"), "application/json"),
        )
        .mount(server)
        .await;
}

async fn mount_player_js(server: &MockServer) {
    // iframe_api references a player version path; the extractor regexes it out.
    let iframe = r#"var u="\/s\/player\/abcd1234\/www-widgetapi.js";"#;
    Mock::given(method("GET"))
        .and(path("/iframe_api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(iframe))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/s/player/abcd1234/player_ias.vflset/en_US/base.js"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(fixture("player/synthetic_player.js")),
        )
        .mount(server)
        .await;
}

#[tokio::test]
async fn extracts_video_with_cipher_solving() {
    let server = MockServer::start().await;
    mount_player(&server).await;
    mount_player_js(&server).await;

    let extractor = YoutubeExtractor::with_base_url(server.uri());
    let ctx = ExtractorContext::new(reqwest::Client::new());
    let url = url::Url::parse("https://www.youtube.com/watch?v=dQw4w9WgXcQ").unwrap();

    let info = extractor.extract(&ctx, &url).await.unwrap();
    let video = match info {
        MediaInfo::Single(v) => v,
        other => panic!("expected single video, got {other:?}"),
    };

    assert_eq!(video.id, "dQw4w9WgXcQ");
    assert_eq!(video.title, "T");
    assert_eq!(video.duration, Some(std::time::Duration::from_secs(212)));
    assert_eq!(video.upload_date.as_deref(), Some("20091025"));
    assert_eq!(video.view_count, Some(123));

    // Progressive format + deciphered adaptive format.
    assert_eq!(video.formats.len(), 2);

    let adaptive = video
        .formats
        .iter()
        .find(|f| f.itag == Some(251))
        .expect("adaptive audio format present");
    assert!(
        adaptive.url.contains("sig="),
        "cipher must be applied, got URL: {}",
        adaptive.url
    );
    assert!(
        adaptive.url.starts_with("https://"),
        "URL must be absolute, got: {}",
        adaptive.url
    );
}

#[tokio::test]
async fn extracts_playlist_as_collection() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/youtubei/v1/browse"))
        .respond_with(|req: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap_or_default();
            if body.get("continuation").and_then(serde_json::Value::as_str)
                == Some("CONT_TOKEN_PAGE2")
            {
                ResponseTemplate::new(200).set_body_raw(
                    fixture("innertube/browse_playlist_page2.json"),
                    "application/json",
                )
            } else {
                ResponseTemplate::new(200).set_body_raw(
                    fixture("innertube/browse_playlist_page1.json"),
                    "application/json",
                )
            }
        })
        .mount(&server)
        .await;

    let extractor = YoutubeExtractor::with_base_url(server.uri());
    let ctx = ExtractorContext::new(reqwest::Client::new());
    let url = url::Url::parse("https://www.youtube.com/playlist?list=PLx").unwrap();

    let info = extractor.extract(&ctx, &url).await.unwrap();
    let collection = match info {
        MediaInfo::Collection(c) => c,
        other => panic!("expected collection, got {other:?}"),
    };
    assert_eq!(collection.id, "PLx");

    let ids: Vec<String> = collection
        .entries
        .map(|r| r.expect("entry").id)
        .collect()
        .await;
    assert_eq!(ids, vec!["aaaaaaaaaaa", "bbbbbbbbbbb", "ccccccccccc"]);
}
