//! Instagram extractor: resolves post/reel URLs via the public GraphQL
//! shortcode query into the post's video — a progressive MP4 plus the split
//! DASH representations Instagram inlines in the response.

use url::Url;

use crate::error::UnavailableReason;
use crate::extractor::shared::{formats_from_mpd, upload_date_from_epoch};
use crate::extractor::{Extractor, ExtractorContext};
use crate::transport::HttpRequest;
use crate::types::{Container, Format, MediaInfo, Thumbnail, VideoInfo, VideoStream};
use crate::{Error, Result};

/// Browser-like User-Agent: Instagram's GraphQL endpoint rejects client UAs.
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

/// The web app id every logged-out GraphQL call carries.
const IG_APP_ID: &str = "936619743392459";

/// The persisted GraphQL document id of the shortcode → media query
/// (`PolarisPostActionLoadPostQueryQuery`, mirrored from yt-dlp).
const DOC_ID: &str = "8845758582119845";

/// What an Instagram URL points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InstagramUrl {
    /// A post/reel/tv item by shortcode.
    Post(String),
    /// An opaque share link that redirects to the canonical post.
    Share(String),
}

/// Classify an Instagram URL, or `None` if it is not one.
fn classify(url: &Url) -> Option<InstagramUrl> {
    let host = url.host_str()?;
    let host = host.strip_prefix("www.").unwrap_or(host);
    if !(host == "instagram.com" || host.ends_with(".instagram.com")) {
        return None;
    }

    let segments: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();

    // Opaque app share links redirect to the canonical post.
    if segments.first() == Some(&"share") && segments.len() >= 2 {
        let path: String = segments
            .iter()
            .map(|s| format!("/{s}"))
            .collect::<Vec<_>>()
            .join("");
        return Some(InstagramUrl::Share(path));
    }

    // `/p|reel|reels|tv/<code>` — possibly prefixed by a username segment.
    // Stories are excluded: they need a logged-in session and a different API.
    let mut prev: Option<&str> = None;
    for seg in &segments {
        if let Some(kind) = prev {
            if matches!(kind, "p" | "reel" | "reels" | "tv") && is_shortcode(seg) {
                return Some(InstagramUrl::Post((*seg).to_string()));
            }
        }
        prev = Some(seg);
    }
    None
}

