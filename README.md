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

## Supported URLs

YouTube is the only extractor registered by default (embedders can add their
own via the [`Extractor`] trait). Accepted hosts: `youtube.com`, `www.`/`m.`/
`music.youtube.com`, `youtube-nocookie.com`, and `youtu.be`.

| Kind | URL shapes |
|---|---|
| Video | `…/watch?v=ID`, `youtu.be/ID`, `…/shorts/ID`, `…/embed/ID`, `…/v/ID`, `…/e/ID` |
| Playlist | `…/playlist?list=ID`, any non-watch URL with `?list=ID` |
| Channel | `…/channel/UC…`, `…/@handle` (streams the channel's Videos tab) |
| Search | `ytsearch:QUERY` pseudo-URL (mirrors yt-dlp) |

A watch URL that also carries `&list=` resolves to the **video** (the `v=`
parameter wins). Anything else fails fast with `Error::UnsupportedUrl` —
no network request is made.

## Browser (WASM) + TypeScript SDK

[`@4thel00z/ytdown`](web) is an npm package that runs ytdown's extraction core —
URL resolution plus signature deciphering — compiled to WebAssembly, so a video
can be resolved and its media bytes downloaded entirely client-side in the
browser.

### The CORS reality

YouTube's InnerTube API and media CDN do not send CORS headers, so a browser
cannot call them directly. The SDK therefore routes **every** request through a
CORS proxy you deploy. A reference Cloudflare Worker lives in [`web/proxy`](web/proxy).
Without a proxy the SDK will not work against YouTube.

### Install + deploy

```bash
npm install @4thel00z/ytdown
# deploy the reference proxy (or bring your own):
cd web/proxy && npx wrangler deploy
```

### Usage

```ts
import { Ytdown } from "@4thel00z/ytdown";

const yt = await Ytdown.create({ proxy: "https://ytdown-proxy.<you>.workers.dev" });
const info = await yt.resolve("https://youtu.be/dQw4w9WgXcQ");
// info is the resolved video metadata + formats; pick a format URL, then:
await yt.download(formatUrl, {
  filename: "video.mp4",
  onProgress: (p) => console.log(p.percent),
});
```

### How it works

The WASM core resolves and deciphers the stream URLs; the SDK then fetches the
bytes through your proxy and writes them to disk via the File System Access API
(streaming, so large files never fully buffer), falling back to an in-memory
Blob on browsers without it (Firefox/Safari). The proxy restores the
browser-forbidden `Origin`/`Referer` headers — which the SDK sends aliased as
`x-ytdown-origin`/`x-ytdown-referer` because browsers silently drop the real
ones — before forwarding to YouTube.

### Limitations

- Playlists, channels, and search (collections) are not yet supported in the
  browser build — `resolve` returns an error for them. Single videos work.
- The Blob fallback buffers the whole file in memory (fine for short clips and
  audio; large videos need the File System Access API, currently Chromium-only).
- The reference proxy is an open passthrough — read the security warning in
  [`web/proxy/README.md`](web/proxy/README.md) before exposing it publicly.

### Try it locally

Run the demo server (behind the `serve` feature) — `make serve` builds the wasm
bundle and launches it:

    make serve
    # equivalent to:
    cd web && bun run build:wasm
    cargo run -p ytdown-cli --features serve -- serve --open

`ytdown serve` hosts a single-file demo page and a local CORS proxy on
`http://127.0.0.1:8080`, so resolving and downloading work with no extra setup.
The page offers preset downloads (best progressive / audio / video), a
download-mechanism toggle (File System Access streaming or Blob), and a bulk
mode for multiple URLs. To install the `ytdown` binary with the `serve`
subcommand baked in, use `make install` (runs `cargo install --path cli
--features serve`). A standalone copy of the page (wasm inlined) is produced by
`make wasm && cd web/demo && bun build.mjs` (writes `web/demo/index.html`).

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

### Subcommands

| Command | Description |
|---|---|
| `get <URL>` | Download a video, playlist, channel, or `ytsearch:` result set |
| `info <URL>` | Print resolved metadata as JSON (`--pretty`, `--limit` for collections) |
| `formats <URL>` | List a video's available formats as a table (`--json`) |
| `search <QUERY>` | List search results as a table (`-n`/`--limit`, default 10; `--json`) |
| `completions <SHELL>` | Generate shell completions (bash, zsh, fish, …) |
| `serve` | Serve the browser (WASM) demo + a local CORS proxy (`--port`, `--open`); requires building with `--features serve` |

Global flags on every command: `-v`/`-vv` (info/debug logs), `-q` (silence logs),
`--user-agent <UA>`, `--cookies <FILE>`. `RUST_LOG` overrides the verbosity
flags when set.

### Cookies (`--cookies`)

When YouTube answers `Sign in to confirm you're not a bot` (reported as
`media unavailable: bot-check`), your network — not the video — has been
flagged, and requests must be authenticated to proceed. Export your browser's
youtube.com cookies to a Netscape-format `cookies.txt` (any "cookies.txt"
browser extension, or `yt-dlp --cookies-from-browser firefox --cookies out.txt`)
and pass it:

```sh
ytdown --cookies cookies.txt get "https://www.youtube.com/watch?v=..."
```

Cookies also unlock age-restricted videos. ytdown attaches the matching
cookies plus the derived `SAPISIDHASH` authorization header to youtube.com
requests only; treat the exported file like a password.

### Format selection (`-f`)

| Value | Meaning |
|---|---|
| *(omitted)* | Best split video+audio merged via ffmpeg; best progressive without ffmpeg |
| `best` | Best progressive (muxed A+V) format |
| `bestvideo` / `bestaudio` | Best video-only / audio-only stream |
| `22` | A specific format by itag |
| `137+140` | Video itag + audio itag, merged via ffmpeg |

`--max-height <H>` and `--container <mp4|webm>` narrow the keyword selections;
combining them with explicit itags is rejected (exit 2). Merging needs ffmpeg
on `PATH` or via `--ffmpeg <path>`.

### Output templates (`-o`, default `{title}.{ext}`)

Placeholders: `{title}` `{id}` `{ext}` `{height}` `{itag}` `{uploader}` `{index}`
(`{index}` is the 1-based position within a playlist/channel/search download).
Substituted values are sanitized to safe path components; literal `/` in the
template creates directories.

### Downloads

`--concurrency <N>` parallel range chunks (with `--chunk-size <BYTES>`),
`--retries <N>`, and resume of partial files by default (`--no-resume` to
disable). Collections download entry-by-entry, honouring `--skip <N>` and
`--limit <N>`; per-entry failures are logged and reported at the end without
aborting the run.

### Interactive picker

Run `ytdown get` on a TTY without `-f` and a format picker opens: arrows to
navigate, `/` to filter, `enter` to select (video-only formats offer pairing
with the best audio), `q` to quit. `--no-tui` (or piping) selects the best
format automatically. The picker and all progress/log output draw on stderr,
so stdout stays clean for piping.

### Exit codes

`0` success (including quitting the picker), `1` runtime errors (network,
extraction, download), `2` usage errors (bad flags, invalid `-f`/`-o` values).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
