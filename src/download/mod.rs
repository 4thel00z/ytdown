//! Downloading resolved formats to disk.
//!
//! The [`Downloader`](crate::download::Downloader) streams a URL to a file, honouring HTTP `Range` requests
//! for resume and (optionally) parallel chunked downloads, with retries and
//! progress reporting.

pub mod progress;

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

pub use progress::{Progress, ProgressCallback};

/// Tunables for a download.
pub struct DownloadOptions {
    /// Parallel range-chunk connections (1 = sequential streaming). Default 1.
    pub concurrency: usize,
    /// Chunk size for parallel mode. Default 10 MiB.
    pub chunk_size: u64,
    /// Resume from existing partial file. Default true.
    pub resume: bool,
    /// Max retry attempts per request. Default 3 (exponential backoff 500ms*2^n, jittered).
    pub retries: u32,
    /// Optional progress observer.
    pub progress: Option<ProgressCallback>,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            concurrency: 1,
            chunk_size: 10 * 1024 * 1024,
            resume: true,
            retries: 3,
            progress: None,
        }
    }
}

impl std::fmt::Debug for DownloadOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadOptions")
            .field("concurrency", &self.concurrency)
            .field("chunk_size", &self.chunk_size)
            .field("resume", &self.resume)
            .field("retries", &self.retries)
            .field("progress", &self.progress.as_ref().map(|_| "<callback>"))
            .finish()
    }
}

/// Streams remote media to local files.
pub struct Downloader {
    http: reqwest::Client,
}

/// What an initial probe of the remote resource discovered.
struct RemoteInfo {
    total: Option<u64>,
    accepts_ranges: bool,
}

