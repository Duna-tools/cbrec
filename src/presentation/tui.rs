//! Panel de exploración y grabación; las operaciones externas llegan por canales.

use crate::domain::value_objects::VideoQuality;
use crate::presentation::output::sanitize_external_text;
use crate::presentation::thumbnail::{PreviewRenderer, BACKGROUND};
use image::DynamicImage;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
    KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

pub(crate) struct TuiRoom {
    pub username: String,
    pub viewers: u64,
    pub show: String,
    pub subject: String,
}

impl TuiRoom {
    pub(crate) fn new(username: String, viewers: u64, show: String, subject: String) -> Self {
        Self {
            username,
            viewers,
            show: sanitize_external_text(&show),
            subject: sanitize_external_text(&subject),
        }
    }
}

pub(crate) enum TuiAction {
    Search(String),
    Preview(String),
    RefreshPreview(String),
    OpenRoom(String),
    OpenSite,
    ConnectSession,
    Record(String, VideoQuality),
    Stop(String),
    Quit,
}

pub(crate) enum TuiEvent {
    Rooms(String, Result<Vec<TuiRoom>, String>),
    Preview(String, Result<Arc<DynamicImage>, String>),
    Recording(String, PathBuf),
    Progress(String, u64),
    Finished(String, String),
    SessionUrl(String),
    SessionFinished(bool, String),
    Notice(String),
}

pub(crate) struct TuiInitial {
    pub tag: String,
    pub session_present: bool,
    pub max_recordings: usize,
    pub quality: VideoQuality,
    pub output_folder: PathBuf,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Home,
    Browse,
    Room,
    Recordings,
}

struct DiscoveryTui {
    screen: Screen,
    query: String,
    tag: String,
    rooms: Vec<TuiRoom>,
    selected: usize,
    editing: bool,
    loading: bool,
    notice: String,
    session_present: bool,
    session_url: Option<String>,
    active: BTreeMap<String, (PathBuf, u64)>,
    preview: Option<(String, Arc<DynamicImage>)>,
    preview_renderer: PreviewRenderer,
    preview_pending: bool,
    preview_error: Option<String>,
    quality: VideoQuality,
    focused: bool,
    zoom: u16,
    confirm_quit: bool,
    max_recordings: usize,
    output_folder: PathBuf,
    buttons: Vec<(Rect, KeyCode)>,
    room_rows: Vec<(Rect, usize)>,
}

impl DiscoveryTui {
    fn new(initial: TuiInitial) -> Self {
        Self {
            query: if initial.tag.is_empty() {
                "gaming".into()
            } else {
                initial.tag.clone()
            },
            tag: initial.tag,
            screen: Screen::Home,
            rooms: Vec::new(),
            selected: 0,
            editing: false,
            loading: false,
            notice: "Enter: explorar · /: cambiar tag · l: abrir sitio · a: conectar sesión".into(),
            session_present: initial.session_present,
            session_url: None,
            active: BTreeMap::new(),
            preview: None,
            preview_renderer: PreviewRenderer::new(),
            preview_pending: false,
            preview_error: None,
            quality: initial.quality,
            focused: false,
            zoom: 1,
            confirm_quit: false,
            max_recordings: initial.max_recordings,
            output_folder: initial.output_folder,
            buttons: Vec::new(),
            room_rows: Vec::new(),
        }
    }

    fn selected_model(&self) -> Option<String> {
        if self.screen == Screen::Recordings {
            self.active.keys().nth(self.selected).cloned()
        } else {
            self.rooms
                .get(self.selected)
                .map(|room| room.username.clone())
        }
    }

