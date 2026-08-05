//! TikTok extractor: resolves tiktok.com video URLs (and `vm.`/`vt.` share
//! shortlinks) by parsing the webpage's embedded
//! `__UNIVERSAL_DATA_FOR_REHYDRATION__` JSON into progressive MP4 formats.

use url::Url;

use crate::error::UnavailableReason;
use crate::extractor::shared::upload_date_from_epoch;
use crate::extractor::{Extractor, ExtractorContext};
use crate::transport::HttpRequest;
use crate::types::{AudioStream, Container, Format, MediaInfo, Thumbnail, VideoInfo, VideoStream};
use crate::{Error, Result};

/// Browser-like User-Agent for webpage fetches: TikTok serves the embedded
/// JSON to browsers and a bare login shell to generic client UAs.
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

/// What a TikTok URL points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TiktokUrl {
    /// A single video by numeric id.
    Video(String),
    /// A photo/slideshow post by numeric id (recognized, but unsupported).
    Photo(String),
    /// A share shortlink code that redirects to the canonical post.
    Shortlink(String),
}

/// Classify a TikTok URL, or `None` if it is not one.
fn classify(url: &Url) -> Option<TiktokUrl> {
    let host = url.host_str()?;
    let host = host.strip_prefix("www.").unwrap_or(host);

    let mut segments = url.path_segments()?.filter(|s| !s.is_empty());

    // Share shortlink hosts: vm.tiktok.com/<code>, vt.tiktok.com/<code>.
    if host == "vm.tiktok.com" || host == "vt.tiktok.com" {
        let code = segments.next()?;
        return (segments.next().is_none() && is_share_code(code))
            .then(|| TiktokUrl::Shortlink(code.to_string()));
    }

    if !(host == "tiktok.com" || host.ends_with(".tiktok.com")) {
        return None;
    }

    let first = segments.next()?;
    // tiktok.com/t/<code> share shortlink.
    if first == "t" {
        let code = segments.next()?;
        return is_share_code(code).then(|| TiktokUrl::Shortlink(code.to_string()));
    }
    // tiktok.com/@<user>/video/<id> or /photo/<id>.
    if first.starts_with('@') {
        return classify_post_path(url.path());
    }
    None
}

/// Classify a `/@<user>/(video|photo)/<id>` path, host-agnostic. Used for the
/// canonical URL a share shortlink redirects to, where tests substitute the
/// host.
fn classify_post_path(path: &str) -> Option<TiktokUrl> {
    let mut segments = path.split('/').filter(|s| !s.is_empty());
    if !segments.next()?.starts_with('@') {
        return None;
    }
    let kind = segments.next()?;
    let id = segments.next()?;
    if !is_item_id(id) {
        return None;
    }
    match kind {
        "video" => Some(TiktokUrl::Video(id.to_string())),
        "photo" => Some(TiktokUrl::Photo(id.to_string())),
        _ => None,
    }
}

