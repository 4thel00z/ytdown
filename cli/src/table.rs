//! Plain-text table rendering for `formats` and `search`, plus small
//! humanizing helpers shared with the picker.

use ytdown::{Container, Entry, Format, FormatKind};

/// `92.3 MiB`-style size string.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{:.1} {}", v, UNITS[unit])
    }
}

/// `M:SS` duration, or `-` when unknown.
#[allow(dead_code)] // used in search (Task 10) and picker (Task 13)
pub fn duration_str(d: Option<std::time::Duration>) -> String {
    match d {
        Some(d) => {
            let s = d.as_secs();
            format!("{}:{:02}", s / 60, s % 60)
        }
        None => "-".into(),
    }
}

fn container_str(c: Option<&Container>) -> String {
    match c {
        Some(Container::Mp4) => "mp4".into(),
        Some(Container::WebM) => "webm".into(),
        Some(Container::M4a) => "m4a".into(),
        Some(Container::Weba) => "weba".into(),
        Some(Container::Other(s)) => s.clone(),
        _ => "?".into(),
    }
}

/// One row of the formats table; also reused as the picker's row label.
pub fn format_row(f: &Format) -> Vec<String> {
    let kind = match f.kind() {
        FormatKind::Progressive => "progressive",
        FormatKind::VideoOnly => "video",
        FormatKind::AudioOnly => "audio",
        _ => "unknown",
    };
    let res = f
        .video
        .as_ref()
        .and_then(|v| v.height)
        .map(|h| format!("{h}p"))
        .unwrap_or_else(|| {
            if f.audio.is_some() {
                "audio".into()
            } else {
                "?".into()
            }
        });
    let fps = f
        .video
        .as_ref()
        .and_then(|v| v.fps)
        .map(|x| format!("{x:.0}"))
        .unwrap_or_default();
    let codecs = match (&f.video, &f.audio) {
        (Some(v), Some(a)) => format!("{}+{}", v.codec, a.codec),
        (Some(v), None) => v.codec.clone(),
        (None, Some(a)) => a.codec.clone(),
        (None, None) => String::new(),
    };
    vec![
        f.itag.map(|i| i.to_string()).unwrap_or_else(|| "-".into()),
        kind.to_string(),
        container_str(f.container.as_ref()),
        res,
        fps,
        codecs,
        f.filesize.map(human_size).unwrap_or_default(),
    ]
}

/// Render the `formats` table.
pub fn formats_table(formats: &[Format]) -> String {
    let header = ["ITAG", "KIND", "CONTAINER", "RES", "FPS", "CODECS", "SIZE"];
    let rows: Vec<Vec<String>> = formats.iter().map(format_row).collect();
    render(&header, &rows)
}

/// Render the `search` results table.
#[allow(dead_code)] // used in search (Task 10)
pub fn entries_table(entries: &[Entry]) -> String {
    let header = ["ID", "DURATION", "TITLE", "URL"];
    let rows: Vec<Vec<String>> = entries
        .iter()
        .map(|e| {
            vec![
                e.id.clone(),
                duration_str(e.duration),
                e.title.clone().unwrap_or_else(|| "-".into()),
                e.url.clone(),
            ]
        })
        .collect();
    render(&header, &rows)
}

fn render(header: &[&str], rows: &[Vec<String>]) -> String {
    let mut widths: Vec<usize> = header.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.len());
        }
    }
    let line = |cells: &[String]| -> String {
        cells
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{:<w$}", c, w = widths[i]))
            .collect::<Vec<_>>()
            .join("  ")
            .trim_end()
            .to_string()
    };
    let header: Vec<String> = header.iter().map(|h| h.to_string()).collect();
    let mut out = line(&header);
    for row in rows {
        out.push('\n');
        out.push_str(&line(row));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ytdown::{AudioStream, VideoStream};

    #[test]
    fn human_size_picks_units() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(96_468_992), "92.0 MiB");
    }

    #[test]
    fn duration_renders_minutes_seconds() {
        assert_eq!(
            duration_str(Some(std::time::Duration::from_secs(212))),
            "3:32"
        );
        assert_eq!(duration_str(None), "-");
    }

    #[test]
    fn formats_table_aligns_columns() {
        let mut f = Format::default();
        f.itag = Some(22);
        f.container = Some(Container::Mp4);
        let mut v = VideoStream::default();
        v.height = Some(720);
        v.codec = "avc1".into();
        f.video = Some(v);
        let mut a = AudioStream::default();
        a.codec = "mp4a".into();
        f.audio = Some(a);
        f.filesize = Some(45 * 1024 * 1024);

        let table = formats_table(&[f]);
        let lines: Vec<&str> = table.lines().collect();
        assert!(lines[0].starts_with("ITAG"));
        assert!(lines[1].contains("progressive"));
        assert!(lines[1].contains("720p"));
        assert!(lines[1].contains("avc1+mp4a"));
        assert!(lines[1].contains("45.0 MiB"));
    }

    #[test]
    fn entries_table_handles_missing_title() {
        let mut e = Entry::default();
        e.id = "x1".into();
        e.url = "https://example.com/x1".into();
        let table = entries_table(&[e]);
        assert!(table.lines().nth(1).unwrap().contains("-"));
    }
}
