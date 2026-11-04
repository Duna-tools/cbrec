//! Conserva la imagen original y codifica para el protocolo gráfico del terminal una sola vez.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{imageops::FilterType, DynamicImage};
use ratatui::{layout::Rect, Frame};
use ratatui_image::{
    picker::{cap_parser::QueryStdioOptions, Picker, ProtocolType},
    protocol::StatefulProtocol,
    Resize, StatefulImage,
};
use std::io::{self, Write};
use std::sync::Arc;
use std::time::Duration;

pub(crate) const BACKGROUND: ratatui::style::Color = ratatui::style::Color::Rgb(16, 18, 24);

struct NativeImage {
    source: Arc<DynamicImage>,
    zoom: u16,
    area: Rect,
    payload: String,
    width: u32,
    height: u32,
}

pub(crate) struct PreviewRenderer {
    picker: Picker,
    encoded: Option<(Arc<DynamicImage>, u16, StatefulProtocol)>,
    native: bool,
    native_image: Option<NativeImage>,
    pending: bool,
    sent: bool,
}

impl PreviewRenderer {
    pub fn new() -> Self {
        Self {
            picker: Picker::halfblocks(),
            encoded: None,
            native: false,
            native_image: None,
            pending: false,
            sent: false,
        }
    }

    pub fn detect(&mut self) -> anyhow::Result<()> {
        let selected = protocol_override(std::env::var("CBREC_IMAGE_PROTOCOL").ok().as_deref())?;
        if selected == Some(ProtocolType::Halfblocks) {
            self.picker = Picker::halfblocks();
            return Ok(());
        }
        // Consultar Kitty clásico evita depender de los placeholders Unicode de ratatui-image.
        self.native =
            (selected.is_none() || selected == Some(ProtocolType::Kitty)) && query_native_support();
        let options = QueryStdioOptions {
            timeout: Duration::from_millis(250),
            ..QueryStdioOptions::default()
        };
        self.picker =
            Picker::from_query_stdio_with_options(options).unwrap_or_else(|_| Picker::halfblocks());
        if let Some(protocol) = selected {
            self.picker.set_protocol_type(protocol);
        }
        self.encoded = None;
        Ok(())
    }

    pub fn label(&self) -> &'static str {
        if self.native {
            return "Kitty · píxeles nativos";
        }
        match self.picker.protocol_type() {
            ProtocolType::Kitty => "Kitty",
            ProtocolType::Sixel => "Sixel",
            ProtocolType::Iterm2 => "iTerm2",
            ProtocolType::Halfblocks => "texto: resolución limitada por celdas",
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, image: &Arc<DynamicImage>, zoom: u16) {
        if area.is_empty() || image.width() == 0 || image.height() == 0 {
            return;
        }
        let zoom = zoom.max(1);
        if self.native {
            let changed = !self.native_image.as_ref().is_some_and(|cached| {
                Arc::ptr_eq(&cached.source, image) && cached.zoom == zoom && cached.area == area
            });
            if changed {
                let font = self.picker.font_size();
                let view = image.crop_imm(
                    (image.width() - (image.width() / u32::from(zoom)).max(1)) / 2,
                    (image.height() - (image.height() / u32::from(zoom)).max(1)) / 2,
                    (image.width() / u32::from(zoom)).max(1),
                    (image.height() / u32::from(zoom)).max(1),
                );
                let (width, height) =
                    fit_pixels(image.width(), image.height(), area, font.width, font.height);
                let view = view.resize_exact(width, height, FilterType::Lanczos3);
                self.native_image = Some(NativeImage {
                    source: image.clone(),
                    zoom,
                    area,
                    width,
                    height,
                    payload: STANDARD.encode(view.to_rgb8().as_raw()),
                });
                self.sent = false;
            }
            self.pending = self.native_image.is_some();
            return;
        }
        if !self
            .encoded
            .as_ref()
            .is_some_and(|(original, previous_zoom, _)| {
                Arc::ptr_eq(original, image) && *previous_zoom == zoom
            })
        {
            let width = (image.width() / u32::from(zoom)).max(1);
            let height = (image.height() / u32::from(zoom)).max(1);
            let view = image.crop_imm(
                (image.width() - width) / 2,
                (image.height() - height) / 2,
                width,
                height,
            );
            self.encoded = Some((image.clone(), zoom, self.picker.new_resize_protocol(view)));
        }
        if let Some((_, _, protocol)) = &mut self.encoded {
            // Ampliar sin pedir zoom suaviza la imagen; en texto, más celdas sí aportan detalle.
            let resize = if zoom > 1 || self.picker.protocol_type() == ProtocolType::Halfblocks {
                Resize::Scale(Some(FilterType::Lanczos3))
            } else {
                Resize::Fit(Some(FilterType::Lanczos3))
            };
            frame.render_stateful_widget(StatefulImage::default().resize(resize), area, protocol);
        }
    }

