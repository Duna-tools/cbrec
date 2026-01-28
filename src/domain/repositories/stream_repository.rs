use crate::domain::value_objects::{ModelName, StreamUrl};
use async_trait::async_trait;

/// Contrato para consultar y descargar streams.
#[async_trait]
pub trait StreamRepository: Send + Sync {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Obtiene la URL del stream si el modelo esta online.
    /// # Arguments
    /// - `model_name`: modelo validado.
    /// # Returns
    /// - `Ok(Some(url))` si hay stream; `Ok(None)` si esta offline.
    /// # Errors
    /// - `Self::Error` si falla la consulta.
    async fn get_stream_url(
        &self,
        model_name: &ModelName,
    ) -> Result<Option<StreamUrl>, Self::Error>;

    /// Descarga el stream HLS y lo guarda en disco.
    /// # Arguments
    /// - `stream_url`: URL validada.
    /// - `output_path`: ruta de salida.
    /// - `quality`: preset de calidad.
    /// # Errors
    /// - `Self::Error` si falla la descarga o ffmpeg.
    async fn download_stream(
        &self,
        stream_url: &StreamUrl,
        output_path: &std::path::Path,
        quality: &str,
    ) -> Result<(), Self::Error>;
}
