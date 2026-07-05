//! Error types for the crate.

/// All errors produced by this crate.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// No registered extractor matches the URL.
    #[error("no extractor supports this URL: {0}")]
    UnsupportedUrl(String),
    /// Underlying HTTP/transport failure.
    #[error("network error during {stage}: {message}")]
    Network {
        /// The operation that was in progress when the failure occurred.
        stage: &'static str,
        /// A human-readable description of the transport failure.
        message: String,
    },
    /// The site responded but the expected data could not be extracted.
    #[error("extraction failed at {stage}: {message}")]
    Extraction {
        /// The extraction phase that failed.
        stage: &'static str,
        /// A human-readable description of what went wrong.
        message: String,
    },
    /// The media exists but cannot be accessed.
    #[error("media unavailable: {reason}{}", if .message.is_empty() { String::new() } else { format!(" \u{2014} {message}") })]
    Unavailable {
        /// The classified reason the media is unavailable.
        reason: UnavailableReason,
        /// The raw message reported by the site, if any.
        message: String,
    },
    /// JS cipher solving failed.
    #[error("cipher error: {0}")]
    Cipher(String),
    /// Requested format does not exist / no format matched the selector.
    #[error("no matching format: {0}")]
    FormatNotFound(String),
    /// Filesystem error during download.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// Postprocessing (ffmpeg) failure.
    #[error("postprocess error: {0}")]
    Postprocess(String),
}

/// Why media is unavailable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum UnavailableReason {
    /// Removed, private, or never existed.
    Gone,
    /// Age-restricted and no authenticated client succeeded.
    AgeRestricted,
    /// The site's anti-bot wall ("Sign in to confirm you're not a bot"):
    /// the requesting network is flagged, not the video itself. Retrying with
    /// authenticated cookies usually resolves it.
    BotCheck,
    /// Blocked in the request region.
    GeoBlocked,
    /// Live content (manifests not supported in v1).
    Live,
    /// Any other documented status from the site.
    Other,
}

impl std::fmt::Display for UnavailableReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            UnavailableReason::Gone => "gone",
            UnavailableReason::AgeRestricted => "age-restricted",
            UnavailableReason::BotCheck => "bot-check",
            UnavailableReason::GeoBlocked => "geo-blocked",
            UnavailableReason::Live => "live",
            UnavailableReason::Other => "other",
        };
        f.write_str(s)
    }
}

/// Crate-wide result alias.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_is_useful() {
        let e = Error::UnsupportedUrl("https://example.com/x".into());
        assert!(e.to_string().contains("example.com"));
    }
}