/// Whether a string is a plausible numeric TikTok item id.
fn is_item_id(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Whether a string is a plausible share-link code.
fn is_share_code(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Pull the `__UNIVERSAL_DATA_FOR_REHYDRATION__` JSON out of a webpage.
fn universal_data(html: &str) -> Option<serde_json::Value> {
    let re = regex::Regex::new(
        r#"(?s)<script[^>]*id="__UNIVERSAL_DATA_FOR_REHYDRATION__"[^>]*>(.*?)</script>"#,
    )
    .expect("static regex");
    let json = re.captures(html)?.get(1)?.as_str();
    serde_json::from_str(json).ok()
}

/// Map a `webapp.video-detail` statusCode to an error. `0` is success.
fn status_error(code: i64, msg: &str) -> Error {
    let message = if msg.is_empty() {
        format!("TikTok answered statusCode {code}")
    } else {
        format!("TikTok answered statusCode {code}: {msg}")
    };
    match code {
        // Known "item is gone" family: deleted, region-blocked id, private.
        10202 | 10204 | 10216 | 10404 => Error::Unavailable {
            reason: UnavailableReason::Gone,
            message,
        },
        _ => Error::Extraction {
            stage: "tiktok",
            message,
        },
    }
}

/// Build formats from an `itemStruct.video` object: one progressive MP4 per
/// `bitrateInfo` rendition, falling back to the bare `playAddr`.
fn formats_from_video(video: &serde_json::Value) -> Vec<Format> {
    let mut out = Vec::new();
    for info in video
        .get("bitrateInfo")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(play) = info.get("PlayAddr") else {
            continue;
        };
        let Some(url) = play
            .get("UrlList")
            .and_then(serde_json::Value::as_array)
            .and_then(|l| l.first())
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        out.push(progressive_mp4(
            url,
            play.get("Width").and_then(as_u32),
            play.get("Height").and_then(as_u32),
            info.get("CodecType")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            info.get("Bitrate").and_then(serde_json::Value::as_u64),
            play.get("DataSize").and_then(as_u64_lenient),
        ));
    }
    if out.is_empty() {
        if let Some(url) = video.get("playAddr").and_then(serde_json::Value::as_str) {
            out.push(progressive_mp4(
                url,
                video.get("width").and_then(as_u32),
                video.get("height").and_then(as_u32),
                "",
                video.get("bitrate").and_then(serde_json::Value::as_u64),
                None,
            ));
        }
    }
    out
}

/// A muxed video+audio MP4 format (TikTok renditions always carry both).
fn progressive_mp4(
    url: &str,
    width: Option<u32>,
    height: Option<u32>,
    codec: &str,
    bitrate: Option<u64>,
    filesize: Option<u64>,
) -> Format {
    Format {
        itag: None,
        url: url.to_string(),
        mime_type: Some("video/mp4".to_string()),
        container: Some(Container::Mp4),
        video: Some(VideoStream {
            width,
            height,
            fps: None,
            codec: codec.to_string(),
        }),
        audio: Some(AudioStream::default()),
        filesize,
        bitrate,
        http_headers: Vec::new(),
    }
}

/// A JSON number as `u32`.
fn as_u32(v: &serde_json::Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok())
}

/// A JSON number or numeric string as `u64` (TikTok's `DataSize` is a string).
fn as_u64_lenient(v: &serde_json::Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str()?.parse().ok())
}

/// TikTok extractor. Holds an optional base-URL override for tests.
pub struct TiktokExtractor {
    /// Base URL for webpage fetches. Defaults to the real TikTok origin;
    /// tests point it at a mock server.
    base_url: String,
}

impl Default for TiktokExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl TiktokExtractor {
    /// Build an extractor targeting the real TikTok origin.
    pub fn new() -> Self {
        Self {
            base_url: "https://www.tiktok.com".to_string(),
        }
    }

    /// Build an extractor targeting an arbitrary base URL (tests).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }

    /// Fetch a webpage path (e.g. `/@user/video/123`), following redirects.
    async fn fetch_page(
        &self,
        ctx: &ExtractorContext,
        path: &str,
    ) -> Result<crate::transport::HttpResponse> {
        let req = HttpRequest::get("tiktok", format!("{}{path}", self.base_url))
            .header("User-Agent", USER_AGENT)
            .header("Accept", "text/html,application/xhtml+xml");
        let resp = ctx.http.execute(req).await?;
        if !resp.is_success() {
            return Err(Error::Extraction {
                stage: "tiktok",
                message: format!("webpage request failed with HTTP {}", resp.status),
            });
        }
        Ok(resp)
    }

