//! InnerTube API client: talks to YouTube's private `youtubei/v1` endpoints
//! while impersonating one of several official client identities.

use futures::StreamExt;
use serde::Deserialize;

use crate::error::{Error, Result, UnavailableReason};

/// Maximum size of a control-plane response body we will buffer (32 MiB).
///
/// InnerTube JSON responses are at most a few MB; this cap defends against a
/// misbehaving, compromised, or hostile endpoint streaming an unbounded body to
/// exhaust memory. `reqwest` imposes no default response-size limit.
pub(crate) const MAX_RESPONSE_BYTES: u64 = 32 * 1024 * 1024;

/// Buffer a response body up to [`MAX_RESPONSE_BYTES`], erroring if exceeded.
///
/// Reads incrementally via `bytes_stream` so an oversized (or slow-drip) body is
/// rejected once the cap is crossed rather than accumulated wholesale.
pub(crate) async fn read_bounded(resp: reqwest::Response, stage: &'static str) -> Result<Vec<u8>> {
    if let Some(len) = resp.content_length() {
        if len > MAX_RESPONSE_BYTES {
            return Err(Error::Extraction {
                stage,
                message: format!("response body too large: {len} bytes"),
            });
        }
    }
    let mut buf = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|source| Error::Network {
            stage,
            message: source.to_string(),
        })?;
        if buf.len() as u64 + chunk.len() as u64 > MAX_RESPONSE_BYTES {
            return Err(Error::Extraction {
                stage,
                message: "response body exceeded size limit".into(),
            });
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

/// Buffer a response body (bounded) and deserialize it as JSON.
///
/// If the body is a Google API error envelope (`{ "error": { "message": ... } }`)
/// rather than the expected payload, surface that message instead of an opaque
/// "missing field" deserialization error. InnerTube answers a stale or
/// malformed request with such an envelope (e.g. HTTP 400 `FAILED_PRECONDITION`).
async fn read_bounded_json<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
    stage: &'static str,
) -> Result<T> {
    let bytes = read_bounded(resp, stage).await?;
    serde_json::from_slice(&bytes).map_err(|e| {
        if let Some(message) = error_envelope_message(&bytes) {
            Error::Extraction {
                stage,
                message: format!("InnerTube error: {message}"),
            }
        } else {
            Error::Extraction {
                stage,
                message: format!("invalid JSON response: {e}"),
            }
        }
    })
}

/// Extract `error.message` from a Google API error envelope, if present.
fn error_envelope_message(bytes: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    value
        .get("error")?
        .get("message")?
        .as_str()
        .map(str::to_string)
}

/// Which InnerTube client identity to impersonate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ClientKind {
    /// Desktop web client.
    Web,
    /// Android mobile app client.
    Android,
    /// Android XR (Oculus Quest) client. Unlike the plain Android client, its
    /// `videoplayback` URLs are not Proof-of-Origin-gated, so full-length ranged
    /// downloads succeed without a PO token. Requires a `visitorData` token in
    /// the request context, or it answers `LOGIN_REQUIRED`.
    AndroidVr,
    /// iOS mobile app client.
    Ios,
    /// Embedded TV/living-room client (useful for age-restricted content).
    Tv,
}

/// The public InnerTube API key real YouTube web/mobile clients send as
/// `X-Goog-Api-Key` (and historically as `?key=`). This is not a secret; it is
/// embedded in YouTube's own client JavaScript.
const INNERTUBE_API_KEY: &str = "AIzaSyAO_FJ2SlqU8Q4STEHLGCilw_Y9_11qcW8";

/// Static parameters describing how to present a given [`ClientKind`].
#[derive(Debug, Clone)]
pub(crate) struct ClientParams {
    /// `context.client.clientName`.
    pub client_name: &'static str,
    /// Numeric client id sent as the `X-YouTube-Client-Name` header
    /// (1=WEB, 3=ANDROID, 5=IOS, 85=TVHTML5_SIMPLY_EMBEDDED_PLAYER).
    pub client_name_id: u32,
    /// `context.client.clientVersion`.
    pub client_version: &'static str,
    /// `User-Agent` header to send.
    pub user_agent: &'static str,
    /// Extra fields merged into `context.client` (e.g. `androidSdkVersion`).
    pub extras: serde_json::Value,
}

