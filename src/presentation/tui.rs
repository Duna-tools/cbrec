//! Panel de exploración y grabación; las operaciones externas llegan por canales.

use crate::domain::value_objects::VideoQuality;
use crate::presentation::output::sanitize_external_text;
use crate::presentation::thumbnail::{PreviewRenderer, BACKGROUND};
use image::DynamicImage;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap,
};
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
    Search(String, usize, u64),
    CancelSession,
    Preview(String),
    RefreshPreview(String),
    OpenRoom(String),
    OpenSite,
    ConnectSession,
    Record(String, VideoQuality),
    Stop(String),
    Quit,
}

pub(crate) struct TuiPage {
    pub rooms: Vec<TuiRoom>,
    pub total: Option<usize>,
    pub has_more: bool,
}

const BROWSERS: &[&str] = &[
    "Firefox",
    "Chrome",
    "Chromium",
    "Edge",
    "Brave",
    "Otro navegador",
];

pub(crate) enum TuiEvent {
    Rooms(String, usize, u64, Result<TuiPage, String>),
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
    pub page_size: usize,
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
    offset: usize,
    page_size: usize,
    total: Option<usize>,
    has_more: bool,
    session_picker: Option<usize>,
    session_browser: usize,
    session_pending: bool,
    last_request: (String, usize),
    search_generation: u64,
}

