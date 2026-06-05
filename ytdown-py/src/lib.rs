//! Python bindings for the `ytdown` crate, built with PyO3/maturin.
//!
//! The Rust API is async; these bindings expose a synchronous Python API by
//! driving futures on a shared multi-threaded tokio runtime. The GIL is
//! released (`Python::detach`) for every blocking operation, so other Python
//! threads keep running and progress callbacks can re-attach.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use futures::StreamExt;
use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;

use ytdown::types::{Container, FormatKind};
use ytdown::{CollectionKind, Entry, Format, MediaInfo, Progress, Thumbnail, VideoInfo};

/// Shared tokio runtime backing all blocking calls.
fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime")
    })
}

// Exception hierarchy mirroring `ytdown::Error`.
create_exception!(ytdown, YtdownError, PyException, "Base error for ytdown.");
create_exception!(
    ytdown,
    UnsupportedUrlError,
    YtdownError,
    "No registered extractor matches the URL."
);
create_exception!(ytdown, NetworkError, YtdownError, "Underlying HTTP failure.");
create_exception!(
    ytdown,
    ExtractionError,
    YtdownError,
    "The site responded but the expected data could not be extracted."
);
create_exception!(
    ytdown,
    UnavailableError,
    YtdownError,
    "The media exists but cannot be accessed (gone/age-restricted/geo-blocked/live)."
);
create_exception!(ytdown, CipherError, YtdownError, "JS cipher solving failed.");
create_exception!(
    ytdown,
    FormatNotFoundError,
    YtdownError,
    "No format matched the selector."
);
create_exception!(ytdown, IoError, YtdownError, "Filesystem error during download.");
create_exception!(
    ytdown,
    PostprocessError,
    YtdownError,
    "Postprocessing (ffmpeg) failure."
);

fn to_pyerr(e: ytdown::Error) -> PyErr {
    use ytdown::Error as E;
    let msg = e.to_string();
    match e {
        E::UnsupportedUrl(_) => UnsupportedUrlError::new_err(msg),
        E::Network { .. } => NetworkError::new_err(msg),
        E::Extraction { .. } => ExtractionError::new_err(msg),
        E::Unavailable { .. } => UnavailableError::new_err(msg),
        E::Cipher(_) => CipherError::new_err(msg),
        E::FormatNotFound(_) => FormatNotFoundError::new_err(msg),
        E::Io(_) => IoError::new_err(msg),
        E::Postprocess(_) => PostprocessError::new_err(msg),
        _ => YtdownError::new_err(msg),
    }
}

fn dur_secs(d: Option<std::time::Duration>) -> Option<f64> {
    d.map(|d| d.as_secs_f64())
}

/// Render an Option Python-style for `__repr__`s: `None` or the Debug value.
fn py_opt<T: std::fmt::Debug>(o: &Option<T>) -> String {
    match o {
        Some(v) => format!("{v:?}"),
        None => "None".into(),
    }
}

fn container_str(c: &Container) -> String {
    match c {
        Container::Mp4 => "mp4".into(),
        Container::WebM => "webm".into(),
        Container::M4a => "m4a".into(),
        Container::Weba => "weba".into(),
        Container::Other(s) => s.clone(),
        _ => "unknown".into(),
    }
}

fn parse_container(s: &str) -> Container {
    match s {
        "mp4" => Container::Mp4,
        "webm" => Container::WebM,
        "m4a" => Container::M4a,
        "weba" => Container::Weba,
        other => Container::Other(other.to_string()),
    }
}

/// A thumbnail image.
#[pyclass(name = "Thumbnail", module = "ytdown", frozen)]
struct PyThumbnail {
    inner: Thumbnail,
}

#[pymethods]
impl PyThumbnail {
    #[getter]
    fn url(&self) -> &str {
        &self.inner.url
    }
    #[getter]
    fn width(&self) -> Option<u32> {
        self.inner.width
    }
    #[getter]
    fn height(&self) -> Option<u32> {
        self.inner.height
    }
    fn __repr__(&self) -> String {
        format!(
            "Thumbnail(url={:?}, width={}, height={})",
            self.inner.url,
            py_opt(&self.inner.width),
            py_opt(&self.inner.height)
        )
    }
}

