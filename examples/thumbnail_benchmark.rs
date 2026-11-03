//! Compara decodificación y arranque por imagen con un JPEG sintético; ejecutar manualmente.
#[path = "../src/application/thumbnail.rs"]
#[allow(
    dead_code,
    reason = "el benchmark solo utiliza el decodificador del módulo compartido"
)]
mod thumbnail;
use std::time::Instant;
use thumbnail::decode_thumbnail;
fn main() {
    let source = image::DynamicImage::ImageRgb8(image::ImageBuffer::from_fn(480, 270, |x, y| {
        image::Rgb([x as u8, y as u8, 80])
    }));
    let mut encoded = std::io::Cursor::new(Vec::new());
    source
        .write_to(&mut encoded, image::ImageFormat::Jpeg)
        .unwrap();
    let bytes = encoded.into_inner();
    let path = std::env::temp_dir().join(format!(
        "cbrec-thumbnail-benchmark-{}.jpg",
        std::process::id()
    ));
    std::fs::write(&path, &bytes).unwrap();
    let mut old = Vec::new();
    let mut new = Vec::new();
    for _ in 0..7 {
        let started = Instant::now();
        let output = std::process::Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-i"])
            .arg(&path)
            .args([
                "-vf",
                "scale=64:36",
                "-frames:v",
                "1",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "pipe:1",
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout.len(), 6912);
        old.push(started.elapsed().as_secs_f64() * 1000.0);
        let started = Instant::now();
        let image = decode_thumbnail(&bytes).unwrap();
        new.push(started.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(image.width(), 480);
    }
    std::fs::remove_file(path).unwrap();
    println!("thumbnail_decode_benchmark ffmpeg_ms={old:?} native_ms={new:?}");
}
