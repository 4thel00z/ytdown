//! Parsing and resolving the `-f` format-selection flag.

use ytdown::{Container, Format, FormatSelector};

/// An argument/usage error: `main` maps it to exit code 2.
#[derive(Debug)]
pub struct UsageError(pub String);

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UsageError {}

/// Parsed value of `-f`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatSpec {
    /// Best progressive (muxed A+V) format.
    Best,
    /// Best video-only format.
    BestVideo,
    /// Best audio-only format.
    BestAudio,
    /// A specific format by itag.
    Itag(u32),
    /// Video itag + audio itag, merged via ffmpeg.
    Merge(u32, u32),
}

impl std::str::FromStr for FormatSpec {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "best" => Ok(Self::Best),
            "bestvideo" => Ok(Self::BestVideo),
            "bestaudio" => Ok(Self::BestAudio),
            _ => {
                if let Some((v, a)) = s.split_once('+') {
                    let v = v.trim().parse().map_err(|_| bad(s))?;
                    let a = a.trim().parse().map_err(|_| bad(s))?;
                    Ok(Self::Merge(v, a))
                } else {
                    s.trim().parse().map(Self::Itag).map_err(|_| bad(s))
                }
            }
        }
    }
}

fn bad(s: &str) -> String {
    format!("invalid format selector {s:?}: expected best, bestvideo, bestaudio, an itag, or VIDEO_ITAG+AUDIO_ITAG")
}

/// What to download: one format, or a video+audio pair to merge.
#[derive(Debug)]
pub enum Selection<'a> {
    /// A single (progressive or stream-only) format.
    Single(&'a Format),
    /// Split streams merged with ffmpeg after download.
    Merged {
        /// Video-only stream.
        video: &'a Format,
        /// Audio-only stream.
        audio: &'a Format,
    },
}

/// Resolve a spec (or the default, when `-f` was omitted) against `formats`.
///
/// `--max-height`/`--container` filters apply to keyword specs; combining
/// them with explicit itags is a [`UsageError`].
pub fn resolve<'a>(
    spec: Option<&FormatSpec>,
    formats: &'a [Format],
    max_height: Option<u32>,
    container: Option<&Container>,
    ffmpeg_available: bool,
) -> anyhow::Result<Selection<'a>> {
    let has_filters = max_height.is_some() || container.is_some();
    let filtered = || {
        let mut s = FormatSelector::new(formats);
        if let Some(h) = max_height {
            s = s.max_height(h);
        }
        if let Some(c) = container {
            s = s.container(c);
        }
        s
    };
    match spec {
        Some(FormatSpec::Itag(i)) => {
            if has_filters {
                return Err(itag_filter_conflict().into());
            }
            Ok(Selection::Single(FormatSelector::new(formats).by_itag(*i)?))
        }
        Some(FormatSpec::Merge(v, a)) => {
            if has_filters {
                return Err(itag_filter_conflict().into());
            }
            let sel = FormatSelector::new(formats);
            Ok(Selection::Merged {
                video: sel.by_itag(*v)?,
                audio: sel.by_itag(*a)?,
            })
        }
        Some(FormatSpec::Best) => Ok(Selection::Single(filtered().best_progressive()?)),
        Some(FormatSpec::BestVideo) => Ok(Selection::Single(filtered().best_video()?)),
        Some(FormatSpec::BestAudio) => Ok(Selection::Single(filtered().best_audio()?)),
        None => {
            if ffmpeg_available {
                if let Ok((video, audio)) = filtered().best_video_audio() {
                    return Ok(Selection::Merged { video, audio });
                }
            }
            Ok(Selection::Single(filtered().best_progressive()?))
        }
    }
}

/// The shared filters-vs-itag conflict error.
pub fn itag_filter_conflict() -> UsageError {
    UsageError("--max-height/--container cannot be combined with explicit itags in -f".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ytdown::{AudioStream, VideoStream};

    /// Non-exhaustive lib structs: build via Default + field mutation.
    fn progressive(itag: u32, height: u32) -> Format {
        let mut f = Format::default();
        f.itag = Some(itag);
        let mut v = VideoStream::default();
        v.height = Some(height);
        f.video = Some(v);
        f.audio = Some(AudioStream::default());
        f
    }

    fn video_only(itag: u32, height: u32) -> Format {
        let mut f = progressive(itag, height);
        f.audio = None;
        f
    }

    fn audio_only(itag: u32) -> Format {
        let mut f = Format::default();
        f.itag = Some(itag);
        f.audio = Some(AudioStream::default());
        f
    }

    fn itag(s: &Selection<'_>) -> u32 {
        match s {
            Selection::Single(f) => f.itag.unwrap(),
            Selection::Merged { video, .. } => video.itag.unwrap(),
        }
    }

    #[test]
    fn parses_keywords_itags_and_merge_pairs() {
        assert_eq!("best".parse::<FormatSpec>().unwrap(), FormatSpec::Best);
        assert_eq!(
            "bestaudio".parse::<FormatSpec>().unwrap(),
            FormatSpec::BestAudio
        );
        assert_eq!("22".parse::<FormatSpec>().unwrap(), FormatSpec::Itag(22));
        assert_eq!(
            "137+140".parse::<FormatSpec>().unwrap(),
            FormatSpec::Merge(137, 140)
        );
        assert!("bogus".parse::<FormatSpec>().is_err());
        assert!("1+2+3".parse::<FormatSpec>().is_err());
    }

    #[test]
    fn itag_spec_selects_exact_format() {
        let formats = vec![progressive(22, 720), progressive(18, 360)];
        let sel = resolve(Some(&FormatSpec::Itag(18)), &formats, None, None, false).unwrap();
        assert_eq!(itag(&sel), 18);
    }

    #[test]
    fn merge_spec_selects_pair() {
        let formats = vec![video_only(137, 1080), audio_only(140)];
        let sel = resolve(
            Some(&FormatSpec::Merge(137, 140)),
            &formats,
            None,
            None,
            true,
        )
        .unwrap();
        match sel {
            Selection::Merged { video, audio } => {
                assert_eq!(video.itag, Some(137));
                assert_eq!(audio.itag, Some(140));
            }
            other => panic!("expected merged, got {other:?}"),
        }
    }

    #[test]
    fn filters_with_itag_are_a_usage_error() {
        let formats = vec![progressive(22, 720)];
        let err = resolve(
            Some(&FormatSpec::Itag(22)),
            &formats,
            Some(720),
            None,
            false,
        )
        .unwrap_err();
        assert!(err.downcast_ref::<UsageError>().is_some());
    }

    #[test]
    fn default_prefers_merge_with_ffmpeg_else_progressive() {
        let formats = vec![progressive(22, 720), video_only(137, 1080), audio_only(140)];
        let with = resolve(None, &formats, None, None, true).unwrap();
        assert!(matches!(with, Selection::Merged { .. }));
        let without = resolve(None, &formats, None, None, false).unwrap();
        assert_eq!(itag(&without), 22);
    }

    #[test]
    fn max_height_filters_keyword_selection() {
        let formats = vec![progressive(22, 720), progressive(37, 1080)];
        let sel = resolve(Some(&FormatSpec::Best), &formats, Some(720), None, false).unwrap();
        assert_eq!(itag(&sel), 22);
    }
}