/// Parameters of a video stream.
#[pyclass(name = "VideoStream", module = "ytdown", frozen)]
struct PyVideoStream {
    inner: ytdown::VideoStream,
}

#[pymethods]
impl PyVideoStream {
    #[getter]
    fn width(&self) -> Option<u32> {
        self.inner.width
    }
    #[getter]
    fn height(&self) -> Option<u32> {
        self.inner.height
    }
    #[getter]
    fn fps(&self) -> Option<f64> {
        self.inner.fps
    }
    #[getter]
    fn codec(&self) -> &str {
        &self.inner.codec
    }
    fn __repr__(&self) -> String {
        format!(
            "VideoStream(width={}, height={}, fps={}, codec={:?})",
            py_opt(&self.inner.width),
            py_opt(&self.inner.height),
            py_opt(&self.inner.fps),
            self.inner.codec
        )
    }
}

/// Parameters of an audio stream.
#[pyclass(name = "AudioStream", module = "ytdown", frozen)]
struct PyAudioStream {
    inner: ytdown::AudioStream,
}

#[pymethods]
impl PyAudioStream {
    #[getter]
    fn codec(&self) -> &str {
        &self.inner.codec
    }
    #[getter]
    fn bitrate(&self) -> Option<u64> {
        self.inner.bitrate
    }
    #[getter]
    fn sample_rate(&self) -> Option<u32> {
        self.inner.sample_rate
    }
    #[getter]
    fn channels(&self) -> Option<u8> {
        self.inner.channels
    }
    fn __repr__(&self) -> String {
        format!(
            "AudioStream(codec={:?}, bitrate={}, sample_rate={}, channels={})",
            self.inner.codec,
            py_opt(&self.inner.bitrate),
            py_opt(&self.inner.sample_rate),
            py_opt(&self.inner.channels)
        )
    }
}

/// One downloadable representation.
#[pyclass(name = "Format", module = "ytdown", frozen)]
struct PyFormat {
    inner: Format,
}

#[pymethods]
impl PyFormat {
    #[getter]
    fn itag(&self) -> Option<u32> {
        self.inner.itag
    }
    #[getter]
    fn url(&self) -> &str {
        &self.inner.url
    }
    #[getter]
    fn mime_type(&self) -> Option<&str> {
        self.inner.mime_type.as_deref()
    }
    #[getter]
    fn container(&self) -> Option<String> {
        self.inner.container.as_ref().map(container_str)
    }
    #[getter]
    fn video(&self) -> Option<PyVideoStream> {
        self.inner.video.clone().map(|inner| PyVideoStream { inner })
    }
    #[getter]
    fn audio(&self) -> Option<PyAudioStream> {
        self.inner.audio.clone().map(|inner| PyAudioStream { inner })
    }
    #[getter]
    fn filesize(&self) -> Option<u64> {
        self.inner.filesize
    }
    #[getter]
    fn bitrate(&self) -> Option<u64> {
        self.inner.bitrate
    }

    /// "progressive" | "video_only" | "audio_only" | "unknown"
    #[getter]
    fn kind(&self) -> &'static str {
        match self.inner.kind() {
            FormatKind::Progressive => "progressive",
            FormatKind::VideoOnly => "video_only",
            FormatKind::AudioOnly => "audio_only",
            _ => "unknown",
        }
    }

    /// Serialize this format to a JSON string.
    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.inner).map_err(|e| YtdownError::new_err(e.to_string()))
    }

    fn __repr__(&self) -> String {
        format!(
            "Format(itag={}, kind={:?}, container={}, height={})",
            py_opt(&self.inner.itag),
            self.kind(),
            py_opt(&self.inner.container.as_ref().map(container_str)),
            py_opt(&self.inner.video.as_ref().and_then(|v| v.height)),
        )
    }
}

