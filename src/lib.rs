#![warn(missing_docs)]
//! ytdown — a Rust library mirroring yt-dlp's core functionality.
//!
//! Resolve media URLs into metadata and formats, select a format, download it.
//! A companion CLI lives in the `ytdown-cli` crate (binary `ytdown`).
//!
//! # Quickstart
//!
//! ```rust,no_run
//! use std::path::Path;
//! use ytdown::Ytdown;
//!
//! #[tokio::main]
//! async fn main() -> ytdown::Result<()> {
//!     let yt = Ytdown::builder().build()?;
//!     let info = yt.resolve("https://youtu.be/dQw4w9WgXcQ").await?;
//!     if let ytdown::MediaInfo::Single(video) = info {
//!         let fmt = video.formats().best_progressive()?;
//!         yt.download(fmt, Path::new("out.mp4"))
//!             .progress(|p| {
//!                 if let Some(pct) = p.percent() {
//!                     eprintln!("{pct:.1}%");
//!                 }
//!             })
//!             .await?;
//!     }
//!     Ok(())
//! }
//! ```

/// JavaScript interpreter for solving extractor ciphers.
pub(crate) mod jsi;

/// Downloading resolved formats to disk.
pub mod download;
pub mod error;
pub mod extractor;
/// Format selection over a video's available representations.
pub mod format;
/// Postprocessing of downloaded media via ffmpeg.
#[cfg(feature = "ffmpeg")]
pub mod postprocess;
pub mod types;

use std::future::IntoFuture;
use std::path::{Path, PathBuf};
use std::pin::Pin;

pub use download::{DownloadOptions, Progress};
pub use error::{Error, Result};
pub use extractor::{Extractor, ExtractorContext, Registry};
pub use format::FormatSelector;
pub use types::*;

use download::{Downloader, ProgressCallback};
use extractor::youtube::YoutubeExtractor;

/// The library entry point: resolves URLs into media and downloads formats.
///
/// Construct one with [`Ytdown::builder`]. A single instance can be shared and
/// reused across many resolves and downloads; it holds a shared HTTP client and
/// the registered [`Extractor`]s.
pub struct Ytdown {
    ctx: ExtractorContext,
    registry: Registry,
    downloader: Downloader,
    #[cfg(feature = "ffmpeg")]
    ffmpeg_binary: PathBuf,
}

/// Builder for [`Ytdown`].
///
/// Registers the YouTube extractor by default; call [`YtdownBuilder::extractor`]
/// to add more.
pub struct YtdownBuilder {
    user_agent: Option<String>,
    client: Option<reqwest::Client>,
    extractors: Vec<Box<dyn Extractor>>,
    #[cfg(feature = "ffmpeg")]
    ffmpeg_binary: PathBuf,
}

impl Default for YtdownBuilder {
    fn default() -> Self {
        Self {
            user_agent: None,
            client: None,
            extractors: vec![Box::new(YoutubeExtractor::new())],
            #[cfg(feature = "ffmpeg")]
            ffmpeg_binary: PathBuf::from("ffmpeg"),
        }
    }
}

impl YtdownBuilder {
    /// Override the `User-Agent` header used by the default HTTP client.
    ///
    /// Ignored if a fully-built [`client`](Self::client) is supplied.
    pub fn user_agent(mut self, ua: &str) -> Self {
        self.user_agent = Some(ua.to_string());
        self
    }

    /// Supply a pre-configured [`reqwest::Client`], overriding the default.
    pub fn client(mut self, c: reqwest::Client) -> Self {
        self.client = Some(c);
        self
    }

    /// Path to the `ffmpeg` binary used by [`Ytdown::download_merged`].
    #[cfg(feature = "ffmpeg")]
    pub fn ffmpeg_binary(mut self, p: impl Into<PathBuf>) -> Self {
        self.ffmpeg_binary = p.into();
        self
    }

    /// Register an additional extractor. Extractors are tried in registration
    /// order, after the default YouTube extractor; first match wins.
    pub fn extractor(mut self, e: Box<dyn Extractor>) -> Self {
        self.extractors.push(e);
        self
    }

    /// Remove all registered extractors, including the default YouTube one.
    ///
    /// Use together with [`extractor`](Self::extractor) to take full control
    /// over which extractors run — e.g. a [`YoutubeExtractor`] pointed at a
    /// mock server in tests.
    pub fn clear_extractors(mut self) -> Self {
        self.extractors.clear();
        self
    }

    /// Finish building.
    ///
    /// Returns an [`Error::Network`] if the default HTTP client cannot be built.
    pub fn build(self) -> Result<Ytdown> {
        let http = match self.client {
            Some(c) => c,
            None => {
                let mut builder = reqwest::Client::builder();
                if let Some(ua) = &self.user_agent {
                    builder = builder.user_agent(ua);
                }
                builder.build().map_err(|source| Error::Network {
                    stage: "client-build",
                    source,
                })?
            }
        };

        Ok(Ytdown {
            ctx: ExtractorContext::new(http.clone()),
            registry: Registry::new(self.extractors),
            downloader: Downloader::new(http),
            #[cfg(feature = "ffmpeg")]
            ffmpeg_binary: self.ffmpeg_binary,
        })
    }
}

impl Ytdown {
    /// Start building an [`Ytdown`] instance.
    pub fn builder() -> YtdownBuilder {
        YtdownBuilder::default()
    }

    /// Resolve any supported URL (or a `ytsearch:query`) into [`MediaInfo`].
    ///
    /// Returns [`Error::UnsupportedUrl`] if no registered extractor matches.
    pub async fn resolve(&self, url: &str) -> Result<MediaInfo> {
        self.registry.resolve(&self.ctx, url).await
    }

