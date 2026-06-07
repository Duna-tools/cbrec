use crate::domain::value_objects::{ModelName, StreamUrl, VideoQuality};
use async_trait::async_trait;

#[async_trait]
pub trait StreamRepository: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    async fn get_stream_url(
        &self,
        model_name: &ModelName,
    ) -> Result<Option<StreamUrl>, Self::Error>;

    async fn download_stream(
        &self,
        stream_url: &StreamUrl,
        output_path: &std::path::Path,
        quality: VideoQuality,
    ) -> Result<(), Self::Error>;
}