impl ClientKind {
    /// The impersonation parameters for this client.
    fn params(&self) -> ClientParams {
        match self {
            ClientKind::Web => ClientParams {
                client_name: "WEB",
                client_name_id: 1,
                client_version: "2.20240620.05.00",
                user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                             (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36",
                extras: serde_json::json!({}),
            },
            ClientKind::Android => ClientParams {
                client_name: "ANDROID",
                client_name_id: 3,
                client_version: "20.10.38",
                user_agent: "com.google.android.youtube/20.10.38 (Linux; U; Android 14) gzip",
                extras: serde_json::json!({
                    "androidSdkVersion": 34,
                    "osName": "Android",
                    "osVersion": "14",
                }),
            },
            ClientKind::AndroidVr => ClientParams {
                client_name: "ANDROID_VR",
                client_name_id: 28,
                client_version: "1.60.19",
                user_agent: "com.google.android.apps.youtube.vr.oculus/1.60.19 \
                             (Linux; U; Android 12; GB) gzip",
                extras: serde_json::json!({
                    "androidSdkVersion": 32,
                    "deviceMake": "Oculus",
                    "deviceModel": "Quest 3",
                    "osName": "Android",
                    "osVersion": "12",
                }),
            },
            ClientKind::Ios => ClientParams {
                client_name: "IOS",
                client_name_id: 5,
                client_version: "20.10.4",
                user_agent:
                    "com.google.ios.youtube/20.10.4 (iPhone16,2; U; CPU iOS 18_3_2 like Mac OS X)",
                extras: serde_json::json!({
                    "deviceMake": "Apple",
                    "deviceModel": "iPhone16,2",
                    "osName": "iPhone",
                    "osVersion": "18.3.2.22D82",
                }),
            },
            ClientKind::Tv => ClientParams {
                client_name: "TVHTML5_SIMPLY_EMBEDDED_PLAYER",
                client_name_id: 85,
                client_version: "2.0",
                user_agent: "Mozilla/5.0 (PlayStation; PlayStation 4/8.03) AppleWebKit/605.1.15 \
                             (KHTML, like Gecko)",
                extras: serde_json::json!({}),
            },
        }
    }
}

/// A browse request: either a fresh `browse_id` or a `continuation` token.
#[derive(Debug, Clone, Default)]
pub(crate) struct BrowseRequest {
    /// Browse target (e.g. a playlist id `VL...` or channel `UC...`).
    pub browse_id: Option<String>,
    /// Continuation token for fetching the next page.
    pub continuation: Option<String>,
    /// Optional `params` (e.g. a channel tab selector).
    pub params: Option<String>,
}

/// Low-level InnerTube client. The base URL is injectable for testing.
pub(crate) struct InnerTube {
    http: reqwest::Client,
    base: String,
}

impl InnerTube {
    /// Build a client targeting an arbitrary base URL (for mock servers).
    pub fn with_base_url(http: reqwest::Client, base: String) -> Self {
        Self {
            http,
            base: base.trim_end_matches('/').to_string(),
        }
    }

    /// Build the `context` object for a given client.
    ///
    /// When `visitor_data` is supplied it is merged into `context.client`; some
    /// clients (notably [`ClientKind::AndroidVr`]) answer `LOGIN_REQUIRED`
    /// without it.
    fn context(client: ClientKind, visitor_data: Option<&str>) -> serde_json::Value {
        let params = client.params();
        let mut clientv = serde_json::json!({
            "clientName": params.client_name,
            "clientVersion": params.client_version,
            "hl": "en",
            "gl": "US",
        });
        if let Some(obj) = clientv.as_object_mut() {
            if let Some(extra) = params.extras.as_object() {
                for (k, v) in extra {
                    obj.insert(k.clone(), v.clone());
                }
            }
            if let Some(vd) = visitor_data {
                obj.insert(
                    "visitorData".into(),
                    serde_json::Value::String(vd.to_string()),
                );
            }
        }
        serde_json::json!({ "client": clientv })
    }

