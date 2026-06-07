use crate::domain::repositories::StreamRepository;
use crate::domain::value_objects::{StreamUrl, VideoQuality};
use crate::infrastructure::{ChaturbateClient, InfrastructureError};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

pub(crate) enum ResultadoGrabacion {
    Guardado(PathBuf),
    Pequeno(PathBuf, u64),
    Cancelado,
}

pub(crate) fn ruta_parcial(ruta: &Path) -> PathBuf {
    ruta.with_file_name(format!(
        "{}.part",
        ruta.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("output")
    ))
}

pub(crate) async fn descargar_grabacion(
    client: &ChaturbateClient,
    stream_url: &StreamUrl,
    ruta: PathBuf,
    quality: VideoQuality,
    min_file_size: u64,
) -> Result<ResultadoGrabacion, InfrastructureError> {
    let parcial = ruta_parcial(&ruta);

    if let Some(parent) = ruta.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    match client.download_stream(stream_url, &parcial, quality).await {
        Ok(()) => {}
        Err(InfrastructureError::RecordingCancelled) => return Ok(ResultadoGrabacion::Cancelado),
        Err(e) => return Err(e),
    }

    let meta = tokio::fs::metadata(&parcial).await?;

    if meta.len() < min_file_size {
        let small_dir = ruta
            .parent()
            .map(|p| p.join("small"))
            .unwrap_or_else(|| PathBuf::from("small"));
        tokio::fs::create_dir_all(&small_dir).await?;
        let destino = small_dir
            .join(ruta.file_name().unwrap_or_else(|| OsStr::new("cbrec.partial")));
        tokio::fs::rename(&parcial, &destino).await?;
        Ok(ResultadoGrabacion::Pequeno(destino, meta.len()))
    } else {
        tokio::fs::rename(&parcial, &ruta).await?;
        Ok(ResultadoGrabacion::Guardado(ruta))
    }
}