    fn move_down(&mut self) {
        let count = if self.screen == Screen::Recordings {
            self.active.len()
        } else {
            self.rooms.len()
        };
        if self.selected + 1 < count {
            self.selected += 1;
        }
    }
    fn move_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    fn request_preview(&mut self, refresh: bool) -> Option<TuiAction> {
        if !matches!(self.screen, Screen::Browse | Screen::Room) {
            return None;
        }
        let model = self.selected_model()?;
        self.preview_pending = true;
        self.preview_error = None;
        if self
            .preview
            .as_ref()
            .is_none_or(|(previous, _)| previous != &model)
        {
            self.preview = None;
            self.zoom = 1;
        }
        Some(if refresh {
            TuiAction::RefreshPreview(model)
        } else {
            TuiAction::Preview(model)
        })
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiAction> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return self.request_quit();
        }
        if self.editing {
            match key.code {
                KeyCode::Esc => self.editing = false,
                KeyCode::Enter => {
                    self.editing = false;
                    return self.search();
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.query.clear()
                }
                KeyCode::Backspace => {
                    self.query.pop();
                }
                KeyCode::Char(character)
                    if self.query.len() < 50
                        && (character.is_ascii_alphanumeric()
                            || matches!(character, '_' | '-' | '#')) =>
                {
                    self.query.push(character)
                }
                _ => {}
            }
            return None;
        }
        if key.code != KeyCode::Char('q') {
            self.confirm_quit = false;
        }
        match key.code {
            KeyCode::Char('q') => self.request_quit(),
            KeyCode::Esc => {
                if self.screen == Screen::Room {
                    self.screen = Screen::Browse;
                    self.focused = false;
                } else if matches!(self.screen, Screen::Browse | Screen::Recordings) {
                    self.screen = Screen::Home;
                } else {
                    return self.request_quit();
                }
                None
            }
            KeyCode::Char('g') => {
                self.screen = Screen::Recordings;
                self.selected = 0;
                None
            }
            KeyCode::Char('h') => {
                self.screen = Screen::Home;
                self.focused = false;
                None
            }
            KeyCode::Char('/') => {
                self.editing = true;
                self.screen = Screen::Home;
                None
            }
            KeyCode::Enter if self.screen == Screen::Home => self.search(),
            KeyCode::Enter if self.screen != Screen::Recordings => {
                self.screen = Screen::Room;
                self.request_preview(false)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let previous = self.selected;
                self.move_down();
                if self.selected != previous {
                    self.request_preview(false)
                } else {
                    None
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let previous = self.selected;
                self.move_up();
                if self.selected != previous {
                    self.request_preview(false)
                } else {
                    None
                }
            }
            KeyCode::Home => {
                let previous = self.selected;
                self.selected = 0;
                if self.selected != previous {
                    self.request_preview(false)
                } else {
                    None
                }
            }
            KeyCode::End => {
                let count = if self.screen == Screen::Recordings {
                    self.active.len()
                } else {
                    self.rooms.len()
                };
                let previous = self.selected;
                self.selected = count.saturating_sub(1);
                if self.selected != previous {
                    self.request_preview(false)
                } else {
                    None
                }
            }
            KeyCode::Char('o') if self.screen != Screen::Home => {
                self.selected_model().map(TuiAction::OpenRoom)
            }
            KeyCode::Char('l') => Some(TuiAction::OpenSite),
            KeyCode::Char('a') if self.session_url.is_none() => Some(TuiAction::ConnectSession),
            KeyCode::Char('t') if self.screen != Screen::Home => self.request_preview(true),
            KeyCode::Char('r') if self.screen != Screen::Home => self
                .selected_model()
                .map(|model| TuiAction::Record(model, self.quality)),
            KeyCode::Char('s') if self.screen != Screen::Home => {
                self.selected_model().map(TuiAction::Stop)
            }
            KeyCode::Char('c') => {
                self.quality = match self.quality {
                    VideoQuality::Best => VideoQuality::P720,
                    VideoQuality::P720 => VideoQuality::P480,
                    VideoQuality::P480 => VideoQuality::AudioOnly,
                    _ => VideoQuality::Best,
                };
                None
            }
            KeyCode::Char('f') if matches!(self.screen, Screen::Browse | Screen::Room) => {
                self.focused = !self.focused;
                None
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.zoom = (self.zoom + 1).min(4);
                None
            }
            KeyCode::Char('-') => {
                self.zoom = self.zoom.saturating_sub(1).max(1);
                None
            }
            KeyCode::Char('R') => self.search(),
            _ => None,
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<TuiAction> {
        let position = (mouse.column, mouse.row).into();
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some((_, code)) = self
                    .buttons
                    .iter()
                    .find(|(area, _)| area.contains(position))
                {
                    let code = *code;
                    self.editing = false;
                    return self.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
                }
                if let Some((_, index)) = self
                    .room_rows
                    .iter()
                    .find(|(area, _)| area.contains(position))
                {
                    self.selected = *index;
                    self.editing = false;
                    self.confirm_quit = false;
                    return self.request_preview(false);
                }
                None
            }
            MouseEventKind::ScrollDown if !self.editing && self.screen != Screen::Home => {
                self.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            }
            MouseEventKind::ScrollUp if !self.editing && self.screen != Screen::Home => {
                self.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
            }
            _ => None,
        }
    }

    fn request_quit(&mut self) -> Option<TuiAction> {
        self.editing = false;
        if !self.active.is_empty() && !self.confirm_quit {
            self.confirm_quit = true;
            self.notice =
                "Hay grabaciones activas. Pulsa q otra vez para detenerlas, conservarlas y salir."
                    .into();
            None
        } else {
            Some(TuiAction::Quit)
        }
    }

    fn search(&mut self) -> Option<TuiAction> {
        if self.loading {
            return None;
        }
        self.loading = true;
        self.screen = Screen::Browse;
        self.notice = "Buscando salas…".into();
        Some(TuiAction::Search(self.query.clone()))
    }