impl DiscoveryTui {
    fn new(initial: TuiInitial) -> Self {
        Self {
            search_generation: 0,
            last_request: (initial.tag.clone(), 0),
            query: initial.tag.clone(),
            tag: initial.tag,
            screen: Screen::Home,
            rooms: Vec::new(),
            selected: 0,
            editing: false,
            loading: false,
            notice: "/: buscar por #tag, @usuario o URL · a: conectar sesión".into(),
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
            offset: 0,
            page_size: initial.page_size,
            total: None,
            has_more: false,
            session_picker: None,
            session_browser: 0,
            session_pending: false,
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
        if self.screen == Screen::Recordings {
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
        if self.session_pending {
            if key.code == KeyCode::Esc {
                self.session_pending = false;
                self.session_url = None;
                return Some(TuiAction::CancelSession);
            }
            return None;
        }
        if let Some(selected) = self.session_picker {
            match key.code {
                KeyCode::Esc => self.session_picker = None,
                KeyCode::Up => self.session_picker = Some(selected.saturating_sub(1)),
                KeyCode::Down => self.session_picker = Some((selected + 1).min(BROWSERS.len() - 1)),
                KeyCode::Enter => return self.connect_session(selected),
                KeyCode::Char(c) if ('1'..='6').contains(&c) => {
                    return self.connect_session(c as usize - '1' as usize)
                }
                _ => {}
            }
            return None;
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
                KeyCode::Char(character) => self.append_query(&character.to_string()),
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
                if self.screen == Screen::Home && self.focused {
                    self.focused = false;
                } else if self.screen == Screen::Room {
                    self.screen = Screen::Browse;
                    self.focused = false;
                } else if matches!(self.screen, Screen::Browse | Screen::Recordings) {
                    return self.home();
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
            KeyCode::Char('h') => self.home(),
            KeyCode::Char('p') => self.home(),
            KeyCode::Char('/') => {
                self.editing = true;
                None
            }
            KeyCode::Enter if self.screen == Screen::Home && self.rooms.is_empty() => self.search(),
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
            KeyCode::Char('a') => {
                self.session_picker = Some(self.session_browser);
                self.editing = false;
                None
            }
            KeyCode::PageDown | KeyCode::Char(']') => self.change_page(true),
            KeyCode::PageUp | KeyCode::Char('[') => self.change_page(false),
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
                self.notice = format!("Calidad para las próximas grabaciones: {}", self.quality);
                None
            }
            KeyCode::Char('f') if self.screen != Screen::Recordings => {
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
            KeyCode::Char('R') => self.load(
                self.last_request.0.clone(),
                self.last_request.1,
                self.screen,
            ),
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
                if self.session_picker.is_some() || self.session_pending {
                    return None;
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
            MouseEventKind::ScrollDown
                if !self.editing && self.session_picker.is_none() && !self.session_pending =>
            {
                self.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            }
            MouseEventKind::ScrollUp
                if !self.editing && self.session_picker.is_none() && !self.session_pending =>
            {
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

    fn append_query(&mut self, text: &str) {
        for c in text
            .chars()
            .filter(|c| c.is_ascii() && !c.is_ascii_control())
        {
            if self.query.len() >= 512 {
                break;
            }
            self.query.push(c);
        }
    }

    fn connect_session(&mut self, browser: usize) -> Option<TuiAction> {
        self.session_browser = browser;
        self.session_picker = None;
        self.session_pending = true;
        self.notice = format!("Esperando la sesión desde {}…", BROWSERS[browser]);
        Some(TuiAction::ConnectSession)
    }

    fn home(&mut self) -> Option<TuiAction> {
        self.focused = false;
        self.query.clear();
        self.load(String::new(), 0, Screen::Home)
    }

    fn search(&mut self) -> Option<TuiAction> {
        self.load(self.query.clone(), 0, Screen::Browse)
    }

    fn load(&mut self, query: String, offset: usize, screen: Screen) -> Option<TuiAction> {
        self.last_request = (query.clone(), offset);
        self.search_generation = self.search_generation.wrapping_add(1);
        self.loading = true;
        self.focused = false;
        self.screen = screen;
        self.notice =
            "Cargando salas… /: otra búsqueda · se conservan los resultados si falla".into();
        Some(TuiAction::Search(query, offset, self.search_generation))
    }

    fn change_page(&mut self, next: bool) -> Option<TuiAction> {
        if self.loading || self.screen == Screen::Recordings {
            return None;
        }
        let offset = if next {
            if !self.has_more {
                return None;
            }
            self.offset.checked_add(self.page_size)?
        } else {
            if self.offset == 0 {
                return None;
            }
            self.offset.saturating_sub(self.page_size)
        };
        let screen = if self.screen == Screen::Room {
            Screen::Browse
        } else {
            self.screen
        };
        self.load(self.tag.clone(), offset, screen)
    }

    fn update(&mut self, event: TuiEvent) {
        match event {
            TuiEvent::Rooms(tag, offset, generation, result) => {
                if generation != self.search_generation {
                    return;
                }
                self.loading = false;
                match result {
                    Ok(page) => {
                        self.tag = tag;
                        self.offset = offset;
                        self.total = page.total;
                        self.has_more = page.has_more;
                        self.rooms = page.rooms;
                        if self.screen != Screen::Recordings {
                            self.selected = 0;
                        }
                        self.preview = None;
                        self.preview_pending = false;
                        self.preview_error = None;
                        self.notice = if self.rooms.is_empty() {
                            "Sin resultados. /: cambiar búsqueda · R: reintentar".into()
                        } else if self.screen == Screen::Home {
                            "Enter: entrar en la sala · PgUp/PgDn: página · /: buscar · a: sesión"
                                .into()
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
                if self.session_pending {
                    self.session_url = Some(url);
                }
            }
            TuiEvent::SessionFinished(present, message) => {
                self.session_url = None;
                self.session_pending = false;
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
        let _ = execute!(
            std::io::stdout(),
            DisableMouseCapture,
            DisableBracketedPaste
        );
    }
}

pub(crate) fn run_discovery_tui(
    initial: TuiInitial,
    commands: mpsc::Sender<TuiAction>,
    mut events: mpsc::Receiver<TuiEvent>,
) -> anyhow::Result<()> {
    let mut app = DiscoveryTui::new(initial);
    let action = if app.tag.is_empty() {
        app.home()
    } else {
        app.search()
    };
    if let Some(action) = action {
        commands
            .try_send(action)
            .map_err(|_| anyhow::anyhow!("No se pudo cargar el catálogo"))?;
    }
    let mut terminal = ratatui::try_init().inspect_err(|_| ratatui::restore())?;
    app.preview_renderer
        .detect()
        .inspect_err(|_| ratatui::restore())?;
    let result = (|| -> std::io::Result<()> {
        let _mouse_capture = MouseCaptureGuard;
        execute!(std::io::stdout(), EnableMouseCapture, EnableBracketedPaste)?;
        let mut previous_view = None;
        loop {
            while let Ok(event) = events.try_recv() {
                let discovered = matches!(&event, TuiEvent::Rooms(_, _, generation, Ok(_)) if *generation == app.search_generation);
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
                app.session_picker,
                app.session_pending,
                app.editing,
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
                Event::Paste(text) if app.editing => {
                    app.append_query(&text);
                    None
                }
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
    if app.screen == Screen::Home && !app.focused {
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
        let [search, catalog] = Layout::vertical([
            Constraint::Length(if app.focused { 0 } else { 3 }),
            Constraint::Min(0),
        ])
        .areas(body);
        render_search(frame, search, app);
        render_catalog(frame, catalog, app);
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
    frame.render_widget(Paragraph::new("↑/↓: sala  Enter: entrar  o: vídeo/chat  r: grabar  s: parar\nh: inicio  g: grabaciones  /: buscar  a: sesión  PgUp/PgDn: página  q: salir"), footer);
    if app.session_picker.is_some() || app.session_pending {
        render_session(frame, app);
    }
}

fn render_catalog(frame: &mut Frame, body: Rect, app: &mut DiscoveryTui) {
    let [catalog, pagination] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(body);
    let mut x = pagination.x;
    for (label, code, enabled) in [
        ("[ Anterior", KeyCode::PageUp, app.offset > 0),
        ("] Siguiente", KeyCode::PageDown, app.has_more),
    ] {
        let width = label.chars().count() as u16 + 2;
        if x.saturating_add(width) <= pagination.right() && pagination.height > 0 {
            let area = Rect::new(x, pagination.y, width, 1);
            frame.render_widget(
                Paragraph::new(format!(" {label} ")).style(Style::default().fg(
                    if enabled && !app.loading {
                        Color::Cyan
                    } else {
                        Color::DarkGray
                    },
                )),
                area,
            );
            if enabled && !app.loading {
                app.buttons.push((area, code));
            }
            x = x.saturating_add(width + 1);
        }
    }
    frame.render_widget(
        Paragraph::new(format!(
            "Página {}{}{}",
            app.offset / app.page_size + 1,
            app.total
                .map(|total| format!(
                    " de {} · {total} salas",
                    total.div_ceil(app.page_size).max(1)
                ))
                .unwrap_or_default(),
            if app.loading { " · cargando…" } else { "" }
        )),
        Rect::new(
            x.min(pagination.right()),
            pagination.y,
            pagination.right().saturating_sub(x),
            pagination.height,
        ),
    );
    let [rooms_area, detail] = if app.focused {
        [Rect::default(), catalog]
    } else {
        Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)]).areas(catalog)
    };
    if !app.focused {
        let items = app.rooms.iter().map(|room| {
            ListItem::new(if app.tag.starts_with('@') {
                room.username.clone()
            } else {
                format!("{}  ·  {} espectadores", room.username, room.viewers)
            })
        });
        let list = List::new(items)
            .block(
                Block::default()
                    .title(format!(
                        " {} · {} salas ",
                        if app.tag.is_empty() {
                            "Populares"
                        } else {
                            &app.tag
                        },
                        app.rooms.len()
                    ))
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
                "{} · {} · {}\nCalidad: {}",
                room.username,
                room.show,
                if app.tag.starts_with('@') {
                    "espectadores: sin datos".into()
                } else {
                    format!("{} espectadores", room.viewers)
                },
                app.quality
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
    if app.session_picker.is_some() || app.session_pending {
        // Los gráficos nativos no respetan la superposición de celdas del diálogo.
    } else if let Some((model, pixels)) = &app.preview {
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
        .unwrap_or("Busca con / o reintenta con R");
    frame.render_widget(
        Paragraph::new(subject).wrap(Wrap { trim: true }).block(
            Block::default()
                .title(" Descripción ")
                .borders(Borders::ALL),
        ),
        description,
    );
}

fn render_toolbar(frame: &mut Frame, area: Rect, app: &mut DiscoveryTui) {
    let items: &[(&str, KeyCode)] = if app.screen == Screen::Home {
        &[
            ("Enter Sala", KeyCode::Enter),
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

fn render_search(frame: &mut Frame, area: Rect, app: &mut DiscoveryTui) {
    let visible = if app.query.is_empty() && !app.editing {
        "Vacío: populares · #gaming · @usuario · https://chaturbate.com/usuario/".into()
    } else {
        format!("{}{}", app.query, if app.editing { "▌" } else { "" })
    };
    let width = usize::from(area.width.saturating_sub(2));
    let visible: String = visible
        .chars()
        .rev()
        .take(width)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    frame.render_widget(
        Paragraph::new(visible).block(
            Block::bordered()
                .title(" Buscar: #tag, @usuario o URL · Enter: buscar · Esc: cancelar ")
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(if app.editing {
                    Color::Yellow
                } else {
                    Color::Cyan
                })),
        ),
        area,
    );
    app.buttons.push((area, KeyCode::Char('/')));
}

fn render_home(frame: &mut Frame, area: Rect, app: &mut DiscoveryTui) {
    let [intro, search, actions, catalog] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(3),
        Constraint::Length(if area.height >= 15 { 3 } else { 0 }),
        Constraint::Min(0),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(format!("EN DIRECTO · Populares del sitio · selecciona una sala para ver su miniatura\nCalidad: {} · Carpeta: {}", app.quality, sanitize_external_text(&app.output_folder.display().to_string())))
        .style(Style::default().fg(Color::Cyan)),
        intro,
    );
    render_search(frame, search, app);
    if actions.height >= 3 {
        let cards = Layout::horizontal([Constraint::Fill(1); 3])
            .spacing(1)
            .split(actions);
        for (card, title, description, code) in [
            (
                cards[0],
                "[p] Populares",
                "Catálogo del sitio".into(),
                KeyCode::Char('p'),
            ),
            (
                cards[1],
                "[g] Grabaciones",
                format!("{} activas", app.active.len()),
                KeyCode::Char('g'),
            ),
            (
                cards[2],
                "[a] Conectar sesión",
                "Elegir navegador".into(),
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
    }
    render_catalog(frame, catalog, app);
}

fn render_session(frame: &mut Frame, app: &mut DiscoveryTui) {
    app.buttons.clear();
    app.room_rows.clear();
    let bounds = frame.area();
    let width = bounds.width.min(94);
    let height = bounds.height.min(19);
    let area = Rect::new(
        bounds.x + bounds.width.saturating_sub(width) / 2,
        bounds.y + bounds.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .title(" Conectar sesión · Esc: cancelar ")
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let heading = if app.session_pending {
        format!("Navegador elegido: {}", BROWSERS[app.session_browser])
    } else {
        "¿Qué navegador tienes abierto?".into()
    };
    let [instructions, choices, hint, cancel] = Layout::vertical([
        Constraint::Length(6),
        Constraint::Length(if app.session_picker.is_some() { 6 } else { 4 }),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(inner);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(heading).style(Style::default().fg(Color::Yellow)),
            Line::from("1. Mantén el navegador abierto y tu sesión iniciada en Chaturbate."),
            Line::from(
                "2. Instala Violentmonkey y scripts/cbrec-violentmonkey.user.js (ver README).",
            ),
            Line::from("3. En esa pestaña: «Enviar sesión a cbrec» y pega la dirección temporal."),
        ])
        .wrap(Wrap { trim: false }),
        instructions,
    );
    frame.render_widget(
        Paragraph::new("[Esc] Cancelar").style(Style::default().fg(Color::Cyan)),
        cancel,
    );
    app.buttons.push((cancel, KeyCode::Esc));
    if let Some(selected) = app.session_picker {
        let items = BROWSERS
            .iter()
            .enumerate()
            .map(|(index, browser)| ListItem::new(format!("[{}] {browser}", index + 1)));
        let mut state = ListState::default().with_selected(Some(selected));
        frame.render_stateful_widget(
            List::new(items)
                .highlight_symbol("> ")
                .highlight_style(Style::default().fg(Color::Cyan)),
            choices,
            &mut state,
        );
        for index in 0..usize::from(choices.height).min(BROWSERS.len()) {
            let browser = state.offset() + index;
            if browser >= BROWSERS.len() {
                break;
            }
            app.buttons.push((
                Rect::new(choices.x, choices.y + index as u16, choices.width, 1),
                KeyCode::Char(char::from(b'1' + browser as u8)),
            ));
        }
        frame.render_widget(
            Paragraph::new("↑/↓ y Enter, número o clic. No se leen perfiles automáticamente.")
                .wrap(Wrap { trim: true }),
            hint,
        );
    } else {
        let text = if let Some(url) = &app.session_url {
            format!("{url}\nCaduca en 120 s. Sesión solo en memoria durante esta ejecución.")
        } else {
            "Preparando la dirección temporal…".into()
        };
        frame.render_widget(
            Paragraph::new(text)
                .style(Style::default().fg(Color::Yellow))
                .wrap(Wrap { trim: false }),
            choices,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn app() -> DiscoveryTui {
        DiscoveryTui::new(TuiInitial {
            tag: String::new(),
            page_size: 20,
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
    fn paging_keeps_the_current_page_on_error_and_stops_at_the_end() {
        let mut app = app();
        assert!(matches!(app.home(), Some(TuiAction::Search(query, 0, _)) if query.is_empty()));
        app.update(TuiEvent::Rooms(
            "#gaming".into(),
            0,
            app.search_generation,
            Ok(TuiPage {
                rooms: vec![TuiRoom::new(
                    "alice".into(),
                    42,
                    "public".into(),
                    "hello".into(),
                )],
                total: Some(39),
                has_more: true,
            }),
        ));
        assert!(app.change_page(false).is_none());
        assert!(
            matches!(app.change_page(true), Some(TuiAction::Search(query, 20, _)) if query == "#gaming")
        );
        assert!(app.change_page(true).is_none());
        app.update(TuiEvent::Rooms(
            String::new(),
            20,
            app.search_generation,
            Err("Sin red".into()),
        ));
        assert_eq!(app.offset, 0);
        assert_eq!(app.selected_model().as_deref(), Some("alice"));
        assert!(app.has_more);
        app.update(TuiEvent::Rooms(
            "#gaming".into(),
            20,
            app.search_generation,
            Ok(TuiPage {
                rooms: vec![],
                total: Some(39),
                has_more: false,
            }),
        ));
        assert!(app.change_page(true).is_none());
        assert!(text(&mut app, 110, 36).contains("Página 2 de 2"));
        let (button, _) = app
            .buttons
            .iter()
            .find(|(_, code)| *code == KeyCode::PageUp)
            .unwrap();
        assert!(matches!(
            app.handle_mouse(MouseEvent {
                column: button.x,
                row: button.y,
                kind: MouseEventKind::Down(MouseButton::Left),
                modifiers: KeyModifiers::NONE
            }),
            Some(TuiAction::Search(_, 0, _))
        ));
    }

    #[test]
    fn pasted_url_does_not_trigger_shortcuts_and_new_search_can_replace_loading() {
        let mut app = app();
        app.home();
        app.handle_key(key(KeyCode::Char('/')));
        for c in "https://chaturbate.com/alice/?q=record".chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
        assert_eq!(app.query, "https://chaturbate.com/alice/?q=record");
        assert!(
            matches!(app.handle_key(key(KeyCode::Enter)), Some(TuiAction::Search(query, 0, _)) if query.contains("/alice/"))
        );
        app.query.clear();
        app.editing = true;
        app.append_query("@bob\n\u{1b}");
        assert_eq!(app.query, "@bob");
        app.append_query(&"x".repeat(600));
        assert_eq!(app.query.len(), 512);
    }

    #[test]
    fn stale_page_cannot_finish_a_newer_search_and_retry_repeats_the_failed_request() {
        let mut app = app();
        app.home();
        let old_generation = app.search_generation;
        app.query = "#music".into();
        app.search();
        let generation = app.search_generation;
        app.update(TuiEvent::Rooms(
            String::new(),
            0,
            old_generation,
            Ok(TuiPage {
                rooms: vec![],
                total: Some(0),
                has_more: false,
            }),
        ));
        assert!(app.loading);
        app.update(TuiEvent::Rooms(
            String::new(),
            0,
            generation,
            Err("Sin red".into()),
        ));
        assert!(!app.loading);
        assert!(
            matches!(app.handle_key(key(KeyCode::Char('R'))), Some(TuiAction::Search(query, 0, _)) if query == "#music")
        );
    }

    #[test]
    fn session_wizard_requires_browser_choice_and_supports_cancel() {
        let mut app = app();
        assert!(app.handle_key(key(KeyCode::Char('a'))).is_none());
        let rendered = text(&mut app, 110, 36);
        assert!(rendered.contains("¿Qué navegador tienes abierto?"));
        assert!(rendered.contains("sesión iniciada"));
        assert!(rendered.contains("Violentmonkey"));
        assert!(app.handle_key(key(KeyCode::Char('r'))).is_none());
        let (button, _) = app
            .buttons
            .iter()
            .find(|(_, code)| *code == KeyCode::Char('2'))
            .unwrap();
        assert!(matches!(
            app.handle_mouse(MouseEvent {
                column: button.x,
                row: button.y,
                kind: MouseEventKind::Down(MouseButton::Left),
                modifiers: KeyModifiers::NONE
            }),
            Some(TuiAction::ConnectSession)
        ));
        assert_eq!(app.session_browser, 1);
        assert!(app.handle_key(key(KeyCode::Char('a'))).is_none());
        assert!(matches!(
            app.handle_key(key(KeyCode::Esc)),
            Some(TuiAction::CancelSession)
        ));
        app.update(TuiEvent::SessionFinished(false, "Cancelada".into()));
        assert!(!app.session_pending);
        app.update(TuiEvent::SessionUrl(
            "http://127.0.0.1:1234/cancelled".into(),
        ));
        assert!(app.session_url.is_none());
        assert!(!app.session_pending);
        for (w, h) in [(1, 1), (20, 8), (80, 24)] {
            app.handle_key(key(KeyCode::Char('a')));
            text(&mut app, w, h);
            app.handle_key(key(KeyCode::Esc));
        }
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
            matches!(app.handle_key(key(KeyCode::Enter)), Some(TuiAction::Search(tag, 0, _)) if tag.is_empty())
        );
        app.update(TuiEvent::Rooms(
            "#gaming".into(),
            0,
            app.search_generation,
            Ok(TuiPage {
                rooms: vec![TuiRoom::new(
                    "alice".into(),
                    42,
                    "public".into(),
                    "hello".into(),
                )],
                total: Some(1),
                has_more: false,
            }),
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
        assert!(
            matches!(app.handle_key(key(KeyCode::Esc)), Some(TuiAction::Search(query, 0, _)) if query.is_empty())
        );
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
            matches!(app.handle_key(key(KeyCode::Enter)), Some(TuiAction::Search(tag, 0, _)) if tag == "q")
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
        app.update(TuiEvent::Rooms(
            "#gaming".into(),
            0,
            app.search_generation,
            Err("Sin conexión".into()),
        ));
        assert!(!app.loading);
        assert!(app.notice.contains("Sin conexión"));
        app.update(TuiEvent::Rooms(
            "#gaming".into(),
            0,
            app.search_generation,
            Ok(TuiPage {
                rooms: Vec::new(),
                total: Some(0),
                has_more: false,
            }),
        ));
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
        app.session_pending = true;
        app.update(TuiEvent::SessionUrl(
            "http://127.0.0.1:1234/cbrec-session/test".into(),
        ));
        assert!(text(&mut app, 110, 36).contains("Violentmonkey"));
        app.update(TuiEvent::SessionFinished(true, "Recibida".into()));
        assert!(app.session_url.is_none());
        assert!(app.session_present);
        assert!(app.handle_key(key(KeyCode::Char('a'))).is_none());
        assert!(matches!(
            app.handle_key(key(KeyCode::Enter)),
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
        assert!(rendered.contains("Buscar: #tag, @usuario o URL"));
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
