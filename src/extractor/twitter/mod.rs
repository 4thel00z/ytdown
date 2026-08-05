//! X/Twitter extractor: resolves tweet URLs via the public syndication API
//! (the CDN endpoint that powers embedded tweets — no login or guest token)
//! into the tweet video's progressive MP4 variants.

use url::Url;

use crate::error::UnavailableReason;
use crate::extractor::{Extractor, ExtractorContext};
use crate::transport::HttpRequest;
use crate::types::{AudioStream, Container, Format, MediaInfo, Thumbnail, VideoInfo, VideoStream};
use crate::{Error, Result};

/// Browser-like User-Agent for syndication requests.
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

/// Extract the numeric tweet id from an X/Twitter URL, or `None`.
///
/// Recognizes `/<user>/status/<id>`, `/i/status/<id>`, and `/i/web/status/<id>`
/// on `x.com` / `twitter.com` (and their mobile subdomains).
fn classify(url: &Url) -> Option<String> {
    let host = url.host_str()?;
    let host = host.strip_prefix("www.").unwrap_or(host);
    if !matches!(
        host,
        "x.com" | "twitter.com" | "mobile.twitter.com" | "m.twitter.com" | "mobile.x.com"
    ) {
        return None;
    }
    let segments: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    for (i, seg) in segments.iter().enumerate() {
        if *seg == "status" {
            if let Some(id) = segments.get(i + 1) {
                if is_tweet_id(id) {
                    return Some((*id).to_string());
                }
            }
        }
    }
    None
}

