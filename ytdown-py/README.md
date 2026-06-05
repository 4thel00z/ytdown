# ytdown-py

Resolve media URLs into metadata and formats, pick a format, download it.
Powered by the [`ytdown`](https://github.com/4thel00z/ytdown) Rust crate, so
it is fast and dependency-free. The API is synchronous and releases the GIL
during network and disk I/O, so it plays well with threads.

## Install

```sh
uv add ytdown-py
```

On platforms without a prebuilt wheel (anything other than Linux x86_64 and
macOS arm64), install from source instead. This needs a Rust toolchain:

```sh
uv add "git+https://github.com/4thel00z/ytdown#subdirectory=ytdown-py"
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

`Ytdown()` holds a shared HTTP client. Create it once and reuse it across
resolves and downloads. It accepts two optional keyword arguments:

```python
yt = Ytdown(user_agent="my-app/1.0", ffmpeg_binary="/opt/ffmpeg/bin/ffmpeg")
```

## Inspecting a video

`resolve()` returns a `VideoInfo` for single videos:

```python
info.id           # "dQw4w9WgXcQ"
info.title        # "Rick Astley - Never Gonna Give You Up ..."
info.duration     # 213.0 (seconds)
info.uploader     # "Rick Astley"
info.view_count   # 1234567
info.upload_date  # "20091025" (YYYYMMDD)
info.thumbnails   # [Thumbnail(url=..., width=..., height=...), ...]
info.formats      # [Format(...), ...]
info.to_json()    # full metadata as a JSON string
```

## Format selection

`VideoInfo.select()` returns a chainable `FormatSelector`. Filters narrow the
set, terminal methods pick a format:

```python
fmt = info.select().progressive().max_height(720).best_video()
audio = info.select().audio_only().best_audio()
video, audio = info.select().best_video_audio()
fmt = info.select().by_itag(22)
worst = info.select().worst()
```

Filters: `progressive()`, `video_only()`, `audio_only()`, `max_height(h)`,
`container("mp4")`, `vcodec_starts_with("avc1")`.

Terminals: `best_progressive()`, `best_video()`, `best_audio()`,
`best_video_audio()`, `worst()`, `by_itag(itag)`.

Each `Format` exposes `itag`, `url`, `kind` (`"progressive"`, `"video_only"`,
`"audio_only"`), `container`, `filesize`, `bitrate`, and nested
`video`/`audio` stream details.

## Downloading

```python
yt.download(
    fmt,
    "out.mp4",
    progress=on_progress,        # callable(Progress) -> None
    concurrency=4,               # parallel range-chunk connections
    chunk_size=10 * 1024 * 1024, # bytes per chunk (parallel mode)
    retries=3,
    resume=True,                 # resume from an existing partial file
)
```

The `progress` callback receives `Progress` snapshots with
`bytes_downloaded`, `total_bytes`, `speed_bps`, `eta`, and `percent()`.

### Merged downloads

The highest quality streams are usually split into separate video and audio.
Download both and mux them with ffmpeg (must be on `PATH`, or pass
`ffmpeg_binary=` to `Ytdown`):

```python
video, audio = info.select().best_video_audio()
yt.download_merged(video, audio, "out.mp4")
```

## Playlists, channels, search

`resolve()` returns a `Collection` for playlists, channels, and
`ytsearch:` queries. Collections are lazy iterators; further pages are
fetched on demand:

```python
from ytdown import Collection

result = yt.resolve("ytsearch:rust programming")
if isinstance(result, Collection):
    for entry in result:
        print(entry.id, entry.title)
        full = yt.resolve(entry.url)  # resolve the full item
        break
```

## Errors

All errors derive from `ytdown.YtdownError`:

`UnsupportedUrlError`, `NetworkError`, `ExtractionError`, `UnavailableError`,
`CipherError`, `FormatNotFoundError`, `IoError`, `PostprocessError`.

```python
from ytdown import FormatNotFoundError

try:
    fmt = info.select().audio_only().by_itag(9999)
except FormatNotFoundError:
    ...
```

## License

MIT OR Apache-2.0.
