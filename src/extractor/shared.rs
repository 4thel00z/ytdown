//! Helpers shared by several site extractors: epoch-to-date conversion and a
//! lightweight parser for flat DASH manifests (Reddit, Instagram).

use crate::types::{AudioStream, Container, Format, VideoStream};

/// Convert a Unix-epoch timestamp to yt-dlp's `YYYYMMDD` (UTC).
///
/// Uses the days-to-civil algorithm (Howard Hinnant) to avoid a date-time
/// dependency for a single conversion.
pub(crate) fn upload_date_from_epoch(epoch: f64) -> Option<String> {
    if !epoch.is_finite() || epoch < 0.0 {
        return None;
    }
    let days = (epoch as i64).div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    Some(format!("{y:04}{m:02}{d:02}"))
}

/// Parse a flat DASH manifest into formats, resolving relative `BaseURL`s
/// against `base` (the manifest's directory URL, with trailing slash).
///
/// Handles the manifests Reddit and Instagram serve: flat lists of single-file
/// MP4 representations — no DASH segmenting is involved and each `BaseURL`
/// downloads directly.
pub(crate) fn formats_from_mpd(xml: &str, base: &str) -> Vec<Format> {
    let rep_re = regex::Regex::new(r"(?s)<Representation\b([^>]*)>(.*?)</Representation>")
        .expect("static regex");
    let attr_re = regex::Regex::new(r#"([A-Za-z][\w:]*)\s*=\s*"([^"]*)""#).expect("static regex");
    let base_url_re =
        regex::Regex::new(r"<BaseURL>\s*([^<\s]+)\s*</BaseURL>").expect("static regex");

    let mut out = Vec::new();
    for cap in rep_re.captures_iter(xml) {
        let attrs: std::collections::HashMap<&str, &str> = attr_re
            .captures_iter(cap.get(1).map_or("", |m| m.as_str()))
            .map(|c| {
                (
                    c.get(1).map_or("", |m| m.as_str()),
                    c.get(2).map_or("", |m| m.as_str()),
                )
            })
            .collect();
        let Some(base_url) = base_url_re
            .captures(cap.get(2).map_or("", |m| m.as_str()))
            .and_then(|c| c.get(1))
            .map(|m| m.as_str())
        else {
            continue;
        };

        let url = if base_url.starts_with("http://") || base_url.starts_with("https://") {
            xml_unescape(base_url)
        } else {
            format!("{base}{}", xml_unescape(base_url))
        };

        let mime = attrs.get("mimeType").copied().unwrap_or_else(|| {
            // Old manifests only carry mimeType on the AdaptationSet; infer the
            // stream kind from the audio file naming.
            let file = base_url.rsplit('/').next().unwrap_or(base_url);
            if file.to_ascii_lowercase().contains("audio") {
                "audio/mp4"
            } else {
                "video/mp4"
            }
        });
        let codec = attrs.get("codecs").copied().unwrap_or_default().to_string();
        let bandwidth = attrs.get("bandwidth").and_then(|s| s.parse::<u64>().ok());

        let (container, video, audio) = if mime.starts_with("audio/") {
            (
                Some(Container::M4a),
                None,
                Some(AudioStream {
                    codec,
                    bitrate: bandwidth,
                    sample_rate: attrs
                        .get("audioSamplingRate")
                        .and_then(|s| s.parse::<u32>().ok()),
                    channels: None,
                }),
            )
        } else {
            (
                Some(Container::Mp4),
                Some(VideoStream {
                    width: attrs.get("width").and_then(|s| s.parse::<u32>().ok()),
                    height: attrs.get("height").and_then(|s| s.parse::<u32>().ok()),
                    fps: attrs.get("frameRate").and_then(parse_frame_rate),
                    codec,
                }),
                None,
            )
        };

        out.push(Format {
            itag: None,
            url,
            mime_type: Some(mime.to_string()),
            container,
            video,
            audio,
            filesize: None,
            bitrate: bandwidth,
            http_headers: Vec::new(),
        });
    }
    out
}

/// Parse a DASH `frameRate`: either a plain number or a `num/den` fraction.
fn parse_frame_rate(s: &&str) -> Option<f64> {
    match s.split_once('/') {
        Some((num, den)) => {
            let num: f64 = num.parse().ok()?;
            let den: f64 = den.parse().ok()?;
            (den != 0.0).then(|| num / den)
        }
        None => s.parse().ok(),
    }
}

/// Undo the XML escaping of a URL embedded in a manifest attribute or text
/// node (Instagram inlines fully-escaped absolute URLs).
fn xml_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_converts_to_yyyymmdd() {
        assert_eq!(
            upload_date_from_epoch(1638316800.0).as_deref(),
            Some("20211201")
        );
        assert_eq!(upload_date_from_epoch(0.0).as_deref(), Some("19700101"));
        assert_eq!(upload_date_from_epoch(f64::NAN), None);
    }

    const MPD: &str = r#"<?xml version="1.0"?>
<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" mediaPresentationDuration="PT14S">
  <Period>
    <AdaptationSet contentType="video">
      <Representation mimeType="video/mp4" codecs="avc1.4D401F" bandwidth="2500000" width="1280" height="720" frameRate="30">
        <BaseURL>DASH_720.mp4</BaseURL>
      </Representation>
      <Representation mimeType="video/mp4" codecs="avc1.4D401E" bandwidth="1200000" width="854" height="480" frameRate="30000/1001">
        <BaseURL>DASH_480.mp4</BaseURL>
      </Representation>
    </AdaptationSet>
    <AdaptationSet contentType="audio">
      <Representation mimeType="audio/mp4" codecs="mp4a.40.2" bandwidth="128000" audioSamplingRate="48000">
        <BaseURL>DASH_AUDIO_128.mp4</BaseURL>
      </Representation>
    </AdaptationSet>
  </Period>
</MPD>"#;

    #[test]
    fn mpd_parses_video_and_audio_representations() {
        let formats = formats_from_mpd(MPD, "https://v.redd.it/media123/");
        assert_eq!(formats.len(), 3);

        let v720 = &formats[0];
        assert_eq!(v720.url, "https://v.redd.it/media123/DASH_720.mp4");
        assert_eq!(v720.container, Some(Container::Mp4));
        assert_eq!(v720.bitrate, Some(2_500_000));
        let video = v720.video.as_ref().expect("video stream");
        assert_eq!(video.width, Some(1280));
        assert_eq!(video.height, Some(720));
        assert_eq!(video.codec, "avc1.4D401F");
        assert_eq!(video.fps, Some(30.0));
        assert!(v720.audio.is_none());

        // Fractional frame rates ("30000/1001") must parse.
        let fps = formats[1].video.as_ref().unwrap().fps.unwrap();
        assert!((fps - 29.97).abs() < 0.01, "fps: {fps}");

        let a = &formats[2];
        assert_eq!(a.url, "https://v.redd.it/media123/DASH_AUDIO_128.mp4");
        assert_eq!(a.container, Some(Container::M4a));
        assert!(a.video.is_none());
        let audio = a.audio.as_ref().expect("audio stream");
        assert_eq!(audio.codec, "mp4a.40.2");
        assert_eq!(audio.sample_rate, Some(48_000));
        assert_eq!(audio.bitrate, Some(128_000));
    }

    #[test]
    fn mpd_infers_kind_from_base_url_without_mime() {
        // Old manifests carry mimeType on the AdaptationSet only; the audio
        // representation is recognized by its BaseURL.
        let xml = r#"<MPD><Period>
          <AdaptationSet contentType="video">
            <Representation bandwidth="1000000" width="640" height="360">
              <BaseURL>DASH_360.mp4</BaseURL>
            </Representation>
          </AdaptationSet>
          <AdaptationSet contentType="audio">
            <Representation bandwidth="64000">
              <BaseURL>DASH_audio.mp4</BaseURL>
            </Representation>
          </AdaptationSet>
        </Period></MPD>"#;
        let formats = formats_from_mpd(xml, "https://v.redd.it/old1/");
        assert_eq!(formats.len(), 2);
        assert!(formats[0].video.is_some(), "DASH_360 is video");
        assert!(formats[1].audio.is_some(), "DASH_audio is audio");
        assert!(formats[1].video.is_none());
    }

    #[test]
    fn mpd_unescapes_absolute_base_urls() {
        // Instagram manifests inline fully-escaped absolute URLs.
        let xml = r#"<MPD><Period><AdaptationSet contentType="video">
          <Representation mimeType="video/mp4" codecs="avc1" bandwidth="1000">
            <BaseURL>https://cdn.example/v.mp4?a=1&amp;b=2</BaseURL>
          </Representation>
        </AdaptationSet></Period></MPD>"#;
        let formats = formats_from_mpd(xml, "https://unused/");
        assert_eq!(formats.len(), 1);
        assert_eq!(formats[0].url, "https://cdn.example/v.mp4?a=1&b=2");
    }
}
