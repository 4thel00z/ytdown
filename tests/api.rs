//! Integration tests for the public `Ytdown` client API.

use std::sync::{Arc, Mutex};

use ytdown::{Error, Format, Ytdown};

/// Build a [`Format`] carrying just a URL. `Format` is `#[non_exhaustive]`, so
/// external crates construct it via `Default` and set fields rather than a
/// struct literal.
fn fmt_url(url: String) -> Format {
    let mut f = Format::default();
    f.url = url;
    f
}

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

    let fmt = fmt_url(format!("{}/file.bin", server.uri()));

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

/// A wiremock responder that records every `Range` header it sees and honours
/// ranges with a 206 slice (or 200 full body when no range is present).
struct RecordingRangeResponder {
    body: Vec<u8>,
    ranges: Arc<Mutex<Vec<Option<String>>>>,
}

impl wiremock::Respond for RecordingRangeResponder {
    fn respond(&self, req: &wiremock::Request) -> wiremock::ResponseTemplate {
        let len = self.body.len() as u64;
        let range = req
            .headers
            .get("range")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        self.ranges.lock().expect("lock").push(range.clone());
        if let Some(range) = range {
            if let Some(spec) = range.strip_prefix("bytes=") {
                if let Some((s, e)) = spec.split_once('-') {
                    if let Ok(start) = s.parse::<u64>() {
                        let end = if e.is_empty() {
                            len - 1
                        } else {
                            e.parse::<u64>().unwrap_or(len - 1).min(len - 1)
                        };
                        let slice = self.body[start as usize..=end as usize].to_vec();
                        return wiremock::ResponseTemplate::new(206)
                            .insert_header("accept-ranges", "bytes")
                            .insert_header(
                                "content-range",
                                format!("bytes {start}-{end}/{len}").as_str(),
                            )
                            .set_body_bytes(slice);
                    }
                }
            }
        }
        wiremock::ResponseTemplate::new(200)
            .insert_header("accept-ranges", "bytes")
            .set_body_bytes(self.body.clone())
    }
}

#[tokio::test]
async fn download_builder_resume_false_redownloads_from_scratch() {
    // resume(false): a pre-existing partial must be ignored. The server must see
    // NO resume Range header (only the bytes=0-0 probe), and the file must equal
    // the full body afterwards.
    let server = wiremock::MockServer::start().await;
    let body: Vec<u8> = (0..4000u32).map(|i| (i % 251) as u8).collect();
    let ranges: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
    wiremock::Mock::given(wiremock::matchers::path("/f"))
        .respond_with(RecordingRangeResponder {
            body: body.clone(),
            ranges: ranges.clone(),
        })
        .mount(&server)
        .await;

    let yt = Ytdown::builder().build().expect("build");
    let dir = tempfile::tempdir().expect("tempdir");
    let dest = dir.path().join("out.bin");
    // Pre-create a partial file that resume(false) must discard.
    std::fs::write(&dest, &body[..1500]).expect("write partial");

    let fmt = fmt_url(format!("{}/f", server.uri()));

    yt.download(&fmt, &dest)
        .resume(false)
        .await
        .expect("download");

    assert_eq!(std::fs::read(&dest).expect("read"), body);
    let seen = ranges.lock().expect("lock");
    // resume(false) re-downloads from scratch: it may use the size probe
    // (bytes=0-0) and full-file chunk ranges starting at offset 0, but must
    // never send a *resume* range anchored at the discarded partial's length.
    assert!(
        !seen.iter().any(|r| r.as_deref() == Some("bytes=1500-")),
        "resume(false) must not send bytes=1500-, saw {seen:?}"
    );
}