/// Fluent format selector. Filter methods return a narrowed copy, terminal
/// methods pick a single format and raise FormatNotFoundError when nothing
/// matches — mirroring `ytdown::format::FormatSelector`.
#[pyclass(name = "FormatSelector", module = "ytdown", frozen)]
struct PyFormatSelector {
    formats: Vec<Format>,
}

impl PyFormatSelector {
    fn retain(&self, pred: impl Fn(&Format) -> bool) -> Self {
        Self {
            formats: self.formats.iter().filter(|f| pred(f)).cloned().collect(),
        }
    }
}

#[pymethods]
impl PyFormatSelector {
    /// Keep only audio-only formats.
    fn audio_only(&self) -> Self {
        self.retain(|f| matches!(f.kind(), FormatKind::AudioOnly))
    }

    /// Keep only video-only formats.
    fn video_only(&self) -> Self {
        self.retain(|f| matches!(f.kind(), FormatKind::VideoOnly))
    }

    /// Keep only progressive (muxed A+V) formats.
    fn progressive(&self) -> Self {
        self.retain(|f| matches!(f.kind(), FormatKind::Progressive))
    }

    /// Keep only formats whose video height is at most `h`.
    fn max_height(&self, h: u32) -> Self {
        self.retain(|f| {
            f.video
                .as_ref()
                .and_then(|v| v.height)
                .is_some_and(|x| x <= h)
        })
    }

    /// Keep only formats with the given container ("mp4", "webm", "m4a", "weba", ...).
    fn container(&self, c: &str) -> Self {
        let want = parse_container(c);
        self.retain(|f| f.container.as_ref().is_some_and(|fc| *fc == want))
    }

    /// Keep only formats whose video codec starts with `prefix`.
    fn vcodec_starts_with(&self, prefix: &str) -> Self {
        self.retain(|f| f.video.as_ref().is_some_and(|v| v.codec.starts_with(prefix)))
    }

    /// The formats currently in the selection.
    #[getter]
    fn formats(&self) -> Vec<PyFormat> {
        self.formats
            .iter()
            .map(|f| PyFormat { inner: f.clone() })
            .collect()
    }

    /// Find the format with the given itag.
    fn by_itag(&self, itag: u32) -> PyResult<PyFormat> {
        ytdown::FormatSelector::new(&self.formats)
            .by_itag(itag)
            .map(|f| PyFormat { inner: f.clone() })
            .map_err(to_pyerr)
    }

    /// Highest-resolution progressive (muxed) format.
    fn best_progressive(&self) -> PyResult<PyFormat> {
        ytdown::FormatSelector::new(&self.formats)
            .best_progressive()
            .map(|f| PyFormat { inner: f.clone() })
            .map_err(to_pyerr)
    }

    /// Best video stream (video-only or progressive).
    fn best_video(&self) -> PyResult<PyFormat> {
        ytdown::FormatSelector::new(&self.formats)
            .best_video()
            .map(|f| PyFormat { inner: f.clone() })
            .map_err(to_pyerr)
    }

    /// Best audio-only stream by bitrate then sample rate.
    fn best_audio(&self) -> PyResult<PyFormat> {
        ytdown::FormatSelector::new(&self.formats)
            .best_audio()
            .map(|f| PyFormat { inner: f.clone() })
            .map_err(to_pyerr)
    }

    /// Lowest-quality format available.
    fn worst(&self) -> PyResult<PyFormat> {
        ytdown::FormatSelector::new(&self.formats)
            .worst()
            .map(|f| PyFormat { inner: f.clone() })
            .map_err(to_pyerr)
    }

    /// Best split (video, audio) pair for ffmpeg muxing.
    fn best_video_audio(&self) -> PyResult<(PyFormat, PyFormat)> {
        ytdown::FormatSelector::new(&self.formats)
            .best_video_audio()
            .map(|(v, a)| {
                (
                    PyFormat { inner: v.clone() },
                    PyFormat { inner: a.clone() },
                )
            })
            .map_err(to_pyerr)
    }