    /// Resolve a share shortlink into the canonical `(path, TiktokUrl)` by
    /// following its redirect and re-classifying the final URL.
    async fn resolve_shortlink(
        &self,
        ctx: &ExtractorContext,
        code: &str,
    ) -> Result<(String, TiktokUrl)> {
        let resp = self.fetch_page(ctx, &format!("/t/{code}")).await?;
        let final_url = resp.final_url.as_deref().ok_or_else(|| Error::Extraction {
            stage: "tiktok",
            message: "transport does not report the shortlink's redirect target".into(),
        })?;
        let parsed = Url::parse(final_url).map_err(|_| Error::Extraction {
            stage: "tiktok",
            message: format!("shortlink resolved to an unparsable URL: {final_url}"),
        })?;
        match classify_post_path(parsed.path()) {
            Some(kind) => Ok((parsed.path().to_string(), kind)),
            None => Err(Error::Unavailable {
                reason: UnavailableReason::Gone,
                message: format!("shortlink did not resolve to a post: {final_url}"),
            }),
        }
    }

    /// Extract a video from its webpage path.
    async fn extract_video(
        &self,
        ctx: &ExtractorContext,
        path: &str,
        id: &str,
    ) -> Result<MediaInfo> {
        let resp = self.fetch_page(ctx, path).await?;
        let html = String::from_utf8_lossy(&resp.body);
        let mut info = build_video(&html, id)?;

        // The media CDN gates play URLs on the session cookies the page
        // response minted (tt_chain_token et al.), so every format must carry
        // them — plus a Referer and the same UA the page was fetched with.
        let cookie_header = session_cookie_header(&resp.headers);
        if let MediaInfo::Single(video) = &mut info {
            for f in &mut video.formats {
                if let Some(cookies) = &cookie_header {
                    f.http_headers.push(("Cookie".into(), cookies.clone()));
                }
                f.http_headers
                    .push(("Referer".into(), "https://www.tiktok.com/".into()));
                f.http_headers
                    .push(("User-Agent".into(), USER_AGENT.into()));
            }
        }
        Ok(info)
    }
}

/// Join the `name=value` parts of every `Set-Cookie` response header into a
/// single `Cookie` header value.
fn session_cookie_header(headers: &[(String, String)]) -> Option<String> {
    let pairs: Vec<&str> = headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
        .filter_map(|(_, v)| v.split(';').next())
        .map(str::trim)
        .filter(|s| s.contains('='))
        .collect();
    if pairs.is_empty() {
        None
    } else {
        Some(pairs.join("; "))
    }
}

