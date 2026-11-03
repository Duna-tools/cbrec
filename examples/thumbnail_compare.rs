//! Sonda manual de pérdida de detalle: misma imagen y renderizador real, sin terminal ni red.
#[path = "../src/application/thumbnail.rs"]
#[allow(
    dead_code,
    reason = "la sonda solo utiliza el decodificador compartido"
)]
mod decoder;
#[path = "../src/presentation/thumbnail.rs"]
#[allow(
    dead_code,
    reason = "la sonda fija el fallback sin consultar una terminal"
)]
mod renderer;

use image::{imageops::FilterType, DynamicImage, ImageBuffer, Rgb, RgbImage};
use ratatui::{backend::TestBackend, style::Color, Terminal};
use std::{io::Cursor, path::Path, sync::Arc};

fn rgb(color: Color) -> Option<Rgb<u8>> {
    match color {
        Color::Rgb(r, g, b) => Some(Rgb([r, g, b])),
        _ => None,
    }
}

fn capture(
    image: &Arc<DynamicImage>,
    width: u16,
    height: u16,
    zoom: u16,
) -> anyhow::Result<RgbImage> {
    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
    let mut renderer = renderer::PreviewRenderer::new();
    terminal.draw(|frame| renderer.render(frame, frame.area(), image, zoom))?;
    let buffer = terminal.backend().buffer();
    let mut painted_width = 0;
    let mut painted_height = 0;
    for y in 0..height {
        for x in 0..width {
            let cell = &buffer[(x, y)];
            if rgb(cell.fg).is_some() || rgb(cell.bg).is_some() {
                painted_width = painted_width.max(u32::from(x) + 1);
                painted_height = painted_height.max(u32::from(y) + 1);
            }
        }
    }
    anyhow::ensure!(
        painted_width > 0 && painted_height > 0,
        "El renderizador no pintó la imagen"
    );
    let mut output = RgbImage::new(painted_width, painted_height * 2);
    for y in 0..painted_height {
        for x in 0..painted_width {
            let cell = &buffer[(x as u16, y as u16)];
            let fg = rgb(cell.fg).unwrap_or(Rgb([0, 0, 0]));
            let bg = rgb(cell.bg).unwrap_or(Rgb([0, 0, 0]));
            let (upper, lower) = match cell.symbol() {
                "▀" => (fg, bg),
                "▄" => (bg, fg),
                " " => (bg, bg),
                _ => anyhow::bail!("El fallback emitió un símbolo no esperado"),
            };
            output.put_pixel(x, y * 2, upper);
            output.put_pixel(x, y * 2 + 1, lower);
        }
    }
    assert!(output.width() <= u32::from(width));
    assert!(output.height() <= u32::from(height) * 2);
    Ok(output)
}

fn main() -> anyhow::Result<()> {
    let arguments: Vec<_> = std::env::args_os().collect();
    anyhow::ensure!(
        (2..=3).contains(&arguments.len()),
        "Uso: thumbnail_compare DIRECTORIO [IMAGEN_JPEG_PNG]"
    );
    let directory = Path::new(&arguments[1]);
    std::fs::create_dir_all(directory)?;
    let synthetic = arguments.get(2).is_none();
    let bytes = if let Some(path) = arguments.get(2) {
        std::fs::read(path)?
    } else {
        let source = DynamicImage::ImageRgb8(ImageBuffer::from_fn(480, 270, |x, y| {
            let band = if y < 90 {
                1
            } else if y < 180 {
                6
            } else {
                12
            };
            let value = if (x / band) % 2 == 0 { 240 } else { 15 };
            Rgb([value, value, value])
        }));
        let mut bytes = Cursor::new(Vec::new());
        source.write_to(&mut bytes, image::ImageFormat::Jpeg)?;
        bytes.into_inner()
    };
    let source = Arc::new(decoder::decode_thumbnail(&bytes)?);
    let reference = image::load_from_memory(&bytes)?;
    assert_eq!(
        source.as_bytes(),
        reference.as_bytes(),
        "El decodificador no debe reducir la imagen"
    );
    source.save(directory.join("decoded.png"))?;
    let input_name = match image::guess_format(&bytes)? {
        image::ImageFormat::Jpeg => "source.jpg",
        image::ImageFormat::Png => "source.png",
        _ => anyhow::bail!("La sonda compara JPEG o PNG"),
    };
    std::fs::write(directory.join(input_name), &bytes)?;
    let mut cases = Vec::new();
    for (width, height, zoom) in [(60, 20, 1), (60, 20, 2), (120, 40, 1)] {
        let rendered = capture(&source, width, height, zoom)?;
        let name = format!("halfblocks-{width}x{height}-zoom{zoom}.png");
        rendered.save(directory.join(&name))?;
        if width == 60 && zoom == 1 {
            image::imageops::resize(
                &rendered,
                source.width(),
                source.height(),
                FilterType::Nearest,
            )
            .save(directory.join("halfblocks-enlarged.png"))?;
        }
        cases.push(serde_json::json!({"cells":[width,height],"zoom":zoom,"effective_pixels":[rendered.width(),rendered.height()],"image":name}));
    }
    let report = serde_json::json!({"source":[source.width(),source.height()],"decoder_preserves_pixels":true,"synthetic":synthetic,"input":input_name,"cases":cases});
    std::fs::write(
        directory.join("rendering.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{report}");
    Ok(())
}