    /// Begin building a download of a single `format` to `dest`.
    ///
    /// The returned [`DownloadBuilder`] is a future: configure it with
    /// [`progress`](DownloadBuilder::progress), [`concurrency`](DownloadBuilder::concurrency),
    /// and [`resume`](DownloadBuilder::resume), then `.await` it.
    pub fn download<'a>(&'a self, format: &Format, dest: impl AsRef<Path>) -> DownloadBuilder<'a> {
        DownloadBuilder {
            downloader: &self.downloader,
            url: format.url.clone(),
            dest: dest.as_ref().to_path_buf(),
            options: DownloadOptions::default(),
        }
    }

    /// Download a split `video` and `audio` format to temporary files beside
    /// `dest`, then mux them into `dest` with `ffmpeg`.
    ///
    /// The temporary files are removed once muxing succeeds.
    #[cfg(feature = "ffmpeg")]
    pub async fn download_merged(
        &self,
        video: &Format,
        audio: &Format,
        dest: impl AsRef<Path>,
    ) -> Result<()> {
        let dest = dest.as_ref();
        let video_tmp = sibling_tmp(dest, "video.part");
        let audio_tmp = sibling_tmp(dest, "audio.part");

        self.downloader
            .download(&video.url, &video_tmp, DownloadOptions::default())
            .await?;
        self.downloader
            .download(&audio.url, &audio_tmp, DownloadOptions::default())
            .await?;

        let merger = postprocess::FfmpegMerger::with_binary(self.ffmpeg_binary.clone());
        merger.merge(&video_tmp, &audio_tmp, dest).await?;

        let _ = tokio::fs::remove_file(&video_tmp).await;
        let _ = tokio::fs::remove_file(&audio_tmp).await;
        Ok(())
    }
}

/// Compute a temporary sibling path next to `dest` with the given suffix.
#[cfg(feature = "ffmpeg")]
fn sibling_tmp(dest: &Path, suffix: &str) -> PathBuf {
    let stem = dest
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("ytdown");
    let name = format!(".{stem}.{suffix}");
    match dest.parent() {
        Some(parent) => parent.join(name),
        None => PathBuf::from(name),
    }
}

/// A configurable, awaitable download of one format to a path.
///
/// Obtain one from [`Ytdown::download`]. It implements [`IntoFuture`], so it is
/// awaited directly:
///
/// ```rust,no_run
/// # async fn run(yt: &ytdown::Ytdown, fmt: &ytdown::Format) -> ytdown::Result<()> {
/// yt.download(fmt, "out.mp4").concurrency(4).resume(true).await?;
/// # Ok(())
/// # }
/// ```
pub struct DownloadBuilder<'a> {
    downloader: &'a Downloader,
    url: String,
    dest: PathBuf,
    options: DownloadOptions,
}

impl<'a> DownloadBuilder<'a> {
    /// Observe progress as the download runs.
    pub fn progress(mut self, f: impl Fn(Progress) + Send + Sync + 'static) -> Self {
        let cb: ProgressCallback = std::sync::Arc::new(f);
        self.options.progress = Some(cb);
        self
    }

    /// Number of parallel range-chunk connections (1 = sequential streaming).
    pub fn concurrency(mut self, n: usize) -> Self {
        self.options.concurrency = n;
        self
    }

    /// Chunk size (in bytes) used by the parallel range-chunk path (default 10 MiB).
    ///
    /// Only takes effect when [`concurrency`](Self::concurrency) is greater than 1
    /// and the server supports range requests.
    pub fn chunk_size(mut self, bytes: u64) -> Self {
        self.options.chunk_size = bytes;
        self
    }

    /// Maximum retry attempts per request (default 3), with jittered exponential
    /// backoff between attempts.
    pub fn retries(mut self, n: u32) -> Self {
        self.options.retries = n;
        self
    }

    /// Whether to resume from an existing partial file (default `true`).
    pub fn resume(mut self, yes: bool) -> Self {
        self.options.resume = yes;
        self
    }
}

impl<'a> IntoFuture for DownloadBuilder<'a> {
    type Output = Result<()>;
    type IntoFuture = Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            self.downloader
                .download(&self.url, &self.dest, self.options)
                .await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Finding 5: `chunk_size()` and `retries()` must actually reach the
    /// underlying `DownloadOptions` (previously unreachable via the builder).
    #[test]
    fn builder_setters_reach_download_options() {
        let yt = Ytdown::builder().build().expect("build");
        let fmt = Format {
            url: "https://example.invalid/x".into(),
            ..Format::default()
        };
        let builder = yt
            .download(&fmt, "out.bin")
            .concurrency(8)
            .chunk_size(1234)
            .retries(7)
            .resume(false);
        assert_eq!(builder.options.concurrency, 8);
        assert_eq!(builder.options.chunk_size, 1234);
        assert_eq!(builder.options.retries, 7);
        assert!(!builder.options.resume);
    }

    /// `clear_extractors` removes the default YouTube extractor, so nothing
    /// matches and embedders can register their own (e.g. a mock-server one).
    #[tokio::test]
    async fn clear_extractors_removes_defaults() {
        let yt = Ytdown::builder().clear_extractors().build().expect("build");
        let err = yt
            .resolve("https://www.youtube.com/watch?v=dQw4w9WgXcQ")
            .await
            .unwrap_err();
        assert!(matches!(err, Error::UnsupportedUrl(_)));
    }
}
