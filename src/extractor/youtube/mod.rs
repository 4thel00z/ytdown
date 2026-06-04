//! YouTube extractor: recognizes YouTube URLs and orchestrates the InnerTube
//! client, the player cipher solver, and lazy pagination into [`MediaInfo`].

pub(crate) mod innertube;
pub(crate) mod pagination;
pub(crate) mod player;

use std::sync::Arc;

use url::Url;

use crate::extractor::{Extractor, ExtractorContext};
use crate::types::{
    AudioStream, CollectionInfo, CollectionKind, Container, Format, MediaInfo, Thumbnail,
    VideoInfo, VideoStream,
};
use crate::{Error, Result};

use self::innertube::{BrowseRequest, ClientKind, InnerTube, PlayerResponse, RawFormat};
use self::pagination::{entry_stream, PageKind};
use self::player::{decipher_url, fetch_player_js, PlayerSolver, SolvedPlayer};

/// What a YouTube URL points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UrlKind<'a> {
    /// A single video, by 11-character id.
    Video(&'a str),
    /// A playlist, by list id.
    Playlist(&'a str),
    /// A channel, by handle or id.
    Channel(ChannelRef<'a>),
    /// A search query (`ytsearch:` scheme).
    Search(&'a str),
}

/// How a channel was referenced in the URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ChannelRef<'a> {
    /// An `@handle`.
    Handle(&'a str),
    /// A `UC...` channel id.
    Id(&'a str),
}

/// Whether a string is a plausible 11-character video id.
fn is_video_id(s: &str) -> bool {
    s.len() == 11
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Classify a URL string into a [`UrlKind`], or `None` if it is not a YouTube URL.
///
/// Borrows substrings of the input, so the returned ids point into `url`.
pub(crate) fn classify(url: &str) -> Option<UrlKind<'_>> {
    // `ytsearch:` pseudo-scheme, mirroring yt-dlp.
    if let Some(query) = url.strip_prefix("ytsearch:") {
        if query.is_empty() {
            return None;
        }
        return Some(UrlKind::Search(query));
    }

    let parsed = Url::parse(url).ok()?;
    let host = parsed.host_str()?;
    let host = host.strip_prefix("www.").unwrap_or(host);

    // youtu.be/<id>
    if host == "youtu.be" {
        let id = parsed.path().trim_start_matches('/');
        let id = id.split('/').next().unwrap_or(id);
        let off = offset_in(url, id)?;
        return is_video_id(&url[off..off + id.len()])
            .then(|| UrlKind::Video(&url[off..off + id.len()]));
    }

    if !matches!(
        host,
        "youtube.com" | "m.youtube.com" | "music.youtube.com" | "youtube-nocookie.com"
    ) {
        return None;
    }

    // A `v=` query parameter wins over any `list=` (a watch URL inside a playlist).
    if let Some((_, v)) = parsed.query_pairs().find(|(k, _)| k == "v") {
        if is_video_id(&v) {
            let off = offset_in(url, &v)?;
            return Some(UrlKind::Video(&url[off..off + v.len()]));
        }
    }

    let mut segments = parsed.path_segments()?.filter(|s| !s.is_empty());
    let first = segments.next();
    match first {
        Some("shorts") | Some("embed") | Some("v") | Some("e") => {
            let id = segments.next()?;
            let off = offset_in(url, id)?;
            return is_video_id(&url[off..off + id.len()])
                .then(|| UrlKind::Video(&url[off..off + id.len()]));
        }
        Some("playlist") => {
            let (_, list) = parsed.query_pairs().find(|(k, _)| k == "list")?;
            if list.is_empty() {
                return None;
            }
            let off = offset_in(url, &list)?;
            return Some(UrlKind::Playlist(&url[off..off + list.len()]));
        }
        Some("channel") => {
            let id = segments.next()?;
            let off = offset_in(url, id)?;
            return Some(UrlKind::Channel(ChannelRef::Id(&url[off..off + id.len()])));
        }
        Some(seg) if seg.starts_with('@') => {
            let handle = &seg[1..];
            if handle.is_empty() {
                return None;
            }
            let off = offset_in(url, handle)?;
            return Some(UrlKind::Channel(ChannelRef::Handle(
                &url[off..off + handle.len()],
            )));
        }
        _ => {}
    }

    // Bare `?list=` on a non-watch path is a playlist.
    if let Some((_, list)) = parsed.query_pairs().find(|(k, _)| k == "list") {
        if !list.is_empty() {
            let off = offset_in(url, &list)?;
            return Some(UrlKind::Playlist(&url[off..off + list.len()]));
        }
    }

    None
}

/// Find the byte offset of `needle` within `haystack`, used to recover a borrowed
/// slice of the original URL string from a decoded query value.
fn offset_in(haystack: &str, needle: &str) -> Option<usize> {
    haystack.find(needle)
}

/// YouTube extractor. Holds an optional base-URL override for tests.
pub struct YoutubeExtractor {
    /// Base URL for both InnerTube and the player JS host. Defaults to the real
    /// YouTube origin; tests point it at a mock server.
    base_url: String,
}

impl Default for YoutubeExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl YoutubeExtractor {
    /// Build an extractor targeting the real YouTube origin.
    pub fn new() -> Self {
        Self {
            base_url: "https://www.youtube.com".to_string(),
        }
    }

    /// Build an extractor targeting an arbitrary base URL.
    ///
    /// Primarily for tests against a mock server, but also usable to point at an
    /// alternate origin. The URL hosts both the InnerTube endpoints and the
    /// player JavaScript.
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }

    /// Construct an InnerTube client bound to this extractor's base URL.
    fn innertube(&self, ctx: &ExtractorContext) -> InnerTube {
        InnerTube::with_base_url(ctx.http.clone(), self.base_url.clone())
    }

    /// Resolve a single video into [`MediaInfo::Single`].
    async fn extract_video(&self, ctx: &ExtractorContext, id: &str) -> Result<MediaInfo> {
        let it = self.innertube(ctx);

        // Try clients in order; age-restriction (LOGIN_REQUIRED → AgeRestricted)
        // falls through to embedded clients that can sometimes still play it.
        let response = self.fetch_player_with_fallback(&it, id).await?;

        let details = &response.video_details;
        let microformat = response
            .microformat
            .as_ref()
            .and_then(|m| m.player_microformat_renderer.as_ref());

        let mut info = VideoInfo {
            id: details.video_id.clone(),
            title: details.title.clone(),
            description: details.short_description.clone(),
            duration: details
                .length_seconds
                .as_deref()
                .and_then(|s| s.parse::<u64>().ok())
                .map(std::time::Duration::from_secs),
            uploader: details.author.clone(),
            uploader_id: None,
            channel_id: details.channel_id.clone(),
            view_count: details
                .view_count
                .as_deref()
                .and_then(|s| s.parse::<u64>().ok()),
            upload_date: microformat
                .and_then(|m| m.upload_date.as_deref())
                .map(format_upload_date),
            thumbnails: details
                .thumbnail
                .as_ref()
                .map(|t| {
                    t.thumbnails
                        .iter()
                        .map(|rt| Thumbnail {
                            url: rt.url.clone(),
                            width: rt.width,
                            height: rt.height,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            webpage_url: format!("https://www.youtube.com/watch?v={}", details.video_id),
            is_live: details.is_live,
            formats: Vec::new(),
        };

        if !info.is_live {
            info.formats = self.build_formats(ctx, &response).await?;
        }

        Ok(MediaInfo::Single(info))
    }

    /// Fetch the player response, retrying with embedded clients on age restriction.
    async fn fetch_player_with_fallback(&self, it: &InnerTube, id: &str) -> Result<PlayerResponse> {
        let mut last_err = None;
        for client in [ClientKind::Android, ClientKind::Tv, ClientKind::Ios] {
            match it.player(id, client).await {
                Ok(resp) => return Ok(resp),
                Err(Error::Unavailable {
                    reason: crate::error::UnavailableReason::AgeRestricted,
                    message,
                }) => {
                    last_err = Some(Error::Unavailable {
                        reason: crate::error::UnavailableReason::AgeRestricted,
                        message,
                    });
                    continue;
                }
                Err(other) => return Err(other),
            }
        }
        Err(last_err.unwrap_or_else(|| Error::Extraction {
            stage: "player",
            message: "no client returned a playable response".into(),
        }))
    }

    /// Decipher every raw format into a [`Format`], lazily solving the player only
    /// when a format requires it. Formats that cannot be deciphered are skipped
    /// with a warning rather than failing the whole extraction.
    async fn build_formats(
        &self,
        ctx: &ExtractorContext,
        response: &PlayerResponse,
    ) -> Result<Vec<Format>> {
        let Some(streaming) = response.streaming_data.as_ref() else {
            return Ok(Vec::new());
        };

        let raws: Vec<&RawFormat> = streaming
            .formats
            .iter()
            .chain(streaming.adaptive_formats.iter())
            .collect();

        // Solve the player once, lazily, only if any format needs it (signatureCipher,
        // or a direct URL carrying an `n` throttling parameter).
        let needs_player = raws
            .iter()
            .any(|r| r.signature_cipher.is_some() || url_has_n_param(r.url.as_deref()));

        let solved = if needs_player {
            Some(self.solved_player(ctx).await?)
        } else {
            None
        };

        let mut out = Vec::with_capacity(raws.len());
        for raw in raws {
            let url = match self.resolve_url(raw, solved.as_deref()) {
                Ok(url) => url,
                Err(e) => {
                    tracing::warn!(itag = raw.itag, error = %e, "skipping undecipherable format");
                    continue;
                }
            };
            out.push(build_format(raw, url));
        }
        Ok(out)
    }

    /// Resolve a single raw format's URL, applying cipher solving when available.
    fn resolve_url(&self, raw: &RawFormat, solved: Option<&SolvedPlayer>) -> Result<String> {
        match solved {
            Some(player) => decipher_url(raw, player),
            None => raw
                .url
                .clone()
                .ok_or_else(|| Error::Cipher("format requires player but none solved".into())),
        }
    }

    /// Fetch + solve the player JS for the current player version, caching the
    /// result so it is reused across formats and across videos sharing a version.
    async fn solved_player(&self, ctx: &ExtractorContext) -> Result<Arc<SolvedPlayer>> {
        let (version, js) = fetch_player_js(&ctx.http, &self.base_url).await?;
        {
            let cache = ctx.player_cache.lock().await;
            if let Some(found) = cache.get(&version) {
                return Ok(found.clone());
            }
        }
        let solved = Arc::new(PlayerSolver::from_js(&js)?);
        let mut cache = ctx.player_cache.lock().await;
        let entry = cache.entry(version).or_insert_with(|| solved.clone());
        Ok(entry.clone())
    }

    /// Resolve a playlist into a lazily-paginated [`MediaInfo::Collection`].
    async fn extract_playlist(&self, ctx: &ExtractorContext, list_id: &str) -> Result<MediaInfo> {
        let it = Arc::new(self.innertube(ctx));
        let browse_id = if list_id.starts_with("VL") {
            list_id.to_string()
        } else {
            format!("VL{list_id}")
        };
        let first = it
            .browse(BrowseRequest {
                browse_id: Some(browse_id),
                ..BrowseRequest::default()
            })
            .await?;
        Ok(MediaInfo::Collection(CollectionInfo {
            id: list_id.to_string(),
            title: None,
            kind: CollectionKind::Playlist,
            entries: entry_stream(it, first, PageKind::Playlist),
        }))
    }

    /// Resolve a channel's uploads into a lazily-paginated collection.
    async fn extract_channel(
        &self,
        ctx: &ExtractorContext,
        channel: &ChannelRef<'_>,
    ) -> Result<MediaInfo> {
        let it = Arc::new(self.innertube(ctx));
        let (browse_id, id_label) = match channel {
            ChannelRef::Id(id) => ((*id).to_string(), (*id).to_string()),
            ChannelRef::Handle(handle) => {
                // Resolve the handle to a browseId via a browse on the handle path.
                let resolved = it
                    .browse(BrowseRequest {
                        browse_id: Some(format!("@{handle}")),
                        ..BrowseRequest::default()
                    })
                    .await?;
                let bid = find_channel_browse_id(&resolved).unwrap_or_else(|| format!("@{handle}"));
                (bid, format!("@{handle}"))
            }
        };
        // The uploads tab selector.
        let first = it
            .browse(BrowseRequest {
                browse_id: Some(browse_id),
                params: Some("EgZ2aWRlb3PyBgQKAjoA".to_string()),
                ..BrowseRequest::default()
            })
            .await?;
        Ok(MediaInfo::Collection(CollectionInfo {
            id: id_label,
            title: None,
            kind: CollectionKind::Channel,
            entries: entry_stream(it, first, PageKind::Channel),
        }))
    }

    /// Resolve a search query into a lazily-paginated collection.
    async fn extract_search(&self, ctx: &ExtractorContext, query: &str) -> Result<MediaInfo> {
        let it = Arc::new(self.innertube(ctx));
        let first = it.search(query, None).await?;
        Ok(MediaInfo::Collection(CollectionInfo {
            id: query.to_string(),
            title: Some(query.to_string()),
            kind: CollectionKind::Search,
            entries: entry_stream(it, first, PageKind::Search(query.to_string())),
        }))
    }
}

/// Whether a (direct) stream URL carries an `n` throttling query parameter that
/// must be transformed by the player's `nsig` function.
fn url_has_n_param(url: Option<&str>) -> bool {
    let Some(url) = url else { return false };
    match Url::parse(url) {
        Ok(parsed) => parsed.query_pairs().any(|(k, _)| k == "n"),
        Err(_) => false,
    }
}

/// Convert an InnerTube `YYYY-MM-DD` upload date to yt-dlp's `YYYYMMDD`.
fn format_upload_date(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// Build a [`Format`] from a raw descriptor and an already-resolved URL.
fn build_format(raw: &RawFormat, url: String) -> Format {
    let (container, video, audio) = parse_mime(&raw.mime_type, raw);
    Format {
        itag: Some(raw.itag),
        url,
        mime_type: Some(raw.mime_type.clone()),
        container,
        video,
        audio,
        filesize: raw
            .content_length
            .as_deref()
            .and_then(|s| s.parse::<u64>().ok()),
        bitrate: raw.bitrate,
    }
}

/// Parse a MIME type like `video/mp4; codecs="avc1.4d401f, mp4a.40.2"` into a
/// container plus video/audio stream descriptors.
///
/// Two codecs => progressive (video + audio); an `audio/` type => audio-only;
/// otherwise video-only.
fn parse_mime(
    mime: &str,
    raw: &RawFormat,
) -> (Option<Container>, Option<VideoStream>, Option<AudioStream>) {
    let (kind, rest) = match mime.split_once('/') {
        Some(parts) => parts,
        None => return (None, None, None),
    };
    let mut subtype = rest;
    let mut codecs: Vec<String> = Vec::new();
    if let Some((sub, params)) = rest.split_once(';') {
        subtype = sub;
        for param in params.split(';') {
            let param = param.trim();
            if let Some(list) = param.strip_prefix("codecs=") {
                let list = list.trim().trim_matches('"');
                codecs = list
                    .split(',')
                    .map(|c| c.trim().to_string())
                    .filter(|c| !c.is_empty())
                    .collect();
            }
        }
    }
    let subtype = subtype.trim();

    let container = container_for(kind, subtype);

    if kind.eq_ignore_ascii_case("audio") {
        let codec = codecs.first().cloned().unwrap_or_default();
        let audio = Some(AudioStream {
            codec,
            bitrate: raw.bitrate,
            sample_rate: raw
                .audio_sample_rate
                .as_deref()
                .and_then(|s| s.parse::<u32>().ok()),
            channels: raw.audio_channels,
        });
        return (container, None, audio);
    }

    // video/* — may be progressive (two codecs) or video-only (one codec).
    let video_codec = codecs.first().cloned().unwrap_or_default();
    let video = Some(VideoStream {
        width: raw.width,
        height: raw.height,
        fps: raw.fps,
        codec: video_codec,
    });
    let audio = if codecs.len() >= 2 {
        Some(AudioStream {
            codec: codecs[1].clone(),
            bitrate: None,
            sample_rate: raw
                .audio_sample_rate
                .as_deref()
                .and_then(|s| s.parse::<u32>().ok()),
            channels: raw.audio_channels,
        })
    } else {
        None
    };
    (container, video, audio)
}

/// Map a MIME kind/subtype pair to a [`Container`].
fn container_for(kind: &str, subtype: &str) -> Option<Container> {
    match (kind.to_ascii_lowercase().as_str(), subtype) {
        ("video", "mp4") => Some(Container::Mp4),
        ("video", "webm") => Some(Container::WebM),
        ("audio", "mp4") => Some(Container::M4a),
        ("audio", "webm") => Some(Container::Weba),
        _ => Some(Container::Other(format!("{kind}/{subtype}"))),
    }
}

/// Dig a channel `browseId` out of a handle-resolution browse response.
fn find_channel_browse_id(value: &serde_json::Value) -> Option<String> {
    fn walk(node: &serde_json::Value) -> Option<String> {
        match node {
            serde_json::Value::Object(map) => {
                if let Some(id) = map
                    .get("browseId")
                    .and_then(serde_json::Value::as_str)
                    .filter(|s| s.starts_with("UC"))
                {
                    return Some(id.to_string());
                }
                map.values().find_map(walk)
            }
            serde_json::Value::Array(items) => items.iter().find_map(walk),
            _ => None,
        }
    }
    walk(value)
}

#[async_trait::async_trait]
impl Extractor for YoutubeExtractor {
    fn name(&self) -> &'static str {
        "youtube"
    }

    fn matches(&self, url: &Url) -> bool {
        classify(url.as_str()).is_some()
    }

    async fn extract(&self, ctx: &ExtractorContext, url: &Url) -> Result<MediaInfo> {
        let url_str = url.as_str();
        let kind = classify(url_str).ok_or_else(|| Error::UnsupportedUrl(url_str.to_string()))?;
        match kind {
            UrlKind::Video(id) => self.extract_video(ctx, id).await,
            UrlKind::Playlist(list) => self.extract_playlist(ctx, list).await,
            UrlKind::Channel(channel) => self.extract_channel(ctx, &channel).await,
            UrlKind::Search(query) => self.extract_search(ctx, query).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_url_kinds() {
        let cases: &[(&str, UrlKind)] = &[
            (
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
                UrlKind::Video("dQw4w9WgXcQ"),
            ),
            (
                "https://youtu.be/dQw4w9WgXcQ?t=1",
                UrlKind::Video("dQw4w9WgXcQ"),
            ),
            (
                "https://www.youtube.com/shorts/abc12345678",
                UrlKind::Video("abc12345678"),
            ),
            (
                "https://www.youtube.com/embed/abc12345678",
                UrlKind::Video("abc12345678"),
            ),
            (
                "https://www.youtube.com/playlist?list=PLx",
                UrlKind::Playlist("PLx"),
            ),
            (
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PLx",
                UrlKind::Video("dQw4w9WgXcQ"),
            ),
            (
                "https://www.youtube.com/@SomeHandle",
                UrlKind::Channel(ChannelRef::Handle("SomeHandle")),
            ),
            (
                "https://www.youtube.com/channel/UCabc",
                UrlKind::Channel(ChannelRef::Id("UCabc")),
            ),
            (
                "ytsearch:rust programming",
                UrlKind::Search("rust programming"),
            ),
        ];
        for (u, expect) in cases {
            assert_eq!(classify(u).as_ref(), Some(expect), "url: {u}");
        }
        assert!(classify("https://vimeo.com/123").is_none());
    }

    #[test]
    fn parse_mime_progressive_has_video_and_audio() {
        let raw = sample_raw("video/mp4; codecs=\"avc1.42001E, mp4a.40.2\"");
        let (container, video, audio) = parse_mime(&raw.mime_type, &raw);
        assert_eq!(container, Some(Container::Mp4));
        assert_eq!(video.unwrap().codec, "avc1.42001E");
        assert_eq!(audio.unwrap().codec, "mp4a.40.2");
    }

    #[test]
    fn parse_mime_audio_only() {
        let raw = sample_raw("audio/webm; codecs=\"opus\"");
        let (container, video, audio) = parse_mime(&raw.mime_type, &raw);
        assert_eq!(container, Some(Container::Weba));
        assert!(video.is_none());
        assert_eq!(audio.unwrap().codec, "opus");
    }

    #[test]
    fn parse_mime_video_only() {
        let raw = sample_raw("video/webm; codecs=\"vp9\"");
        let (container, video, audio) = parse_mime(&raw.mime_type, &raw);
        assert_eq!(container, Some(Container::WebM));
        assert_eq!(video.unwrap().codec, "vp9");
        assert!(audio.is_none());
    }

    #[test]
    fn upload_date_formats_to_yyyymmdd() {
        assert_eq!(format_upload_date("2009-10-25"), "20091025");
    }

    fn sample_raw(mime: &str) -> RawFormat {
        RawFormat {
            itag: 1,
            url: None,
            signature_cipher: None,
            mime_type: mime.to_string(),
            width: Some(640),
            height: Some(360),
            fps: Some(30.0),
            bitrate: Some(500_000),
            content_length: Some("1000".into()),
            audio_sample_rate: Some("48000".into()),
            audio_channels: Some(2),
        }
    }
}