    fn update(&mut self, event: TuiEvent) {
        match event {
            TuiEvent::Rooms(tag, result) => {
                self.loading = false;
                match result {
                    Ok(rooms) => {
                        self.tag = tag;
                        self.rooms = rooms;
                        self.selected = 0;
                        self.preview = None;
                        self.preview_pending = false;
                        self.preview_error = None;
                        self.notice = if self.rooms.is_empty() {
                            "Sin resultados. /: cambiar tag · R: reintentar".into()
                        } else {
                            "Enter: sala · o: vídeo y chat · r: grabar · t: miniatura".into()
                        };
                    }
                    Err(error) => self.notice = error,
                }
            }
            TuiEvent::Preview(model, result) => {
                if self.selected_model().as_deref() == Some(&model) {
                    self.preview_pending = false;
                    match result {
                        Ok(image) => {
                            self.preview = Some((model, image));
                            self.preview_error = None;
                        }
                        Err(error) => {
                            self.preview = None;
                            self.preview_error = Some(error.clone());
                            self.notice = error;
                        }
                    }
                }
            }
            TuiEvent::Recording(model, path) => {
                self.active.insert(model, (path, 0));
            }
            TuiEvent::Progress(model, bytes) => {
                if let Some(info) = self.active.get_mut(&model) {
                    info.1 = bytes;
                }
            }
            TuiEvent::Finished(model, message) => {
                self.active.remove(&model);
                if self.screen == Screen::Recordings {
                    self.selected = self.selected.min(self.active.len().saturating_sub(1));
                }
                self.notice = message;
            }
            TuiEvent::SessionUrl(url) => {
                self.session_url = Some(url);
                self.screen = Screen::Home;
            }
            TuiEvent::SessionFinished(present, message) => {
                self.session_url = None;
                self.session_present = present;
                self.notice = message;
            }
            TuiEvent::Notice(message) => self.notice = message,
        }
    }
}

struct MouseCaptureGuard;

impl Drop for MouseCaptureGuard {
    fn drop(&mut self) {
        let _ = execute!(std::io::stdout(), DisableMouseCapture);
    }
}

pub(crate) fn run_discovery_tui(
    initial: TuiInitial,
    commands: mpsc::Sender<TuiAction>,
    mut events: mpsc::Receiver<TuiEvent>,
) -> anyhow::Result<()> {
    let mut app = DiscoveryTui::new(initial);
    if !app.tag.is_empty() {
        if let Some(action) = app.search() {
            commands
                .try_send(action)
                .map_err(|_| anyhow::anyhow!("No se pudo iniciar la búsqueda"))?;
        }
    }
    let mut terminal = ratatui::try_init().inspect_err(|_| ratatui::restore())?;
    app.preview_renderer
        .detect()
        .inspect_err(|_| ratatui::restore())?;
    let result = (|| -> std::io::Result<()> {
        let _mouse_capture = MouseCaptureGuard;
        execute!(std::io::stdout(), EnableMouseCapture)?;
        let mut previous_view = None;
        loop {
            while let Ok(event) = events.try_recv() {
                let discovered = matches!(&event, TuiEvent::Rooms(_, Ok(_)));
                app.update(event);
                if discovered {
                    if let Some(action) = app.request_preview(false) {
                        if commands.try_send(action).is_err() {
                            app.preview_pending = false;
                            app.preview_error = Some(
                                "No se pudo solicitar la miniatura. Pulsa t para reintentar."
                                    .into(),
                            );
                        }
                    }
                }
            }
            let view = (
                app.screen,
                app.selected,
                app.focused,
                app.zoom,
                app.active.len(),
                app.preview.as_ref().map(|(_, pixels)| Arc::as_ptr(pixels)),
                terminal.size()?,
            );
            if app.preview_renderer.is_native() && previous_view.as_ref() != Some(&view) {
                app.preview_renderer.erase(&mut std::io::stdout())?;
                execute!(std::io::stdout(), ratatui::crossterm::style::ResetColor)?;
                terminal.clear()?;
            }
            previous_view = Some(view);
            app.preview_renderer.begin_frame();
            terminal.draw(|frame| render(frame, &mut app))?;
            app.preview_renderer.flush(&mut std::io::stdout())?;
            if commands.is_closed() {
                return Ok(());
            }
            if !event::poll(Duration::from_millis(100))? {
                continue;
            }
            let action = match event::read()? {
                Event::Key(key)
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                {
                    app.handle_key(key)
                }
                Event::Mouse(mouse) => app.handle_mouse(mouse),
                _ => None,
            };
            if let Some(action) = action {
                let quit = matches!(action, TuiAction::Quit);
                let preview = matches!(
                    &action,
                    TuiAction::Preview(_) | TuiAction::RefreshPreview(_)
                );
                if commands.try_send(action).is_err() {
                    app.loading = false;
                    if preview {
                        app.preview_pending = false;
                        app.preview_error = Some(
                            "No se pudo solicitar la miniatura. Pulsa t para reintentar.".into(),
                        );
                    }
                    app.notice = "Espera a que termine la operación anterior".into();
                } else if quit {
                    return Ok(());
                }
            }
        }
    })();
    let cleanup = (|| -> std::io::Result<()> {
        if app.preview_renderer.is_native() {
            app.preview_renderer.begin_frame();
            app.preview_renderer.erase(&mut std::io::stdout())?;
            execute!(std::io::stdout(), ratatui::crossterm::style::ResetColor)?;
            terminal.clear()?;
            terminal.draw(|frame| {
                frame.render_widget(
                    Block::default().style(Style::default().bg(BACKGROUND)),
                    frame.area(),
                )
            })?;
            app.preview_renderer.flush(&mut std::io::stdout())?;
        }
        Ok(())
    })();
    ratatui::restore();
    if !app.active.is_empty() {
        eprintln!("Finalizando grabaciones y metadatos; espera a que termine el cierre…");
    }
    result?;
    cleanup?;
    Ok(())
}

