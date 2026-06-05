"""Type stubs for the ytdown extension module."""

from collections.abc import Callable, Iterator
from os import PathLike
from typing import final

__version__: str

class YtdownError(Exception): ...
class UnsupportedUrlError(YtdownError): ...
class NetworkError(YtdownError): ...
class ExtractionError(YtdownError): ...
class UnavailableError(YtdownError): ...
class CipherError(YtdownError): ...
class FormatNotFoundError(YtdownError): ...
class IoError(YtdownError): ...
class PostprocessError(YtdownError): ...

@final
class Thumbnail:
    url: str
    width: int | None
    height: int | None

@final
class VideoStream:
    width: int | None
    height: int | None
    fps: float | None
    codec: str

@final
class AudioStream:
    codec: str
    bitrate: int | None
    sample_rate: int | None
    channels: int | None

@final
class Format:
    itag: int | None
    url: str
    mime_type: str | None
    container: str | None
    video: VideoStream | None
    audio: AudioStream | None
    filesize: int | None
    bitrate: int | None
    kind: str
    def to_json(self) -> str: ...

@final
class FormatSelector:
    formats: list[Format]
    def audio_only(self) -> FormatSelector: ...
    def video_only(self) -> FormatSelector: ...
    def progressive(self) -> FormatSelector: ...
    def max_height(self, h: int) -> FormatSelector: ...
    def container(self, c: str) -> FormatSelector: ...
    def vcodec_starts_with(self, prefix: str) -> FormatSelector: ...
    def by_itag(self, itag: int) -> Format: ...
    def best_progressive(self) -> Format: ...
    def best_video(self) -> Format: ...
    def best_audio(self) -> Format: ...
    def worst(self) -> Format: ...
    def best_video_audio(self) -> tuple[Format, Format]: ...
    def __len__(self) -> int: ...

@final
class VideoInfo:
    id: str
    title: str
    description: str | None
    duration: float | None
    uploader: str | None
    uploader_id: str | None
    channel_id: str | None
    view_count: int | None
    upload_date: str | None
    thumbnails: list[Thumbnail]
    webpage_url: str
    is_live: bool
    formats: list[Format]
    def select(self) -> FormatSelector: ...
    def to_json(self) -> str: ...

@final
class Entry:
    id: str
    title: str | None
    url: str
    duration: float | None
    thumbnails: list[Thumbnail]

@final
class Collection(Iterator[Entry]):
    id: str
    title: str | None
    kind: str
    def __iter__(self) -> Collection: ...
    def __next__(self) -> Entry: ...

@final
class Progress:
    bytes_downloaded: int
    total_bytes: int | None
    speed_bps: float | None
    eta: float | None
    def percent(self) -> float | None: ...

@final
class Ytdown:
    def __init__(
        self,
        *,
        user_agent: str | None = None,
        ffmpeg_binary: str | PathLike[str] | None = None,
    ) -> None: ...
    def resolve(self, url: str) -> VideoInfo | Collection: ...
    def download(
        self,
        format: Format,
        dest: str | PathLike[str],
        *,
        progress: Callable[[Progress], None] | None = None,
        concurrency: int = 1,
        chunk_size: int | None = None,
        retries: int = 3,
        resume: bool = True,
    ) -> None: ...
    def download_merged(
        self,
        video: Format,
        audio: Format,
        dest: str | PathLike[str],
    ) -> None: ...
