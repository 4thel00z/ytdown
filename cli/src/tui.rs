//! Ratatui renderer/event loop over the pure `picker` state machine.
//! Draws on stderr so stdout stays clean for piping.

use std::io;

use anyhow::Context;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use crate::picker::{Key, Mode, Outcome, Picker, PickerRow};

/// The user's choice.
pub enum Pick {
    /// Index into the original formats slice.
    Single(usize),
    /// Video-only index to merge with best audio.
    Merged(usize),
}

/// Run the picker; `None` means the user quit without choosing.
pub fn pick(title: &str, rows: Vec<PickerRow>) -> anyhow::Result<Option<Pick>> {
    let mut picker = Picker::new(rows);
    enable_raw_mode().context("failed to enable raw terminal mode")?;
    crossterm::execute!(io::stderr(), EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stderr());
    let mut terminal = Terminal::new(backend)?;
    let result = run_loop(&mut terminal, &mut picker, title);
    let _ = disable_raw_mode();
    let _ = crossterm::execute!(io::stderr(), LeaveAlternateScreen);
    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stderr>>,
    picker: &mut Picker,
    title: &str,
) -> anyhow::Result<Option<Pick>> {
    loop {
        terminal.draw(|f| draw(f, picker, title))?;
        let Event::Key(k) = event::read()? else {
            continue;
        };
        if k.kind != KeyEventKind::Press {
            continue;
        }
        let key = match k.code {
            KeyCode::Up => Key::Up,
            KeyCode::Down => Key::Down,
            KeyCode::Enter => Key::Enter,
            KeyCode::Esc => Key::Esc,
            KeyCode::Backspace => Key::Backspace,
            KeyCode::Char('/') if picker.mode != Mode::Filter => Key::Slash,
            KeyCode::Char(c) => Key::Char(c),
            _ => continue,
        };
        match picker.on_key(key) {
            Outcome::Continue => {}
            Outcome::Cancel => return Ok(None),
            Outcome::Pick(i) => return Ok(Some(Pick::Single(i))),
            Outcome::PickMerged(i) => return Ok(Some(Pick::Merged(i))),
        }
    }
}

fn draw(f: &mut Frame, picker: &Picker, title: &str) {
    let chunks = Layout::vertical([
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(f.area());
    let items: Vec<ListItem> = picker
        .visible()
        .iter()
        .map(|r| ListItem::new(r.label.clone()))
        .collect();
    let mut state = ListState::default();
    state.select(Some(picker.cursor));
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title.to_string()),
        )
        .highlight_symbol("> ");
    f.render_stateful_widget(list, chunks[0], &mut state);
    let status = match picker.mode {
        Mode::Filter => format!("filter: {}_", picker.filter),
        Mode::ConfirmPair { .. } => {
            "video-only format: pair with best audio and merge? [y/n]".to_string()
        }
        Mode::Browse if !picker.filter.is_empty() => format!("filter: {}", picker.filter),
        Mode::Browse => String::new(),
    };
    f.render_widget(Paragraph::new(status), chunks[1]);
    f.render_widget(
        Paragraph::new("[enter] select  [/] filter  [q] quit"),
        chunks[2],
    );
}