fn render(frame: &mut Frame, app: &mut DiscoveryTui) {
    frame.render_widget(
        Block::default().style(
            Style::default()
                .bg(BACKGROUND)
                .fg(Color::Rgb(224, 229, 237)),
        ),
        frame.area(),
    );
    app.buttons.clear();
    app.room_rows.clear();
    let [header, toolbar, body, activity, notice, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(if app.active.is_empty() { 0 } else { 3 }),
        Constraint::Length(3),
        Constraint::Length(2),
    ])
    .areas(frame.area());
    let session = if app.session_present {
        "Sesión recibida (sin validar)"
    } else {
        "Sin sesión del grabador"
    };
    let title = format!(
        "cbrec  |  {}  |  {}  |  Grabando {}/{}",
        if app.screen == Screen::Home {
            "Inicio"
        } else if app.screen == Screen::Browse {
            "Explorar"
        } else if app.screen == Screen::Recordings {
            "Grabaciones"
        } else {
            "Sala"
        },
        session,
        app.active.len(),
        app.max_recordings
    );
    frame.render_widget(
        Paragraph::new(title)
            .style(Style::default().fg(Color::Cyan))
            .block(Block::default().borders(Borders::ALL)),
        header,
    );
    render_toolbar(frame, toolbar, app);
    if app.screen == Screen::Home {
        render_home(frame, body, app);
    } else if app.screen == Screen::Recordings {
        let items = app.active.iter().map(|(model, (path, bytes))| {
            ListItem::new(format!(
                "{model} · {:.1} MiB\n{}",
                *bytes as f64 / 1_048_576.0,
                sanitize_external_text(&path.display().to_string())
            ))
        });
        let list = List::new(items)
            .block(
                Block::default()
                    .title(" Grabaciones · s: detener seleccionada · o: abrir sala ")
                    .borders(Borders::ALL),
            )
            .highlight_symbol("> ")
            .highlight_style(
                Style::default()
                    .fg(Color::Rgb(111, 224, 206))
                    .bg(Color::Rgb(30, 48, 58)),
            );
        let mut selection =
            ListState::default().with_selected((!app.active.is_empty()).then_some(app.selected));
        frame.render_stateful_widget(list, body, &mut selection);
        let inner = Block::bordered().inner(body);
        for index in 0..usize::from(inner.height / 2) {
            let room = selection.offset() + index;
            if room >= app.active.len() {
                break;
            }
            app.room_rows.push((
                Rect::new(inner.x, inner.y + index as u16 * 2, inner.width, 2),
                room,
            ));
        }
        if app.active.is_empty() {
            frame.render_widget(
                Paragraph::new("No hay grabaciones activas. h: volver al inicio"),
                Block::default().borders(Borders::ALL).inner(body),
            );
        }
    } else {
        let [rooms_area, detail] = if app.focused {
            [Rect::default(), body]
        } else {
            Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)]).areas(body)
        };
        if !app.focused {
            let items = app.rooms.iter().map(|room| {
                ListItem::new(format!(
                    "{}  ·  {} espectadores",
                    room.username, room.viewers
                ))
            });
            let list = List::new(items)
                .block(
                    Block::default()
                        .title(format!(" #{} · {} salas ", app.tag, app.rooms.len()))
                        .borders(Borders::ALL),
                )
                .highlight_symbol("> ")
                .highlight_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .bg(Color::Rgb(30, 48, 58))
                        .add_modifier(Modifier::BOLD),
                );
            let mut selection =
                ListState::default().with_selected((!app.rooms.is_empty()).then_some(app.selected));
            frame.render_stateful_widget(list, rooms_area, &mut selection);
            let inner = Block::bordered().inner(rooms_area);
            for index in 0..usize::from(inner.height) {
                let room = selection.offset() + index;
                if room >= app.rooms.len() {
                    break;
                }
                app.room_rows.push((
                    Rect::new(inner.x, inner.y + index as u16, inner.width, 1),
                    room,
                ));
            }
        }
        let [room_header, preview, description] = Layout::vertical([
            Constraint::Length(if app.focused { 1 } else { 3 }),
            Constraint::Min(1),
            Constraint::Length(if app.focused { 0 } else { 5 }),
        ])
        .areas(detail);
        let info = app
            .rooms
            .get(app.selected)
            .map(|room| {
                format!(
                    "{} · {} · {} espectadores\nCalidad: {}",
                    room.username, room.show, room.viewers, app.quality
                )
            })
            .unwrap_or_else(|| {
                if app.loading {
                    "Buscando…".into()
                } else {
                    "Sin resultados".into()
                }
            });
        frame.render_widget(
            Paragraph::new(info).block(Block::default().borders(Borders::ALL)),
            room_header,
        );
        let block = Block::default()
            .title(format!(
                " Miniatura [{}] · t: actualizar · +/-: zoom · f: ampliar ",
                app.preview_renderer.label()
            ))
            .borders(Borders::ALL);
        let inner = block.inner(preview);
        frame.render_widget(block, preview);
        if let Some((model, pixels)) = &app.preview {
            if app.selected_model().as_deref() == Some(model) {
                app.preview_renderer.render(frame, inner, pixels, app.zoom);
            } else {
                frame.render_widget(
                    Paragraph::new("Pulsa t o Enter para cargar la miniatura"),
                    inner,
                );
            }
        } else {
            frame.render_widget(
                Paragraph::new(if let Some(error) = &app.preview_error {
                    format!(
                        "No se pudo cargar la miniatura: {}\nPulsa t para reintentar.",
                        sanitize_external_text(error)
                    )
                } else if app.preview_pending {
                    "Cargando miniatura…".into()
                } else {
                    "Pulsa t o Enter para cargar la miniatura".into()
                })
                .wrap(Wrap { trim: true }),
                inner,
            );
        }
        let subject = app
            .rooms
            .get(app.selected)
            .map(|room| room.subject.as_str())
            .unwrap_or("Cambia el tag con / o reintenta con R");
        frame.render_widget(
            Paragraph::new(subject).wrap(Wrap { trim: true }).block(
                Block::default()
                    .title(" Descripción ")
                    .borders(Borders::ALL),
            ),
            description,
        );
    }
    if !app.active.is_empty() {
        let text = app
            .active
            .iter()
            .map(|(model, (_, bytes))| format!("{model}: {:.1} MiB", *bytes as f64 / 1_048_576.0))
            .collect::<Vec<_>>()
            .join("  |  ");
        frame.render_widget(
            Paragraph::new(text)
                .style(Style::default().fg(Color::Red))
                .block(
                    Block::default()
                        .title(" Grabaciones activas · s: detener sala ")
                        .borders(Borders::ALL),
                ),
            activity,
        );
    }
    frame.render_widget(
        Paragraph::new(sanitize_external_text(&app.notice))
            .wrap(Wrap { trim: true })
            .block(Block::default().title(" Estado ").borders(Borders::ALL)),
        notice,
    );
    frame.render_widget(Paragraph::new("↑/↓: sala  Enter: entrar  o: vídeo/chat  r: grabar  s: parar  c: calidad\nh: inicio  g: grabaciones  /: buscar  a: sesión  f: ampliar  q: salir"), footer);
}

