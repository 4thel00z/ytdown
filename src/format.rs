//! Format selection over a video's available representations.

use crate::error::{Error, Result};
use crate::types::{Container, Format, FormatKind};

/// Fluent, borrowing format selector. Entry point: [`crate::types::VideoInfo::formats`].
///
/// Filter methods consume `self` and return a narrowed selector, so they can be
/// chained. Terminal methods inspect the current set and pick a single format
/// (or pair), returning [`Error::FormatNotFound`] when nothing matches.
///
/// ```
/// use ytdown::types::{Format, VideoStream};
/// use ytdown::format::FormatSelector;
///
/// // `Format`/`VideoStream` are `#[non_exhaustive]`; build them via `Default`.
/// let mut video = VideoStream::default();
/// video.width = Some(1280);
/// video.height = Some(720);
/// video.fps = Some(30.0);
/// video.codec = "avc1".into();
///
/// let mut fmt = Format::default();
/// fmt.itag = Some(22);
/// fmt.video = Some(video);
///
/// let formats = vec![fmt];
/// let best = FormatSelector::new(&formats).best_video().unwrap();
/// assert_eq!(best.itag, Some(22));
/// ```
pub struct FormatSelector<'a> {
    formats: Vec<&'a Format>,
}

/// Video ranking key: (height, fps, bitrate), all descending.
fn video_rank(f: &Format) -> (u32, u64, u64) {
    let height = f.video.as_ref().and_then(|v| v.height).unwrap_or(0);
    let fps = f
        .video
        .as_ref()
        .and_then(|v| v.fps)
        .map(|x| x as u64)
        .unwrap_or(0);
    let bitrate = f.bitrate.unwrap_or(0);
    (height, fps, bitrate)
}

/// Audio ranking key: (bitrate, sample_rate), descending.
fn audio_rank(f: &Format) -> (u64, u32) {
    let audio = f.audio.as_ref();
    let bitrate = audio.and_then(|a| a.bitrate).or(f.bitrate).unwrap_or(0);
    let sample_rate = audio.and_then(|a| a.sample_rate).unwrap_or(0);
    (bitrate, sample_rate)
}

impl<'a> FormatSelector<'a> {
    /// Create a selector over all of `formats`.
    pub fn new(formats: &'a [Format]) -> Self {
        Self {
            formats: formats.iter().collect(),
        }
    }

    /// Keep only audio-only formats.
    #[must_use]
    pub fn audio_only(mut self) -> Self {
        self.formats
            .retain(|f| matches!(f.kind(), FormatKind::AudioOnly));
        self
    }

    /// Keep only video-only formats.
    #[must_use]
    pub fn video_only(mut self) -> Self {
        self.formats
            .retain(|f| matches!(f.kind(), FormatKind::VideoOnly));
        self
    }

    /// Keep only progressive (muxed A+V) formats.
    #[must_use]
    pub fn progressive(mut self) -> Self {
        self.formats
            .retain(|f| matches!(f.kind(), FormatKind::Progressive));
        self
    }

    /// Keep only formats whose video height is at most `h`.
    ///
    /// Formats without a known height are dropped.
    #[must_use]
    pub fn max_height(mut self, h: u32) -> Self {
        self.formats.retain(|f| {
            f.video
                .as_ref()
                .and_then(|v| v.height)
                .is_some_and(|x| x <= h)
        });
        self
    }

    /// Keep only formats with the given container.
    #[must_use]
    pub fn container(mut self, c: &Container) -> Self {
        self.formats
            .retain(|f| f.container.as_ref().is_some_and(|fc| fc == c));
        self
    }

    /// Keep only formats whose video codec starts with `prefix`.
    ///
    /// Formats without a video stream are dropped.
    #[must_use]
    pub fn vcodec_starts_with(mut self, prefix: &str) -> Self {
        self.formats.retain(|f| {
            f.video
                .as_ref()
                .is_some_and(|v| v.codec.starts_with(prefix))
        });
        self
    }

