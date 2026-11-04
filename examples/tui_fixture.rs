//! QA visual de la TUI real con salas e imágenes sintéticas, sin red ni grabaciones.
pub use cbrec::domain;
#[path = "../src/presentation/thumbnail.rs"]
pub(crate) mod thumbnail;
mod presentation {
    pub(crate) use super::thumbnail;
    #[allow(dead_code)]
    pub(crate) mod output {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/presentation/output.rs"
        ));
    }
    #[cfg(test)]
    pub use output::{ConsoleOutput, Output};
}
#[allow(dead_code)]
#[path = "../src/presentation/tui.rs"]
mod tui;

use image::{DynamicImage, ImageBuffer, Rgb};
use std::sync::Arc;
use tui::{TuiAction, TuiEvent, TuiInitial, TuiPage, TuiRoom};

fn main() -> anyhow::Result<()> {
    let (commands, mut actions) = tokio::sync::mpsc::channel(16);
    let (events, updates) = tokio::sync::mpsc::channel(16);
    let worker = std::thread::spawn(move || {
        while let Some(action) = actions.blocking_recv() {
            let event = match action {
                TuiAction::Search(tag, offset, generation) => TuiEvent::Rooms(
                    tag,
                    offset,
                    generation,
                    Ok(TuiPage {
                        rooms: vec![
                            TuiRoom::new(
                                "sala_demo_1".into(),
                                120,
                                "public".into(),
                                "Patrón sintético · QA sin red".into(),
                            ),
                            TuiRoom::new(
                                "sala_demo_2".into(),
                                48,
                                "public".into(),
                                "Segunda imagen · QA sin red".into(),
                            ),
                        ],
                        total: Some(2),
                        has_more: false,
                    }),
                ),
                TuiAction::Preview(model) | TuiAction::RefreshPreview(model) => {
                    let second = model.ends_with('2');
                    let image = DynamicImage::ImageRgb8(ImageBuffer::from_fn(480, 270, |x, y| {
                        let stripe = if x % 8 < 4 { 240 } else { 20 };
                        if second {
                            Rgb([20, stripe, (y * 255 / 270) as u8])
                        } else {
                            Rgb([stripe, (x * 255 / 480) as u8, (y * 255 / 270) as u8])
                        }
                    }));
                    TuiEvent::Preview(model, Ok(Arc::new(image)))
                }
                TuiAction::Quit => break,
                _ => TuiEvent::Notice("QA: no se ejecutan operaciones externas".into()),
            };
            if events.blocking_send(event).is_err() {
                break;
            }
        }
    });
    let result = tui::run_discovery_tui(
        TuiInitial {
            tag: String::new(),
            page_size: 20,
            session_present: false,
            max_recordings: 2,
            quality: domain::value_objects::VideoQuality::Best,
            output_folder: std::env::temp_dir().join("cbrec-tui-fixture"),
        },
        commands,
        updates,
    );
    worker
        .join()
        .map_err(|_| anyhow::anyhow!("Falló el canal de QA"))?;
    result
}