/// Whether a string is a plausible media shortcode.
fn is_shortcode(s: &str) -> bool {
    s.len() >= 5
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Instagram extractor. Holds an optional base-URL override for tests.
pub struct InstagramExtractor {
    /// Base URL for API and share-link fetches. Defaults to the real
    /// Instagram origin; tests point it at a mock server.
    base_url: String,
}

impl Default for InstagramExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl InstagramExtractor {
    /// Build an extractor targeting the real Instagram origin.
    pub fn new() -> Self {
        Self {
            base_url: "https://www.instagram.com".to_string(),
        }
    }

    /// Build an extractor targeting an arbitrary base URL (tests).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }

    /// Resolve an app share link into the canonical shortcode by following
    /// its redirect and re-classifying the final URL's path.
    async fn resolve_share(&self, ctx: &ExtractorContext, path: &str) -> Result<String> {
        let req = HttpRequest::get("instagram", format!("{}{path}", self.base_url))
            .header("User-Agent", USER_AGENT);
        let resp = ctx.http.execute(req).await?;
        let final_url = resp.final_url.as_deref().ok_or_else(|| Error::Extraction {
            stage: "instagram",
            message: "transport does not report the share link's redirect target".into(),
        })?;
        let parsed = Url::parse(final_url).map_err(|_| Error::Extraction {
            stage: "instagram",
            message: format!("share link resolved to an unparsable URL: {final_url}"),
        })?;
        match classify(&parsed) {
            Some(InstagramUrl::Post(code)) => Ok(code),
            _ => {
                // The mock/test hosts aren't instagram.com; fall back to a
                // host-agnostic scan of the final path.
                shortcode_from_path(parsed.path()).ok_or_else(|| Error::Unavailable {
                    reason: UnavailableReason::Gone,
                    message: format!("share link did not resolve to a post: {final_url}"),
                })
            }
        }
    }

    /// Query the persisted GraphQL document for a shortcode's media.
    async fn fetch_media(&self, ctx: &ExtractorContext, code: &str) -> Result<serde_json::Value> {
        let variables = serde_json::json!({
            "shortcode": code,
            "fetch_tagged_user_count": null,
            "hoisted_comment_id": null,
            "hoisted_reply_id": null,
        });
        let body = format!(
            "variables={}&server_timestamps=true&doc_id={DOC_ID}",
            urlencode(&variables.to_string())
        );
        let req = HttpRequest::post("instagram", format!("{}/graphql/query", self.base_url))
            .header("User-Agent", USER_AGENT)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("X-IG-App-ID", IG_APP_ID)
            .header("X-FB-LSD", "AVqbxe3J_YA")
            .header("X-ASBD-ID", "129477")
            .header("Accept", "*/*")
            .body(body.into_bytes());
        let resp = ctx.http.execute(req).await?;
        match resp.status {
            404 => {
                return Err(Error::Unavailable {
                    reason: UnavailableReason::Gone,
                    message: format!("post {code} not found"),
                })
            }
            s if s == 400 || s == 401 || s == 403 => {
                return Err(Error::Unavailable {
                    reason: UnavailableReason::BotCheck,
                    message: format!(
                        "Instagram rejected the request (HTTP {s}); retry with cookies"
                    ),
                })
            }
            s if !(200..300).contains(&s) => {
                return Err(Error::Extraction {
                    stage: "instagram",
                    message: format!("GraphQL request failed with HTTP {s}"),
                })
            }
            _ => {}
        }
        serde_json::from_slice(&resp.body).map_err(|e| Error::Extraction {
            stage: "instagram",
            message: format!("invalid GraphQL response: {e}"),
        })
    }
}

/// Host-agnostic `/p|reel|reels|tv/<code>` scan of a resolved share path.
fn shortcode_from_path(path: &str) -> Option<String> {
    let mut prev: Option<&str> = None;
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        if let Some(kind) = prev {
            if matches!(kind, "p" | "reel" | "reels" | "tv") && is_shortcode(seg) {
                return Some(seg.to_string());
            }
        }
        prev = Some(seg);
    }
    None
}

/// Percent-encode a form value (RFC 3986 unreserved characters pass through).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Build formats for one video node: the progressive `video_url` plus the
/// split DASH representations from the inlined manifest.
fn formats_from_node(node: &serde_json::Value) -> Vec<Format> {
    let mut out = Vec::new();
    if let Some(url) = node.get("video_url").and_then(serde_json::Value::as_str) {
        out.push(Format {
            itag: None,
            url: url.to_string(),
            mime_type: Some("video/mp4".to_string()),
            container: Some(Container::Mp4),
            video: Some(VideoStream {
                width: node.pointer("/dimensions/width").and_then(as_u32),
                height: node.pointer("/dimensions/height").and_then(as_u32),
                fps: None,
                codec: String::new(),
            }),
            audio: Some(crate::types::AudioStream::default()),
            filesize: None,
            bitrate: None,
            http_headers: Vec::new(),
        });
    }
    if let Some(manifest) = node
        .pointer("/dash_info/video_dash_manifest")
        .and_then(serde_json::Value::as_str)
    {
        out.extend(formats_from_mpd(manifest, ""));
    }
    out
}

/// A JSON number as `u32`.
fn as_u32(v: &serde_json::Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok())
}