    /// Best-effort fetch of a `visitorData` session token via the `visitor_id`
    /// endpoint. Returns `None` on any failure; callers proceed without it.
    pub async fn visitor_data(&self) -> Option<String> {
        let body = serde_json::json!({ "context": Self::context(ClientKind::Web, None) });
        let resp = self.post("visitor_id", ClientKind::Web, body).await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let value: serde_json::Value =
            read_bounded_json(resp, "innertube/visitor_id").await.ok()?;
        value
            .get("responseContext")?
            .get("visitorData")?
            .as_str()
            .map(str::to_string)
    }

    /// POST a JSON body to a `youtubei/v1` endpoint, returning the response.
    ///
    /// Sends the headers real InnerTube clients require: the public API key
    /// (`X-Goog-Api-Key`, plus `?key=` for legacy endpoints), the numeric and
    /// string client identity (`X-YouTube-Client-Name`/`-Version`), and
    /// `Origin`/`Referer`. Without these the endpoints commonly answer `400`/`403`
    /// or return a degraded response.
    async fn post(
        &self,
        path: &str,
        client: ClientKind,
        body: serde_json::Value,
    ) -> Result<reqwest::Response> {
        let params = client.params();
        let url = format!(
            "{}/youtubei/v1/{}?key={}",
            self.base, path, INNERTUBE_API_KEY
        );
        self.http
            .post(url)
            .header("User-Agent", params.user_agent)
            .header("Content-Type", "application/json")
            .header("X-Goog-Api-Key", INNERTUBE_API_KEY)
            .header("X-YouTube-Client-Name", params.client_name_id.to_string())
            .header("X-YouTube-Client-Version", params.client_version)
            .header("Origin", "https://www.youtube.com")
            .header("Referer", "https://www.youtube.com/")
            .json(&body)
            .send()
            .await
            .map_err(|source| Error::Network {
                stage: "innertube",
                message: source.to_string(),
            })
    }

    /// Fetch the `player` response for a video.
    ///
    /// When `sts` (the `signatureTimestamp` scraped from the player JS) is
    /// supplied, it is sent in `playbackContext.contentPlaybackContext`. Clients
    /// whose streams are protected by `signatureCipher` (e.g. the TV embedded
    /// client) require it to return valid streaming data.
    ///
    /// A non-`OK` `playabilityStatus` is mapped to a typed [`Error::Unavailable`].
    pub async fn player(
        &self,
        video_id: &str,
        client: ClientKind,
        sts: Option<u64>,
        visitor_data: Option<&str>,
    ) -> Result<PlayerResponse> {
        let mut body = serde_json::json!({
            "videoId": video_id,
            "context": Self::context(client, visitor_data),
            "contentCheckOk": true,
            "racyCheckOk": true,
        });
        if let (Some(obj), Some(sts)) = (body.as_object_mut(), sts) {
            obj.insert(
                "playbackContext".into(),
                serde_json::json!({
                    "contentPlaybackContext": { "signatureTimestamp": sts }
                }),
            );
        }
        let resp = self.post("player", client, body).await?;
        let parsed: PlayerResponse = read_bounded_json(resp, "innertube/player").await?;
        parsed.playability_status.ensure_ok()?;
        Ok(parsed)
    }

    /// Fetch a `browse` response (playlist/channel) as a raw JSON value.
    pub async fn browse(&self, req: BrowseRequest) -> Result<serde_json::Value> {
        let client = ClientKind::Web;
        let mut body = serde_json::json!({ "context": Self::context(client, None) });
        if let Some(obj) = body.as_object_mut() {
            if let Some(id) = req.browse_id {
                obj.insert("browseId".into(), serde_json::Value::String(id));
            }
            if let Some(cont) = req.continuation {
                obj.insert("continuation".into(), serde_json::Value::String(cont));
            }
            if let Some(params) = req.params {
                obj.insert("params".into(), serde_json::Value::String(params));
            }
        }
        let resp = self.post("browse", client, body).await?;
        read_bounded_json(resp, "innertube/browse").await
    }