impl Downloader {
    /// Construct a downloader over an existing shared HTTP client.
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }

    /// Download `url` to `dest`.
    ///
    /// A ranged probe `GET` discovers the content length and whether the server
    /// supports range requests. With `concurrency == 1` (or no range support)
    /// the body is streamed via a single `GET` (resuming from any existing
    /// partial file when `resume` is set); otherwise the file is split into
    /// `chunk_size` ranges downloaded with bounded concurrency and written at
    /// their offsets. Progress is aggregated through a shared atomic counter
    /// and emitted through the optional callback on a ~250ms tick and at
    /// completion.
    pub async fn download(
        &self,
        url: &str,
        dest: &Path,
        opts: DownloadOptions,
    ) -> crate::Result<()> {
        let info = self.probe(url, &opts).await?;

        // Bytes already present on disk we can resume from.
        let existing = if opts.resume {
            match tokio::fs::metadata(dest).await {
                Ok(m) => m.len(),
                Err(_) => 0,
            }
        } else {
            0
        };

        // Already complete?
        if let Some(total) = info.total {
            if existing >= total && total > 0 {
                emit_final(&opts, total);
                return Ok(());
            }
        }

        let downloaded = Arc::new(AtomicU64::new(existing));
        let (tick_handle, tick_stop) = spawn_progress_ticker(&opts, downloaded.clone(), info.total);

        let use_parallel =
            opts.concurrency > 1 && info.accepts_ranges && info.total.is_some() && existing == 0;

        let result = if use_parallel {
            self.download_parallel(url, dest, &opts, info.total.unwrap_or(0), &downloaded)
                .await
        } else {
            self.download_streaming(url, dest, &opts, existing, &downloaded)
                .await
        };

        // Stop the ticker and emit a final snapshot.
        if let Some(stop) = tick_stop {
            let _ = stop.send(());
        }
        if let Some(handle) = tick_handle {
            let _ = handle.await;
        }

        result?;

        let final_bytes = downloaded.load(Ordering::SeqCst);
        emit_final(&opts, final_bytes);
        Ok(())
    }

    /// Discover content length and range support.
    ///
    /// Issues a single-byte ranged `GET` (`Range: bytes=0-0`): a `206` reply
    /// with a `Content-Range: bytes 0-0/<total>` header confirms range support
    /// and reveals the total size, while a `200` reply means the server ignored
    /// the range (no resume / parallel support) and its `Content-Length` is the
    /// total. A ranged probe is more reliable than `HEAD`, which many servers
    /// (and test doubles) answer with a body-derived `Content-Length: 0`.
    async fn probe(&self, url: &str, opts: &DownloadOptions) -> crate::Result<RemoteInfo> {
        let resp = self
            .with_retries(opts, || {
                self.http
                    .get(url)
                    .header(reqwest::header::RANGE, "bytes=0-0")
                    .send()
            })
            .await?;

        if resp.status() == reqwest::StatusCode::PARTIAL_CONTENT {
            let total = resp
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_range_total);
            Ok(RemoteInfo {
                total,
                accepts_ranges: true,
            })
        } else {
            Ok(RemoteInfo {
                total: resp.content_length(),
                accepts_ranges: false,
            })
        }
    }

    /// Single streaming `GET`, optionally resuming from `start_offset`.
    ///
    /// When `start_offset > 0` a `Range` header is sent, but resume only happens
    /// if the server honours it with a `206 Partial Content`. If the server
    /// instead replies `200` (range ignored, full body), the file is truncated
    /// and rewritten from offset 0 and the progress counter is reset — otherwise
    /// the full body would be written at `start_offset`, silently corrupting the
    /// output and double-counting progress.
    async fn download_streaming(
        &self,
        url: &str,
        dest: &Path,
        opts: &DownloadOptions,
        start_offset: u64,
        downloaded: &Arc<AtomicU64>,
    ) -> crate::Result<()> {
        let resp = self
            .with_retries(opts, || {
                let mut req = self.http.get(url);
                if start_offset > 0 {
                    req = req.header(reqwest::header::RANGE, format!("bytes={start_offset}-"));
                }
                req.send()
            })
            .await?;

        // Only resume if we asked for a range AND the server honoured it (206).
        let resuming = start_offset > 0 && resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;

        let mut file = if resuming {
            let mut f = tokio::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(dest)
                .await?;
            f.seek(std::io::SeekFrom::Start(start_offset)).await?;
            f
        } else {
            // Either a fresh download, or the server ignored our Range and sent
            // the full body (200). Start from scratch and reset the counter so
            // the pre-existing bytes are not double-counted.
            if start_offset > 0 {
                downloaded.store(0, Ordering::SeqCst);
            }
            tokio::fs::File::create(dest).await?
        };

        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| crate::Error::Network {
                stage: "download",
                source: e,
            })?;
            file.write_all(&chunk).await?;
            downloaded.fetch_add(chunk.len() as u64, Ordering::SeqCst);
        }
        file.flush().await?;
        Ok(())
    }

    /// Parallel chunked download using range requests written at offsets.
    ///
    /// The pre-sized file is written to a temporary `.part` sibling and only
    /// renamed onto `dest` once every chunk succeeds. This keeps a failed
    /// parallel download from leaving a full-length (zero-filled) file at `dest`
    /// that a later resume would mistake for a completed download.
    async fn download_parallel(
        &self,
        url: &str,
        dest: &Path,
        opts: &DownloadOptions,
        total: u64,
        downloaded: &Arc<AtomicU64>,
    ) -> crate::Result<()> {
        let part = part_path(dest);

        // Pre-allocate the temp file to the full size so chunks can be written at
        // their offsets concurrently.
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&part)
            .await?;
        file.set_len(total).await?;
        drop(file);

        let result = self
            .download_parallel_chunks(url, &part, opts, total, downloaded)
            .await;

        match result {
            Ok(()) => {
                // Atomically publish the completed download.
                tokio::fs::rename(&part, dest).await?;
                Ok(())
            }
            Err(e) => {
                // Remove the partial (full-length, incomplete) temp file so a
                // later resume does not treat it as anything.
                let _ = tokio::fs::remove_file(&part).await;
                Err(e)
            }
        }
    }

    /// Download all chunks of a pre-sized file at `path` with bounded concurrency.
    async fn download_parallel_chunks(
        &self,
        url: &str,
        path: &Path,
        opts: &DownloadOptions,
        total: u64,
        downloaded: &Arc<AtomicU64>,
    ) -> crate::Result<()> {
        let chunk_size = opts.chunk_size.max(1);
        let mut ranges = Vec::new();
        let mut start = 0u64;
        while start < total {
            let end = (start + chunk_size - 1).min(total - 1);
            ranges.push((start, end));
            start = end + 1;
        }

        let tasks = futures::stream::iter(ranges.into_iter().map(|(start, end)| {
            let url = url.to_string();
            let path = path.to_path_buf();
            let downloaded = downloaded.clone();
            async move {
                self.download_chunk(&url, &path, start, end, opts, &downloaded)
                    .await
            }
        }))
        .buffer_unordered(opts.concurrency);

        tasks
            .collect::<Vec<crate::Result<()>>>()
            .await
            .into_iter()
            .collect::<crate::Result<Vec<()>>>()?;
        Ok(())
    }

    /// Download a single byte range and write it at its offset.
    async fn download_chunk(
        &self,
        url: &str,
        dest: &Path,
        start: u64,
        end: u64,
        opts: &DownloadOptions,
        downloaded: &Arc<AtomicU64>,
    ) -> crate::Result<()> {
        let resp = self
            .with_retries(opts, || {
                self.http
                    .get(url)
                    .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
                    .send()
            })
            .await?;

        let mut file = tokio::fs::OpenOptions::new().write(true).open(dest).await?;
        file.seek(std::io::SeekFrom::Start(start)).await?;

        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| crate::Error::Network {
                stage: "download",
                source: e,
            })?;
            file.write_all(&chunk).await?;
            downloaded.fetch_add(chunk.len() as u64, Ordering::SeqCst);
        }
        file.flush().await?;
        Ok(())
    }

    /// Run a request-producing closure with bounded retries and jittered
    /// exponential backoff. Returns the first response with a successful HTTP
    /// status, or the last error once retries are exhausted.
    async fn with_retries<F, Fut>(
        &self,
        opts: &DownloadOptions,
        mut make: F,
    ) -> crate::Result<reqwest::Response>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = reqwest::Result<reqwest::Response>>,
    {
        let mut attempt: u32 = 0;
        loop {
            let outcome = make().await.and_then(|r| r.error_for_status());
            match outcome {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    if attempt >= opts.retries {
                        return Err(crate::Error::Network {
                            stage: "download",
                            source: e,
                        });
                    }
                    let backoff = backoff_delay(attempt);
                    tokio::time::sleep(backoff).await;
                    attempt += 1;
                }
            }
        }
    }
}

