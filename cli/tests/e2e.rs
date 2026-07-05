//! End-to-end: run the real binary against a wiremock fake YouTube via the
//! hidden YTDOWN_BASE_URL env var.

use assert_cmd::Command;
use predicates::prelude::*;
use wiremock::matchers::{header_exists, method, path};
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

/// Fake YouTube behind an anti-bot wall: the player endpoint answers the
/// bot-check unless the request carries a Cookie header.
async fn mount_bot_walled_youtube(server: &MockServer) {
    // Authenticated requests (any Cookie header) get the real player response.
    Mock::given(method("POST"))
        .and(path("/youtubei/v1/player"))
        .and(header_exists("Cookie"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture("innertube/player_android.json"), "application/json"),
        )
        .with_priority(1)
        .mount(server)
        .await;
    // Anonymous requests hit the wall.
    Mock::given(method("POST"))
        .and(path("/youtubei/v1/player"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "playabilityStatus": {
                "status": "LOGIN_REQUIRED",
                "reason": "Sign in to confirm you\u{2019}re not a bot"
            }
        })))
        .with_priority(5)
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
async fn bot_check_is_reported_with_cookie_hint() {
    let server = MockServer::start().await;
    mount_bot_walled_youtube(&server).await;
    let uri = server.uri();
    let assert = tokio::task::spawn_blocking(move || {
        Command::cargo_bin("ytdown")
            .unwrap()
            .env("YTDOWN_BASE_URL", uri)
            .args(["formats", "https://www.youtube.com/watch?v=dQw4w9WgXcQ"])
            .assert()
    })
    .await
    .unwrap();
    assert
        .failure()
        .stderr(predicate::str::contains("bot-check"))
        .stderr(predicate::str::contains("--cookies"))
        .stderr(predicate::str::contains("age-restricted").not());
}

#[tokio::test]
async fn cookies_flag_authenticates_past_the_bot_wall() {
    let server = MockServer::start().await;
    mount_bot_walled_youtube(&server).await;
    let uri = server.uri();
    // The mock listens on 127.0.0.1 over plain HTTP, so bind the cookie to
    // that host with secure=FALSE.
    let dir = tempfile::tempdir().unwrap();
    let cookies = dir.path().join("cookies.txt");
    std::fs::write(
        &cookies,
        "# Netscape HTTP Cookie File\n127.0.0.1\tFALSE\t/\tFALSE\t0\tSAPISID\tabc123\n",
    )
    .unwrap();
    let assert = tokio::task::spawn_blocking(move || {
        Command::cargo_bin("ytdown")
            .unwrap()
            .env("YTDOWN_BASE_URL", uri)
            .args([
                "--cookies",
                cookies.to_str().unwrap(),
                "formats",
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            ])
            .assert()
    })
    .await
    .unwrap();
    assert.success().stdout(predicate::str::contains("ITAG"));
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

#[tokio::test]
async fn formats_renders_table() {
    let server = MockServer::start().await;
    mount_youtube(&server).await;
    let uri = server.uri();
    let assert = tokio::task::spawn_blocking(move || {
        Command::cargo_bin("ytdown")
            .unwrap()
            .env("YTDOWN_BASE_URL", uri)
            .args(["formats", "https://www.youtube.com/watch?v=dQw4w9WgXcQ"])
            .assert()
    })
    .await
    .unwrap();
    assert
        .success()
        .stdout(predicate::str::contains("ITAG"))
        .stdout(predicate::str::contains("KIND"));
}