    pub fn is_native(&self) -> bool {
        self.native
    }

    pub fn begin_frame(&mut self) {
        self.pending = false;
    }

    pub fn erase(&mut self, output: &mut impl Write) -> io::Result<()> {
        if self.native {
            write!(output, "\x1b_Ga=d,d=I,i={},p=1,q=2;\x1b\\", image_id())?;
            output.flush()?;
            self.sent = false;
        }
        Ok(())
    }

    pub fn flush(&mut self, output: &mut impl Write) -> io::Result<()> {
        if self.native && !self.pending && !self.sent {
            // La colocación transparente fuerza el repintado tras borrar la última imagen en Konsole.
            write!(
                output,
                "\x1b_Ga=T,f=32,s=1,v=1,i={},p=1,C=1,q=2;AAAAAA==\x1b\\",
                image_id()
            )?;
            output.flush()?;
            self.sent = true;
        }
        if self.pending && !self.sent {
            if let Some(image) = &self.native_image {
                write!(output, "\x1b[{};{}H", image.area.y + 1, image.area.x + 1)?;
                write_native_image(output, &image.payload, image.width, image.height)?;
                output.flush()?;
                self.sent = true;
            }
        }
        Ok(())
    }
}

fn image_id() -> u32 {
    (std::process::id() & 0x3fff_ffff) | 0x4000_0000
}

fn fit_pixels(
    width: u32,
    height: u32,
    area: Rect,
    font_width: u16,
    font_height: u16,
) -> (u32, u32) {
    let scale = (f64::from(area.width) * f64::from(font_width.max(1)) / f64::from(width))
        .min(f64::from(area.height) * f64::from(font_height.max(1)) / f64::from(height))
        .min(1.0);
    (
        (f64::from(width) * scale).floor().max(1.0) as u32,
        (f64::from(height) * scale).floor().max(1.0) as u32,
    )
}

fn write_native_image(
    output: &mut impl Write,
    payload: &str,
    width: u32,
    height: u32,
) -> io::Result<()> {
    for (index, chunk) in payload.as_bytes().chunks(4096).enumerate() {
        let more = u8::from((index + 1) * 4096 < payload.len());
        if index == 0 {
            write!(
                output,
                "\x1b_Ga=T,f=24,s={width},v={height},i={},p=1,C=1,q=2,m={more};",
                image_id()
            )?;
        } else {
            write!(output, "\x1b_Gm={more};")?;
        }
        output.write_all(chunk)?;
        output.write_all(b"\x1b\\")?;
    }
    Ok(())
}

#[cfg(unix)]
fn query_native_support() -> bool {
    use std::fs::OpenOptions;
    let Ok(mut tty) = OpenOptions::new().read(true).write(true).open("/dev/tty") else {
        return false;
    };
    query_native(&mut tty)
}