    fn __len__(&self) -> usize {
        self.formats.len()
    }

    fn __repr__(&self) -> String {
        format!("FormatSelector({} formats)", self.formats.len())
    }
}

/// Metadata + formats for one video.
#[pyclass(name = "VideoInfo", module = "ytdown", frozen)]
struct PyVideoInfo {
    inner: VideoInfo,
}

#[pymethods]
impl PyVideoInfo {
    #[getter]
    fn id(&self) -> &str {
        &self.inner.id
    }
    #[getter]
    fn title(&self) -> &str {
        &self.inner.title
    }
    #[getter]
    fn description(&self) -> Option<&str> {
        self.inner.description.as_deref()
    }
    /// Duration in seconds, if known.
    #[getter]
    fn duration(&self) -> Option<f64> {
        dur_secs(self.inner.duration)
    }
    #[getter]
    fn uploader(&self) -> Option<&str> {
        self.inner.uploader.as_deref()
    }
    #[getter]
    fn uploader_id(&self) -> Option<&str> {
        self.inner.uploader_id.as_deref()
    }
    #[getter]
    fn channel_id(&self) -> Option<&str> {
        self.inner.channel_id.as_deref()
    }
    #[getter]
    fn view_count(&self) -> Option<u64> {
        self.inner.view_count
    }
    /// Upload date as YYYYMMDD, like yt-dlp.
    #[getter]
    fn upload_date(&self) -> Option<&str> {
        self.inner.upload_date.as_deref()
    }
    #[getter]
    fn thumbnails(&self) -> Vec<PyThumbnail> {
        self.inner
            .thumbnails
            .iter()
            .map(|t| PyThumbnail { inner: t.clone() })
            .collect()
    }
    #[getter]
    fn webpage_url(&self) -> &str {
        &self.inner.webpage_url
    }
    #[getter]
    fn is_live(&self) -> bool {
        self.inner.is_live
    }
    #[getter]
    fn formats(&self) -> Vec<PyFormat> {
        self.inner
            .formats
            .iter()
            .map(|f| PyFormat { inner: f.clone() })
            .collect()
    }

    /// Entry point for fluent format selection over this video's formats.
    fn select(&self) -> PyFormatSelector {
        PyFormatSelector {
            formats: self.inner.formats.clone(),
        }
    }

    /// Serialize this video's metadata to a JSON string.
    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.inner).map_err(|e| YtdownError::new_err(e.to_string()))
    }

    fn __repr__(&self) -> String {
        format!(
            "VideoInfo(id={:?}, title={:?}, formats={})",
            self.inner.id,
            self.inner.title,
            self.inner.formats.len()
        )
    }
}

/// A lightweight reference to an item inside a collection. Pass `url` back to
/// `Ytdown.resolve()` to fetch the full item.
#[pyclass(name = "Entry", module = "ytdown", frozen)]
struct PyEntry {
    inner: Entry,
}

#[pymethods]
impl PyEntry {
    #[getter]
    fn id(&self) -> &str {
        &self.inner.id
    }
    #[getter]
    fn title(&self) -> Option<&str> {
        self.inner.title.as_deref()
    }
    #[getter]
    fn url(&self) -> &str {
        &self.inner.url
    }
    /// Duration in seconds, if known.
    #[getter]
    fn duration(&self) -> Option<f64> {
        dur_secs(self.inner.duration)
    }
    #[getter]
    fn thumbnails(&self) -> Vec<PyThumbnail> {
        self.inner
            .thumbnails
            .iter()
            .map(|t| PyThumbnail { inner: t.clone() })
            .collect()
    }
    fn __repr__(&self) -> String {
        format!(
            "Entry(id={:?}, title={})",
            self.inner.id,
            py_opt(&self.inner.title)
        )
    }
}

type EntryStream = futures::stream::BoxStream<'static, ytdown::Result<Entry>>;

/// A playlist/channel/search collection. Iterating yields `Entry` objects
/// lazily, fetching further pages on demand.
#[pyclass(name = "Collection", module = "ytdown", frozen)]
struct PyCollection {
    id: String,
    title: Option<String>,
    kind: CollectionKind,
    entries: Arc<Mutex<EntryStream>>,
}

