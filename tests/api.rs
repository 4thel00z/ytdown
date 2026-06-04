//! Integration tests for the public `Ytdown` client API.

use std::sync::{Arc, Mutex};

use ytdown::{Error, Format, Ytdown};

#[tokio::test]
async fn builder_constructs_and_resolves_via_registry() {
    let yt = Ytdown::builder().build().expect("build");
    let err = yt
        .resolve("https://example.com/not-supported")
        .await
        .expect_err("unsupported url should error");
    assert!(matches!(err, Error::UnsupportedUrl(_)));
}

#[tokio::test]
async fn download_builder_runs_with_progress() {
    let server = wiremock::MockServer::start().await;
    let body = vec![42u8; 50_000];
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/file.bin"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&server)
        .await;

    let yt = Ytdown::builder().build().expect("build");
    let dir = tempfile::tempdir().expect("tempdir");
    let dest = dir.path().join("out.bin");

    let fmt = Format {
        url: format!("{}/file.bin", server.uri()),
        ..Format::default()
    };

    let seen = Arc::new(Mutex::new(Vec::<u64>::new()));
    let seen_cb = seen.clone();
    yt.download(&fmt, &dest)
        .progress(move |p| {
            seen_cb.lock().expect("lock").push(p.bytes_downloaded);
        })
        .await
        .expect("download");

    assert_eq!(std::fs::read(&dest).expect("read"), body);
    let events = seen.lock().expect("lock");
    assert!(
        events.iter().last().copied() == Some(50_000),
        "final progress event should equal total: {events:?}"
    );
}

#[tokio::test]
async fn download_builder_honours_options() {
    // A no-network smoke check that the builder setters are chainable and the
    // future resolves; concurrency/resume just tune the underlying options.
    let server = wiremock::MockServer::start().await;
    let body = vec![1u8; 10_000];
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/f"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&server)
        .await;

    let yt = Ytdown::builder().build().expect("build");
    let dir = tempfile::tempdir().expect("tempdir");
    let dest = dir.path().join("out.bin");
    let fmt = Format {
        url: format!("{}/f", server.uri()),
        ..Format::default()
    };

    yt.download(&fmt, &dest)
        .concurrency(1)
        .resume(false)
        .await
        .expect("download");
    assert_eq!(std::fs::read(&dest).expect("read"), body);
}

#[cfg(feature = "ffmpeg")]
#[tokio::test]
async fn download_merged_downloads_both_then_merges() {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/video"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(vec![1u8; 1000]))
        .mount(&server)
        .await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/audio"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(vec![2u8; 1000]))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().expect("tempdir");
    // Fake ffmpeg: records args and writes the output file (last arg).
    let script = dir.path().join("fake-ffmpeg.sh");
    {
        let mut f = std::fs::File::create(&script).expect("create script");
        writeln!(
            f,
            "#!/bin/sh\nfor a in \"$@\"; do out=\"$a\"; done\necho merged > \"$out\""
        )
        .expect("write script");
    }
    let mut perms = std::fs::metadata(&script).expect("meta").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).expect("chmod");

    let yt = Ytdown::builder()
        .ffmpeg_binary(&script)
        .build()
        .expect("build");
    let dest = dir.path().join("merged.mp4");

    let video = Format {
        url: format!("{}/video", server.uri()),
        ..Format::default()
    };
    let audio = Format {
        url: format!("{}/audio", server.uri()),
        ..Format::default()
    };

    yt.download_merged(&video, &audio, &dest)
        .await
        .expect("merge");
    assert!(dest.exists(), "merged output should exist");
}