fn render_toolbar(frame: &mut Frame, area: Rect, app: &mut DiscoveryTui) {
    let items: &[(&str, KeyCode)] = if app.screen == Screen::Home {
        &[
            ("Enter Explorar", KeyCode::Enter),
            ("/ Buscar", KeyCode::Char('/')),
            ("g Grabaciones", KeyCode::Char('g')),
            ("a Sesión", KeyCode::Char('a')),
            ("l Sitio", KeyCode::Char('l')),
        ]
    } else {
        &[
            ("h Inicio", KeyCode::Char('h')),
            ("/ Buscar", KeyCode::Char('/')),
            ("o Vídeo/chat", KeyCode::Char('o')),
            ("r Grabar", KeyCode::Char('r')),
            ("s Detener", KeyCode::Char('s')),
            ("t Miniatura", KeyCode::Char('t')),
            ("f Ampliar", KeyCode::Char('f')),
            ("c Calidad", KeyCode::Char('c')),
        ]
    };
    let mut x = area.x;
    for (label, code) in items {
        let width = label.chars().count() as u16 + 2;
        if x.saturating_add(width) > area.right() || area.height == 0 {
            break;
        }
        let button = Rect::new(x, area.y, width, 1);
        frame.render_widget(
            Paragraph::new(format!(" {label} ")).style(
                Style::default()
                    .fg(Color::Rgb(171, 238, 226))
                    .bg(Color::Rgb(30, 48, 58)),
            ),
            button,
        );
        app.buttons.push((button, *code));
        x = x.saturating_add(width + 1);
    }
}

