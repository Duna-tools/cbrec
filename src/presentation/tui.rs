//! Renders a read-only tag discovery screen in an alternate terminal buffer.
//!
//! Data loading stays outside this module. It owns only terminal lifecycle,
//! keyboard navigation, and rendering.

use crate::presentation::output::sanitize_external_text;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use std::io::{self, IsTerminal};
use std::time::Duration;

/// Sanitized public room data ready for terminal rendering.
pub(crate) struct TuiRoom {
    username: String,
    viewers: u64,
    show: String,
    subject: String,
}

impl TuiRoom {
    /// Builds a room row while removing terminal control characters.
    pub(crate) fn new(username: String, viewers: u64, show: String, subject: String) -> Self {
        Self {
            username,
            viewers,
            show: sanitize_external_text(&show),
            subject: sanitize_external_text(&subject),
        }
    }
}

struct DiscoveryTui {
    tag: String,
    rooms: Vec<TuiRoom>,
    selected: usize,
}

impl DiscoveryTui {
    fn move_down(&mut self) {
        if self.selected + 1 < self.rooms.len() {
            self.selected += 1;
        }
    }

    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn should_quit(key: &KeyEvent) -> bool {
        matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
            || key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
    }
}

/// Runs the read-only discovery screen and restores the terminal on exit.
pub(crate) fn run_discovery_tui(tag: String, rooms: Vec<TuiRoom>) -> anyhow::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        anyhow::bail!("La TUI requiere una terminal interactiva");
    }

    let mut app = DiscoveryTui {
        tag,
        rooms,
        selected: 0,
    };
    ratatui::run(|terminal| -> io::Result<()> {
        loop {
            terminal.draw(|frame| render(frame, &app))?;
            if !event::poll(Duration::from_millis(250))? {
                continue;
            }
            let Event::Key(key) = event::read()? else {
                continue;
            };
            if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                continue;
            }
            if DiscoveryTui::should_quit(&key) {
                return Ok(());
            }
            match key.code {
                KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                KeyCode::Home => app.selected = 0,
                KeyCode::End if !app.rooms.is_empty() => app.selected = app.rooms.len() - 1,
                _ => {}
            }
        }
    })?;
    Ok(())
}

fn render(frame: &mut Frame, app: &DiscoveryTui) {
    let [header, rooms_area, detail, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(5),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let title = Paragraph::new(format!("#{} · {} modelo(s)", app.tag, app.rooms.len()))
        .block(Block::default().title("cbrec").borders(Borders::ALL));
    frame.render_widget(title, header);

    let items = app.rooms.iter().map(|room| {
        ListItem::new(format!(
            "{}  ·  {} espectadores  ·  {}",
            room.username, room.viewers, room.show
        ))
    });
    let list = List::new(items)
        .block(Block::default().title("Modelos").borders(Borders::ALL))
        .highlight_symbol("> ")
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    let mut list_state =
        ListState::default().with_selected((!app.rooms.is_empty()).then_some(app.selected));
    frame.render_stateful_widget(list, rooms_area, &mut list_state);

    let subject = app
        .rooms
        .get(app.selected)
        .map(|room| room.subject.as_str())
        .unwrap_or("Sin resultados");
    frame.render_widget(
        Paragraph::new(subject)
            .wrap(Wrap { trim: true })
            .block(Block::default().title("Descripción").borders(Borders::ALL)),
        detail,
    );
    frame.render_widget(
        Paragraph::new("↑/↓ o j/k: navegar · q/Esc: salir")
            .style(Style::default().fg(Color::DarkGray)),
        footer,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn navigation_stays_inside_room_bounds() {
        let mut app = DiscoveryTui {
            tag: "gaming".to_string(),
            rooms: vec![
                TuiRoom::new("alice".into(), 1, "public".into(), "one".into()),
                TuiRoom::new("bob".into(), 2, "public".into(), "two".into()),
            ],
            selected: 0,
        };

        app.move_up();
        assert_eq!(app.selected, 0);
        app.move_down();
        app.move_down();
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn room_text_is_sanitized_before_rendering() {
        let room = TuiRoom::new(
            "alice".into(),
            1,
            "public\n".into(),
            "hello\u{1b}[31m".into(),
        );

        assert_eq!(room.show, "public ");
        assert_eq!(room.subject, "hello [31m");
    }

    #[test]
    fn render_shows_tag_rooms_and_selected_subject() {
        let app = DiscoveryTui {
            tag: "gaming".to_string(),
            rooms: vec![TuiRoom::new(
                "alice".into(),
                42,
                "public".into(),
                "hello".into(),
            )],
            selected: 0,
        };
        let mut terminal = Terminal::new(TestBackend::new(60, 16)).unwrap();

        terminal.draw(|frame| render(frame, &app)).unwrap();

        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("#gaming"));
        assert!(text.contains("alice"));
        assert!(text.contains("42 espectadores"));
        assert!(text.contains("hello"));
    }
}
