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
    let ctx = ExtractorContext::new(std::sync::Arc::new(ytdown::ReqwestClient::new(
        reqwest::Client::new(),
    )));
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
    // Assert the EXACT deciphered URL: the fixture's `s=0123456789` is run
    // through the synthetic player's sig transform (reverse, splice(0,2), swap)
    // which yields `67543210`. A no-op / broken cipher (echoing the raw `s`)
    // would produce `?sig=0123456789` and fail this assertion — unlike a mere
    // `contains("sig=")` check, which the hard-coded `sp` key would always pass.
    assert_eq!(
        adaptive.url, "https://r1.test/a.webm?sig=67543210",
        "cipher must transform the signature, got URL: {}",
        adaptive.url
    );
}

#[tokio::test]
async fn channel_handle_resolves_via_resolve_url_then_browses() {
    let server = MockServer::start().await;

    // The handle is resolved via navigation/resolve_url -> UC… browseId.
    Mock::given(method("POST"))
        .and(path("/youtubei/v1/navigation/resolve_url"))
        .and(|req: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap_or_default();
            body["url"] == "https://www.youtube.com/@SomeHandle"
        })
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "endpoint": {
                "browseEndpoint": { "browseId": "UCresolved00000000000000" }
            }
        })))
        .mount(&server)
        .await;

    // The uploads browse must then use the resolved UC… id, NOT "@handle".
    Mock::given(method("POST"))
        .and(path("/youtubei/v1/browse"))
        .and(|req: &wiremock::Request| {
            let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap_or_default();
            // The resolved UC… id AND the uploads-tab params selector must be sent.
            body["browseId"] == "UCresolved00000000000000"
                && body["params"] == "EgZ2aWRlb3PyBgQKAjoA"
        })
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            fixture("innertube/browse_playlist_page2.json"),
            "application/json",
        ))
        .mount(&server)
        .await;

    let extractor = YoutubeExtractor::with_base_url(server.uri());
    let ctx = ExtractorContext::new(std::sync::Arc::new(ytdown::ReqwestClient::new(
        reqwest::Client::new(),
    )));
    let url = url::Url::parse("https://www.youtube.com/@SomeHandle").unwrap();

    let info = extractor.extract(&ctx, &url).await.unwrap();
    let collection = match info {
        MediaInfo::Collection(c) => c,
        other => panic!("expected collection, got {other:?}"),
    };
    let ids: Vec<String> = collection
        .entries
        .map(|r| r.expect("entry").id)
        .collect()
        .await;
    assert_eq!(ids, vec!["ccccccccccc"]);
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
    let ctx = ExtractorContext::new(std::sync::Arc::new(ytdown::ReqwestClient::new(
        reqwest::Client::new(),
    )));
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
