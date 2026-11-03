//! Sonda manual: imagen sintética para verificar el protocolo gráfico sin cuentas ni red.
#[path = "../src/presentation/thumbnail.rs"]
mod thumbnail;
use image::{DynamicImage, ImageBuffer, Rgb};
use ratatui::{
    crossterm::{execute, style::ResetColor},
    layout::Rect,
    style::Style,
    widgets::{Block, Paragraph},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn main() -> anyhow::Result<()> {
    let image = Arc::new(DynamicImage::ImageRgb8(ImageBuffer::from_fn(
        480,
        270,
        |x, y| {
            let band = if (x / 24 + y / 24) % 2 == 0 { 220 } else { 35 };
            Rgb([band, (x * 255 / 480) as u8, (y * 255 / 270) as u8])
        },
    )));
    let mut terminal = ratatui::try_init()?;
    let result = (|| -> anyhow::Result<()> {
        let mut renderer = thumbnail::PreviewRenderer::new();
        renderer.detect()?;
        let started = Instant::now();
        let mut previous = None;
        while started.elapsed() < Duration::from_secs(10) {
            let phase = started.elapsed().as_secs() / 2;
            let view = (phase, terminal.size()?);
            if renderer.is_native() && previous.as_ref() != Some(&view) {
                renderer.erase(&mut std::io::stdout())?;
                execute!(std::io::stdout(), ResetColor)?;
                terminal.clear()?;
            }
            previous = Some(view);
            renderer.begin_frame();
            terminal.draw(|frame| {
                let area = frame.area();
                frame.render_widget(
                    Block::default().style(Style::default().bg(thumbnail::BACKGROUND)),
                    area,
                );
                frame.render_widget(
                    Paragraph::new(format!(
                        "cbrec · Imagen sintética 480x270 · {} · fase {}",
                        renderer.label(),
                        phase
                    )),
                    Rect::new(0, 0, area.width, 2),
                );
                if phase != 2 && phase != 4 {
                    renderer.render(
                        frame,
                        Rect::new(
                            2,
                            3,
                            if phase == 3 {
                                area.width.saturating_sub(4) / 2
                            } else {
                                area.width.saturating_sub(4)
                            },
                            if phase == 3 {
                                area.height.saturating_sub(5) / 2
                            } else {
                                area.height.saturating_sub(5)
                            },
                        ),
                        &image,
                        if phase == 1 { 2 } else { 1 },
                    );
                } else {
                    frame.render_widget(
                        Paragraph::new("Inicio · Sin imagen: no deben quedar gráficos de la sala"),
                        Rect::new(2, 3, area.width.saturating_sub(4), 3),
                    );
                }
            })?;
            renderer.flush(&mut std::io::stdout())?;
            std::thread::sleep(Duration::from_millis(100));
        }
        renderer.begin_frame();
        renderer.erase(&mut std::io::stdout())?;
        execute!(std::io::stdout(), ResetColor)?;
        terminal.clear()?;
        terminal.draw(|frame| {
            frame.render_widget(
                Block::default().style(Style::default().bg(thumbnail::BACKGROUND)),
                frame.area(),
            )
        })?;
        renderer.flush(&mut std::io::stdout())?;
        Ok(())
    })();
    ratatui::restore();
    result
}