#[tokio::test]
async fn download_builder_resume_true_sends_range() {
    // resume(true): a pre-existing partial must be resumed with Range: bytes=N-.
    let server = wiremock::MockServer::start().await;
    let body: Vec<u8> = (0..4000u32).map(|i| (i % 251) as u8).collect();
    let ranges: Arc<Mutex<Vec<Option<String>>>> = Arc::new(Mutex::new(Vec::new()));
    wiremock::Mock::given(wiremock::matchers::path("/f"))
        .respond_with(RecordingRangeResponder {
            body: body.clone(),
            ranges: ranges.clone(),
        })
        .mount(&server)
        .await;

    let yt = Ytdown::builder().build().expect("build");
    let dir = tempfile::tempdir().expect("tempdir");
    let dest = dir.path().join("out.bin");
    std::fs::write(&dest, &body[..1500]).expect("write partial");

    let fmt = fmt_url(format!("{}/f", server.uri()));

    yt.download(&fmt, &dest)
        .resume(true)
        .await
        .expect("download");

    assert_eq!(std::fs::read(&dest).expect("read"), body);
    let seen = ranges.lock().expect("lock");
    // The tail is fetched as bounded chunks starting at the partial's length
    // (an open-ended `bytes=1500-` would be throttled by googlevideo).
    assert!(
        seen.iter().any(|r| r
            .as_deref()
            .is_some_and(|r| r.starts_with("bytes=1500-") && !r.ends_with('-'))),
        "resume(true) must send a bounded Range starting at 1500, saw {seen:?}"
    );
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
    let argv_log = dir.path().join("argv.txt");
    let sizes_log = dir.path().join("sizes.txt");
    // Fake ffmpeg: records its full argv (one arg per line), records the byte
    // size of every `-i` input file AT MERGE TIME, then writes the output file
    // (the final positional argument).
    let script = dir.path().join("fake-ffmpeg.sh");
    {
        let mut f = std::fs::File::create(&script).expect("create script");
        writeln!(f, "#!/bin/sh").expect("write");
        writeln!(f, ": > '{}'", argv_log.display()).expect("write");
        writeln!(f, ": > '{}'", sizes_log.display()).expect("write");
        // Record argv.
        writeln!(
            f,
            "for a in \"$@\"; do printf '%s\\n' \"$a\" >> '{}'; done",
            argv_log.display()
        )
        .expect("write");
        // Record the size of each input that follows a `-i` flag.
        writeln!(f, "prev=").expect("write");
        writeln!(f, "for a in \"$@\"; do").expect("write");
        writeln!(
            f,
            "  if [ \"$prev\" = \"-i\" ]; then printf '%s %s\\n' \"$a\" \"$(wc -c < \"$a\")\" >> '{}'; fi",
            sizes_log.display()
        )
        .expect("write");
        writeln!(f, "  prev=\"$a\"").expect("write");
        writeln!(f, "done").expect("write");
        // Write the output file (final positional argument).
        writeln!(f, "out=").expect("write");
        writeln!(f, "for a in \"$@\"; do out=\"$a\"; done").expect("write");
        writeln!(f, "echo merged > \"$out\"").expect("write");
    }
    let mut perms = std::fs::metadata(&script).expect("meta").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).expect("chmod");

    let yt = Ytdown::builder()
        .ffmpeg_binary(&script)
        .build()
        .expect("build");
    let dest = dir.path().join("merged.mp4");

    let video = fmt_url(format!("{}/video", server.uri()));
    let audio = fmt_url(format!("{}/audio", server.uri()));

    yt.download_merged(&video, &audio, &dest)
        .await
        .expect("merge");
    assert!(dest.exists(), "merged output should exist");

    // The temporary .part siblings download_merged uses (see sibling_tmp).
    let video_part = dir.path().join(".merged.mp4.video.part");
    let audio_part = dir.path().join(".merged.mp4.audio.part");

    // Argv ordering: -i <video.part> -i <audio.part> -c copy <dest>.
    let argv = std::fs::read_to_string(&argv_log).expect("read argv");
    let lines: Vec<&str> = argv.lines().collect();
    let expected = [
        "-i",
        video_part.to_str().unwrap(),
        "-i",
        audio_part.to_str().unwrap(),
        "-c",
        "copy",
        dest.to_str().unwrap(),
    ];
    // The arg list ends with the expected ordered tail (a leading "-y" precedes it).
    assert!(
        lines.len() >= expected.len() && lines[lines.len() - expected.len()..] == expected[..],
        "argv tail must be `-i video -i audio -c copy dest`, got {lines:?}"
    );

    // Both .part inputs carried their expected bytes at merge time (1000 each).
    // `wc -c` may pad the count with leading whitespace, so parse each line into
    // (path, size) rather than matching a fixed-width string.
    let sizes = std::fs::read_to_string(&sizes_log).expect("read sizes");
    let size_of = |part: &std::path::Path| -> Option<u64> {
        let target = part.to_str().unwrap();
        sizes.lines().find_map(|line| {
            let (path, count) = line.rsplit_once(char::is_whitespace)?;
            (path.trim() == target).then(|| count.trim().parse::<u64>().ok())?
        })
    };
    assert_eq!(
        size_of(&video_part),
        Some(1000),
        "video.part must be 1000 bytes at merge time, got {sizes:?}"
    );
    assert_eq!(
        size_of(&audio_part),
        Some(1000),
        "audio.part must be 1000 bytes at merge time, got {sizes:?}"
    );

    // Temporaries are cleaned up after a successful merge.
    assert!(!video_part.exists(), "video .part must be cleaned up");
    assert!(!audio_part.exists(), "audio .part must be cleaned up");
}
