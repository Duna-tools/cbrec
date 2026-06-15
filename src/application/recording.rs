use crate::domain::repositories::StreamRepository;
use crate::domain::value_objects::{StreamUrl, VideoQuality};
use crate::infrastructure::InfrastructureError;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use tokio::task::JoinHandle;

pub(crate) enum ResultadoGrabacion {
    Guardado(PathBuf),
    Pequeno(PathBuf, u64),
    Cancelado,
}

pub(crate) fn ruta_parcial(ruta: &Path) -> PathBuf {
    let nombre = match (
        ruta.file_stem().and_then(|n| n.to_str()),
        ruta.extension().and_then(|n| n.to_str()),
    ) {
        (Some(stem), Some(extension)) => format!("{stem}.part.{extension}"),
        (Some(stem), None) => format!("{stem}.part"),
        _ => "output.part".to_string(),
    };
    ruta.with_file_name(nombre)
}

pub(crate) async fn descargar_grabacion<R>(
    client: &R,
    stream_url: &StreamUrl,
    ruta: PathBuf,
    quality: VideoQuality,
    min_file_size: Option<u64>,
) -> Result<ResultadoGrabacion, InfrastructureError>
where
    R: StreamRepository<Error = InfrastructureError>,
{
    let parcial = ruta_parcial(&ruta);

    if let Some(parent) = ruta.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    match client.download_stream(stream_url, &parcial, quality).await {
        Ok(()) => {}
        Err(InfrastructureError::RecordingCancelled) => {
            if !parcial_aprovechable(&parcial).await {
                limpiar_parcial(&parcial).await;
                return Ok(ResultadoGrabacion::Cancelado);
            }
        }
        Err(e) => {
            limpiar_parcial(&parcial).await;
            return Err(e);
        }
    }

    let meta = match tokio::fs::metadata(&parcial).await {
        Ok(meta) => meta,
        Err(e) => {
            limpiar_parcial(&parcial).await;
            return Err(e.into());
        }
    };

    if min_file_size.is_some_and(|min_file_size| meta.len() < min_file_size) {
        let small_dir = ruta
            .parent()
            .map(|p| p.join("small"))
            .unwrap_or_else(|| PathBuf::from("small"));
        tokio::fs::create_dir_all(&small_dir).await?;
        let destino = small_dir.join(
            ruta.file_name()
                .unwrap_or_else(|| OsStr::new("cbrec.partial")),
        );
        tokio::fs::rename(&parcial, &destino).await?;
        Ok(ResultadoGrabacion::Pequeno(destino, meta.len()))
    } else {
        tokio::fs::rename(&parcial, &ruta).await?;
        Ok(ResultadoGrabacion::Guardado(ruta))
    }
}

async fn limpiar_parcial(parcial: &Path) {
    let _ = tokio::fs::remove_file(parcial).await;
}

async fn parcial_aprovechable(parcial: &Path) -> bool {
    tokio::fs::metadata(parcial)
        .await
        .map(|meta| meta.len() > 0)
        .unwrap_or(false)
}