    /// Resolve a canonical URL (e.g. `https://www.youtube.com/@handle`) into its
    /// navigation endpoint via the `navigation/resolve_url` endpoint.
    ///
    /// This is how real clients (and yt-dlp) turn an `@handle` into a `UC…`
    /// channel `browseId`: passing `@handle` directly to `browse` is invalid.
    pub async fn resolve_url(&self, url: &str) -> Result<serde_json::Value> {
        let client = ClientKind::Web;
        let body = serde_json::json!({
            "context": Self::context(client, None),
            "url": url,
        });
        let resp = self.post("navigation/resolve_url", client, body).await?;
        read_bounded_json(resp, "innertube/resolve_url").await
    }

    /// Fetch a `search` response as a raw JSON value (video-only filter).
    ///
    /// A continuation request carries ONLY `{context, continuation}`: real
    /// InnerTube rejects (or ignores, repeating page 1) a continuation that also
    /// re-sends `query`/`params`. The first page carries `query` + the
    /// video-only `params` filter instead.
    pub async fn search(
        &self,
        query: &str,
        continuation: Option<&str>,
    ) -> Result<serde_json::Value> {
        let client = ClientKind::Web;
        let body = match continuation {
            Some(cont) => serde_json::json!({
                "context": Self::context(client, None),
                "continuation": cont,
            }),
            None => serde_json::json!({
                "context": Self::context(client, None),
                "query": query,
                "params": "EgIQAQ%3D%3D",
            }),
        };
        let resp = self.post("search", client, body).await?;
        read_bounded_json(resp, "innertube/search").await
    }
}

/// Typed (partial) player response. Unknown fields are ignored.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlayerResponse {
    /// Whether the video is playable, and why not if it isn't.
    pub playability_status: PlayabilityStatus,
    /// Core video metadata. Optional because non-playable responses (e.g.
    /// `LOGIN_REQUIRED`) omit it; keeping it optional lets such responses
    /// deserialize so [`PlayabilityStatus::ensure_ok`] maps them to a typed
    /// error instead of failing as a missing field (which would abort the
    /// client fallback loop).
    #[serde(default)]
    pub video_details: Option<VideoDetails>,
    /// Stream descriptors (absent for unplayable videos).
    pub streaming_data: Option<StreamingData>,
    /// Extra metadata (upload date, etc.).
    pub microformat: Option<Microformat>,
}

/// Playability status block.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlayabilityStatus {
    /// e.g. `"OK"`, `"LOGIN_REQUIRED"`, `"ERROR"`, `"UNPLAYABLE"`.
    pub status: String,
    /// Human-readable reason, when present.
    #[serde(default)]
    pub reason: Option<String>,
}

impl PlayabilityStatus {
    /// Map a non-OK status to a typed [`Error::Unavailable`].
    fn ensure_ok(&self) -> Result<()> {
        if self.status == "OK" {
            return Ok(());
        }
        let message = self.reason.clone().unwrap_or_default();
        let reason = match self.status.as_str() {
            "LOGIN_REQUIRED" => UnavailableReason::AgeRestricted,
            "ERROR" => UnavailableReason::Gone,
            "LIVE_STREAM_OFFLINE" => UnavailableReason::Live,
            "UNPLAYABLE" => {
                let lower = message.to_lowercase();
                if lower.contains("not available in your country")
                    || lower.contains("geo")
                    || lower.contains("country")
                {
                    UnavailableReason::GeoBlocked
                } else {
                    UnavailableReason::Other
                }
            }
            _ => UnavailableReason::Other,
        };
        Err(Error::Unavailable { reason, message })
    }
}

/// Core video metadata.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VideoDetails {
    /// The 11-character video id.
    pub video_id: String,
    /// Video title.
    pub title: String,
    /// Duration in whole seconds, as a string.
    #[serde(default)]
    pub length_seconds: Option<String>,
    /// Uploader/author name.
    #[serde(default)]
    pub author: Option<String>,
    /// Owning channel id.
    #[serde(default)]
    pub channel_id: Option<String>,
    /// View count, as a string.
    #[serde(default)]
    pub view_count: Option<String>,
    /// Short description text.
    #[serde(default)]
    pub short_description: Option<String>,
    /// Thumbnail set.
    #[serde(default)]
    pub thumbnail: Option<ThumbnailSet>,
    /// Whether this is a live broadcast.
    #[serde(default)]
    pub is_live: bool,
}