#[cfg(unix)]
fn query_native(tty: &mut (impl std::io::Read + Write + std::os::fd::AsRawFd)) -> bool {
    use std::time::Instant;
    let id = image_id() - 1;
    if write!(tty, "\x1b_Gi={id},s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\")
        .and_then(|_| tty.flush())
        .is_err()
    {
        return false;
    }
    let deadline = Instant::now() + Duration::from_millis(250);
    let mut reply = Vec::new();
    while Instant::now() < deadline && reply.len() < 512 {
        let mut poll = libc::pollfd {
            fd: tty.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let timeout = deadline
            .saturating_duration_since(Instant::now())
            .as_millis() as i32;
        // SAFETY: pollfd está inicializado, su dirección es válida y tty conserva abierto el descriptor.
        let ready = unsafe { libc::poll(&mut poll, 1, timeout) };
        if ready < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
            continue;
        }
        if ready <= 0 || poll.revents & libc::POLLIN == 0 {
            break;
        }
        let mut buffer = [0; 128];
        match tty.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => reply.extend_from_slice(&buffer[..count]),
        }
        if native_reply(&reply, id) {
            return true;
        }
    }
    false
}

fn native_reply(reply: &[u8], id: u32) -> bool {
    let expected = format!("\x1b_Gi={id};OK\x1b\\");
    reply
        .windows(expected.len())
        .any(|part| part == expected.as_bytes())
}

#[cfg(not(unix))]
fn query_native_support() -> bool {
    false
}

