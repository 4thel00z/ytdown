//! Progress reporting for downloads.
//!
//! [`Progress`] snapshots are delivered to the [`ProgressCallback`] registered
//! on a download.

/// Snapshot of download progress.
#[derive(Debug, Clone)]
pub struct Progress {
    /// Bytes written to disk so far (including any pre-existing resumed bytes).
    pub bytes_downloaded: u64,
    /// Total expected size, if the server reported a content length.
    pub total_bytes: Option<u64>,
    /// Smoothed bytes/sec over the last few seconds.
    pub speed_bps: Option<f64>,
    /// Estimated time remaining, if total size and speed are known.
    pub eta: Option<std::time::Duration>,
}

impl Progress {
    /// Completion percentage in `0.0..=100.0`, if the total size is known.
    ///
    /// ```
    /// use ytdown::download::Progress;
    /// let p = Progress {
    ///     bytes_downloaded: 50,
    ///     total_bytes: Some(200),
    ///     speed_bps: None,
    ///     eta: None,
    /// };
    /// assert_eq!(p.percent(), Some(25.0));
    /// ```
    pub fn percent(&self) -> Option<f64> {
        match self.total_bytes {
            Some(total) if total > 0 => Some((self.bytes_downloaded as f64 / total as f64) * 100.0),
            _ => None,
        }
    }
}

/// Observer invoked with [`Progress`] snapshots during a download.
pub type ProgressCallback = std::sync::Arc<dyn Fn(Progress) + Send + Sync>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_computes_ratio() {
        let p = Progress {
            bytes_downloaded: 25,
            total_bytes: Some(100),
            speed_bps: None,
            eta: None,
        };
        assert_eq!(p.percent(), Some(25.0));
    }

    #[test]
    fn percent_is_none_without_total() {
        let p = Progress {
            bytes_downloaded: 25,
            total_bytes: None,
            speed_bps: None,
            eta: None,
        };
        assert_eq!(p.percent(), None);
    }

    #[test]
    fn percent_is_none_for_zero_total() {
        let p = Progress {
            bytes_downloaded: 0,
            total_bytes: Some(0),
            speed_bps: None,
            eta: None,
        };
        assert_eq!(p.percent(), None);
    }
}