fn render_home(frame: &mut Frame, area: Rect, app: &mut DiscoveryTui) {
    let width = area.width.min(88);
    let deck = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        width,
        ..area
    };
    let large = deck.width >= 62 && deck.height >= 22 && app.session_url.is_none();
    let [brand, subtitle, search, actions, details] = Layout::vertical([
        Constraint::Length(if large { 7 } else { 2 }),
        Constraint::Length(2),
        Constraint::Length(3),
        Constraint::Length(4),
        Constraint::Min(0),
    ])
    .areas(deck);
    let logo = if large {
        " ██████╗██████╗ ██████╗ ███████╗ ██████╗\n██╔════╝██╔══██╗██╔══██╗██╔════╝██╔════╝\n██║     ██████╔╝██████╔╝█████╗  ██║     \n██║     ██╔══██╗██╔══██╗██╔══╝  ██║     \n╚██████╗██████╔╝██║  ██║███████╗╚██████╗\n ╚═════╝╚═════╝ ╚═╝  ╚═╝╚══════╝ ╚═════╝"
    } else {
        "C B R E C"
    };
    frame.render_widget(
        Paragraph::new(logo).alignment(Alignment::Center).style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        brand,
    );
    frame.render_widget(
        Paragraph::new("Salas en directo · grabación · sesión").alignment(Alignment::Center),
        subtitle,
    );
    let query = format!("Tag: {}{}", app.query, if app.editing { "▌" } else { "" });
    frame.render_widget(
        Paragraph::new(query).block(
            Block::default()
                .title(" Buscar salas por tag ")
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(if app.editing {
                    Color::Yellow
                } else {
                    Color::Cyan
                })),
        ),
        search,
    );
    app.buttons.push((search, KeyCode::Char('/')));
    let shortcuts = vec![
        Line::from("[/] Escribir un tag    [Enter] Explorar"),
        Line::from(""),
        Line::from(format!(
            "[g] Grabaciones ({})    [a] Conectar sesión    [l] Abrir sitio",
            app.active.len()
        )),
    ];
    if deck.width >= 62 && actions.height >= 3 {
        let cards = Layout::horizontal([Constraint::Fill(1); 3])
            .spacing(1)
            .split(actions);
        for (card, title, description, code) in [
            (
                cards[0],
                "[Enter] Explorar".to_string(),
                "Salas y miniaturas".to_string(),
                KeyCode::Enter,
            ),
            (
                cards[1],
                "[g] Grabaciones".to_string(),
                format!("{} activas", app.active.len()),
                KeyCode::Char('g'),
            ),
            (
                cards[2],
                "[a] Conectar sesión".to_string(),
                "Acceso del grabador".to_string(),
                KeyCode::Char('a'),
            ),
        ] {
            frame.render_widget(
                Paragraph::new(description)
                    .alignment(Alignment::Center)
                    .block(
                        Block::bordered()
                            .border_type(BorderType::Rounded)
                            .title(title)
                            .border_style(Style::default().fg(Color::Rgb(111, 224, 206))),
                    ),
                card,
            );
            app.buttons.push((card, code));
        }
    } else {
        frame.render_widget(
            Paragraph::new(shortcuts)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true }),
            actions,
        );
    }
    let details_text = if let Some(url) = &app.session_url {
        vec![
            Line::from("En Violentmonkey: «Enviar sesión a cbrec» y pega:"),
            Line::from(url.clone()).style(Style::default().fg(Color::Yellow)),
            Line::from("Caduca en 120 s · la cookie se conserva en memoria"),
        ]
    } else {
        vec![
            Line::from(format!(
                "Calidad: {} · Límite: {} grabaciones",
                app.quality, app.max_recordings
            )),
            Line::from(format!(
                "Base de salida: {}",
                sanitize_external_text(&app.output_folder.display().to_string())
            )),
        ]
    };
    frame.render_widget(
        Paragraph::new(details_text)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false }),
        details,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn app() -> DiscoveryTui {
        DiscoveryTui::new(TuiInitial {
            tag: String::new(),
            session_present: false,
            max_recordings: 3,
            quality: VideoQuality::Best,
            output_folder: PathBuf::from("/tmp/cbrec-test"),
        })
    }
    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    fn text(app: &mut DiscoveryTui, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| render(frame, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn changing_room_requests_its_thumbnail() {
        let mut app = app();
        app.screen = Screen::Browse;
        app.rooms = vec![
            TuiRoom::new("alice".into(), 10, "public".into(), "Uno".into()),
            TuiRoom::new("bob".into(), 20, "public".into(), "Dos".into()),
        ];
        assert!(
            matches!(app.handle_key(key(KeyCode::Down)), Some(TuiAction::Preview(model)) if model == "bob")
        );
        assert!(app.preview_pending);
        assert!(app.handle_key(key(KeyCode::Down)).is_none());
        assert!(
            matches!(app.handle_key(key(KeyCode::Up)), Some(TuiAction::Preview(model)) if model == "alice")
        );
    }

    #[test]
    fn obsolete_thumbnail_events_cannot_finish_the_selected_request() {
        let mut app = app();
        app.screen = Screen::Browse;
        app.rooms = vec![TuiRoom::new(
            "bob".into(),
            20,
            "public".into(),
            "Dos".into(),
        )];
        app.preview_pending = true;
        app.update(TuiEvent::Preview(
            "alice".into(),
            Err("Respuesta anterior".into()),
        ));
        assert!(app.preview_pending);
        assert!(!app.notice.contains("Respuesta anterior"));
    }

    #[test]
    fn preview_failure_is_visible_and_retry_restores_loading() {
        let mut app = app();
        app.screen = Screen::Room;
        app.rooms = vec![TuiRoom::new(
            "alice".into(),
            10,
            "public".into(),
            "Uno".into(),
        )];
        app.update(TuiEvent::Preview(
            "alice".into(),
            Err("El CDN no respondió".into()),
        ));
        let rendered = text(&mut app, 110, 36);
        assert!(rendered.contains("No se pudo cargar la miniatura"));
        assert!(rendered.contains("El CDN no respondió"));
        assert!(
            matches!(app.handle_key(key(KeyCode::Char('t'))), Some(TuiAction::RefreshPreview(model)) if model == "alice")
        );
        assert!(app.preview_error.is_none());
        assert!(app.preview_pending);
        assert!(text(&mut app, 110, 36).contains("Cargando miniatura"));
        app.update(TuiEvent::Preview(
            "alice".into(),
            Ok(Arc::new(DynamicImage::new_rgb8(480, 270))),
        ));
        assert!(!app.preview_pending);
        assert!(app.preview_error.is_none());
        assert!(app.preview.is_some());
    }

    #[test]
    fn toolbar_clicks_and_room_scrolling_use_the_keyboard_actions() {
        let mut app = app();
        text(&mut app, 110, 36);
        let (button, _) = app
            .buttons
            .iter()
            .find(|(_, code)| *code == KeyCode::Char('g'))
            .unwrap();
        let click = |column, row, kind| MouseEvent {
            column,
            row,
            kind,
            modifiers: KeyModifiers::NONE,
        };
        assert!(app
            .handle_mouse(click(
                button.x,
                button.y,
                MouseEventKind::Down(MouseButton::Left)
            ))
            .is_none());
        assert!(app.screen == Screen::Recordings);
        app.screen = Screen::Browse;
        app.rooms = vec![
            TuiRoom::new("alice".into(), 10, "public".into(), "Uno".into()),
            TuiRoom::new("bob".into(), 20, "public".into(), "Dos".into()),
        ];
        text(&mut app, 110, 36);
        let row = app.room_rows[1].0;
        assert!(
            matches!(app.handle_mouse(click(row.x, row.y, MouseEventKind::Down(MouseButton::Left))), Some(TuiAction::Preview(model)) if model == "bob")
        );
        assert!(
            matches!(app.handle_mouse(click(row.x, row.y, MouseEventKind::ScrollUp)), Some(TuiAction::Preview(model)) if model == "alice")
        );
        assert!(app
            .handle_mouse(click(
                row.x,
                row.y,
                MouseEventKind::Down(MouseButton::Right)
            ))
            .is_none());
        app.editing = true;
        assert!(app
            .handle_mouse(click(row.x, row.y, MouseEventKind::ScrollDown))
            .is_none());
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn toolbar_is_clipped_and_recording_rows_select_without_loading_images() {
        let mut app = app();
        for (width, height) in [(1, 1), (20, 8), (110, 36)] {
            text(&mut app, width, height);
            assert!(app
                .buttons
                .iter()
                .all(|(area, _)| area.right() <= width && area.bottom() <= height));
        }
        app.active.insert("alice".into(), ("alice.mp4".into(), 10));
        app.active.insert("bob".into(), ("bob.mp4".into(), 20));
        app.screen = Screen::Recordings;
        app.confirm_quit = true;
        text(&mut app, 110, 36);
        let row = app.room_rows[1].0;
        assert!(app
            .handle_mouse(MouseEvent {
                column: row.x,
                row: row.y,
                kind: MouseEventKind::Down(MouseButton::Left),
                modifiers: KeyModifiers::NONE
            })
            .is_none());
        assert_eq!(app.selected_model().as_deref(), Some("bob"));
        assert!(!app.confirm_quit);
    }

    #[test]
    fn home_search_room_navigation_and_quit_are_distinct() {
        let mut app = app();
        assert!(text(&mut app, 100, 32).contains("Inicio"));
        assert!(
            matches!(app.handle_key(key(KeyCode::Enter)), Some(TuiAction::Search(tag)) if tag == "gaming")
        );
        app.update(TuiEvent::Rooms(
            "gaming".into(),
            Ok(vec![TuiRoom::new(
                "alice".into(),
                42,
                "public".into(),
                "hello".into(),
            )]),
        ));
        app.move_up();
        app.move_down();
        assert_eq!(app.selected, 0);
        assert!(
            matches!(app.handle_key(key(KeyCode::Enter)), Some(TuiAction::Preview(model)) if model == "alice")
        );
        let rendered = text(&mut app, 100, 32);
        assert!(rendered.contains("alice"));
        assert!(rendered.contains("42 espectadores"));
        assert!(rendered.contains("hello"));
        assert!(app.handle_key(key(KeyCode::Esc)).is_none());
        assert!(app.screen == Screen::Browse);
        assert!(app.handle_key(key(KeyCode::Esc)).is_none());
        assert!(app.screen == Screen::Home);
        assert!(matches!(
            app.handle_key(key(KeyCode::Esc)),
            Some(TuiAction::Quit)
        ));
    }

    #[test]
    fn editing_does_not_activate_record_or_quit_shortcuts() {
        let mut app = app();
        app.query.clear();
        app.handle_key(key(KeyCode::Char('/')));
        assert!(app.handle_key(key(KeyCode::Char('q'))).is_none());
        assert!(app.handle_key(key(KeyCode::Char('r'))).is_none());
        assert_eq!(app.query, "qr");
        app.handle_key(key(KeyCode::Backspace));
        assert!(
            matches!(app.handle_key(key(KeyCode::Enter)), Some(TuiAction::Search(tag)) if tag == "q")
        );
    }

    #[test]
    fn recording_requires_selection_and_quit_confirms_active_jobs() {
        let mut app = app();
        assert!(app.handle_key(key(KeyCode::Char('r'))).is_none());
        app.update(TuiEvent::Recording("alice".into(), PathBuf::from("a.mp4")));
        assert!(app.handle_key(key(KeyCode::Char('q'))).is_none());
        assert!(matches!(
            app.handle_key(key(KeyCode::Char('q'))),
            Some(TuiAction::Quit)
        ));
        app.confirm_quit = false;
        app.editing = true;
        assert!(app
            .handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
            .is_none());
        assert!(!app.editing);
        assert!(matches!(
            app.handle_key(key(KeyCode::Char('q'))),
            Some(TuiAction::Quit)
        ));
        app.update(TuiEvent::Finished("alice".into(), "Conservado".into()));
        assert!(app.active.is_empty());
    }

    #[test]
    fn preview_and_empty_error_states_render_at_small_sizes() {
        let mut app = app();
        app.update(TuiEvent::Rooms("gaming".into(), Err("Sin conexión".into())));
        assert!(!app.loading);
        assert!(app.notice.contains("Sin conexión"));
        app.update(TuiEvent::Rooms("gaming".into(), Ok(Vec::new())));
        app.screen = Screen::Browse;
        for (width, height) in [(1, 1), (20, 8), (80, 24), (160, 50)] {
            text(&mut app, width, height);
        }
        app.rooms.push(TuiRoom::new(
            "alice".into(),
            1,
            "public\n".into(),
            "hello\u{1b}[31m".into(),
        ));
        assert_eq!(app.rooms[0].subject, "hello [31m");
        app.update(TuiEvent::Preview(
            "alice".into(),
            Ok(Arc::new(DynamicImage::new_rgb8(480, 270))),
        ));
        assert!(text(&mut app, 100, 40).contains("Miniatura"));
        assert_eq!(app.preview.as_ref().unwrap().1.width(), 480);
        app.update(TuiEvent::Preview(
            "alice".into(),
            Err("Imagen no válida".into()),
        ));
        assert!(app.notice.contains("Imagen no válida"));
        app.handle_key(key(KeyCode::Char('f')));
        app.handle_key(key(KeyCode::Char('+')));
        text(&mut app, 160, 50);
        assert!(app.focused);
        assert_eq!(app.zoom, 2);
    }
    #[test]
    fn recordings_remain_selectable_after_changing_search_and_home_cannot_record_a_hidden_room() {
        let mut app = app();
        app.rooms.push(TuiRoom::new(
            "alice".into(),
            1,
            "public".into(),
            "hello".into(),
        ));
        assert!(app.handle_key(key(KeyCode::Char('r'))).is_none());
        app.update(TuiEvent::Recording("bob".into(), PathBuf::from("bob.mp4")));
        app.handle_key(key(KeyCode::Char('g')));
        assert!(
            matches!(app.handle_key(key(KeyCode::Char('s'))), Some(TuiAction::Stop(model)) if model == "bob")
        );
        app.handle_key(key(KeyCode::End));
        assert_eq!(app.selected, 0);
        app.update(TuiEvent::Progress("bob".into(), 1024));
        assert!(text(&mut app, 100, 32).contains("bob"));
        app.handle_key(key(KeyCode::Char('h')));
        assert!(app.screen == Screen::Home);
        app.update(TuiEvent::SessionUrl(
            "http://127.0.0.1:1234/cbrec-session/test".into(),
        ));
        assert!(text(&mut app, 110, 36).contains("Violentmonkey"));
        app.update(TuiEvent::SessionFinished(true, "Recibida".into()));
        assert!(app.session_url.is_none());
        assert!(app.session_present);
        assert!(matches!(
            app.handle_key(key(KeyCode::Char('a'))),
            Some(TuiAction::ConnectSession)
        ));
    }

    #[test]
    fn preview_zoom_keeps_source_resolution() {
        let mut app = app();
        let image = Arc::new(DynamicImage::new_rgb8(480, 270));
        app.rooms.push(TuiRoom::new(
            "alice".into(),
            1,
            "public".into(),
            "hello".into(),
        ));
        app.screen = Screen::Room;
        app.update(TuiEvent::Preview("alice".into(), Ok(image.clone())));
        app.handle_key(key(KeyCode::Char('+')));
        text(&mut app, 80, 24);
        assert_eq!(app.zoom, 2);
        assert_eq!((image.width(), image.height()), (480, 270));
    }

    #[test]
    fn home_is_a_search_deck_with_visible_actions() {
        let mut app = app();
        let rendered = text(&mut app, 110, 36);
        assert!(rendered.contains("Buscar salas por tag"));
        assert!(rendered.contains("[g] Grabaciones"));
        assert!(rendered.contains("[a] Conectar"));
        assert!(!rendered.contains("La sesión del navegador y la del grabador son independientes."));
        let (card, _) = app
            .buttons
            .iter()
            .rev()
            .find(|(_, code)| *code == KeyCode::Char('g'))
            .unwrap();
        assert!(card.height >= 3, "La tarjeta central debe ser interactiva");
        let click = MouseEvent {
            column: card.x + 1,
            row: card.y + 1,
            kind: MouseEventKind::Down(MouseButton::Left),
            modifiers: KeyModifiers::NONE,
        };
        app.handle_mouse(click);
        assert!(app.screen == Screen::Recordings);
    }
}
