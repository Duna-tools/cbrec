//! Coordinates recording file finalization and its local metadata sidecar.
//!
//! This module owns output reservation, partial-file recovery, MP4 validation,
//! and metadata persistence. It does not resolve streams or start FFmpeg.

use crate::domain::repositories::StreamRepository;
use crate::domain::value_objects::{StreamUrl, VideoQuality};
use crate::infrastructure::InfrastructureError;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::ffi::OsStr;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use tokio::fs::OpenOptions;
use tokio::task::JoinHandle;

pub(crate) enum ResultadoGrabacion {
    Guardado(PathBuf),
    Pequeno(PathBuf, u64),
    Cancelado,
    Interrupted(PathBuf, String),
}

#[derive(Serialize)]
struct RecordingMetadata<'a> {
    schema_version: u8,
    model: &'a str,
    requested_quality: String,
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    file_size_bytes: u64,
    classification: &'static str,
}

// Solo reconoce nombres anteriores para evitar colisiones con grabaciones existentes.
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
        if ruta_disponible(&ruta).await? && reserve_recording_file(&ruta).await? {
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
    let download_error = client
        .download_stream(stream_url, &ruta, quality)
        .await
        .err();
    let size = match tokio::fs::metadata(&ruta).await {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error.into()),
    };
    if size == 0 {
        remove_empty_file(&ruta).await;
        return match download_error {
            Some(InfrastructureError::RecordingCancelled) => Ok(ResultadoGrabacion::Cancelado),
            Some(error) => Err(error),
            None => Err(InfrastructureError::RecordingError(
                "la grabacion no contiene datos".to_string(),
            )),
        };
    }

    let finalized = archivo_finalizable(&ruta).await;
    if let Some(error) = download_error {
        if !matches!(error, InfrastructureError::RecordingCancelled) || !finalized {
            return Ok(ResultadoGrabacion::Interrupted(
                ruta.clone(),
                format!(
                    "{error}; grabacion interrumpida conservada en {}",
                    ruta.display()
                ),
            ));
        }
    }
    if !finalized {
        return Ok(ResultadoGrabacion::Interrupted(
            ruta.clone(),
            format!(
                "MP4 sin finalizar; grabacion conservada en {}",
                ruta.display()
            ),
        ));
    }

    if min_file_size.is_some_and(|minimum| size < minimum) {
        let small_dir = ruta
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("small");
        tokio::fs::create_dir_all(&small_dir).await?;
        let destination =
            small_dir.join(ruta.file_name().unwrap_or_else(|| OsStr::new("cbrec.mp4")));
        tokio::fs::rename(&ruta, &destination).await?;
        Ok(ResultadoGrabacion::Pequeno(destination, size))
    } else {
        Ok(ResultadoGrabacion::Guardado(ruta))
    }
}

/// Registra el resultado sin incluir URLs, cookies ni el detalle del error.
pub(crate) async fn write_recording_metadata(
    result: &ResultadoGrabacion,
    model: &str,
    quality: VideoQuality,
    started_at: DateTime<Utc>,
) -> Result<(), InfrastructureError> {
    let (path, classification) = match result {
        ResultadoGrabacion::Guardado(path) => (path, "saved"),
        ResultadoGrabacion::Pequeno(path, _) => (path, "small"),
        ResultadoGrabacion::Cancelado => return Ok(()),
        ResultadoGrabacion::Interrupted(path, _) => (path, "interrupted"),
    };
    let metadata = RecordingMetadata {
        schema_version: 1,
        model,
        requested_quality: quality.to_string(),
        started_at,
        finished_at: Utc::now(),
        file_size_bytes: tokio::fs::metadata(path).await?.len(),
        classification,
    };
    let mut content = serde_json::to_vec_pretty(&metadata).map_err(|error| {
        InfrastructureError::RecordingError(format!(
            "no se pudieron serializar los metadatos: {error}"
        ))
    })?;
    content.push(b'\n');

    let destination = path_with_suffix(path, ".json");
    let temporary = path_with_suffix(&destination, ".part");
    if let Err(error) = tokio::fs::write(&temporary, content).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(InfrastructureError::RecordingError(format!(
            "no se pudieron guardar los metadatos; video conservado en {}: {error}",
            path.display()
        )));
    }
    if let Err(error) = tokio::fs::rename(&temporary, &destination).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(InfrastructureError::RecordingError(format!(
            "no se pudieron guardar los metadatos; video conservado en {}: {error}",
            path.display()
        )));
    }

    Ok(())
}

