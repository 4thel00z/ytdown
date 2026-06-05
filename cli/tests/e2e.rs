//! End-to-end: run the real binary against a wiremock fake YouTube via the
//! hidden YTDOWN_BASE_URL env var.

use assert_cmd::Command;
use predicates::prelude::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(rel: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../tests/fixtures")
        .join(rel);
    std::fs::read_to_string(p).expect("fixture present")
}

async fn mount_youtube(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/youtubei/v1/player"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture("innertube/player_android.json"), "application/json"),
        )
        .mount(server)
        .await;
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
async fn info_emits_video_json() {
    let server = MockServer::start().await;
    mount_youtube(&server).await;
    let uri = server.uri();
    // assert_cmd is blocking; run it off the async runtime so the mock
    // server keeps serving while the binary runs.
    let assert = tokio::task::spawn_blocking(move || {
        Command::cargo_bin("ytdown")
            .unwrap()
            .env("YTDOWN_BASE_URL", uri)
            .args(["info", "https://www.youtube.com/watch?v=dQw4w9WgXcQ"])
            .assert()
    })
    .await
    .unwrap();
    assert
        .success()
        .stdout(predicate::str::contains("\"id\":\"dQw4w9WgXcQ\""));
}
