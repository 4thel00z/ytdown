//! Core media types: the shared data contract for the crate.

use serde::Serialize;

/// Resolved media: a single item or a lazily-paginated collection.
#[derive(Debug)]
// `Single` carries the full `VideoInfo` by value as part of the public contract;
// boxing it would change the shared type signature relied on by other modules.
#[allow(clippy::large_enum_variant)]
pub enum MediaInfo {
    /// A single resolved video.
    Single(VideoInfo),
    /// A collection (playlist/channel/search) of entries.
    Collection(CollectionInfo),
}

/// Metadata + formats for one video.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct VideoInfo {
    /// Site-specific video identifier.
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Long-form description, if available.
    pub description: Option<String>,
    /// Total duration of the media.
    pub duration: Option<std::time::Duration>,
    /// Display name of the uploader/author.
    pub uploader: Option<String>,
    /// Site-specific uploader identifier.
    pub uploader_id: Option<String>,
    /// Site-specific channel identifier.
    pub channel_id: Option<String>,
    /// Number of views, if reported.
    pub view_count: Option<u64>,
    /// Upload date as `YYYYMMDD`, like yt-dlp.
    pub upload_date: Option<String>,
    /// Available thumbnails.
    pub thumbnails: Vec<Thumbnail>,
    /// Canonical URL of the media's web page.
    pub webpage_url: String,
    /// Whether the media is a live broadcast.
    pub is_live: bool,
    /// Downloadable representations.
    pub formats: Vec<Format>,
}

impl VideoInfo {
    /// Entry point for fluent format selection over this video's [`Format`]s.
    pub fn formats(&self) -> crate::format::FormatSelector<'_> {
        crate::format::FormatSelector::new(&self.formats)
    }
}

/// A single thumbnail image.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct Thumbnail {
    /// Thumbnail image URL.
    pub url: String,
    /// Pixel width, if known.
    pub width: Option<u32>,
    /// Pixel height, if known.
    pub height: Option<u32>,
}

/// One downloadable representation.
#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct Format {
    /// YouTube-style integer tag identifying the stream, if any.
    pub itag: Option<u32>,
    /// Direct (possibly deciphered) download URL.
    pub url: String,
    /// Raw MIME type string, e.g. `video/mp4; codecs="avc1.42001E, mp4a.40.2"`.
    pub mime_type: Option<String>,
    /// Container format.
    pub container: Option<Container>,
    /// Video stream parameters, if this format has video.
    pub video: Option<VideoStream>,
    /// Audio stream parameters, if this format has audio.
    pub audio: Option<AudioStream>,
    /// Total file size in bytes, if known.
    pub filesize: Option<u64>,
    /// Total bitrate in bits/s, if known.
    pub bitrate: Option<u64>,
}

/// Parameters of a video stream.
#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct VideoStream {
    /// Pixel width, if known.
    pub width: Option<u32>,
    /// Pixel height, if known.
    pub height: Option<u32>,
    /// Frames per second, if known.
    pub fps: Option<f64>,
    /// Codec string, e.g. `avc1.64001F`.
    pub codec: String,
}

/// Parameters of an audio stream.
#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct AudioStream {
    /// Codec string, e.g. `mp4a.40.2`.
    pub codec: String,
    /// Bitrate in bits/s, if known.
    pub bitrate: Option<u64>,
    /// Sample rate in Hz, if known.
    pub sample_rate: Option<u32>,
    /// Number of audio channels, if known.
    pub channels: Option<u8>,
}

/// Media container format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub enum Container {
    /// MPEG-4 video container.
    Mp4,
    /// WebM video container.
    WebM,
    /// MPEG-4 audio container.
    M4a,
    /// WebM audio container.
    Weba,
    /// Any other container, identified by its raw string.
    Other(String),
}