/// Build [`MediaInfo`] from an `xdt_shortcode_media` node.
fn build_video(media: &serde_json::Value, code: &str) -> Result<MediaInfo> {
    // For carousels, the media (and its formats) come from the first video
    // child; the title/owner metadata stays on the parent post.
    let video_node = if media.get("is_video").and_then(serde_json::Value::as_bool) == Some(true) {
        Some(media)
    } else {
        media
            .pointer("/edge_sidecar_to_children/edges")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|e| e.get("node"))
            .find(|n| n.get("is_video").and_then(serde_json::Value::as_bool) == Some(true))
    };
    let Some(video_node) = video_node else {
        return Err(Error::Extraction {
            stage: "instagram",
            message: "post has no video (image-only post)".into(),
        });
    };

    let sidecar_total = media
        .pointer("/edge_sidecar_to_children/edges")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if sidecar_total > 1 {
        tracing::warn!(
            children = sidecar_total,
            "carousel post: extracting only the first video item"
        );
    }

    let formats = formats_from_node(video_node);
    if formats.is_empty() {
        return Err(Error::Extraction {
            stage: "instagram",
            message: "no playable formats in media data".into(),
        });
    }

    let caption = media
        .pointer("/edge_media_to_caption/edges/0/node/text")
        .and_then(serde_json::Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let thumbnails: Vec<Thumbnail> = media
        .get("display_resources")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| {
            Some(Thumbnail {
                url: r.get("src")?.as_str()?.to_string(),
                width: r.get("config_width").and_then(as_u32),
                height: r.get("config_height").and_then(as_u32),
            })
        })
        .collect();

    Ok(MediaInfo::Single(VideoInfo {
        id: code.to_string(),
        title: caption
            .clone()
            .unwrap_or_else(|| format!("Instagram post {code}")),
        description: caption,
        duration: video_node
            .get("video_duration")
            .and_then(serde_json::Value::as_f64)
            .map(std::time::Duration::from_secs_f64),
        uploader: media
            .pointer("/owner/username")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        uploader_id: None,
        channel_id: media
            .pointer("/owner/id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        view_count: media
            .get("video_view_count")
            .and_then(serde_json::Value::as_u64),
        upload_date: media
            .get("taken_at_timestamp")
            .and_then(serde_json::Value::as_f64)
            .and_then(upload_date_from_epoch),
        thumbnails,
        webpage_url: format!("https://www.instagram.com/p/{code}/"),
        is_live: false,
        formats,
    }))
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Extractor for InstagramExtractor {
    fn name(&self) -> &'static str {
        "instagram"
    }

    fn matches(&self, url: &Url) -> bool {
        classify(url).is_some()
    }

    async fn extract(&self, ctx: &ExtractorContext, url: &Url) -> Result<MediaInfo> {
        let kind = classify(url).ok_or_else(|| Error::UnsupportedUrl(url.to_string()))?;
        let code = match kind {
            InstagramUrl::Post(code) => code,
            InstagramUrl::Share(path) => self.resolve_share(ctx, &path).await?,
        };
        let response = self.fetch_media(ctx, &code).await?;
        let media = response
            .pointer("/data/xdt_shortcode_media")
            .filter(|m| m.is_object())
            .ok_or_else(|| Error::Unavailable {
                reason: UnavailableReason::BotCheck,
                message: "Instagram returned no media data (login wall); retry with cookies".into(),
            })?;
        build_video(media, &code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(u: &str) -> Url {
        Url::parse(u).unwrap()
    }

    #[test]
    fn classify_recognizes_post_urls() {
        let cases = [
            (
                "https://www.instagram.com/p/Cxample123/",
                InstagramUrl::Post("Cxample123".into()),
            ),
            (
                "https://www.instagram.com/reel/Cxample123/?igsh=abc",
                InstagramUrl::Post("Cxample123".into()),
            ),
            (
                "https://instagram.com/reels/Cxample123",
                InstagramUrl::Post("Cxample123".into()),
            ),
            (
                "https://www.instagram.com/tv/Cxample123/",
                InstagramUrl::Post("Cxample123".into()),
            ),
            (
                "https://www.instagram.com/someuser/p/Cxample123/",
                InstagramUrl::Post("Cxample123".into()),
            ),
            (
                "https://www.instagram.com/share/reel/AbCd12345",
                InstagramUrl::Share("/share/reel/AbCd12345".into()),
            ),
            (
                "https://www.instagram.com/share/AbCd12345",
                InstagramUrl::Share("/share/AbCd12345".into()),
            ),
        ];
        for (u, expect) in cases {
            assert_eq!(classify(&parse(u)).as_ref(), Some(&expect), "url: {u}");
        }
    }

    #[test]
    fn classify_rejects_non_post_urls() {
        for u in [
            "https://www.instagram.com/someuser/",
            "https://www.instagram.com/explore/",
            "https://www.instagram.com/p/",
            "https://www.instagram.com/stories/someuser/123/",
            "https://notinstagram.com/p/Cxample123/",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        ] {
            assert_eq!(classify(&parse(u)), None, "url: {u}");
        }
    }
}