/// A set of thumbnails.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ThumbnailSet {
    /// The individual thumbnails.
    #[serde(default)]
    pub thumbnails: Vec<RawThumbnail>,
}

/// A single thumbnail descriptor.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct RawThumbnail {
    /// Image URL.
    pub url: String,
    /// Width in pixels.
    #[serde(default)]
    pub width: Option<u32>,
    /// Height in pixels.
    #[serde(default)]
    pub height: Option<u32>,
}

/// Streaming data: progressive and adaptive formats.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StreamingData {
    /// Progressive (muxed A+V) formats.
    #[serde(default)]
    pub formats: Vec<RawFormat>,
    /// Adaptive (split) formats.
    #[serde(default)]
    pub adaptive_formats: Vec<RawFormat>,
    /// HLS manifest URL for live streams.
    // v1 treats live content as metadata-only (per the design spec), so the HLS
    // manifest is parsed but not yet turned into formats. Kept on the typed
    // contract for completeness and future live support.
    #[allow(dead_code)]
    #[serde(default)]
    pub hls_manifest_url: Option<String>,
}

/// A single raw format descriptor from InnerTube.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawFormat {
    /// Internal tag identifying the format.
    pub itag: u32,
    /// Direct (already-deciphered) URL, when present.
    #[serde(default)]
    pub url: Option<String>,
    /// Ciphered URL+signature blob, when the URL must be solved.
    #[serde(default)]
    pub signature_cipher: Option<String>,
    /// MIME type with codecs parameter.
    pub mime_type: String,
    /// Video width.
    #[serde(default)]
    pub width: Option<u32>,
    /// Video height.
    #[serde(default)]
    pub height: Option<u32>,
    /// Frame rate.
    #[serde(default)]
    pub fps: Option<f64>,
    /// Total bitrate in bits/sec.
    #[serde(default)]
    pub bitrate: Option<u64>,
    /// Content length in bytes, as a string.
    #[serde(default)]
    pub content_length: Option<String>,
    /// Audio sample rate in Hz, as a string.
    #[serde(default)]
    pub audio_sample_rate: Option<String>,
    /// Number of audio channels.
    #[serde(default)]
    pub audio_channels: Option<u8>,
}

/// Microformat metadata block.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Microformat {
    /// The renderer holding the interesting fields.
    #[serde(default)]
    pub player_microformat_renderer: Option<PlayerMicroformatRenderer>,
}