/// Classification of a format's stream composition.
///
/// `Progressive` = muxed A+V; otherwise split streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FormatKind {
    /// Muxed video + audio.
    Progressive,
    /// Video stream only.
    VideoOnly,
    /// Audio stream only.
    AudioOnly,
    /// Neither video nor audio detected.
    Unknown,
}

impl Format {
    /// Classify this format by which streams it carries.
    pub fn kind(&self) -> FormatKind {
        match (&self.video, &self.audio) {
            (Some(_), Some(_)) => FormatKind::Progressive,
            (Some(_), None) => FormatKind::VideoOnly,
            (None, Some(_)) => FormatKind::AudioOnly,
            (None, None) => FormatKind::Unknown,
        }
    }
}

/// A collection (playlist/channel/search) whose entries stream in lazily.
///
/// [`entries`](CollectionInfo::entries) is a [`futures::Stream`]; consume it with
/// [`futures::StreamExt`](https://docs.rs/futures/latest/futures/stream/trait.StreamExt.html)
/// (`next`, `take`, `collect`, …). Add `futures` to your `Cargo.toml` to bring
/// the extension trait into scope. Each [`Entry::url`] can be passed back to
/// [`Ytdown::resolve`](crate::Ytdown::resolve) to fetch the full item.
///
/// ```rust,no_run
/// use futures::StreamExt;
/// use ytdown::{MediaInfo, Ytdown};
///
/// # async fn run(yt: &Ytdown) -> ytdown::Result<()> {
/// if let MediaInfo::Collection(mut col) = yt.resolve("ytsearch:rust").await? {
///     while let Some(entry) = col.entries.next().await {
///         let entry = entry?;
///         println!("{}: {:?}", entry.id, entry.title);
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub struct CollectionInfo {
    /// Site-specific collection identifier.
    pub id: String,
    /// Collection title, if available.
    pub title: Option<String>,
    /// The kind of collection.
    pub kind: CollectionKind,
    /// Lazily-paginated entries.
    pub entries: futures::stream::BoxStream<'static, crate::error::Result<Entry>>,
}

impl std::fmt::Debug for CollectionInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CollectionInfo")
            .field("id", &self.id)
            .field("title", &self.title)
            .field("kind", &self.kind)
            .field("entries", &"<stream>")
            .finish()
    }
}

/// The kind of a collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CollectionKind {
    /// An ordered playlist.
    Playlist,
    /// A channel's uploads.
    Channel,
    /// Search results.
    Search,
}

/// A lightweight reference to an item inside a collection.
///
/// Resolve the full item by passing its [`url`](Entry::url) back to
/// [`Ytdown::resolve`](crate::Ytdown::resolve):
///
/// ```rust,no_run
/// # async fn run(yt: &ytdown::Ytdown, entry: &ytdown::Entry) -> ytdown::Result<()> {
/// let info = yt.resolve(&entry.url).await?;
/// # let _ = info;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Default, Serialize)]
#[non_exhaustive]
pub struct Entry {
    /// Site-specific item identifier.
    pub id: String,
    /// Item title, if available.
    pub title: Option<String>,
    /// URL that resolves to the full item.
    pub url: String,
    /// Item duration, if known.
    pub duration: Option<std::time::Duration>,
    /// Available thumbnails.
    pub thumbnails: Vec<Thumbnail>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_kind_classification() {
        let f = Format {
            itag: Some(22),
            video: Some(VideoStream {
                width: Some(1280),
                height: Some(720),
                fps: Some(30.0),
                codec: "avc1.64001F".into(),
            }),
            audio: Some(AudioStream {
                codec: "mp4a.40.2".into(),
                bitrate: Some(192_000),
                sample_rate: Some(44_100),
                channels: Some(2),
            }),
            ..Format::default()
        };
        assert!(matches!(f.kind(), FormatKind::Progressive));
        let v = Format {
            video: f.video.clone(),
            audio: None,
            ..Format::default()
        };
        assert!(matches!(v.kind(), FormatKind::VideoOnly));
    }
}
