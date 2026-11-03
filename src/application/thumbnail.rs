//! Decodificación limitada y caché efímera de imágenes públicas; no persiste sesiones.

use image::{DynamicImage, ImageReader, Limits};
use lru::LruCache;
use std::io::Cursor;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(crate) fn decode_thumbnail(bytes: &[u8]) -> anyhow::Result<DynamicImage> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(1280);
    limits.max_image_height = Some(720);
    limits.max_alloc = Some(16 * 1024 * 1024);
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    reader.limits(limits);
    Ok(reader.decode()?)
}

struct CachedThumbnail {
    source: String,
    image: Arc<DynamicImage>,
    expires: Instant,
}

pub(crate) struct ThumbnailCache {
    entries: LruCache<String, CachedThumbnail>,
}

impl ThumbnailCache {
    pub fn new() -> Self {
        Self {
            entries: LruCache::new(NonZeroUsize::new(8).unwrap()),
        }
    }

    pub fn get(&mut self, model: &str, source: &str) -> Option<Arc<DynamicImage>> {
        let cached = self.entries.get(model)?;
        (cached.source == source && Instant::now() < cached.expires).then(|| cached.image.clone())
    }

    pub fn insert(
        &mut self,
        model: String,
        source: String,
        image: Arc<DynamicImage>,
        freshness: Duration,
    ) {
        let bytes = image.as_bytes().len();
        self.entries.pop(&model);
        if bytes > 16 * 1024 * 1024 || freshness.is_zero() {
            return;
        }
        while self
            .entries
            .iter()
            .map(|(_, value)| value.image.as_bytes().len())
            .sum::<usize>()
            + bytes
            > 16 * 1024 * 1024
        {
            if self.entries.pop_lru().is_none() {
                break;
            }
        }
        self.entries.put(
            model,
            CachedThumbnail {
                source,
                image,
                expires: Instant::now() + freshness.min(Duration::from_secs(15)),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, ImageFormat, Rgb};

    fn jpeg() -> Vec<u8> {
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_fn(480, 270, |x, y| {
            Rgb([x as u8, y as u8, 80])
        }));
        let mut encoded = Cursor::new(Vec::new());
        image.write_to(&mut encoded, ImageFormat::Jpeg).unwrap();
        encoded.into_inner()
    }

    #[test]
    fn decoder_preserves_source_resolution_and_rejects_invalid_images() {
        let decoded = decode_thumbnail(&jpeg()).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (480, 270));
        assert!(decode_thumbnail(b"<html>blocked</html>").is_err());
        let large = DynamicImage::new_rgb8(1281, 1);
        let mut encoded = Cursor::new(Vec::new());
        large.write_to(&mut encoded, ImageFormat::Png).unwrap();
        assert!(decode_thumbnail(&encoded.into_inner()).is_err());
    }

    #[test]
    fn cache_reuses_pixels_expires_and_evicts_without_growing() {
        let mut cache = ThumbnailCache::new();
        let image = Arc::new(decode_thumbnail(&jpeg()).unwrap());
        cache.insert(
            "alice".into(),
            "url-1".into(),
            image.clone(),
            Duration::from_secs(15),
        );
        assert!(Arc::ptr_eq(&cache.get("alice", "url-1").unwrap(), &image));
        assert!(cache.get("alice", "url-2").is_none());
        cache.entries.get_mut("alice").unwrap().expires = Instant::now() - Duration::from_secs(1);
        assert!(cache.get("alice", "url-1").is_none());
        for index in 0..10 {
            cache.insert(
                index.to_string(),
                "url".into(),
                image.clone(),
                Duration::from_secs(15),
            );
        }
        assert_eq!(cache.entries.len(), 8);
        assert!(cache.get("0", "url").is_none());
        cache.insert("no-store".into(), "url".into(), image, Duration::ZERO);
        assert!(cache.get("no-store", "url").is_none());
        let image = Arc::new(DynamicImage::new_rgb8(1280, 720));
        for index in 0..8 {
            cache.insert(
                format!("large-{index}"),
                "url".into(),
                image.clone(),
                Duration::from_secs(15),
            );
        }
        assert!(
            cache
                .entries
                .iter()
                .map(|(_, value)| value.image.as_bytes().len())
                .sum::<usize>()
                <= 16 * 1024 * 1024
        );
        cache.insert("large-7".into(), "url".into(), image, Duration::ZERO);
        assert!(cache.get("large-7", "url").is_none());
    }
}
