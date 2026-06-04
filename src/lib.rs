#![warn(missing_docs)]
//! ytdown — a Rust library mirroring yt-dlp's core functionality.
//!
//! Resolve media URLs into metadata and formats, select a format, download it.

pub mod error;
pub mod types;

pub use error::{Error, Result};
pub use types::*;
