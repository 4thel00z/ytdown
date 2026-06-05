# ytdown

[![CI](https://github.com/4thel00z/ytdown/actions/workflows/ci.yaml/badge.svg)](https://github.com/4thel00z/ytdown/actions/workflows/ci.yaml)
[![crates.io](https://img.shields.io/crates/v/ytdown.svg)](https://crates.io/crates/ytdown)
[![docs.rs](https://img.shields.io/docsrs/ytdown)](https://docs.rs/ytdown)

A Rust **library** (and companion **CLI**) mirroring [yt-dlp](https://github.com/yt-dlp/yt-dlp)'s
core: resolve a media URL into structured metadata and stream formats, select a
format, and download it to disk.

## Quickstart

```rust,no_run
use std::path::Path;
use ytdown::Ytdown;

#[tokio::main]
async fn main() -> ytdown::Result<()> {
    let yt = Ytdown::builder().build()?;
    let info = yt.resolve("https://youtu.be/dQw4w9WgXcQ").await?;
    if let ytdown::MediaInfo::Single(video) = info {
        let fmt = video.formats().best_progressive()?;
        yt.download(fmt, Path::new("out.mp4"))
            .progress(|p| {
                if let Some(pct) = p.percent() {
                    eprintln!("{pct:.1}%");
                }
            })
            .await?;
    }
    Ok(())
}
```

## Collections (playlists / channels / search)

`resolve` returns `MediaInfo::Collection` for playlists, channels, and
`ytsearch:` queries. Its `entries` field is a `futures::Stream` that paginates
lazily, so consume it with [`futures::StreamExt`] (`next`, `take`, `collect`, …).
Add `futures` to your `Cargo.toml` to bring the extension trait into scope:

```toml
[dependencies]
futures = "0.3"
```

```rust,no_run
use futures::StreamExt;
use ytdown::{MediaInfo, Ytdown};

#[tokio::main]
async fn main() -> ytdown::Result<()> {
    let yt = Ytdown::builder().build()?;
    if let MediaInfo::Collection(mut col) = yt.resolve("ytsearch:rust async").await? {
        // Take the first 5 entries without fetching the whole collection.
        while let Some(entry) = col.entries.next().await {
            let entry = entry?;
            println!("{} — {}", entry.id, entry.title.as_deref().unwrap_or(""));
            // Resolve an entry's full metadata + formats on demand:
            // let info = yt.resolve(&entry.url).await?;
        }
    }
    Ok(())
}
```

[`futures::StreamExt`]: https://docs.rs/futures/latest/futures/stream/trait.StreamExt.html

## Features

| Feature  | Default | Description                                                          |
|----------|---------|----------------------------------------------------------------------|
| `ffmpeg` | off     | Mux separate DASH audio + video streams via the system `ffmpeg` binary. |

## Architecture

```
src/
├── lib.rs              # Public API: Ytdown client (builder), re-exports
├── error.rs            # Error enum: Network, Extraction, Cipher, UnsupportedUrl, ...
├── types.rs            # MediaInfo, Format, Thumbnail, Entry, enums (Container, ...)
├── extractor/
│   ├── mod.rs          # Extractor trait + Registry (URL → extractor dispatch)
│   └── youtube/
│       ├── mod.rs          # URL recognition + orchestration
│       ├── innertube.rs    # InnerTube client: player/browse/search endpoints
│       ├── player.rs       # JS player fetch + sig/nsig function extraction
│       └── pagination.rs   # Continuation-token Stream for playlists/channels/search
├── jsi.rs              # boa_engine wrapper: execute extracted cipher fns
├── download/
│   ├── mod.rs          # Downloader: chunked GET, Range resume, retry/backoff
│   └── progress.rs     # Progress events + callback plumbing
├── format.rs           # FormatSelector: best/worst/filters
└── postprocess.rs      # [feature "ffmpeg"] mux / convert via system ffmpeg
```

A [`Registry`] holds boxed [`Extractor`]s; `Ytdown::resolve` dispatches to the first
matching extractor or returns `Error::UnsupportedUrl`. The shared `reqwest::Client`,
config, and caches travel through an `ExtractorContext`.

## Testing

Unit and offline integration tests (wiremock-backed) run with:

```bash
cargo test --all-features
```

Live tests in `tests/live.rs` hit the real YouTube network and are marked
`#[ignore = "network"]`, so they are skipped by default and in CI. Run them
explicitly:

```bash
cargo test --all-features -- --ignored
```

[`Registry`]: https://docs.rs/ytdown/latest/ytdown/struct.Registry.html
[`Extractor`]: https://docs.rs/ytdown/latest/ytdown/trait.Extractor.html

## CLI

The `ytdown-cli` crate ships a `ytdown` binary over the same engine:

```sh
cargo install ytdown-cli
```

```sh
# Inspect available formats
ytdown formats https://youtu.be/dQw4w9WgXcQ

# Download: best merged video+audio (needs ffmpeg), or best progressive
ytdown get https://youtu.be/dQw4w9WgXcQ -o '{title}.{ext}'

# Explicit formats: keywords, itags, or video+audio merge pairs
ytdown get -f 137+140 https://youtu.be/dQw4w9WgXcQ

# Metadata as JSON, search from the terminal
ytdown info https://youtu.be/dQw4w9WgXcQ | jq .title
ytdown search "rust async" -n 5

# Playlists/channels download entry-by-entry
ytdown get 'https://www.youtube.com/playlist?list=…' --limit 10 -o '{index} - {title}.{ext}'
```

Run `ytdown get` on a TTY without `-f` and an interactive format picker opens;
`--no-tui` (or piping) selects the best format automatically. Shell completions:
`ytdown completions zsh`.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
