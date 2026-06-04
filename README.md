# ytdown

[![CI](https://github.com/4thel00z/ytdown/actions/workflows/ci.yaml/badge.svg)](https://github.com/4thel00z/ytdown/actions/workflows/ci.yaml)
[![crates.io](https://img.shields.io/crates/v/ytdown.svg)](https://crates.io/crates/ytdown)
[![docs.rs](https://img.shields.io/docsrs/ytdown)](https://docs.rs/ytdown)

A Rust **library** mirroring [yt-dlp](https://github.com/yt-dlp/yt-dlp)'s core: resolve a media
URL into structured metadata and stream formats, select a format, and download it to disk — built
around an extractor architecture that ships YouTube first. **Library only — there is no CLI and no
binary.**

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

## Features

| Feature  | Default | Description                                                          |
|----------|---------|----------------------------------------------------------------------|
| `ffmpeg` | off     | Mux separate DASH audio + video streams via the system `ffmpeg` binary. |

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