/// Compute the temporary `.part` sibling path used for atomic parallel downloads.
fn part_path(dest: &Path) -> std::path::PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    match dest.parent() {
        Some(parent) => parent.join(name),
        None => std::path::PathBuf::from(name),
    }
}

/// Parse the total size out of a `Content-Range: bytes <start>-<end>/<total>` header.
fn parse_content_range_total(value: &str) -> Option<u64> {
    let total = value.rsplit('/').next()?.trim();
    if total == "*" {
        None
    } else {
        total.parse().ok()
    }
}

/// Exponential backoff (500ms * 2^attempt) with a small deterministic jitter.
fn backoff_delay(attempt: u32) -> Duration {
    let base_ms = 500u64.saturating_mul(1u64 << attempt.min(16));
    // Cheap jitter without an RNG dependency: derive from the current time.
    let jitter = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_millis() as u64)
        .unwrap_or(0)
        % 250;
    Duration::from_millis(base_ms + jitter)
}

/// Emit a final progress snapshot with `total` known to equal the downloaded count.
fn emit_final(opts: &DownloadOptions, bytes: u64) {
    if let Some(cb) = &opts.progress {
        cb(Progress {
            bytes_downloaded: bytes,
            total_bytes: Some(bytes),
            speed_bps: None,
            eta: None,
        });
    }
}

