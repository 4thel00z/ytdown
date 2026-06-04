//! InnerTube API client: talks to YouTube's private `youtubei/v1` endpoints
//! while impersonating one of several official client identities.
//!
// This module is consumed by the YouTube player, pagination, and extractor
// orchestration modules (later plan tasks). Until those land, several items are
// only exercised by this module's own tests, so dead-code analysis would flag
// them; allow it crate-internally rather than weaken the contract.
#![allow(dead_code)]

use serde::Deserialize;

use crate::error::{Error, Result, UnavailableReason};

/// Which InnerTube client identity to impersonate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ClientKind {
    /// Desktop web client.
    Web,
    /// Android mobile app client.
    Android,
    /// iOS mobile app client.
    Ios,
    /// Embedded TV/living-room client (useful for age-restricted content).
    Tv,
}

/// Static parameters describing how to present a given [`ClientKind`].
#[derive(Debug, Clone)]
pub(crate) struct ClientParams {
    /// `context.client.clientName`.
    pub client_name: &'static str,
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
                client_version: "2.20240101.00.00",
                user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                             (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
                extras: serde_json::json!({}),
            },
            ClientKind::Android => ClientParams {
                client_name: "ANDROID",
                client_version: "19.09.37",
                user_agent: "com.google.android.youtube/19.09.37 (Linux; U; Android 11) gzip",
                extras: serde_json::json!({ "androidSdkVersion": 30 }),
            },
            ClientKind::Ios => ClientParams {
                client_name: "IOS",
                client_version: "19.09.3",
                user_agent:
                    "com.google.ios.youtube/19.09.3 (iPhone14,3; U; CPU iOS 15_6 like Mac OS X)",
                extras: serde_json::json!({ "deviceModel": "iPhone14,3" }),
            },
            ClientKind::Tv => ClientParams {
                client_name: "TVHTML5_SIMPLY_EMBEDDED_PLAYER",
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
    /// Build a client targeting the real YouTube endpoint.
    pub fn new(http: reqwest::Client) -> Self {
        Self::with_base_url(http, "https://www.youtube.com".into())
    }

    /// Build a client targeting an arbitrary base URL (for mock servers).
    pub fn with_base_url(http: reqwest::Client, base: String) -> Self {
        Self {
            http,
            base: base.trim_end_matches('/').to_string(),
        }
    }

    /// Build the `context` object for a given client.
    fn context(client: ClientKind) -> serde_json::Value {
        let params = client.params();
        let mut clientv = serde_json::json!({
            "clientName": params.client_name,
            "clientVersion": params.client_version,
            "hl": "en",
            "gl": "US",
        });
        if let (Some(obj), Some(extra)) = (clientv.as_object_mut(), params.extras.as_object()) {
            for (k, v) in extra {
                obj.insert(k.clone(), v.clone());
            }
        }
        serde_json::json!({ "client": clientv })
    }

    /// POST a JSON body to a `youtubei/v1` endpoint, returning the response.
    async fn post(
        &self,
        path: &str,
        client: ClientKind,
        body: serde_json::Value,
    ) -> Result<reqwest::Response> {
        let url = format!("{}/youtubei/v1/{}", self.base, path);
        self.http
            .post(url)
            .header("User-Agent", client.params().user_agent)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|source| Error::Network {
                stage: "innertube",
                source,
            })
    }

    /// Fetch the `player` response for a video.
    ///
    /// A non-`OK` `playabilityStatus` is mapped to a typed [`Error::Unavailable`].
    pub async fn player(&self, video_id: &str, client: ClientKind) -> Result<PlayerResponse> {
        let body = serde_json::json!({
            "videoId": video_id,
            "context": Self::context(client),
            "contentCheckOk": true,
            "racyCheckOk": true,
        });
        let resp = self.post("player", client, body).await?;
        let parsed: PlayerResponse = resp.json().await.map_err(|source| Error::Network {
            stage: "innertube/player",
            source,
        })?;
        parsed.playability_status.ensure_ok()?;
        Ok(parsed)
    }

    /// Fetch a `browse` response (playlist/channel) as a raw JSON value.
    pub async fn browse(&self, req: BrowseRequest) -> Result<serde_json::Value> {
        let client = ClientKind::Web;
        let mut body = serde_json::json!({ "context": Self::context(client) });
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
        resp.json().await.map_err(|source| Error::Network {
            stage: "innertube/browse",
            source,
        })
    }

    /// Fetch a `search` response as a raw JSON value (video-only filter).
    pub async fn search(
        &self,
        query: &str,
        continuation: Option<&str>,
    ) -> Result<serde_json::Value> {
        let client = ClientKind::Web;
        let mut body = serde_json::json!({
            "context": Self::context(client),
            "query": query,
            "params": "EgIQAQ%3D%3D",
        });
        if let (Some(obj), Some(cont)) = (body.as_object_mut(), continuation) {
            obj.insert(
                "continuation".into(),
                serde_json::Value::String(cont.to_string()),
            );
        }
        let resp = self.post("search", client, body).await?;
        resp.json().await.map_err(|source| Error::Network {
            stage: "innertube/search",
            source,
        })
    }
}

/// Typed (partial) player response. Unknown fields are ignored.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlayerResponse {
    /// Whether the video is playable, and why not if it isn't.
    pub playability_status: PlayabilityStatus,
    /// Core video metadata.
    pub video_details: VideoDetails,
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
        let resp = it.player("dQw4w9WgXcQ", ClientKind::Android).await.unwrap();
        assert_eq!(resp.video_details.video_id, "dQw4w9WgXcQ");
        assert_eq!(resp.streaming_data.unwrap().formats.len(), 1);
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
        let resp = it.player("dQw4w9WgXcQ", ClientKind::Android).await.unwrap();
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
        it.player("x", ClientKind::Web).await.unwrap_err()
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
