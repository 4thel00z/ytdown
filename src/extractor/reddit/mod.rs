//! Reddit extractor: resolves reddit.com post URLs into the post's hosted
//! video (v.redd.it), exposing the DASH representations as split video/audio
//! formats plus the progressive-less `fallback_url` as a last resort.

use url::Url;

use crate::error::UnavailableReason;
use crate::extractor::shared::{formats_from_mpd, upload_date_from_epoch};
use crate::extractor::{Extractor, ExtractorContext};
use crate::transport::HttpRequest;
use crate::types::{Container, Format, MediaInfo, Thumbnail, VideoInfo, VideoStream};
use crate::{Error, Result};

/// Browser-like User-Agent sent with Reddit requests: the public JSON endpoint
/// answers 403 to generic client UAs.
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

/// Extract the post id from a Reddit URL, or `None` if it is not a post URL.
///
/// Recognizes `reddit.com` (any subdomain) paths containing `/comments/<id>`
/// and `redd.it/<id>` shortlinks. Direct `v.redd.it` media links are not
/// supported: they only redirect to the post they belong to.
fn classify(url: &Url) -> Option<String> {
    let host = url.host_str()?;
    let host = host.strip_prefix("www.").unwrap_or(host);

    let mut segments = url.path_segments()?.filter(|s| !s.is_empty());

    // redd.it/<id> shortlink (but not v.redd.it / i.redd.it media hosts).
    if host == "redd.it" {
        let id = segments.next()?;
        return (segments.next().is_none() && is_post_id(id)).then(|| id.to_string());
    }

    if !(host == "reddit.com" || host.ends_with(".reddit.com")) {
        return None;
    }

    // Any path shape carrying `/comments/<id>`: `/r/<sub>/comments/<id>/…`,
    // `/user/<name>/comments/<id>/…`, or a bare `/comments/<id>`.
    let mut prev_was_comments = false;
    for seg in segments {
        if prev_was_comments {
            return is_post_id(seg).then(|| seg.to_string());
        }
        prev_was_comments = seg == "comments";
    }
    None
}

/// Whether a string is a plausible base36 post id.
fn is_post_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 13
        && s.bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
}

/// Reddit extractor. Holds an optional base-URL override for tests.
pub struct RedditExtractor {
    /// Base URL for the post-JSON endpoint. Defaults to the real Reddit origin;
    /// tests point it at a mock server.
    base_url: String,
}

impl Default for RedditExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl RedditExtractor {
    /// Build an extractor targeting the real Reddit origin.
    pub fn new() -> Self {
        Self {
            base_url: "https://www.reddit.com".to_string(),
        }
    }

    /// Build an extractor targeting an arbitrary base URL (tests).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }

    /// Fetch and validate the post JSON for `id`.
    async fn fetch_post(&self, ctx: &ExtractorContext, id: &str) -> Result<serde_json::Value> {
        let url = format!("{}/comments/{id}.json?raw_json=1", self.base_url);
        let req = HttpRequest::get("reddit", url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json");
        let resp = ctx.http.execute(req).await?;
        match resp.status {
            403 => {
                return Err(Error::Unavailable {
                    reason: UnavailableReason::BotCheck,
                    message: "Reddit blocked the request (HTTP 403); retry with cookies".into(),
                })
            }
            404 => {
                return Err(Error::Unavailable {
                    reason: UnavailableReason::Gone,
                    message: format!("post {id} not found"),
                })
            }
            s if !(200..300).contains(&s) => {
                return Err(Error::Extraction {
                    stage: "reddit",
                    message: format!("post JSON request failed with HTTP {s}"),
                })
            }
            _ => {}
        }
        serde_json::from_slice(&resp.body).map_err(|e| Error::Extraction {
            stage: "reddit",
            message: format!("invalid post JSON: {e}"),
        })
    }

    /// Fetch the DASH manifest and turn its representations into formats.
    async fn fetch_mpd_formats(
        &self,
        ctx: &ExtractorContext,
        dash_url: &str,
    ) -> Result<Vec<Format>> {
        let req = HttpRequest::get("reddit-dash", dash_url).header("User-Agent", USER_AGENT);
        let resp = ctx.http.execute(req).await?;
        if !resp.is_success() {
            return Err(Error::Extraction {
                stage: "reddit-dash",
                message: format!("manifest request failed with HTTP {}", resp.status),
            });
        }
        let xml = String::from_utf8_lossy(&resp.body);
        Ok(formats_from_mpd(&xml, &mpd_base(dash_url)))
    }
}

/// The directory a manifest's relative `BaseURL`s resolve against: the
/// `dash_url` with query and filename stripped, keeping the trailing slash.
fn mpd_base(dash_url: &str) -> String {
    let no_query = dash_url.split(['?', '#']).next().unwrap_or(dash_url);
    match no_query.rfind('/') {
        Some(i) => no_query[..=i].to_string(),
        None => String::new(),
    }
}

/// Locate the `reddit_video` object: on the post itself, on a crosspost
/// parent, or as a link post's video preview.
fn find_reddit_video(post: &serde_json::Value) -> Option<&serde_json::Value> {
    for media_key in ["secure_media", "media"] {
        if let Some(rv) = post
            .get(media_key)
            .and_then(|m| m.get("reddit_video"))
            .filter(|v| v.is_object())
        {
            return Some(rv);
        }
    }
    if let Some(parents) = post
        .get("crosspost_parent_list")
        .and_then(serde_json::Value::as_array)
    {
        if let Some(rv) = parents.iter().find_map(find_reddit_video) {
            return Some(rv);
        }
    }
    post.get("preview")
        .and_then(|p| p.get("reddit_video_preview"))
        .filter(|v| v.is_object())
}

/// Build the video-only format described by `fallback_url`, used when the
/// DASH manifest cannot be fetched.
fn fallback_format(rv: &serde_json::Value, url: &str) -> Format {
    Format {
        itag: None,
        url: url.to_string(),
        mime_type: Some("video/mp4".to_string()),
        container: Some(Container::Mp4),
        video: Some(VideoStream {
            width: rv.get("width").and_then(as_u32),
            height: rv.get("height").and_then(as_u32),
            fps: None,
            codec: String::new(),
        }),
        audio: None,
        filesize: None,
        bitrate: rv
            .get("bitrate_kbps")
            .and_then(serde_json::Value::as_u64)
            .map(|k| k * 1000),
        http_headers: Vec::new(),
    }
}

/// Collect thumbnails: the post thumbnail, then the preview image's
/// resolutions, then the full-size preview source (largest last).
fn thumbnails_from_post(post: &serde_json::Value) -> Vec<Thumbnail> {
    let mut out = Vec::new();
    if let Some(t) = post
        .get("thumbnail")
        .and_then(serde_json::Value::as_str)
        .filter(|t| t.starts_with("http"))
    {
        out.push(Thumbnail {
            url: t.to_string(),
            width: None,
            height: None,
        });
    }
    if let Some(image) = post.pointer("/preview/images/0") {
        for res in image
            .get("resolutions")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .chain(image.get("source"))
        {
            if let Some(thumb) = thumbnail_from(res) {
                out.push(thumb);
            }
        }
    }
    out
}

/// Map a `{url, width, height}` preview object to a [`Thumbnail`].
fn thumbnail_from(v: &serde_json::Value) -> Option<Thumbnail> {
    Some(Thumbnail {
        url: v.get("url")?.as_str()?.to_string(),
        width: v.get("width").and_then(as_u32),
        height: v.get("height").and_then(as_u32),
    })
}

/// A JSON number as `u32`.
fn as_u32(v: &serde_json::Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok())
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Extractor for RedditExtractor {
    fn name(&self) -> &'static str {
        "reddit"
    }