/// Spawn a background task that emits progress snapshots roughly every 250ms.
///
/// Returns the join handle and a oneshot sender used to stop it.
fn spawn_progress_ticker(
    opts: &DownloadOptions,
    downloaded: Arc<AtomicU64>,
    total: Option<u64>,
) -> (
    Option<tokio::task::JoinHandle<()>>,
    Option<tokio::sync::oneshot::Sender<()>>,
) {
    let cb = match &opts.progress {
        Some(cb) => cb.clone(),
        None => return (None, None),
    };

    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel();
    let handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(250));
        let started = Instant::now();
        let mut last_bytes = downloaded.load(Ordering::SeqCst);
        let mut last_at = started;
        loop {
            tokio::select! {
                _ = &mut stop_rx => break,
                _ = interval.tick() => {
                    let now = Instant::now();
                    let bytes = downloaded.load(Ordering::SeqCst);
                    let dt = now.duration_since(last_at).as_secs_f64();
                    let speed_bps = if dt > 0.0 {
                        Some((bytes.saturating_sub(last_bytes)) as f64 / dt)
                    } else {
                        None
                    };
                    let eta = match (total, speed_bps) {
                        (Some(total), Some(bps)) if bps > 0.0 && total > bytes => {
                            Some(Duration::from_secs_f64((total - bytes) as f64 / bps))
                        }
                        _ => None,
                    };
                    cb(Progress {
                        bytes_downloaded: bytes,
                        total_bytes: total,
                        speed_bps,
                        eta,
                    });
                    last_bytes = bytes;
                    last_at = now;
                }
            }
        }
    });

    (Some(handle), Some(stop_tx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    /// A responder that honours `Range` headers: returns 206 with the requested
    /// slice and an `Accept-Ranges: bytes` header, or 200 with the full body.
    struct RangeResponder {
        body: Vec<u8>,
    }

    impl Respond for RangeResponder {
        fn respond(&self, req: &Request) -> ResponseTemplate {
            let len = self.body.len() as u64;
            if req.method.as_str().eq_ignore_ascii_case("HEAD") {
                return ResponseTemplate::new(200)
                    .insert_header("accept-ranges", "bytes")
                    .insert_header("content-length", len.to_string().as_str());
            }
            if let Some(range) = req.headers.get("range") {
                if let Ok(range) = range.to_str() {
                    if let Some((start, end)) = parse_range(range, len) {
                        let slice = self.body[start as usize..=end as usize].to_vec();
                        return ResponseTemplate::new(206)
                            .insert_header("accept-ranges", "bytes")
                            .insert_header(
                                "content-range",
                                format!("bytes {start}-{end}/{len}").as_str(),
                            )
                            .set_body_bytes(slice);
                    }
                }
            }
            ResponseTemplate::new(200)
                .insert_header("accept-ranges", "bytes")
                .set_body_bytes(self.body.clone())
        }
    }

    fn parse_range(header: &str, len: u64) -> Option<(u64, u64)> {
        let spec = header.strip_prefix("bytes=")?;
        let (start, end) = spec.split_once('-')?;
        let start: u64 = start.parse().ok()?;
        let end = if end.is_empty() {
            len - 1
        } else {
            end.parse().ok()?
        };
        Some((start, end.min(len - 1)))
    }

    async fn mount_range_mock(server: &MockServer, route: &str, body: Vec<u8>) {
        Mock::given(path(route))
            .respond_with(RangeResponder { body })
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn downloads_full_body_to_path() {
        let server = MockServer::start().await;
        let body = vec![7u8; 2_000_000];
        mount_range_mock(&server, "/file", body.clone()).await;
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.bin");
        let dl = Downloader::new(reqwest::Client::new());
        dl.download(
            &format!("{}/file", server.uri()),
            &dest,
            DownloadOptions::default(),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
    }

    #[tokio::test]
    async fn resumes_from_partial_file() {
        let server = MockServer::start().await;
        let body: Vec<u8> = (0..2000u32).map(|i| (i % 251) as u8).collect();

        let received_range: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let captured = received_range.clone();
        let full_body = body.clone();

        // Custom responder that records the Range header it received on GET.
        struct CapturingResponder {
            body: Vec<u8>,
            captured: Arc<Mutex<Option<String>>>,
        }
        impl Respond for CapturingResponder {
            fn respond(&self, req: &Request) -> ResponseTemplate {
                let len = self.body.len() as u64;
                if req.method.as_str().eq_ignore_ascii_case("HEAD") {
                    return ResponseTemplate::new(200)
                        .insert_header("accept-ranges", "bytes")
                        .insert_header("content-length", len.to_string().as_str());
                }
                if let Some(range) = req.headers.get("range") {
                    if let Ok(range) = range.to_str() {
                        // Ignore the single-byte probe range; record only the
                        // resume range issued by the streaming download.
                        if range != "bytes=0-0" {
                            *self.captured.lock().unwrap() = Some(range.to_string());
                        }
                        let spec = range.strip_prefix("bytes=").unwrap();
                        let (start, end) = spec.split_once('-').unwrap();
                        let start: u64 = start.parse().unwrap();
                        let end = if end.is_empty() {
                            len - 1
                        } else {
                            end.parse().unwrap()
                        };
                        let slice = self.body[start as usize..=end as usize].to_vec();
                        return ResponseTemplate::new(206)
                            .insert_header("accept-ranges", "bytes")
                            .set_body_bytes(slice);
                    }
                }
                ResponseTemplate::new(200)
                    .insert_header("accept-ranges", "bytes")
                    .set_body_bytes(self.body.clone())
            }
        }

        Mock::given(path("/file"))
            .respond_with(CapturingResponder {
                body: full_body,
                captured,
            })
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.bin");
        // Pre-write the first 1000 bytes.
        std::fs::write(&dest, &body[..1000]).unwrap();

        let dl = Downloader::new(reqwest::Client::new());
        dl.download(
            &format!("{}/file", server.uri()),
            &dest,
            DownloadOptions::default(),
        )
        .await
        .unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert_eq!(
            received_range.lock().unwrap().as_deref(),
            Some("bytes=1000-")
        );
    }

    #[tokio::test]
    async fn reports_progress_monotonically() {
        let server = MockServer::start().await;
        let body = vec![3u8; 1_500_000];
        mount_range_mock(&server, "/file", body.clone()).await;

        let events: Arc<Mutex<Vec<Progress>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let cb: ProgressCallback = Arc::new(move |p: Progress| {
            sink.lock().unwrap().push(p);
        });

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.bin");
        let dl = Downloader::new(reqwest::Client::new());
        let opts = DownloadOptions {
            progress: Some(cb),
            ..DownloadOptions::default()
        };
        dl.download(&format!("{}/file", server.uri()), &dest, opts)
            .await
            .unwrap();

        let events = events.lock().unwrap();
        assert!(!events.is_empty());
        let mut last = 0u64;
        for p in events.iter() {
            assert!(p.bytes_downloaded >= last);
            last = p.bytes_downloaded;
        }
        let final_event = events.last().unwrap();
        assert_eq!(final_event.bytes_downloaded, body.len() as u64);
        assert_eq!(final_event.total_bytes, Some(body.len() as u64));
    }

    #[tokio::test]
    async fn retries_transient_failures() {
        let server = MockServer::start().await;
        let body = vec![5u8; 500_000];

        // First GET returns 503, then success. The first request is the ranged
        // probe, which is what trips the transient failure; retry recovers.
        Mock::given(method("GET"))
            .and(path("/file"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/file"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.bin");
        let dl = Downloader::new(reqwest::Client::new());
        dl.download(
            &format!("{}/file", server.uri()),
            &dest,
            DownloadOptions::default(),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
    }

    #[tokio::test]
    async fn parallel_chunked_download() {
        let server = MockServer::start().await;
        let body: Vec<u8> = (0..3_000_000u32).map(|i| (i % 251) as u8).collect();

        // Capture every Range header so we can assert that the parallel path
        // actually issued multiple distinct sub-range GETs (not a single
        // full-body fetch from a regressed streaming fallback).
        let ranges: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        struct RecordingRangeResponder {
            body: Vec<u8>,
            ranges: Arc<Mutex<Vec<String>>>,
        }
        impl Respond for RecordingRangeResponder {
            fn respond(&self, req: &Request) -> ResponseTemplate {
                let len = self.body.len() as u64;
                if let Some(range) = req.headers.get("range") {
                    if let Ok(range) = range.to_str() {
                        self.ranges.lock().unwrap().push(range.to_string());
                        if let Some((start, end)) = parse_range(range, len) {
                            let slice = self.body[start as usize..=end as usize].to_vec();
                            return ResponseTemplate::new(206)
                                .insert_header("accept-ranges", "bytes")
                                .insert_header(
                                    "content-range",
                                    format!("bytes {start}-{end}/{len}").as_str(),
                                )
                                .set_body_bytes(slice);
                        }
                    }
                }
                ResponseTemplate::new(200)
                    .insert_header("accept-ranges", "bytes")
                    .set_body_bytes(self.body.clone())
            }
        }

        Mock::given(path("/file"))
            .respond_with(RecordingRangeResponder {
                body: body.clone(),
                ranges: ranges.clone(),
            })
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.bin");
        let dl = Downloader::new(reqwest::Client::new());
        let opts = DownloadOptions {
            concurrency: 4,
            chunk_size: 512 * 1024,
            ..DownloadOptions::default()
        };
        dl.download(&format!("{}/file", server.uri()), &dest, opts)
            .await
            .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);

        // Count distinct sub-range GETs (excluding the bytes=0-0 probe). A 3MB
        // body with 512KiB chunks must produce >1 distinct ranged GET.
        let recorded = ranges.lock().unwrap();
        let distinct: std::collections::BTreeSet<&str> = recorded
            .iter()
            .map(String::as_str)
            .filter(|r| *r != "bytes=0-0")
            .collect();
        assert!(
            distinct.len() > 1,
            "parallel download must issue multiple distinct sub-range GETs, got {distinct:?}"
        );

        // The temp .part file must have been renamed away on success.
        assert!(!super::part_path(&dest).exists());
    }

    /// Regression for the streaming-resume corruption: when the server ignores
    /// the `Range` header and replies `200` with the full body, the file must be
    /// rewritten from offset 0 (not appended at `start_offset`), and progress
    /// must reflect only the real body length.
    #[tokio::test]
    async fn resume_against_non_range_server_does_not_corrupt() {
        let server = MockServer::start().await;
        let body: Vec<u8> = (0..2000u32).map(|i| (i % 251) as u8).collect();

        // A responder that ALWAYS returns 200 with the full body, ignoring Range.
        struct AlwaysFullResponder {
            body: Vec<u8>,
        }
        impl Respond for AlwaysFullResponder {
            fn respond(&self, _req: &Request) -> ResponseTemplate {
                ResponseTemplate::new(200).set_body_bytes(self.body.clone())
            }
        }
        Mock::given(path("/file"))
            .respond_with(AlwaysFullResponder { body: body.clone() })
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.bin");
        // Pre-write a partial prefix to trigger the resume path.
        std::fs::write(&dest, &body[..1000]).unwrap();

        let events: Arc<Mutex<Vec<Progress>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let cb: ProgressCallback = Arc::new(move |p: Progress| {
            sink.lock().unwrap().push(p);
        });

        let dl = Downloader::new(reqwest::Client::new());
        let opts = DownloadOptions {
            progress: Some(cb),
            ..DownloadOptions::default()
        };
        dl.download(&format!("{}/file", server.uri()), &dest, opts)
            .await
            .unwrap();

        // Exactly the body, no duplicated head / garbage middle / extra length.
        assert_eq!(std::fs::read(&dest).unwrap(), body);

        // Progress must end at the real body length, not start_offset + total.
        let events = events.lock().unwrap();
        let final_event = events.last().unwrap();
        assert_eq!(final_event.bytes_downloaded, body.len() as u64);
    }

    /// Regression for finding 2: a failed parallel download must not leave a
    /// full-length file at `dest` that a later resume accepts as complete.
    #[tokio::test]
    async fn failed_parallel_download_leaves_no_complete_file() {
        let server = MockServer::start().await;
        let len = 3_000_000u64;

        // Probe (bytes=0-0) succeeds with a content-range so total is known and
        // ranges are accepted; every other request fails with 500, so chunks
        // never complete.
        struct ProbeOkThenFail {
            len: u64,
        }
        impl Respond for ProbeOkThenFail {
            fn respond(&self, req: &Request) -> ResponseTemplate {
                if let Some(range) = req.headers.get("range") {
                    if range.to_str().map(|r| r == "bytes=0-0").unwrap_or(false) {
                        return ResponseTemplate::new(206)
                            .insert_header("accept-ranges", "bytes")
                            .insert_header(
                                "content-range",
                                format!("bytes 0-0/{}", self.len).as_str(),
                            )
                            .set_body_bytes(vec![0u8]);
                    }
                }
                ResponseTemplate::new(500)
            }
        }
        Mock::given(path("/file"))
            .respond_with(ProbeOkThenFail { len })
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.bin");
        let dl = Downloader::new(reqwest::Client::new());
        let opts = DownloadOptions {
            concurrency: 4,
            chunk_size: 512 * 1024,
            retries: 0,
            ..DownloadOptions::default()
        };
        let result = dl
            .download(&format!("{}/file", server.uri()), &dest, opts)
            .await;
        assert!(result.is_err(), "parallel download should fail");

        // No full-length (or any) file should remain at dest, and no leftover
        // .part file either.
        assert!(
            !dest.exists(),
            "failed parallel download must not leave a file at dest"
        );
        assert!(!super::part_path(&dest).exists(), "no leftover .part file");
    }

    /// Regression for finding 17: retry exhaustion must surface as
    /// `Error::Network` with the download stage, bounded by `retries`.
    #[tokio::test]
    async fn retry_exhaustion_yields_network_error() {
        let server = MockServer::start().await;
        // Always 503: every attempt fails, so retries are exhausted.
        Mock::given(method("GET"))
            .and(path("/file"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.bin");
        let dl = Downloader::new(reqwest::Client::new());
        let opts = DownloadOptions {
            retries: 1,
            ..DownloadOptions::default()
        };
        let err = dl
            .download(&format!("{}/file", server.uri()), &dest, opts)
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                crate::Error::Network {
                    stage: "download",
                    ..
                }
            ),
            "expected Network error, got {err:?}"
        );
    }
}
