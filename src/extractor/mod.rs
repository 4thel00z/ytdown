//! Extractor trait and registry: URL-to-media dispatch.

/// Instagram extractor implementation.
pub mod instagram;
/// Reddit extractor implementation.
pub mod reddit;
/// Helpers shared by several site extractors.
pub(crate) mod shared;
/// TikTok extractor implementation.
pub mod tiktok;
/// X/Twitter extractor implementation.
pub mod twitter;
/// YouTube extractor implementation.
pub mod youtube;

use std::collections::HashMap;
use std::sync::Arc;

use self::youtube::player::SolvedPlayer;
use crate::{Error, MediaInfo, Result};

/// Shared state handed to extractors.
pub struct ExtractorContext {
    /// Shared HTTP client.
    pub http: std::sync::Arc<dyn crate::transport::HttpClient>,
    /// Cipher-solver cache, keyed by player version, so that a solved player is
    /// reused across every video sharing that version.
    pub(crate) player_cache: tokio::sync::Mutex<HashMap<String, Arc<SolvedPlayer>>>,
}

impl ExtractorContext {
    /// Build a context around an existing transport.
    pub fn new(http: std::sync::Arc<dyn crate::transport::HttpClient>) -> Self {
        Self {
            http,
            player_cache: tokio::sync::Mutex::new(HashMap::new()),
        }
    }
}

/// A site-specific extractor: tests URLs and resolves them into media.
///
/// On native targets the `extract` future is `Send` (the whole async extraction
/// stack is `Send`). On `wasm32` it is `?Send`, mirroring the [`HttpClient`]
/// transport: a JS `fetch`-backed future is not `Send`.
///
/// [`HttpClient`]: crate::transport::HttpClient
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
pub trait Extractor: Send + Sync {
    /// Stable identifier, e.g. `"youtube"`.
    fn name(&self) -> &'static str;
    /// Cheap URL test — no network.
    fn matches(&self, url: &url::Url) -> bool;
    /// Full extraction.
    async fn extract(&self, ctx: &ExtractorContext, url: &url::Url) -> Result<MediaInfo>;
}

/// Ordered extractor list; first match wins.
pub struct Registry {
    extractors: Vec<Box<dyn Extractor>>,
}

impl Registry {
    /// Build a registry from an ordered list of extractors.
    pub fn new(extractors: Vec<Box<dyn Extractor>>) -> Self {
        Self { extractors }
    }

    /// Parse the URL and dispatch to the first matching extractor.
    ///
    /// A parse failure or no match yields [`Error::UnsupportedUrl`].
    pub async fn resolve(&self, ctx: &ExtractorContext, url: &str) -> Result<MediaInfo> {
        let parsed = url::Url::parse(url).map_err(|_| Error::UnsupportedUrl(url.to_string()))?;
        for extractor in &self.extractors {
            if extractor.matches(&parsed) {
                return extractor.extract(ctx, &parsed).await;
            }
        }
        Err(Error::UnsupportedUrl(url.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::VideoInfo;

    fn test_video_info() -> VideoInfo {
        VideoInfo {
            id: String::new(),
            title: String::new(),
            description: None,
            duration: None,
            uploader: None,
            uploader_id: None,
            channel_id: None,
            view_count: None,
            upload_date: None,
            thumbnails: Vec::new(),
            webpage_url: String::new(),
            is_live: false,
            formats: Vec::new(),
        }
    }

    struct Dummy;

    #[async_trait::async_trait]
    impl Extractor for Dummy {
        fn name(&self) -> &'static str {
            "dummy"
        }
        fn matches(&self, url: &url::Url) -> bool {
            url.host_str() == Some("dummy.test")
        }
        async fn extract(&self, _ctx: &ExtractorContext, url: &url::Url) -> Result<MediaInfo> {
            Ok(MediaInfo::Single(VideoInfo {
                id: "x".into(),
                title: "t".into(),
                webpage_url: url.to_string(),
                ..test_video_info()
            }))
        }
    }

    #[tokio::test]
    async fn registry_dispatches_by_match() {
        let reg = Registry::new(vec![Box::new(Dummy)]);
        let ctx = ExtractorContext::new(std::sync::Arc::new(crate::transport::ReqwestClient::new(
            reqwest::Client::new(),
        )));
        assert!(reg.resolve(&ctx, "https://dummy.test/v/1").await.is_ok());
        let err = reg.resolve(&ctx, "https://other.test/").await.unwrap_err();
        assert!(matches!(err, Error::UnsupportedUrl(_)));
    }
}
