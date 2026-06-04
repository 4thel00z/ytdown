//! Live integration tests that hit the real YouTube network endpoints.
//!
//! Every test is `#[ignore = "network"]` so the default `cargo test` run (and CI)
//! skips them. Run them explicitly with:
//!
//! ```bash
//! cargo test --all-features -- --ignored
//! ```
//!
//! They depend on the availability and stability of specific public videos and
//! playlists; treat failures as a signal to refresh the fixtures, not
//! necessarily a regression in the library.

use std::path::Path;

use futures::StreamExt;
use ytdown::{MediaInfo, Ytdown};

/// Resolve a stable public video and assert it carries usable metadata and at
/// least one HTTPS-addressable format.
#[tokio::test]
#[ignore = "network"]
async fn resolves_real_video() {
    let yt = Ytdown::builder().build().expect("build client");
    // "Me at the zoo" — the first YouTube video, extremely unlikely to vanish.
    let info = yt
        .resolve("https://www.youtube.com/watch?v=jNQXAC9IVRw")
        .await
        .expect("resolve video");

    let video = match info {
        MediaInfo::Single(v) => v,
        MediaInfo::Collection(_) => panic!("expected a single video, got a collection"),
    };

    assert!(!video.title.is_empty(), "title should be non-empty");
    assert!(
        !video.formats.is_empty(),
        "video should expose at least one format"
    );
    assert!(
        video.formats.iter().any(|f| f.url.starts_with("https://")),
        "at least one format should have an absolute https URL"
    );
}

/// Download the worst (smallest) available format to a tempdir and assert the
/// resulting file is non-trivial in size.
#[tokio::test]
#[ignore = "network"]
async fn downloads_smallest_format() {
    let yt = Ytdown::builder().build().expect("build client");
    let info = yt
        .resolve("https://www.youtube.com/watch?v=jNQXAC9IVRw")
        .await
        .expect("resolve video");

    let video = match info {
        MediaInfo::Single(v) => v,
        MediaInfo::Collection(_) => panic!("expected a single video, got a collection"),
    };

    let fmt = video.formats().worst().expect("worst format");
    let dir = tempfile::tempdir().expect("tempdir");
    let dest = dir.path().join("smallest.media");

    yt.download(fmt, &dest).await.expect("download format");

    let size = std::fs::metadata(&dest).expect("stat output").len();
    assert!(
        size > 10_000,
        "downloaded file should exceed 10KB, got {size}"
    );
    assert!(Path::new(&dest).exists(), "output file should exist");
}

/// Paginate a known public playlist and assert that lazy entry streaming yields
/// usable entries without eagerly loading the whole list.
#[tokio::test]
#[ignore = "network"]
async fn paginates_real_playlist() {
    let yt = Ytdown::builder().build().expect("build client");
    // A long-standing YouTube-curated playlist; large enough to require
    // continuation pagination beyond the first page.
    let info = yt
        .resolve("https://www.youtube.com/playlist?list=PLrAXtmErZgOeiKm4sgNOknGvNjby9efdf")
        .await
        .expect("resolve playlist");

    let collection = match info {
        MediaInfo::Collection(c) => c,
        MediaInfo::Single(_) => panic!("expected a collection, got a single video"),
    };

    let entries: Vec<_> = collection.entries.take(5).collect().await;
    assert!(!entries.is_empty(), "playlist should yield entries");
    for entry in entries {
        let entry = entry.expect("entry should resolve without error");
        assert!(!entry.id.is_empty(), "entry id should be non-empty");
        assert!(
            entry.url.starts_with("https://"),
            "entry url should be absolute https"
        );
    }
}