#[pymethods]
impl PyCollection {
    #[getter]
    fn id(&self) -> &str {
        &self.id
    }
    #[getter]
    fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }
    /// "playlist" | "channel" | "search"
    #[getter]
    fn kind(&self) -> &'static str {
        match self.kind {
            CollectionKind::Playlist => "playlist",
            CollectionKind::Channel => "channel",
            CollectionKind::Search => "search",
            _ => "unknown",
        }
    }

    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&self, py: Python<'_>) -> PyResult<Option<PyEntry>> {
        let entries = self.entries.clone();
        let next = py.detach(move || {
            let mut guard = entries.lock().expect("entry stream lock poisoned");
            runtime().block_on(guard.next())
        });
        match next {
            None => Ok(None),
            Some(Ok(e)) => Ok(Some(PyEntry { inner: e })),
            Some(Err(e)) => Err(to_pyerr(e)),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "Collection(id={:?}, kind={:?}, title={})",
            self.id,
            self.kind(),
            py_opt(&self.title)
        )
    }
}

/// Snapshot of download progress, delivered to the `progress` callback.
#[pyclass(name = "Progress", module = "ytdown", frozen)]
struct PyProgress {
    inner: Progress,
}

#[pymethods]
impl PyProgress {
    #[getter]
    fn bytes_downloaded(&self) -> u64 {
        self.inner.bytes_downloaded
    }
    #[getter]
    fn total_bytes(&self) -> Option<u64> {
        self.inner.total_bytes
    }
    #[getter]
    fn speed_bps(&self) -> Option<f64> {
        self.inner.speed_bps
    }
    /// Estimated time remaining in seconds, if known.
    #[getter]
    fn eta(&self) -> Option<f64> {
        dur_secs(self.inner.eta)
    }
    /// Completion percentage in 0.0..=100.0, if the total size is known.
    fn percent(&self) -> Option<f64> {
        self.inner.percent()
    }
    fn __repr__(&self) -> String {
        format!(
            "Progress(bytes_downloaded={}, total_bytes={}, percent={})",
            self.inner.bytes_downloaded,
            py_opt(&self.inner.total_bytes),
            py_opt(&self.inner.percent())
        )
    }
}

/// The library entry point: resolves URLs into media and downloads formats.
///
/// One instance holds a shared HTTP client and can be reused across many
/// resolves and downloads.
#[pyclass(name = "Ytdown", module = "ytdown", frozen)]
struct PyYtdown {
    inner: ytdown::Ytdown,
}

#[pymethods]
impl PyYtdown {
    /// Ytdown(user_agent=None, ffmpeg_binary=None)
    #[new]
    #[pyo3(signature = (*, user_agent=None, ffmpeg_binary=None))]
    fn new(user_agent: Option<&str>, ffmpeg_binary: Option<PathBuf>) -> PyResult<Self> {
        let mut builder = ytdown::Ytdown::builder();
        if let Some(ua) = user_agent {
            builder = builder.user_agent(ua);
        }
        if let Some(bin) = ffmpeg_binary {
            builder = builder.ffmpeg_binary(bin);
        }
        Ok(Self {
            inner: builder.build().map_err(to_pyerr)?,
        })
    }

    /// Resolve any supported URL (or a "ytsearch:query") into a VideoInfo or
    /// Collection. Raises UnsupportedUrlError if no extractor matches.
    fn resolve(&self, py: Python<'_>, url: &str) -> PyResult<Py<PyAny>> {
        let info = py
            .detach(|| runtime().block_on(self.inner.resolve(url)))
            .map_err(to_pyerr)?;
        match info {
            MediaInfo::Single(v) => Ok(Py::new(py, PyVideoInfo { inner: v })?.into_any()),
            MediaInfo::Collection(c) => Ok(Py::new(
                py,
                PyCollection {
                    id: c.id,
                    title: c.title,
                    kind: c.kind,
                    entries: Arc::new(Mutex::new(c.entries)),
                },
            )?
            .into_any()),
        }
    }

