//! Extractor trait and registry: URL-to-media dispatch.

use crate::{Error, MediaInfo, Result};

/// Shared state handed to extractors.
pub struct ExtractorContext {
    /// Shared HTTP client.
    pub http: reqwest::Client,
}

impl ExtractorContext {
    /// Build a context around an existing HTTP client.
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }
}

/// A site-specific extractor: tests URLs and resolves them into media.
#[async_trait::async_trait]
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
        let ctx = ExtractorContext::new(reqwest::Client::new());
        assert!(reg.resolve(&ctx, "https://dummy.test/v/1").await.is_ok());
        let err = reg.resolve(&ctx, "https://other.test/").await.unwrap_err();
        assert!(matches!(err, Error::UnsupportedUrl(_)));
    }
}
