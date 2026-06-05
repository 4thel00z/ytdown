# ytdown (Python bindings)

Python bindings for the [`ytdown`](https://github.com/4thel00z/ytdown) Rust
crate: extract, select, and download media — yt-dlp's core, library only.

Built with [maturin](https://maturin.rs) / [PyO3](https://pyo3.rs). The API is
synchronous; blocking calls release the GIL and run on a shared tokio runtime.

## Install

```sh
pip install maturin
maturin develop --release   # from this directory, inside a venv
# or build a wheel:
maturin build --release
```

## Quickstart

```python
from ytdown import Ytdown, VideoInfo

yt = Ytdown()
info = yt.resolve("https://youtu.be/dQw4w9WgXcQ")

if isinstance(info, VideoInfo):
    fmt = info.select().best_progressive()
    yt.download(
        fmt,
        "out.mp4",
        progress=lambda p: print(f"{p.percent():.1f}%" if p.percent() else "..."),
    )
```

### Format selection

`VideoInfo.select()` returns a chainable `FormatSelector` mirroring the Rust
API:

```python
fmt = info.select().progressive().max_height(720).best_video()
audio = info.select().audio_only().best_audio()
video, audio = info.select().best_video_audio()
fmt = info.select().by_itag(22)
```

### Playlists, channels, search

Collections are lazy iterators; pages are fetched on demand:

```python
from ytdown import Collection

result = yt.resolve("ytsearch:rust programming")
if isinstance(result, Collection):
    for entry in result:
        print(entry.id, entry.title)
        full = yt.resolve(entry.url)  # resolve the full item
        break
```

### Merged (split-stream) downloads

Requires `ffmpeg` on `PATH` (or pass `ffmpeg_binary=` to `Ytdown`):

```python
video, audio = info.select().best_video_audio()
yt.download_merged(video, audio, "out.mp4")
```

### Download options

```python
yt.download(
    fmt,
    "out.mp4",
    progress=on_progress,        # callable(Progress) -> None
    concurrency=4,               # parallel range-chunk connections
    chunk_size=10 * 1024 * 1024, # bytes per chunk (parallel mode)
    retries=3,
    resume=True,                 # resume from existing partial file
)
```

### Errors

All errors derive from `ytdown.YtdownError`:

`UnsupportedUrlError`, `NetworkError`, `ExtractionError`, `UnavailableError`,
`CipherError`, `FormatNotFoundError`, `IoError`, `PostprocessError`.

```python
from ytdown import YtdownError, FormatNotFoundError

try:
    fmt = info.select().audio_only().by_itag(9999)
except FormatNotFoundError:
    ...
```

## Releasing to PyPI

Releases are automated. When a release-please PR is merged on `master`:

1. release-please tags the release and bumps the version in both
   `Cargo.toml` files (`ytdown-py/Cargo.toml` is synced via the
   `x-release-please-version` annotation).
2. `.github/workflows/release-please.yaml` publishes the crate to crates.io,
   then builds wheels (Linux x86_64/aarch64, macOS x86_64/aarch64) via
   `PyO3/maturin-action` and publishes them to PyPI.

No sdist is published (maturin can't vendor the `path = ".."` dependency
cleanly). On platforms without a wheel, install from git instead:

```sh
pip install "git+https://github.com/4thel00z/ytdown#subdirectory=ytdown-py"
```

Authentication uses [PyPI Trusted Publishing](https://docs.pypi.org/trusted-publishers/)
(OIDC) — no token secret. One-time setup on pypi.org for the `ytdown` project:

- **Owner**: `4thel00z`  **Repository**: `ytdown`
- **Workflow name**: `release-please.yaml`
- **Environment**: (leave empty)

## License

MIT OR Apache-2.0, same as the Rust crate.