/// Whether a string is a plausible numeric tweet id.
fn is_tweet_id(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// The access token the syndication API expects: the tweet id scaled by
/// `π / 1e15`, rendered in base 36 with all zeros and the dot stripped
/// (a straight port of the embed widget's JavaScript).
fn syndication_token(id: &str) -> Option<String> {
    let n: f64 = id.parse().ok()?;
    if !n.is_finite() || n <= 0.0 {
        return None;
    }
    let rendered = to_radix36((n / 1e15) * std::f64::consts::PI);
    Some(
        rendered
            .chars()
            .filter(|c| *c != '0' && *c != '.')
            .collect(),
    )
}

/// Render a positive double in base 36 exactly like JS `Number.toString(36)`
/// (V8's `DoubleToRadixCString`): fraction digits are emitted until the
/// remaining value drops below half an ULP, with carry-propagating rounding.
fn to_radix36(value: f64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let next_up = |v: f64| f64::from_bits(v.to_bits() + 1);

    let mut integer = value.trunc();
    let mut fraction = value.fract();
    let mut delta = (0.5 * (next_up(value) - value)).max(next_up(0.0));

    let mut frac_digits: Vec<u8> = Vec::new();
    if fraction >= delta {
        loop {
            fraction *= 36.0;
            delta *= 36.0;
            let digit = fraction as usize;
            frac_digits.push(DIGITS[digit]);
            fraction -= digit as f64;
            if (fraction > 0.5 || (fraction == 0.5 && digit % 2 == 1)) && fraction + delta > 1.0 {
                // Round the last digit up, propagating any carry.
                loop {
                    match frac_digits.pop() {
                        None => {
                            integer += 1.0;
                            break;
                        }
                        Some(c) => {
                            let d = if c.is_ascii_digit() {
                                (c - b'0') as usize
                            } else {
                                (c - b'a' + 10) as usize
                            };
                            if d + 1 < 36 {
                                frac_digits.push(DIGITS[d + 1]);
                                break;
                            }
                        }
                    }
                }
                break;
            }
            if fraction < delta {
                break;
            }
        }
    }

    let mut int_digits: Vec<u8> = Vec::new();
    if integer < 1.0 {
        int_digits.push(b'0');
    }
    while integer >= 1.0 {
        int_digits.push(DIGITS[(integer % 36.0) as usize]);
        integer = (integer / 36.0).trunc();
    }
    int_digits.reverse();

    let mut out = String::from_utf8(int_digits).expect("base36 digits are ASCII");
    if !frac_digits.is_empty() {
        out.push('.');
        out.push_str(std::str::from_utf8(&frac_digits).expect("base36 digits are ASCII"));
    }
    out
}

/// X/Twitter extractor. Holds an optional base-URL override for tests.
pub struct TwitterExtractor {
    /// Base URL of the syndication API. Defaults to the real CDN origin;
    /// tests point it at a mock server.
    base_url: String,
}

impl Default for TwitterExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl TwitterExtractor {
    /// Build an extractor targeting the real syndication origin.
    pub fn new() -> Self {
        Self {
            base_url: "https://cdn.syndication.twimg.com".to_string(),
        }
    }

    /// Build an extractor targeting an arbitrary base URL (tests).
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
impl Extractor for TwitterExtractor {
    fn name(&self) -> &'static str {
        "twitter"
    }

    fn matches(&self, url: &Url) -> bool {
        classify(url).is_some()
    }

    async fn extract(&self, ctx: &ExtractorContext, url: &Url) -> Result<MediaInfo> {
        let id = classify(url).ok_or_else(|| Error::UnsupportedUrl(url.to_string()))?;
        let token = syndication_token(&id).ok_or_else(|| Error::Extraction {
            stage: "twitter",
            message: format!("cannot compute access token for tweet id {id}"),
        })?;
        let req = HttpRequest::get(
            "twitter",
            format!(
                "{}/tweet-result?id={id}&token={token}&lang=en",
                self.base_url
            ),
        )
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/json");
        let resp = ctx.http.execute(req).await?;
        match resp.status {
            404 => {
                return Err(Error::Unavailable {
                    reason: UnavailableReason::Gone,
                    message: format!("tweet {id} not found (deleted, protected, or login-gated)"),
                })
            }
            s if !(200..300).contains(&s) => {
                return Err(Error::Extraction {
                    stage: "twitter",
                    message: format!("syndication request failed with HTTP {s}"),
                })
            }
            _ => {}
        }
        let tweet: serde_json::Value =
            serde_json::from_slice(&resp.body).map_err(|e| Error::Extraction {
                stage: "twitter",
                message: format!("invalid syndication response: {e}"),
            })?;
        build_video(&tweet, &id)
    }
}

/// Build [`MediaInfo`] from a syndication `tweet-result` payload.
fn build_video(tweet: &serde_json::Value, id: &str) -> Result<MediaInfo> {
    if tweet.get("__typename").and_then(serde_json::Value::as_str) == Some("TweetTombstone") {
        let text = tweet
            .pointer("/tombstone/text/text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("tweet is unavailable");
        let reason = if text.to_ascii_lowercase().contains("age") {
            UnavailableReason::AgeRestricted
        } else {
            UnavailableReason::Gone
        };
        return Err(Error::Unavailable {
            reason,
            message: text.to_string(),
        });
    }

    let str_at = |ptr: &str| {
        tweet
            .pointer(ptr)
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };

    let media: Vec<&serde_json::Value> = tweet
        .get("mediaDetails")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|m| {
            matches!(
                m.get("type").and_then(serde_json::Value::as_str),
                Some("video") | Some("animated_gif")
            )
        })
        .collect();
    let Some(detail) = media.first() else {
        return Err(Error::Extraction {
            stage: "twitter",
            message: "tweet has no video".into(),
        });
    };
    if media.len() > 1 {
        tracing::warn!(
            videos = media.len(),
            "tweet carries several videos: extracting only the first"
        );
    }

    let is_gif = detail.get("type").and_then(serde_json::Value::as_str) == Some("animated_gif");
    let formats: Vec<Format> = detail
        .pointer("/video_info/variants")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|v| v.get("content_type").and_then(serde_json::Value::as_str) == Some("video/mp4"))
        .filter_map(|v| {
            let url = v.get("url")?.as_str()?;
            let (width, height) = dimensions_from_url(url);
            Some(Format {
                itag: None,
                url: url.to_string(),
                mime_type: Some("video/mp4".to_string()),
                container: Some(Container::Mp4),
                video: Some(VideoStream {
                    width,
                    height,
                    fps: None,
                    codec: String::new(),
                }),
                // Animated GIFs are re-encoded as soundless MP4s.
                audio: (!is_gif).then(AudioStream::default),
                filesize: None,
                bitrate: v.get("bitrate").and_then(serde_json::Value::as_u64),
                http_headers: Vec::new(),
            })
        })
        .collect();
    if formats.is_empty() {
        return Err(Error::Extraction {
            stage: "twitter",
            message: "tweet video has no MP4 variants".into(),
        });
    }

    let text = str_at("/text");
    let screen_name = str_at("/user/screen_name");
    let thumbnails = detail
        .get("media_url_https")
        .and_then(serde_json::Value::as_str)
        .map(|u| {
            vec![Thumbnail {
                url: u.to_string(),
                width: None,
                height: None,
            }]
        })
        .unwrap_or_default();

    Ok(MediaInfo::Single(VideoInfo {
        id: str_at("/id_str").unwrap_or_else(|| id.to_string()),
        title: text.clone().unwrap_or_else(|| format!("Tweet {id}")),
        description: text,
        duration: detail
            .pointer("/video_info/duration_millis")
            .and_then(serde_json::Value::as_u64)
            .map(std::time::Duration::from_millis),
        uploader: str_at("/user/name"),
        uploader_id: screen_name.clone(),
        channel_id: str_at("/user/id_str"),
        view_count: tweet
            .pointer("/video/viewCount")
            .and_then(serde_json::Value::as_u64),
        upload_date: str_at("/created_at").and_then(|iso| {
            let date: String = iso.chars().take(10).filter(char::is_ascii_digit).collect();
            (date.len() == 8).then_some(date)
        }),
        thumbnails,
        webpage_url: format!(
            "https://x.com/{}/status/{id}",
            screen_name.unwrap_or_else(|| "i".to_string())
        ),
        is_live: false,
        formats,
    }))
}