    fn matches(&self, url: &Url) -> bool {
        classify(url).is_some()
    }

    async fn extract(&self, ctx: &ExtractorContext, url: &Url) -> Result<MediaInfo> {
        let id = classify(url).ok_or_else(|| Error::UnsupportedUrl(url.to_string()))?;
        let listing = self.fetch_post(ctx, &id).await?;
        let post = listing
            .get(0)
            .and_then(|l| l.pointer("/data/children/0/data"))
            .filter(|p| p.is_object())
            .ok_or_else(|| Error::Extraction {
                stage: "reddit",
                message: "post data missing from listing response".into(),
            })?;

        let rv = find_reddit_video(post).ok_or_else(|| {
            let message = match post
                .get("url_overridden_by_dest")
                .and_then(serde_json::Value::as_str)
            {
                Some(external) => {
                    format!("post has no Reddit-hosted video; it links to {external}")
                }
                None => "post has no Reddit-hosted video".to_string(),
            };
            Error::Extraction {
                stage: "reddit",
                message,
            }
        })?;

        let mut formats = Vec::new();
        if let Some(dash_url) = rv.get("dash_url").and_then(serde_json::Value::as_str) {
            match self.fetch_mpd_formats(ctx, dash_url).await {
                Ok(f) => formats = f,
                Err(e) => {
                    tracing::warn!(error = %e, "reddit DASH manifest failed; using fallback_url")
                }
            }
        }
        if formats.is_empty() {
            if let Some(fb) = rv.get("fallback_url").and_then(serde_json::Value::as_str) {
                formats.push(fallback_format(rv, fb));
            }
        }
        if formats.is_empty() {
            return Err(Error::Extraction {
                stage: "reddit",
                message: "reddit video has no downloadable formats".into(),
            });
        }

        let str_field = |key: &str| {
            post.get(key)
                .and_then(serde_json::Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        };

        Ok(MediaInfo::Single(VideoInfo {
            id: str_field("id").unwrap_or(id),
            title: str_field("title").unwrap_or_default(),
            description: str_field("selftext"),
            duration: rv
                .get("duration")
                .and_then(serde_json::Value::as_f64)
                .map(std::time::Duration::from_secs_f64),
            uploader: str_field("author"),
            uploader_id: None,
            channel_id: str_field("subreddit"),
            view_count: None,
            upload_date: post
                .get("created_utc")
                .and_then(serde_json::Value::as_f64)
                .and_then(upload_date_from_epoch),
            thumbnails: thumbnails_from_post(post),
            webpage_url: match str_field("permalink") {
                Some(permalink) => format!("https://www.reddit.com{permalink}"),
                None => url.to_string(),
            },
            is_live: false,
            formats,
        }))
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
                "https://www.reddit.com/r/videos/comments/1abc23/some_title/",
                "1abc23",
            ),
            ("https://old.reddit.com/r/videos/comments/1abc23", "1abc23"),
            ("https://reddit.com/comments/1abc23", "1abc23"),
            (
                "https://www.reddit.com/user/someone/comments/1abc23/title/",
                "1abc23",
            ),
            ("https://redd.it/1abc23", "1abc23"),
        ];
        for (u, id) in cases {
            assert_eq!(classify(&parse(u)).as_deref(), Some(id), "url: {u}");
        }
    }

    #[test]
    fn classify_rejects_non_post_urls() {
        for u in [
            "https://www.reddit.com/r/videos/",
            "https://www.reddit.com/user/someone/",
            "https://v.redd.it/media123",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://www.reddit.com/r/videos/comments/",
            "https://notreddit.com/r/videos/comments/1abc23",
        ] {
            assert_eq!(classify(&parse(u)), None, "url: {u}");
        }
    }
}