/// Parse a video webpage into [`MediaInfo`].
fn build_video(html: &str, id: &str) -> Result<MediaInfo> {
    let data = universal_data(html).ok_or_else(|| Error::Unavailable {
        reason: UnavailableReason::BotCheck,
        message: "TikTok served a page without embedded video data (login/captcha wall); \
                  retry with cookies"
            .into(),
    })?;
    let detail = data
        .pointer("/__DEFAULT_SCOPE__/webapp.video-detail")
        .ok_or_else(|| Error::Unavailable {
            reason: UnavailableReason::BotCheck,
            message: "TikTok page carries no video-detail data (login/captcha wall); \
                      retry with cookies"
                .into(),
        })?;

    let status = detail
        .get("statusCode")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    if status != 0 {
        let msg = detail
            .get("statusMsg")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        return Err(status_error(status, msg));
    }

    let item = detail
        .pointer("/itemInfo/itemStruct")
        .ok_or_else(|| Error::Extraction {
            stage: "tiktok",
            message: "video-detail carries no itemStruct".into(),
        })?;
    if item.get("imagePost").is_some() {
        return Err(Error::Extraction {
            stage: "tiktok",
            message: "photo/slideshow posts are not supported".into(),
        });
    }

    let str_field = |obj: &serde_json::Value, key: &str| {
        obj.get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };

    let video = item.get("video").cloned().unwrap_or_default();
    let formats = formats_from_video(&video);
    if formats.is_empty() {
        return Err(Error::Extraction {
            stage: "tiktok",
            message: "no playable formats in video data".into(),
        });
    }

    let id = str_field(item, "id").unwrap_or_else(|| id.to_string());
    let author = item.get("author").cloned().unwrap_or_default();
    let unique_id = str_field(&author, "uniqueId");
    let desc = str_field(item, "desc");

    let thumbnails = ["cover", "originCover"]
        .iter()
        .filter_map(|k| str_field(&video, k))
        .map(|url| Thumbnail {
            url,
            width: None,
            height: None,
        })
        .collect();

    Ok(MediaInfo::Single(VideoInfo {
        title: desc.clone().unwrap_or_else(|| format!("TikTok video {id}")),
        description: desc,
        duration: video
            .get("duration")
            .and_then(serde_json::Value::as_f64)
            .map(std::time::Duration::from_secs_f64),
        uploader: str_field(&author, "nickname"),
        uploader_id: unique_id.clone(),
        channel_id: str_field(&author, "id"),
        view_count: item
            .pointer("/stats/playCount")
            .and_then(serde_json::Value::as_u64),
        upload_date: item
            .get("createTime")
            .and_then(as_u64_lenient)
            .and_then(|t| upload_date_from_epoch(t as f64)),
        thumbnails,
        webpage_url: format!(
            "https://www.tiktok.com/@{}/video/{id}",
            unique_id.unwrap_or_default()
        ),
        is_live: false,
        formats,
        id,
    }))
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Extractor for TiktokExtractor {
    fn name(&self) -> &'static str {
        "tiktok"
    }

    fn matches(&self, url: &Url) -> bool {
        classify(url).is_some()
    }

    async fn extract(&self, ctx: &ExtractorContext, url: &Url) -> Result<MediaInfo> {
        let kind = classify(url).ok_or_else(|| Error::UnsupportedUrl(url.to_string()))?;
        let (path, kind) = match kind {
            TiktokUrl::Shortlink(code) => self.resolve_shortlink(ctx, &code).await?,
            other => (url.path().to_string(), other),
        };
        match kind {
            TiktokUrl::Video(id) => self.extract_video(ctx, &path, &id).await,
            TiktokUrl::Photo(_) => Err(Error::Extraction {
                stage: "tiktok",
                message: "photo/slideshow posts are not supported".into(),
            }),
            TiktokUrl::Shortlink(_) => unreachable!("shortlink resolved above"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(u: &str) -> Url {
        Url::parse(u).unwrap()
    }

    #[test]
    fn classify_recognizes_tiktok_urls() {
        let cases = [
            (
                "https://www.tiktok.com/@someuser/video/7123456789012345678",
                TiktokUrl::Video("7123456789012345678".into()),
            ),
            (
                "https://www.tiktok.com/@some.user_1/video/7123456789012345678?is_from_webapp=1",
                TiktokUrl::Video("7123456789012345678".into()),
            ),
            (
                "https://m.tiktok.com/@someuser/video/7123456789012345678",
                TiktokUrl::Video("7123456789012345678".into()),
            ),
            (
                "https://www.tiktok.com/@someuser/photo/7123456789012345678",
                TiktokUrl::Photo("7123456789012345678".into()),
            ),
            (
                "https://www.tiktok.com/t/ZTabc123",
                TiktokUrl::Shortlink("ZTabc123".into()),
            ),
            (
                "https://vm.tiktok.com/ZTabc123/",
                TiktokUrl::Shortlink("ZTabc123".into()),
            ),
            (
                "https://vt.tiktok.com/ZTabc123",
                TiktokUrl::Shortlink("ZTabc123".into()),
            ),
        ];
        for (u, expect) in cases {
            assert_eq!(classify(&parse(u)).as_ref(), Some(&expect), "url: {u}");
        }
    }

    #[test]
    fn classify_rejects_non_video_urls() {
        for u in [
            "https://www.tiktok.com/@someuser",
            "https://www.tiktok.com/foryou",
            "https://www.tiktok.com/@someuser/video/notanumber",
            "https://www.tiktok.com/t/",
            "https://nottiktok.com/@someuser/video/7123456789012345678",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        ] {
            assert_eq!(classify(&parse(u)), None, "url: {u}");
        }
    }
}