    /// Download a single format to `dest`.
    ///
    /// `progress` is called with a `Progress` snapshot as the download runs;
    /// an exception raised inside it is re-raised after the download finishes.
    #[pyo3(signature = (format, dest, *, progress=None, concurrency=1, chunk_size=None, retries=3, resume=true))]
    #[allow(clippy::too_many_arguments)]
    fn download(
        &self,
        py: Python<'_>,
        format: &PyFormat,
        dest: PathBuf,
        progress: Option<Py<PyAny>>,
        concurrency: usize,
        chunk_size: Option<u64>,
        retries: u32,
        resume: bool,
    ) -> PyResult<()> {
        let fmt = format.inner.clone();
        let cb_err: Arc<Mutex<Option<PyErr>>> = Arc::new(Mutex::new(None));
        let cb_err_inner = cb_err.clone();

        let result = py.detach(|| {
            runtime().block_on(async {
                let mut b = self
                    .inner
                    .download(&fmt, &dest)
                    .concurrency(concurrency)
                    .retries(retries)
                    .resume(resume);
                if let Some(cs) = chunk_size {
                    b = b.chunk_size(cs);
                }
                if let Some(cb) = progress {
                    b = b.progress(move |p| {
                        Python::attach(|py| {
                            // Stop calling back after the first callback error.
                            let mut slot = cb_err_inner.lock().expect("callback error lock");
                            if slot.is_some() {
                                return;
                            }
                            if let Err(e) = cb.call1(py, (PyProgress { inner: p },)) {
                                *slot = Some(e);
                            }
                        });
                    });
                }
                b.await
            })
        });

        // A callback exception happened first and halted further callbacks, so
        // it takes precedence over any later download failure.
        if let Some(e) = cb_err.lock().expect("callback error lock").take() {
            return Err(e);
        }
        result.map_err(to_pyerr)
    }

    /// Download a split video and audio format, then mux them into `dest`
    /// with ffmpeg. Requires ffmpeg on PATH (or `ffmpeg_binary` set).
    fn download_merged(
        &self,
        py: Python<'_>,
        video: &PyFormat,
        audio: &PyFormat,
        dest: PathBuf,
    ) -> PyResult<()> {
        let v = video.inner.clone();
        let a = audio.inner.clone();
        py.detach(|| runtime().block_on(self.inner.download_merged(&v, &a, &dest)))
            .map_err(to_pyerr)
    }
}

/// ytdown — extract, select, and download media (yt-dlp core in Rust).
// The fn is named `ytdown_py` to avoid shadowing the `ytdown` dependency
// crate; `name = "ytdown"` keeps the Python module importable as `ytdown`.
#[pymodule(name = "ytdown")]
fn ytdown_py(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_class::<PyYtdown>()?;
    m.add_class::<PyVideoInfo>()?;
    m.add_class::<PyCollection>()?;
    m.add_class::<PyEntry>()?;
    m.add_class::<PyFormat>()?;
    m.add_class::<PyFormatSelector>()?;
    m.add_class::<PyVideoStream>()?;
    m.add_class::<PyAudioStream>()?;
    m.add_class::<PyThumbnail>()?;
    m.add_class::<PyProgress>()?;
    m.add("YtdownError", py.get_type::<YtdownError>())?;
    m.add("UnsupportedUrlError", py.get_type::<UnsupportedUrlError>())?;
    m.add("NetworkError", py.get_type::<NetworkError>())?;
    m.add("ExtractionError", py.get_type::<ExtractionError>())?;
    m.add("UnavailableError", py.get_type::<UnavailableError>())?;
    m.add("CipherError", py.get_type::<CipherError>())?;
    m.add("FormatNotFoundError", py.get_type::<FormatNotFoundError>())?;
    m.add("IoError", py.get_type::<IoError>())?;
    m.add("PostprocessError", py.get_type::<PostprocessError>())?;
    Ok(())
}
