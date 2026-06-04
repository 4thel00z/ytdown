#![warn(missing_docs)]
//! ytdown — a Rust library mirroring yt-dlp's core functionality.
//!
//! Resolve media URLs into metadata and formats, select a format, download it.

/// JavaScript interpreter for solving extractor ciphers.
pub(crate) mod jsi;

/// Downloading resolved formats to disk.
pub mod download;
pub mod error;
pub mod extractor;
/// Format selection over a video's available representations.
pub mod format;
/// Postprocessing of downloaded media via ffmpeg.
#[cfg(feature = "ffmpeg")]
pub mod postprocess;
pub mod types;

pub use error::{Error, Result};
pub use types::*;