/// The player microformat renderer.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlayerMicroformatRenderer {
    /// Upload date in `YYYY-MM-DD` form.
    #[serde(default)]
    pub upload_date: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    fn fixture(name: &str) -> String {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/innertube")
            .join(name);
        std::fs::read_to_string(p).expect("fixture present")
    }

    #[tokio::test]
    async fn player_request_sends_client_context_and_parses() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/youtubei/v1/player"))
            .and(|req: &Request| {
                let body: serde_json::Value = match serde_json::from_slice(&req.body) {
                    Ok(v) => v,
                    Err(_) => return false,
                };
                body["context"]["client"]["clientName"] == "ANDROID"
            })
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(fixture("player_android.json"), "application/json"),
            )
            .mount(&server)
            .await;

        let it = InnerTube::with_base_url(reqwest::Client::new(), server.uri());
        let resp = it
            .player("dQw4w9WgXcQ", ClientKind::Android, None, None)
            .await
            .unwrap();
        assert_eq!(resp.video_details.unwrap().video_id, "dQw4w9WgXcQ");
        assert_eq!(resp.streaming_data.unwrap().formats.len(), 1);
    }

    #[tokio::test]
    async fn player_request_sends_innertube_headers_and_sts() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/youtubei/v1/player"))
            .and(|req: &Request| {
                // Required InnerTube headers must be present and correct.
                let header = |name: &str| {
                    req.headers
                        .get(name)
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string)
                };
                let key_ok = header("x-goog-api-key").as_deref() == Some(INNERTUBE_API_KEY);
                let cname_ok = header("x-youtube-client-name").as_deref() == Some("3");
                let cver_ok = header("x-youtube-client-version").as_deref() == Some("20.10.38");
                let origin_ok = header("origin").as_deref() == Some("https://www.youtube.com");
                // The per-client User-Agent must be the Android one (refreshed in
                // 0f1e101 to client version 20.10.38).
                let ua_ok = header("user-agent").as_deref()
                    == Some("com.google.android.youtube/20.10.38 (Linux; U; Android 14) gzip");
                // sts threaded into the playback context.
                let body: serde_json::Value = match serde_json::from_slice(&req.body) {
                    Ok(v) => v,
                    Err(_) => return false,
                };
                let sts_ok = body["playbackContext"]["contentPlaybackContext"]
                    ["signatureTimestamp"]
                    == 19834;
                // The Android `extras` must be merged into context.client.
                let client = &body["context"]["client"];
                let extras_ok = client["androidSdkVersion"] == 34
                    && client["osName"] == "Android"
                    && client["osVersion"] == "14"
                    && client["clientVersion"] == "20.10.38";
                key_ok && cname_ok && cver_ok && origin_ok && ua_ok && sts_ok && extras_ok
            })
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(fixture("player_android.json"), "application/json"),
            )
            .mount(&server)
            .await;

        let it = InnerTube::with_base_url(reqwest::Client::new(), server.uri());
        // Mismatched headers/sts would 404 the mock -> error; success proves them.
        it.player("dQw4w9WgXcQ", ClientKind::Android, Some(19834), None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn ios_player_request_sends_ios_user_agent_and_device_model() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/youtubei/v1/player"))
            .and(|req: &Request| {
                let header = |name: &str| {
                    req.headers
                        .get(name)
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string)
                };
                let ua_ok = header("user-agent").as_deref()
                    == Some(
                        "com.google.ios.youtube/20.10.4 (iPhone16,2; U; CPU iOS 18_3_2 like Mac OS X)",
                    );
                let cname_ok = header("x-youtube-client-name").as_deref() == Some("5");
                let body: serde_json::Value = match serde_json::from_slice(&req.body) {
                    Ok(v) => v,
                    Err(_) => return false,
                };
                let client = &body["context"]["client"];
                let extras_ok = client["deviceMake"] == "Apple"
                    && client["deviceModel"] == "iPhone16,2"
                    && client["clientVersion"] == "20.10.4";
                ua_ok && cname_ok && extras_ok
            })
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(fixture("player_android.json"), "application/json"),
            )
            .mount(&server)
            .await;

        let it = InnerTube::with_base_url(reqwest::Client::new(), server.uri());
        // A mismatched UA/extras would not match the mock and 404 -> error.
        it.player("dQw4w9WgXcQ", ClientKind::Ios, None, None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn search_continuation_omits_query_and_params() {
        let server = MockServer::start().await;
        // First page: query + params, no continuation.
        Mock::given(method("POST"))
            .and(path("/youtubei/v1/search"))
            .and(|req: &Request| {
                let body: serde_json::Value =
                    serde_json::from_slice(&req.body).unwrap_or(serde_json::Value::Null);
                body.get("continuation").is_none()
                    && body["query"] == "rust"
                    && body["params"] == "EgIQAQ%3D%3D"
            })
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(fixture("search.json"), "application/json"),
            )
            .mount(&server)
            .await;
        // Continuation page: continuation only, NO query/params.
        Mock::given(method("POST"))
            .and(path("/youtubei/v1/search"))
            .and(|req: &Request| {
                let body: serde_json::Value =
                    serde_json::from_slice(&req.body).unwrap_or(serde_json::Value::Null);
                body.get("continuation").and_then(serde_json::Value::as_str) == Some("TOK")
                    && body.get("query").is_none()
                    && body.get("params").is_none()
            })
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(fixture("search.json"), "application/json"),
            )
            .mount(&server)
            .await;

        let it = InnerTube::with_base_url(reqwest::Client::new(), server.uri());
        it.search("rust", None).await.unwrap();
        it.search("rust", Some("TOK")).await.unwrap();
    }

    #[tokio::test]
    async fn player_parses_adaptive_and_microformat() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/youtubei/v1/player"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(fixture("player_android.json"), "application/json"),
            )
            .mount(&server)
            .await;

        let it = InnerTube::with_base_url(reqwest::Client::new(), server.uri());
        let resp = it
            .player("dQw4w9WgXcQ", ClientKind::Android, None, None)
            .await
            .unwrap();
        let sd = resp.streaming_data.unwrap();
        assert_eq!(sd.adaptive_formats.len(), 1);
        let af = &sd.adaptive_formats[0];
        assert_eq!(af.itag, 251);
        assert!(af.signature_cipher.is_some());
        assert!(af.url.is_none());
        let mf = resp
            .microformat
            .unwrap()
            .player_microformat_renderer
            .unwrap();
        assert_eq!(mf.upload_date.as_deref(), Some("2009-10-25"));
    }

    #[tokio::test]
    async fn browse_parses_entries_and_continuation() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/youtubei/v1/browse"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(fixture("browse_playlist.json"), "application/json"),
            )
            .mount(&server)
            .await;

        let it = InnerTube::with_base_url(reqwest::Client::new(), server.uri());
        let value = it
            .browse(BrowseRequest {
                browse_id: Some("VLPLx".into()),
                ..Default::default()
            })
            .await
            .unwrap();
        let list = &value["contents"]["twoColumnBrowseResultsRenderer"]["tabs"][0]["tabRenderer"]
            ["content"]["sectionListRenderer"]["contents"][0]["itemSectionRenderer"]["contents"][0]
            ["playlistVideoListRenderer"]["contents"];
        assert_eq!(list[0]["playlistVideoRenderer"]["videoId"], "aaaaaaaaaaa");
        assert_eq!(
            list[2]["continuationItemRenderer"]["continuationEndpoint"]["continuationCommand"]
                ["token"],
            "CONT_TOKEN_1"
        );
    }

    #[tokio::test]
    async fn search_sends_query_and_returns_raw_value() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/youtubei/v1/search"))
            .and(|req: &Request| {
                let body: serde_json::Value = match serde_json::from_slice(&req.body) {
                    Ok(v) => v,
                    Err(_) => return false,
                };
                body["query"] == "rust"
            })
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(fixture("search.json"), "application/json"),
            )
            .mount(&server)
            .await;

        let it = InnerTube::with_base_url(reqwest::Client::new(), server.uri());
        let value = it.search("rust", None).await.unwrap();
        assert!(value["contents"]["twoColumnSearchResultsRenderer"].is_object());
    }

    async fn player_status_error(status: &str, reason: Option<&str>) -> Error {
        let server = MockServer::start().await;
        let mut body = serde_json::json!({
            "playabilityStatus": { "status": status },
            "videoDetails": { "videoId": "x", "title": "t" }
        });
        if let (Some(obj), Some(r)) = (body["playabilityStatus"].as_object_mut(), reason) {
            obj.insert("reason".into(), serde_json::Value::String(r.to_string()));
        }
        Mock::given(method("POST"))
            .and(path("/youtubei/v1/player"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        let it = InnerTube::with_base_url(reqwest::Client::new(), server.uri());
        it.player("x", ClientKind::Web, None, None)
            .await
            .unwrap_err()
    }

    #[tokio::test]
    async fn unavailable_status_maps_to_typed_error() {
        assert!(matches!(
            player_status_error("LOGIN_REQUIRED", None).await,
            Error::Unavailable {
                reason: UnavailableReason::AgeRestricted,
                ..
            }
        ));
        assert!(matches!(
            player_status_error("ERROR", None).await,
            Error::Unavailable {
                reason: UnavailableReason::Gone,
                ..
            }
        ));
        assert!(matches!(
            player_status_error(
                "UNPLAYABLE",
                Some("This video is not available in your country")
            )
            .await,
            Error::Unavailable {
                reason: UnavailableReason::GeoBlocked,
                ..
            }
        ));
        assert!(matches!(
            player_status_error("LIVE_STREAM_OFFLINE", None).await,
            Error::Unavailable {
                reason: UnavailableReason::Live,
                ..
            }
        ));
    }
}