/// Recover `WxH` pixel dimensions from a variant URL path like
/// `…/vid/avc1/720x1280/name.mp4`.
fn dimensions_from_url(url: &str) -> (Option<u32>, Option<u32>) {
    let re = regex::Regex::new(r"/(\d{2,5})x(\d{2,5})/").expect("static regex");
    match re.captures(url) {
        Some(c) => (
            c.get(1).and_then(|m| m.as_str().parse().ok()),
            c.get(2).and_then(|m| m.as_str().parse().ok()),
        ),
        None => (None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(u: &str) -> Url {
        Url::parse(u).unwrap()
    }

    #[test]
    fn classify_recognizes_tweet_urls() {
        let cases = [
            (
                "https://x.com/jack/status/1668680561921038336",
                "1668680561921038336",
            ),
            (
                "https://twitter.com/jack/status/1668680561921038336?s=20",
                "1668680561921038336",
            ),
            (
                "https://mobile.twitter.com/jack/status/1668680561921038336",
                "1668680561921038336",
            ),
            (
                "https://x.com/i/status/1668680561921038336",
                "1668680561921038336",
            ),
            (
                "https://x.com/i/web/status/1668680561921038336",
                "1668680561921038336",
            ),
            (
                "https://x.com/jack/status/1668680561921038336/video/1",
                "1668680561921038336",
            ),
        ];
        for (u, id) in cases {
            assert_eq!(classify(&parse(u)).as_deref(), Some(id), "url: {u}");
        }
    }

    #[test]
    fn classify_rejects_non_tweet_urls() {
        for u in [
            "https://x.com/jack",
            "https://x.com/jack/status/notanumber",
            "https://x.com/home",
            "https://notx.com/jack/status/1668680561921038336",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        ] {
            assert_eq!(classify(&parse(u)), None, "url: {u}");
        }
    }

    #[test]
    fn syndication_token_matches_embed_widget_output() {
        // Reference values produced by the widget's JS:
        // ((Number(id) / 1e15) * Math.PI).toString(36).replace(/(0+|\.)/g, '')
        let cases = [
            ("1288158940940222464", "34evcdrq711"),
            ("1668680561921038336", "41mbbppzkg"),
            ("1000000000000000", "353i5ab8p5f"),
        ];
        for (id, expect) in cases {
            assert_eq!(syndication_token(id).as_deref(), Some(expect), "id: {id}");
        }
    }
}