fn protocol_override(value: Option<&str>) -> anyhow::Result<Option<ProtocolType>> {
    Ok(match value.unwrap_or("auto") {
        "auto" => None,
        "text" => Some(ProtocolType::Halfblocks),
        "kitty" => Some(ProtocolType::Kitty),
        "sixel" => Some(ProtocolType::Sixel),
        "iterm2" => Some(ProtocolType::Iterm2),
        _ => anyhow::bail!("CBREC_IMAGE_PROTOCOL debe ser auto, text, kitty, sixel o iterm2"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn classic_image_preserves_pixels_caches_and_clears_its_placement() {
        let image = Arc::new(DynamicImage::ImageRgb8(image::ImageBuffer::from_fn(
            480,
            270,
            |x, y| image::Rgb([(x % 251) as u8, (y % 251) as u8, 79]),
        )));
        let mut renderer = PreviewRenderer::new();
        renderer.native = true;
        let mut terminal = Terminal::new(TestBackend::new(80, 40)).unwrap();
        renderer.begin_frame();
        terminal
            .draw(|frame| renderer.render(frame, Rect::new(2, 3, 60, 20), &image, 1))
            .unwrap();
        let pixels = STANDARD
            .decode(&renderer.native_image.as_ref().unwrap().payload)
            .unwrap();
        assert_eq!(pixels, image.to_rgb8().into_raw());
        let mut output = Vec::new();
        renderer.flush(&mut output).unwrap();
        assert!(output.starts_with(b"\x1b[4;3H\x1b_Ga=T,f=24,s=480,v=270"));
        assert!(!output.windows(4).any(|part| part == b"U=1,"));
        output.clear();
        renderer.flush(&mut output).unwrap();
        assert!(
            output.is_empty(),
            "No retransmitir una imagen estática en cada frame"
        );
        renderer.erase(&mut output).unwrap();
        assert_eq!(
            String::from_utf8(output.clone()).unwrap(),
            format!("\x1b_Ga=d,d=I,i={},p=1,q=2;\x1b\\", image_id())
        );
        renderer.begin_frame();
        output.clear();
        renderer.flush(&mut output).unwrap();
        assert!(
            String::from_utf8(output.clone())
                .unwrap()
                .contains("f=32,s=1,v=1"),
            "Inicio debe forzar el repintado con un píxel transparente"
        );
        output.clear();
        renderer.flush(&mut output).unwrap();
        assert!(output.is_empty());
        terminal
            .draw(|frame| renderer.render(frame, Rect::new(2, 3, 20, 10), &image, 2))
            .unwrap();
        let cached = renderer.native_image.as_ref().unwrap();
        assert_eq!(cached.zoom, 2);
        assert_eq!((cached.width, cached.height), (200, 112));
        assert_eq!(
            STANDARD.decode(&cached.payload).unwrap().len(),
            200 * 112 * 3
        );
        assert!(renderer.label().contains("nativos"));
    }

    #[test]
    fn classic_payload_chunks_and_final_marker_are_bounded() {
        let mut output = Vec::new();
        write_native_image(&mut output, &"A".repeat(9000), 100, 30).unwrap();
        let text = String::from_utf8(output).unwrap();
        let commands: Vec<_> = text
            .split("\x1b\\")
            .filter(|part| !part.is_empty())
            .collect();
        assert_eq!(commands.len(), 3);
        for (index, command) in commands.iter().enumerate() {
            let (header, payload) = command.split_once(';').unwrap();
            assert!(payload.len() <= 4096);
            assert!(header.ends_with(if index == 2 { "m=0" } else { "m=1" }));
        }
    }

    #[test]
    fn native_query_accepts_only_complete_matching_success() {
        assert!(native_reply(b"noise\x1b_Gi=42;OK\x1b\\", 42));
        assert!(!native_reply(b"\x1b_Gi=41;OK\x1b\\", 42));
        assert!(!native_reply(b"\x1b_Gi=42;ERROR\x1b\\", 42));
        assert!(!native_reply(b"\x1b_Gi=42;OK", 42));
        assert!(!native_reply(b"", 42));
        assert_eq!(
            fit_pixels(480, 270, Rect::new(0, 0, 60, 20), 10, 20),
            (480, 270)
        );
        assert_eq!(
            fit_pixels(480, 270, Rect::new(0, 0, 20, 10), 10, 20),
            (200, 112)
        );
    }

    #[cfg(unix)]
    #[test]
    fn native_query_handles_fragmented_replies_timeout_and_closed_peer() {
        use std::{io::Read, os::unix::net::UnixStream};
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let peer = std::thread::spawn(move || {
            server
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let expected = format!("\x1b_Gi={},s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\", image_id() - 1);
            let mut request = vec![0; expected.len()];
            server.read_exact(&mut request).unwrap();
            assert_eq!(request, expected.as_bytes());
            write!(server, "\x1b_Gi={};O", image_id() - 1).unwrap();
            std::thread::sleep(Duration::from_millis(10));
            server.write_all(b"K\x1b\\").unwrap();
        });
        assert!(query_native(&mut client));
        peer.join().unwrap();
        let (mut client, server) = UnixStream::pair().unwrap();
        let start = std::time::Instant::now();
        assert!(!query_native(&mut client));
        assert!(start.elapsed() < Duration::from_secs(2));
        drop(server);
        assert!(!query_native(&mut client));
    }

    #[test]
    fn native_protocol_encodes_the_source_and_reuses_the_state() {
        let image = Arc::new(DynamicImage::new_rgb8(480, 270));
        let mut renderer = PreviewRenderer::new();
        renderer.picker.set_protocol_type(ProtocolType::Kitty);
        let mut terminal = Terminal::new(TestBackend::new(30, 12)).unwrap();
        terminal
            .draw(|frame| renderer.render(frame, frame.area(), &image, 1))
            .unwrap();
        let symbols = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(
            symbols.contains("\u{1b}_G"),
            "Kitty debe recibir una imagen, no bloques de texto"
        );
        let encoded = &renderer.encoded.as_ref().unwrap().0;
        assert!(Arc::ptr_eq(encoded, &image));
        terminal
            .draw(|frame| renderer.render(frame, frame.area(), &image, 1))
            .unwrap();
        assert_eq!(renderer.label(), "Kitty");
        terminal
            .draw(|frame| renderer.render(frame, frame.area(), &image, 2))
            .unwrap();
        assert_eq!(renderer.encoded.as_ref().unwrap().1, 2);
        assert_eq!((image.width(), image.height()), (480, 270));
    }
    #[test]
    fn graphical_preview_keeps_natural_pixels_until_zoom_is_requested() {
        let image = Arc::new(DynamicImage::ImageRgb8(image::ImageBuffer::from_fn(
            480,
            260,
            |x, y| image::Rgb([(x % 251) as u8, (y % 251) as u8, 79]),
        )));
        let mut renderer = PreviewRenderer::new();
        renderer.picker.set_protocol_type(ProtocolType::Iterm2);
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        let pixels = |renderer: &PreviewRenderer| {
            let protocol = &renderer.encoded.as_ref().unwrap().2;
            let ratatui_image::protocol::StatefulProtocolType::ITerm2(encoded) =
                protocol.protocol_type()
            else {
                panic!("Se esperaba iTerm2");
            };
            let payload = encoded
                .data
                .split_once("doNotMoveCursor=1:")
                .unwrap()
                .1
                .trim_end_matches('\u{7}');
            image::load_from_memory(&STANDARD.decode(payload).unwrap())
                .unwrap()
                .to_rgb8()
        };
        terminal
            .draw(|frame| renderer.render(frame, frame.area(), &image, 1))
            .unwrap();
        let original = pixels(&renderer);
        assert_eq!(original.dimensions(), image.to_rgb8().dimensions());
        assert!(
            original == image.to_rgb8(),
            "Un panel grande debe conservar cada píxel original"
        );
        terminal
            .draw(|frame| renderer.render(frame, frame.area(), &image, 2))
            .unwrap();
        assert!(
            pixels(&renderer).width() > image.width() / 2,
            "El zoom explícito debe seguir ampliando el recorte"
        );
        terminal
            .draw(|frame| renderer.render(frame, Rect::new(0, 0, 20, 8), &image, 1))
            .unwrap();
        let fitted = pixels(&renderer);
        assert!(fitted.width() <= 200 && fitted.height() <= 160);
        assert_eq!((image.width(), image.height()), (480, 260));
    }

    #[test]
    fn small_text_preview_uses_available_cells_for_detail() {
        let image = Arc::new(DynamicImage::ImageRgb8(image::ImageBuffer::from_pixel(
            10,
            10,
            image::Rgb([240, 80, 40]),
        )));
        let mut renderer = PreviewRenderer::new();
        let mut terminal = Terminal::new(TestBackend::new(30, 12)).unwrap();
        terminal
            .draw(|frame| renderer.render(frame, frame.area(), &image, 1))
            .unwrap();
        let painted = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .filter(|cell| matches!(cell.fg, ratatui::style::Color::Rgb(240, 80, 40)))
            .count();
        assert!(
            painted > 1,
            "El modo texto debe aprovechar más de una celda"
        );
    }

    #[test]
    fn protocol_selection_is_explicit_and_validated() {
        assert_eq!(protocol_override(None).unwrap(), None);
        for (name, protocol) in [
            ("text", ProtocolType::Halfblocks),
            ("kitty", ProtocolType::Kitty),
            ("sixel", ProtocolType::Sixel),
            ("iterm2", ProtocolType::Iterm2),
        ] {
            assert_eq!(protocol_override(Some(name)).unwrap(), Some(protocol));
        }
        assert!(protocol_override(Some("invalid")).is_err());
    }
    #[test]
    fn text_fallback_paints_pixels_without_reducing_the_source() {
        let image = Arc::new(DynamicImage::ImageRgb8(image::ImageBuffer::from_fn(
            480,
            270,
            |_, y| image::Rgb([240, (y % 255) as u8, 40]),
        )));
        let mut renderer = PreviewRenderer::new();
        let mut terminal = Terminal::new(TestBackend::new(30, 12)).unwrap();
        terminal
            .draw(|frame| renderer.render(frame, frame.area(), &image, 1))
            .unwrap();
        assert!(terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(
                |cell| matches!(cell.fg, ratatui::style::Color::Rgb(240, _, 40))
                    || matches!(cell.bg, ratatui::style::Color::Rgb(240, _, 40))
            ));
        assert_eq!((image.width(), image.height()), (480, 270));
        assert!(renderer.label().contains("texto"));
    }
}
