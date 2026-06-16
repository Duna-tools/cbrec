use crate::domain::repositories::StreamRepository;
use crate::domain::value_objects::{StreamUrl, VideoQuality};
use crate::infrastructure::InfrastructureError;
use std::ffi::OsStr;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use tokio::fs::OpenOptions;
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

pub(crate) async fn preparar_ruta_grabacion(
    ruta_base: PathBuf,
) -> Result<PathBuf, InfrastructureError> {
    let parent = ruta_base.parent().ok_or_else(|| {
        InfrastructureError::RecordingError("ruta de salida sin directorio padre".to_string())
    })?;
    tokio::fs::create_dir_all(parent).await?;
    probar_directorio_escribible(parent).await?;

    for intento in 0..1000 {
        let ruta = ruta_con_sufijo(&ruta_base, intento);
        if ruta_disponible(&ruta).await? && reservar_parcial(&ruta).await? {
            return Ok(ruta);
        }
    }

    Err(InfrastructureError::RecordingError(
        "no se pudo encontrar un nombre de archivo disponible".to_string(),
    ))
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

            if !archivo_finalizable(&parcial).await {
                return Ok(ResultadoGrabacion::Cancelado);
            }
        }
        Err(e) => {
            limpiar_parcial(&parcial).await;
            return Err(e);
        }
    }

    if !archivo_finalizable(&parcial).await {
        limpiar_parcial(&parcial).await;
        return Err(InfrastructureError::RecordingError(
            "archivo parcial no parece un MP4 finalizado".to_string(),
        ));
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

async fn probar_directorio_escribible(dir: &Path) -> Result<(), InfrastructureError> {
    let probe = dir.join(format!(".cbrec_write_test_{}", std::process::id()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .await;

    match file {
        Ok(_) => {
            let _ = tokio::fs::remove_file(&probe).await;
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e.into()),
    }
}

async fn ruta_disponible(ruta: &Path) -> Result<bool, InfrastructureError> {
    if existe(ruta).await? || existe(&ruta_parcial(ruta)).await? {
        return Ok(false);
    }

    let small = ruta
        .parent()
        .map(|p| p.join("small"))
        .unwrap_or_else(|| PathBuf::from("small"));
    let destino_small = small.join(ruta.file_name().unwrap_or_else(|| OsStr::new("cbrec.mp4")));

    Ok(!existe(&destino_small).await?)
}

async fn reservar_parcial(ruta: &Path) -> Result<bool, InfrastructureError> {
    let parcial = ruta_parcial(ruta);
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&parcial)
        .await
    {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e.into()),
    }
}

async fn existe(path: &Path) -> Result<bool, InfrastructureError> {
    match tokio::fs::metadata(path).await {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

fn ruta_con_sufijo(ruta: &Path, intento: usize) -> PathBuf {
    if intento == 0 {
        return ruta.to_path_buf();
    }

    let stem = ruta
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("output");
    let nombre = match ruta.extension().and_then(|n| n.to_str()) {
        Some(extension) => format!("{stem}_{intento:03}.{extension}"),
        None => format!("{stem}_{intento:03}"),
    };
    ruta.with_file_name(nombre)
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

async fn archivo_finalizable(path: &Path) -> bool {
    if !requiere_validacion_mp4(path) {
        return true;
    }

    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || mp4_tiene_moov(&path))
        .await
        .unwrap_or(false)
}

fn requiere_validacion_mp4(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| matches!(e.to_ascii_lowercase().as_str(), "mp4" | "m4v" | "mov"))
        .unwrap_or(false)
}

fn mp4_tiene_moov(path: &Path) -> bool {
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let total = match file.metadata() {
        Ok(meta) => meta.len(),
        Err(_) => return false,
    };
    let mut offset = 0_u64;

    while offset.saturating_add(8) <= total {
        if file.seek(SeekFrom::Start(offset)).is_err() {
            return false;
        }

        let mut header = [0_u8; 8];
        if file.read_exact(&mut header).is_err() {
            return false;
        }

        let size32 = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as u64;
        let atom = &header[4..8];
        let mut header_size = 8_u64;
        let size = if size32 == 1 {
            let mut extended = [0_u8; 8];
            if file.read_exact(&mut extended).is_err() {
                return false;
            }
            header_size = 16;
            u64::from_be_bytes(extended)
        } else if size32 == 0 {
            total.saturating_sub(offset)
        } else {
            size32
        };

        if atom == b"moov" {
            return true;
        }
        if size < header_size {
            return false;
        }

        offset = match offset.checked_add(size) {
            Some(next) if next > offset => next,
            _ => return false,
        };
    }

    false
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
    struct RepoCanceladoConMp4Valido;
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
    impl StreamRepository for RepoCanceladoConMp4Valido {
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
            tokio::fs::write(output_path, mp4_minimo_valido()).await?;
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
            tokio::fs::write(output_path, mp4_minimo_valido()).await?;
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

    fn mp4_minimo_valido() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&16_u32.to_be_bytes());
        bytes.extend_from_slice(b"ftyp");
        bytes.extend_from_slice(b"isom0000");
        bytes.extend_from_slice(&8_u32.to_be_bytes());
        bytes.extend_from_slice(b"moov");
        bytes
    }

    #[test]
    fn ruta_parcial_conserva_extension_para_ffmpeg() {
        let ruta = PathBuf::from("/tmp/alice.mp4");

        assert_eq!(ruta_parcial(&ruta), PathBuf::from("/tmp/alice.part.mp4"));
    }

    #[tokio::test]
    async fn preparar_ruta_grabacion_crea_dir_y_reserva_parcial() {
        let dir = ruta_temporal("preflight_dir");
        let ruta = dir.join("alice.mp4");

        let preparada = preparar_ruta_grabacion(ruta.clone())
            .await
            .expect("prepara ruta");

        assert_eq!(preparada, ruta);
        assert!(ruta_parcial(&preparada).exists());
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn preparar_ruta_grabacion_no_pisa_archivo_existente() {
        let dir = ruta_temporal("preflight_final");
        tokio::fs::create_dir_all(&dir).await.expect("crea dir");
        let ruta = dir.join("alice.mp4");
        tokio::fs::write(&ruta, b"existente")
            .await
            .expect("crea archivo existente");

        let preparada = preparar_ruta_grabacion(ruta)
            .await
            .expect("prepara ruta alternativa");

        assert_eq!(preparada, dir.join("alice_001.mp4"));
        assert!(ruta_parcial(&preparada).exists());
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn preparar_ruta_grabacion_no_pisa_parcial_existente() {
        let dir = ruta_temporal("preflight_partial");
        tokio::fs::create_dir_all(&dir).await.expect("crea dir");
        let ruta = dir.join("alice.mp4");
        tokio::fs::write(ruta_parcial(&ruta), b"parcial")
            .await
            .expect("crea parcial existente");

        let preparada = preparar_ruta_grabacion(ruta)
            .await
            .expect("prepara ruta alternativa");

        assert_eq!(preparada, dir.join("alice_001.mp4"));
        assert!(ruta_parcial(&preparada).exists());
        let _ = tokio::fs::remove_dir_all(dir).await;
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
    async fn descargar_grabacion_conserva_parcial_sin_renombrar_si_se_cancela() {
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

        assert!(matches!(resultado, Ok(ResultadoGrabacion::Cancelado)));
        assert!(parcial.exists());
        assert!(!ruta.exists());
        let _ = tokio::fs::remove_file(parcial).await;
    }

    #[tokio::test]
    async fn descargar_grabacion_guarda_mp4_finalizado_si_se_cancela() {
        let repo = RepoCanceladoConMp4Valido;
        let stream_url = StreamUrl::try_from("https://example.com/stream.m3u8").unwrap();
        let ruta = ruta_temporal("cancelado_mp4_valido");

        let resultado =
            descargar_grabacion(&repo, &stream_url, ruta.clone(), VideoQuality::Best, None).await;

        let Ok(ResultadoGrabacion::Guardado(destino)) = resultado else {
            panic!("se esperaba archivo guardado");
        };
        assert_eq!(destino, ruta);
        assert!(destino.exists());
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

    #[tokio::test]
    async fn mp4_tiene_moov_detecta_mp4_finalizado() {
        let ruta = ruta_temporal("moov");
        tokio::fs::write(&ruta, mp4_minimo_valido())
            .await
            .expect("crea mp4 minimo");

        assert!(archivo_finalizable(&ruta).await);

        let _ = tokio::fs::remove_file(ruta).await;
    }

    #[tokio::test]
    async fn mp4_tiene_moov_rechaza_parcial_sin_moov() {
        let ruta = ruta_temporal("sin_moov");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&16_u32.to_be_bytes());
        bytes.extend_from_slice(b"ftyp");
        bytes.extend_from_slice(b"isom0000");
        bytes.extend_from_slice(&16_u32.to_be_bytes());
        bytes.extend_from_slice(b"mdat");
        bytes.extend_from_slice(b"datosxxx");
        tokio::fs::write(&ruta, bytes)
            .await
            .expect("crea mp4 parcial");

        assert!(!archivo_finalizable(&ruta).await);

        let _ = tokio::fs::remove_file(ruta).await;
    }
}