pub(crate) async fn detener_tarea_progreso(task: JoinHandle<()>) {
    task.abort();
    let _ = task.await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::value_objects::ModelName;
    use async_trait::async_trait;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct RepoCancelado;
    struct RepoCanceladoConParcial;
    struct RepoOkConParcial;
    struct RepoErrorConParcial;

    #[async_trait]
    impl StreamRepository for RepoCancelado {
        type Error = InfrastructureError;

        async fn get_stream_url(
            &self,
            _model_name: &ModelName,
        ) -> Result<Option<StreamUrl>, Self::Error> {
            Ok(None)
        }

        async fn download_stream(
            &self,
            _stream_url: &StreamUrl,
            _output_path: &Path,
            _quality: VideoQuality,
        ) -> Result<(), Self::Error> {
            Err(InfrastructureError::RecordingCancelled)
        }
    }

    #[async_trait]
    impl StreamRepository for RepoCanceladoConParcial {
        type Error = InfrastructureError;

        async fn get_stream_url(
            &self,
            _model_name: &ModelName,
        ) -> Result<Option<StreamUrl>, Self::Error> {
            Ok(None)
        }

        async fn download_stream(
            &self,
            _stream_url: &StreamUrl,
            output_path: &Path,
            _quality: VideoQuality,
        ) -> Result<(), Self::Error> {
            tokio::fs::write(output_path, b"parcial").await?;
            Err(InfrastructureError::RecordingCancelled)
        }
    }

    #[async_trait]
    impl StreamRepository for RepoErrorConParcial {
        type Error = InfrastructureError;

        async fn get_stream_url(
            &self,
            _model_name: &ModelName,
        ) -> Result<Option<StreamUrl>, Self::Error> {
            Ok(None)
        }

        async fn download_stream(
            &self,
            _stream_url: &StreamUrl,
            output_path: &Path,
            _quality: VideoQuality,
        ) -> Result<(), Self::Error> {
            tokio::fs::write(output_path, b"parcial").await?;
            Err(InfrastructureError::RecordingError("fallo".to_string()))
        }
    }

    #[async_trait]
    impl StreamRepository for RepoOkConParcial {
        type Error = InfrastructureError;

        async fn get_stream_url(
            &self,
            _model_name: &ModelName,
        ) -> Result<Option<StreamUrl>, Self::Error> {
            Ok(None)
        }

        async fn download_stream(
            &self,
            _stream_url: &StreamUrl,
            output_path: &Path,
            _quality: VideoQuality,
        ) -> Result<(), Self::Error> {
            tokio::fs::write(output_path, b"parcial").await?;
            Ok(())
        }
    }

    fn ruta_temporal(nombre: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        std::env::temp_dir().join(format!("cbrec_test_{}_{}.mp4", nombre, nanos))
    }

    #[test]
    fn ruta_parcial_conserva_extension_para_ffmpeg() {
        let ruta = PathBuf::from("/tmp/alice.mp4");

        assert_eq!(ruta_parcial(&ruta), PathBuf::from("/tmp/alice.part.mp4"));
    }

    #[tokio::test]
    async fn descargar_grabacion_devuelve_cancelado_si_repo_cancela() {
        let repo = RepoCancelado;
        let stream_url = StreamUrl::try_from("https://example.com/stream.m3u8").unwrap();
        let ruta = ruta_temporal("cancelado");

        let resultado =
            descargar_grabacion(&repo, &stream_url, ruta, VideoQuality::Best, Some(1)).await;

        assert!(matches!(resultado, Ok(ResultadoGrabacion::Cancelado)));
    }

    #[tokio::test]
    async fn descargar_grabacion_conserva_parcial_si_se_cancela() {
        let repo = RepoCanceladoConParcial;
        let stream_url = StreamUrl::try_from("https://example.com/stream.m3u8").unwrap();
        let ruta = ruta_temporal("cancelado_con_parcial");
        let parcial = ruta_parcial(&ruta);

        let resultado = descargar_grabacion(
            &repo,
            &stream_url,
            ruta.clone(),
            VideoQuality::Best,
            Some(1024),
        )
        .await;

        let Ok(ResultadoGrabacion::Pequeno(destino, bytes)) = resultado else {
            panic!("se esperaba parcial pequeno");
        };
        assert_eq!(bytes, 7);
        assert_eq!(
            destino,
            ruta.parent()
                .expect("ruta con parent")
                .join("small")
                .join(ruta.file_name().expect("ruta con filename"))
        );
        assert!(destino.exists());
        assert!(!parcial.exists());
        let _ = tokio::fs::remove_file(destino).await;
    }

    #[tokio::test]
    async fn descargar_grabacion_limpia_parcial_si_falla_descarga() {
        let repo = RepoErrorConParcial;
        let stream_url = StreamUrl::try_from("https://example.com/stream.m3u8").unwrap();
        let ruta = ruta_temporal("error_con_parcial");
        let parcial = ruta_parcial(&ruta);

        let resultado =
            descargar_grabacion(&repo, &stream_url, ruta, VideoQuality::Best, Some(1)).await;

        assert!(resultado.is_err());
        assert!(!parcial.exists());
    }

    #[tokio::test]
    async fn descargar_grabacion_sin_umbral_guarda_clip_corto() {
        let repo = RepoOkConParcial;
        let stream_url = StreamUrl::try_from("https://example.com/stream.m3u8").unwrap();
        let ruta = ruta_temporal("clip_corto");

        let resultado =
            descargar_grabacion(&repo, &stream_url, ruta.clone(), VideoQuality::Best, None).await;

        let Ok(ResultadoGrabacion::Guardado(destino)) = resultado else {
            panic!("se esperaba archivo guardado");
        };
        assert_eq!(destino, ruta);
        assert!(destino.exists());
        let _ = tokio::fs::remove_file(destino).await;
    }
}