    /// Find the format with the given `itag`.
    pub fn by_itag(&self, itag: u32) -> Result<&'a Format> {
        self.formats
            .iter()
            .copied()
            .find(|f| f.itag == Some(itag))
            .ok_or_else(|| Error::FormatNotFound(format!("by_itag({itag})")))
    }

    /// Highest-resolution progressive (muxed) format.
    pub fn best_progressive(&self) -> Result<&'a Format> {
        self.formats
            .iter()
            .copied()
            .filter(|f| matches!(f.kind(), FormatKind::Progressive))
            .max_by_key(|f| video_rank(f))
            .ok_or_else(|| Error::FormatNotFound("best_progressive".into()))
    }

    /// Best video stream, considering both video-only and progressive formats.
    pub fn best_video(&self) -> Result<&'a Format> {
        self.formats
            .iter()
            .copied()
            .filter(|f| matches!(f.kind(), FormatKind::VideoOnly | FormatKind::Progressive))
            .max_by_key(|f| video_rank(f))
            .ok_or_else(|| Error::FormatNotFound("best_video".into()))
    }

    /// Best audio-only stream by bitrate then sample rate.
    pub fn best_audio(&self) -> Result<&'a Format> {
        self.formats
            .iter()
            .copied()
            .filter(|f| matches!(f.kind(), FormatKind::AudioOnly))
            .max_by_key(|f| audio_rank(f))
            .ok_or_else(|| Error::FormatNotFound("best_audio".into()))
    }

    /// Lowest-quality format available, preferring the smallest video then audio.
    ///
    /// Video-bearing formats are ranked below audio-only/unknown formats (a
    /// leading `0` vs `1` category), so the smallest video is chosen whenever any
    /// video format exists; audio-only is only returned when there is no video.
    /// Within a category, ranking is by resolution/fps/bitrate (or bitrate for
    /// audio), ascending.
    pub fn worst(&self) -> Result<&'a Format> {
        self.formats
            .iter()
            .copied()
            .min_by_key(|f| match f.kind() {
                FormatKind::VideoOnly | FormatKind::Progressive => {
                    let (h, fps, br) = video_rank(f);
                    (0u8, h, fps, br)
                }
                _ => (1u8, 0, 0, f.bitrate.unwrap_or(0)),
            })
            .ok_or_else(|| Error::FormatNotFound("worst".into()))
    }

    /// Best split video/audio pair for ffmpeg muxing.
    ///
    /// Pairs the best video-only stream with the best audio-only stream. When no
    /// split streams exist, falls back to the best progressive format returned as
    /// both elements of the pair.
    pub fn best_video_audio(&self) -> Result<(&'a Format, &'a Format)> {
        let video = self
            .formats
            .iter()
            .copied()
            .filter(|f| matches!(f.kind(), FormatKind::VideoOnly))
            .max_by_key(|f| video_rank(f));
        let audio = self
            .formats
            .iter()
            .copied()
            .filter(|f| matches!(f.kind(), FormatKind::AudioOnly))
            .max_by_key(|f| audio_rank(f));
        match (video, audio) {
            (Some(v), Some(a)) => Ok((v, a)),
            _ => {
                let prog = self
                    .best_progressive()
                    .map_err(|_| Error::FormatNotFound("best_video_audio".into()))?;
                Ok((prog, prog))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AudioStream, VideoStream};

    fn fixtures() -> Vec<Format> {
        vec![
            // 1080p video-only vp9
            Format {
                itag: Some(248),
                video: Some(VideoStream {
                    width: Some(1920),
                    height: Some(1080),
                    fps: Some(30.0),
                    codec: "vp9".into(),
                }),
                audio: None,
                container: Some(Container::WebM),
                bitrate: Some(2_500_000),
                ..Format::default()
            },
            // 720p progressive avc1 + aac
            Format {
                itag: Some(22),
                video: Some(VideoStream {
                    width: Some(1280),
                    height: Some(720),
                    fps: Some(30.0),
                    codec: "avc1.64001F".into(),
                }),
                audio: Some(AudioStream {
                    codec: "mp4a.40.2".into(),
                    bitrate: Some(192_000),
                    sample_rate: Some(44_100),
                    channels: Some(2),
                }),
                container: Some(Container::Mp4),
                bitrate: Some(1_500_000),
                ..Format::default()
            },
            // 360p progressive
            Format {
                itag: Some(18),
                video: Some(VideoStream {
                    width: Some(640),
                    height: Some(360),
                    fps: Some(30.0),
                    codec: "avc1.42001E".into(),
                }),
                audio: Some(AudioStream {
                    codec: "mp4a.40.2".into(),
                    bitrate: Some(96_000),
                    sample_rate: Some(44_100),
                    channels: Some(2),
                }),
                container: Some(Container::Mp4),
                bitrate: Some(500_000),
                ..Format::default()
            },
            // audio-only opus 160k
            Format {
                itag: Some(251),
                video: None,
                audio: Some(AudioStream {
                    codec: "opus".into(),
                    bitrate: Some(160_000),
                    sample_rate: Some(48_000),
                    channels: Some(2),
                }),
                container: Some(Container::Weba),
                bitrate: Some(160_000),
                ..Format::default()
            },
            // audio-only aac 128k
            Format {
                itag: Some(140),
                video: None,
                audio: Some(AudioStream {
                    codec: "mp4a.40.2".into(),
                    bitrate: Some(128_000),
                    sample_rate: Some(44_100),
                    channels: Some(2),
                }),
                container: Some(Container::M4a),
                bitrate: Some(128_000),
                ..Format::default()
            },
        ]
    }

    #[test]
    fn best_progressive_picks_highest_res_muxed() {
        let f = fixtures();
        let sel = FormatSelector::new(&f);
        let chosen = sel.best_progressive().unwrap();
        assert_eq!(chosen.itag, Some(22));
    }

    #[test]
    fn best_video_picks_1080_video_only() {
        let f = fixtures();
        let sel = FormatSelector::new(&f);
        let chosen = sel.best_video().unwrap();
        assert_eq!(chosen.itag, Some(248));
    }

    #[test]
    fn best_audio_picks_highest_bitrate_audio_only() {
        let f = fixtures();
        let sel = FormatSelector::new(&f);
        let chosen = sel.best_audio().unwrap();
        assert_eq!(chosen.itag, Some(251));
    }

    #[test]
    fn best_video_audio_pair_returns_video_and_audio() {
        let f = fixtures();
        let sel = FormatSelector::new(&f);
        let (video, audio) = sel.best_video_audio().unwrap();
        assert_eq!(video.itag, Some(248));
        assert_eq!(audio.itag, Some(251));
    }

    #[test]
    fn filter_by_max_height() {
        let f = fixtures();
        let chosen = FormatSelector::new(&f)
            .max_height(720)
            .best_video()
            .unwrap();
        assert_eq!(chosen.itag, Some(22));
    }

    #[test]
    fn worst_picks_smallest_video_not_audio() {
        // The mixed set has 1080/720/360 videos and two audio-only formats.
        // worst() must return the smallest VIDEO (360p, itag 18), per its doc,
        // not the lowest-bitrate audio-only format (itag 140).
        let f = fixtures();
        let sel = FormatSelector::new(&f);
        let chosen = sel.worst().unwrap();
        assert_eq!(chosen.itag, Some(18));
    }

    #[test]
    fn worst_falls_back_to_audio_when_no_video() {
        let f = fixtures();
        let audio_only: Vec<Format> = f
            .into_iter()
            .filter(|fmt| matches!(fmt.kind(), FormatKind::AudioOnly))
            .collect();
        let sel = FormatSelector::new(&audio_only);
        // Lowest-bitrate audio is itag 140 (128k).
        assert_eq!(sel.worst().unwrap().itag, Some(140));
    }

    #[test]
    fn by_itag_found_and_missing() {
        let f = fixtures();
        let sel = FormatSelector::new(&f);
        assert_eq!(sel.by_itag(22).unwrap().itag, Some(22));
        assert!(matches!(
            sel.by_itag(9999),
            Err(crate::error::Error::FormatNotFound(_))
        ));
    }

    #[test]
    fn empty_formats_is_format_not_found() {
        let f: Vec<Format> = vec![];
        let sel = FormatSelector::new(&f);
        assert!(matches!(
            sel.best_progressive(),
            Err(crate::error::Error::FormatNotFound(_))
        ));
        assert!(matches!(
            sel.best_video(),
            Err(crate::error::Error::FormatNotFound(_))
        ));
        assert!(matches!(
            sel.best_audio(),
            Err(crate::error::Error::FormatNotFound(_))
        ));
        assert!(matches!(
            sel.worst(),
            Err(crate::error::Error::FormatNotFound(_))
        ));
        assert!(matches!(
            sel.best_video_audio(),
            Err(crate::error::Error::FormatNotFound(_))
        ));
    }
}