fn path_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

pub(crate) async fn probar_directorio_escribible(dir: &Path) -> Result<(), InfrastructureError> {
    for attempt in 0..1000 {
        let probe = dir.join(format!(
            ".cbrec_write_test_{}_{attempt}",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)
            .await
        {
            Ok(file) => {
                drop(file);
                tokio::fs::remove_file(&probe).await?;
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(InfrastructureError::RecordingError(
        "no se pudo reservar un archivo de prueba".to_string(),
    ))
}

async fn ruta_disponible(ruta: &Path) -> Result<bool, InfrastructureError> {
    let metadata = path_with_suffix(ruta, ".json");
    if existe(ruta).await?
        || existe(&ruta_parcial(ruta)).await?
        || existe(&metadata).await?
        || existe(&path_with_suffix(&metadata, ".part")).await?
    {
        return Ok(false);
    }

    let small = ruta
        .parent()
        .map(|p| p.join("small"))
        .unwrap_or_else(|| PathBuf::from("small"));
    let destino_small = small.join(ruta.file_name().unwrap_or_else(|| OsStr::new("cbrec.mp4")));

    let metadata_small = path_with_suffix(&destino_small, ".json");
    Ok(!existe(&destino_small).await?
        && !existe(&metadata_small).await?
        && !existe(&path_with_suffix(&metadata_small, ".part")).await?)
}

async fn reserve_recording_file(ruta: &Path) -> Result<bool, InfrastructureError> {
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(ruta)
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

async fn remove_empty_file(path: &Path) {
    if tokio::fs::metadata(path)
        .await
        .is_ok_and(|metadata| metadata.len() == 0)
    {
        let _ = tokio::fs::remove_file(path).await;
    }
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
    let mut has_moov = false;
    let mut has_media = false;

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

        if size < header_size || size > total - offset {
            return false;
        }
        if atom == b"moov" {
            has_moov = true;
        }
        if atom == b"mdat" && size > header_size {
            has_media = true;
        }

        offset = match offset.checked_add(size) {
            Some(next) if next > offset => next,
            _ => return false,
        };
    }

    offset == total && has_moov && has_media
}

pub(crate) struct RecordingProgress(pub(crate) JoinHandle<()>);

impl Drop for RecordingProgress {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(crate) async fn detener_tarea_progreso(mut task: RecordingProgress) {
    task.0.abort();
    let _ = (&mut task.0).await;
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

    #[tokio::test]
    async fn recording_reserves_mp4_without_creating_part_file() {
        let root = ruta_temporal("direct_mp4");
        let base = root.join("alice.mp4");
        let chosen = preparar_ruta_grabacion(base.clone()).await.unwrap();
        assert_eq!(chosen, base);
        assert!(chosen.exists());
        assert!(!ruta_parcial(&chosen).exists());
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn concurrent_recordings_reserve_distinct_mp4_files() {
        let root = ruta_temporal("concurrent_mp4");
        let base = root.join("alice.mp4");
        let (first, second) = tokio::join!(
            preparar_ruta_grabacion(base.clone()),
            preparar_ruta_grabacion(base)
        );
        let (first, second) = (first.unwrap(), second.unwrap());
        assert_ne!(first, second);
        assert!(first.exists());
        assert!(second.exists());
        assert!(std::fs::read_dir(&root).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".part")));
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

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
        bytes.extend_from_slice(&9_u32.to_be_bytes());
        bytes.extend_from_slice(b"mdat");
        bytes.push(0);
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
        assert!(preparada.exists());
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
        assert!(preparada.exists());
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
        assert!(preparada.exists());
        let _ = tokio::fs::remove_dir_all(dir).await;
    }

    #[tokio::test]
    async fn recording_path_does_not_overwrite_existing_metadata() {
        let dir = ruta_temporal("preflight_metadata");
        tokio::fs::create_dir_all(&dir).await.expect("crea dir");
        let ruta = dir.join("alice.mp4");
        tokio::fs::write(path_with_suffix(&ruta, ".json"), b"{}")
            .await
            .expect("crea metadata existente");

        let preparada = preparar_ruta_grabacion(ruta)
            .await
            .expect("prepara ruta alternativa");

        assert_eq!(preparada, dir.join("alice_001.mp4"));
        assert!(preparada.exists());
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
        let parcial = ruta.clone();

        let resultado = descargar_grabacion(
            &repo,
            &stream_url,
            ruta.clone(),
            VideoQuality::Best,
            Some(1024),
        )
        .await;

        let Ok(ResultadoGrabacion::Interrupted(path, reason)) = resultado else {
            panic!("la interrupcion debe conservar su estado y su archivo");
        };
        assert_eq!(path, ruta);
        assert!(reason.contains(&parcial.display().to_string()));
        assert_eq!(tokio::fs::read(&parcial).await.unwrap(), b"parcial");
        assert!(ruta.exists());
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
    async fn descargar_grabacion_conserva_parcial_si_falla_descarga() {
        let repo = RepoErrorConParcial;
        let stream_url = StreamUrl::try_from("https://example.com/stream.m3u8").unwrap();
        let ruta = ruta_temporal("error_con_parcial");
        let parcial = ruta.clone();

        let resultado =
            descargar_grabacion(&repo, &stream_url, ruta, VideoQuality::Best, Some(1)).await;

        assert!(matches!(
            resultado,
            Ok(ResultadoGrabacion::Interrupted(_, _))
        ));
        assert_eq!(tokio::fs::read(&parcial).await.unwrap(), b"parcial");
        let _ = tokio::fs::remove_file(parcial).await;
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
    async fn recording_metadata_is_versioned_and_adjacent() {
        let path = ruta_temporal("metadata");
        tokio::fs::write(&path, b"video")
            .await
            .expect("crea grabacion");
        let result = ResultadoGrabacion::Guardado(path.clone());
        let started_at = "2026-08-02T12:00:00Z".parse().expect("fecha valida");

        write_recording_metadata(&result, "alice", VideoQuality::P720, started_at)
            .await
            .expect("escribe metadatos");

        let sidecar = path_with_suffix(&path, ".json");
        let content = tokio::fs::read_to_string(&sidecar)
            .await
            .expect("lee metadatos");
        let metadata: serde_json::Value =
            serde_json::from_str(&content).expect("metadatos validos");
        assert_eq!(metadata["schema_version"], 1);
        assert_eq!(metadata["model"], "alice");
        assert_eq!(metadata["requested_quality"], "720p");
        assert_eq!(metadata["started_at"], "2026-08-02T12:00:00Z");
        assert_eq!(metadata["file_size_bytes"], 5);
        assert_eq!(metadata["classification"], "saved");
        assert!(metadata["finished_at"].as_str().is_some());
        assert_eq!(
            sidecar.file_name().unwrap().to_string_lossy(),
            format!("{}.json", path.file_name().unwrap().to_string_lossy())
        );

        let _ = tokio::fs::remove_file(sidecar).await;
        let _ = tokio::fs::remove_file(path).await;
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

    #[tokio::test]
    async fn mp4_rejects_truncated_moov() {
        let path = ruta_temporal("truncated_moov");
        let mut bytes = 100_u32.to_be_bytes().to_vec();
        bytes.extend_from_slice(b"moov");
        tokio::fs::write(&path, bytes).await.unwrap();
        assert!(!archivo_finalizable(&path).await);
        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn mp4_header_without_media_is_not_finalized() {
        let path = ruta_temporal("empty_moov");
        let mut bytes = mp4_minimo_valido();
        bytes.truncate(bytes.len() - 9);
        tokio::fs::write(&path, bytes).await.unwrap();
        assert!(!archivo_finalizable(&path).await);
        tokio::fs::remove_file(path).await.unwrap();
    }

    #[tokio::test]
    async fn metadata_failure_preserves_media_and_reports_its_path() {
        let root = ruta_temporal("metadata_failure");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let path = root.join("alice.mp4");
        tokio::fs::write(&path, b"video").await.unwrap();
        tokio::fs::create_dir(path_with_suffix(&path, ".json.part"))
            .await
            .unwrap();
        let error = write_recording_metadata(
            &ResultadoGrabacion::Guardado(path.clone()),
            "alice",
            VideoQuality::Best,
            Utc::now(),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains(&path.display().to_string()));
        assert_eq!(tokio::fs::read(path).await.unwrap(), b"video");
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn recording_preserves_orphan_small_metadata() {
        for suffix in [".json", ".json.part"] {
            let root = ruta_temporal("small_metadata");
            let base = root.join("alice.mp4");
            let small = root.join("small");
            tokio::fs::create_dir_all(&small).await.unwrap();
            let existing = path_with_suffix(&small.join("alice.mp4"), suffix);
            tokio::fs::write(&existing, b"previous metadata")
                .await
                .unwrap();

            let chosen = preparar_ruta_grabacion(base.clone()).await.unwrap();

            assert_ne!(chosen, base);
            assert_eq!(
                tokio::fs::read(existing).await.unwrap(),
                b"previous metadata"
            );
            tokio::fs::remove_dir_all(root).await.unwrap();
        }
    }

    #[tokio::test]
    async fn recording_probe_preserves_colliding_file() {
        let root = ruta_temporal("probe_collision");
        tokio::fs::create_dir_all(&root).await.unwrap();
        let existing = root.join(format!(".cbrec_write_test_{}_0", std::process::id()));
        tokio::fs::write(&existing, b"previous data").await.unwrap();

        probar_directorio_escribible(&root).await.unwrap();

        assert_eq!(tokio::fs::read(existing).await.unwrap(), b"previous data");
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn failed_recording_preserves_bytes_and_next_recording_uses_another_path() {
        let root = ruta_temporal("failure_retry");
        let base = root.join("alice.mp4");
        let chosen = preparar_ruta_grabacion(base.clone()).await.unwrap();
        let partial = chosen.clone();
        let url = StreamUrl::new("https://example.com/stream.m3u8").unwrap();

        let result = descargar_grabacion(
            &RepoErrorConParcial,
            &url,
            chosen,
            VideoQuality::Best,
            Some(1),
        )
        .await;

        let Ok(ResultadoGrabacion::Interrupted(path, reason)) = result else {
            panic!("se esperaba grabacion interrumpida");
        };
        assert_eq!(path, base);
        assert!(reason.contains(&partial.display().to_string()));
        write_recording_metadata(
            &ResultadoGrabacion::Interrupted(path, reason),
            "alice",
            VideoQuality::Best,
            Utc::now(),
        )
        .await
        .unwrap();
        let metadata: serde_json::Value = serde_json::from_slice(
            &tokio::fs::read(path_with_suffix(&base, ".json"))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(metadata["classification"], "interrupted");
        let retry = preparar_ruta_grabacion(base.clone()).await.unwrap();
        assert_ne!(retry, base);
        assert_eq!(tokio::fs::read(partial).await.unwrap(), b"parcial");
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}
