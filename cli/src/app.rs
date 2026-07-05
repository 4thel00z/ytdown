//! Shared CLI plumbing: building the library handle and runtime checks.

use std::path::Path;

use anyhow::Context;
use ytdown::{CollectionKind, Ytdown};

/// Build the library handle from global flags.
///
/// The hidden `YTDOWN_BASE_URL` env var swaps the default YouTube extractor
/// for one pointed at an alternate origin — used by the e2e tests.
pub fn build_ytdown(user_agent: Option<&str>, cookies: Option<&Path>) -> anyhow::Result<Ytdown> {
    let mut b = Ytdown::builder();
    if let Some(ua) = user_agent {
        b = b.user_agent(ua);
    }
    if let Some(path) = cookies {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read cookies file {}", path.display()))?;
        let jar = ytdown::cookies::CookieJar::parse_netscape(&text)
            .with_context(|| format!("failed to parse cookies file {}", path.display()))?;
        b = b.cookies(jar);
    }
    if let Ok(base) = std::env::var("YTDOWN_BASE_URL") {
        b = b.clear_extractors().extractor(Box::new(
            ytdown::extractor::youtube::YoutubeExtractor::with_base_url(base),
        ));
    }
    b.build().context("failed to build HTTP client")
}

/// True if the ffmpeg binary at `path` runs.
pub async fn ffmpeg_available(path: &Path) -> bool {
    tokio::process::Command::new(path)
        .arg("-version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Stable lowercase name for a collection kind (used in JSON output).
pub fn kind_str(k: CollectionKind) -> &'static str {
    match k {
        CollectionKind::Playlist => "playlist",
        CollectionKind::Channel => "channel",
        CollectionKind::Search => "search",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_ffmpeg_binary_is_unavailable() {
        assert!(!ffmpeg_available(Path::new("/nonexistent/ffmpeg-xyz")).await);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn zero_exit_binary_counts_as_available() {
        // `true` ignores its args and exits 0 — a stand-in for ffmpeg.
        assert!(ffmpeg_available(Path::new("true")).await);
    }
}
