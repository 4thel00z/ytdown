//! Pure state machine behind the interactive format picker. No terminal
//! handles here — `tui.rs` renders it and feeds it keys.

use ytdown::{Container, Format, FormatKind};

use crate::table;

/// One selectable row.
pub struct PickerRow {
    /// Index into the original `formats` slice.
    pub index: usize,
    /// Pre-rendered single-line label.
    pub label: String,
    /// Whether this format has video but no audio (offers pairing).
    pub video_only: bool,
}

/// Build picker rows, honouring the same `--max-height`/`--container`
/// filters as non-interactive selection.
pub fn rows(
    formats: &[Format],
    max_height: Option<u32>,
    container: Option<&Container>,
) -> Vec<PickerRow> {
    formats
        .iter()
        .enumerate()
        .filter(|(_, f)| {
            max_height.is_none_or(|h| {
                f.video
                    .as_ref()
                    .and_then(|v| v.height)
                    .is_none_or(|fh| fh <= h)
            })
        })
        .filter(|(_, f)| container.is_none_or(|c| f.container.as_ref() == Some(c)))
        .map(|(i, f)| PickerRow {
            index: i,
            label: table::format_row(f).join("  "),
            video_only: matches!(f.kind(), FormatKind::VideoOnly),
        })
        .collect()
}

/// Abstract key events (mapped from crossterm in `tui.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Enter,
    Esc,
    Slash,
    Backspace,
    Char(char),
}

/// What a key press produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Keep going.
    Continue,
    /// User quit without choosing.
    Cancel,
    /// Download the format at this index (into the original slice).
    Pick(usize),
    /// Download this video-only index merged with best audio.
    PickMerged(usize),
}

/// Picker interaction mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Navigating the list.
    Browse,
    /// Editing the filter string.
    Filter,
    /// Confirming audio pairing for a video-only pick.
    ConfirmPair {
        /// The pending video-only format index.
        index: usize,
    },
}

/// Picker state.
pub struct Picker {
    rows: Vec<PickerRow>,
    /// Current filter text.
    pub filter: String,
    /// Cursor position within the *visible* rows.
    pub cursor: usize,
    /// Current mode.
    pub mode: Mode,
}

impl Picker {
    /// Start browsing `rows`.
    pub fn new(rows: Vec<PickerRow>) -> Self {
        Self {
            rows,
            filter: String::new(),
            cursor: 0,
            mode: Mode::Browse,
        }
    }

    /// Rows matching the current filter (case-insensitive substring).
    pub fn visible(&self) -> Vec<&PickerRow> {
        let needle = self.filter.to_lowercase();
        self.rows
            .iter()
            .filter(|r| needle.is_empty() || r.label.to_lowercase().contains(&needle))
            .collect()
    }

    /// Advance the state machine by one key press.
    pub fn on_key(&mut self, key: Key) -> Outcome {
        match (self.mode, key) {
            (Mode::Browse, Key::Up) => {
                self.cursor = self.cursor.saturating_sub(1);
                Outcome::Continue
            }
            (Mode::Browse, Key::Down) => {
                let max = self.visible().len().saturating_sub(1);
                self.cursor = (self.cursor + 1).min(max);
                Outcome::Continue
            }
            (Mode::Browse, Key::Slash) => {
                self.mode = Mode::Filter;
                Outcome::Continue
            }
            (Mode::Browse, Key::Enter) => match self.visible().get(self.cursor) {
                Some(row) if row.video_only => {
                    let index = row.index;
                    self.mode = Mode::ConfirmPair { index };
                    Outcome::Continue
                }
                Some(row) => Outcome::Pick(row.index),
                None => Outcome::Continue,
            },
            (Mode::Browse, Key::Esc) | (Mode::Browse, Key::Char('q')) => Outcome::Cancel,
            (Mode::Browse, _) => Outcome::Continue,

            (Mode::Filter, Key::Char(c)) => {
                self.filter.push(c);
                self.cursor = 0;
                Outcome::Continue
            }
            (Mode::Filter, Key::Backspace) => {
                self.filter.pop();
                Outcome::Continue
            }
            (Mode::Filter, Key::Enter) | (Mode::Filter, Key::Esc) => {
                self.mode = Mode::Browse;
                Outcome::Continue
            }
            (Mode::Filter, _) => Outcome::Continue,

            (Mode::ConfirmPair { index }, Key::Char('y'))
            | (Mode::ConfirmPair { index }, Key::Enter) => Outcome::PickMerged(index),
            (Mode::ConfirmPair { .. }, Key::Char('n')) | (Mode::ConfirmPair { .. }, Key::Esc) => {
                self.mode = Mode::Browse;
                Outcome::Continue
            }
            (Mode::ConfirmPair { .. }, _) => Outcome::Continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ytdown::{AudioStream, VideoStream};

    fn fixtures() -> Vec<Format> {
        let mut prog = Format::default();
        prog.itag = Some(22);
        let mut v = VideoStream::default();
        v.height = Some(720);
        v.codec = "avc1".into();
        prog.video = Some(v);
        prog.audio = Some(AudioStream::default());

        let mut vid = Format::default();
        vid.itag = Some(137);
        let mut v2 = VideoStream::default();
        v2.height = Some(1080);
        v2.codec = "avc1".into();
        vid.video = Some(v2);

        let mut aud = Format::default();
        aud.itag = Some(140);
        let mut a = AudioStream::default();
        a.codec = "mp4a".into();
        aud.audio = Some(a);

        vec![prog, vid, aud]
    }

    #[test]
    fn rows_honour_max_height_filter() {
        let formats = fixtures();
        let rows = rows(&formats, Some(720), None);
        // The 1080p video-only format is filtered out; audio (no height) stays.
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].index, 0);
        assert_eq!(rows[1].index, 2);
    }

    #[test]
    fn filter_narrows_and_enter_picks_original_index() {
        let formats = fixtures();
        let mut p = Picker::new(rows(&formats, None, None));
        assert_eq!(p.on_key(Key::Slash), Outcome::Continue);
        for c in "mp4a".chars() {
            p.on_key(Key::Char(c));
        }
        p.on_key(Key::Enter); // leave filter mode
        assert_eq!(p.visible().len(), 1);
        assert_eq!(p.on_key(Key::Enter), Outcome::Pick(2));
    }

    #[test]
    fn video_only_pick_asks_for_pairing() {
        let formats = fixtures();
        let mut p = Picker::new(rows(&formats, None, None));
        p.on_key(Key::Down); // onto the 137 video-only row
        assert_eq!(p.on_key(Key::Enter), Outcome::Continue);
        assert!(matches!(p.mode, Mode::ConfirmPair { index: 1 }));
        assert_eq!(p.on_key(Key::Char('y')), Outcome::PickMerged(1));
    }

    #[test]
    fn confirm_pair_can_be_declined() {
        let formats = fixtures();
        let mut p = Picker::new(rows(&formats, None, None));
        p.on_key(Key::Down);
        p.on_key(Key::Enter);
        assert_eq!(p.on_key(Key::Char('n')), Outcome::Continue);
        assert_eq!(p.mode, Mode::Browse);
    }

    #[test]
    fn q_cancels_and_cursor_clamps() {
        let formats = fixtures();
        let mut p = Picker::new(rows(&formats, None, None));
        for _ in 0..10 {
            p.on_key(Key::Down);
        }
        assert_eq!(p.cursor, 2);
        assert_eq!(p.on_key(Key::Char('q')), Outcome::Cancel);
    }
}
