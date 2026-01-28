use crate::domain::repositories::StreamRepository;
use crate::domain::value_objects::{ModelName, StreamUrl};
use crate::infrastructure::InfrastructureError;
use async_trait::async_trait;
use quick_m3u8::config::ParsingOptionsBuilder;
use quick_m3u8::tag::{hls, KnownTag};
use quick_m3u8::{HlsLine, Reader};
use reqwest::Client;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tokio::sync::watch;
use tokio::time::{sleep, Duration};

const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36";
const HTTP_TIMEOUT_SECS: u64 = 10;
const MIN_FILE_SIZE_BYTES: u64 = 1024;
const BYTES_PER_MEGABYTE: f64 = 1_048_576.0;
const HTTP_RETRY_MAX: usize = 3;
const HTTP_RETRY_BASE_MS: u64 = 200;

#[derive(Debug, Deserialize)]
struct ChatVideoContext {
    hls_source: Option<String>,
    #[allow(dead_code)]
    room_status: String,
}

/// Cliente HTTP para consultar y descargar streams de Chaturbate.
#[derive(Clone)]
pub struct ChaturbateClient {
    client: Client,
    base_url: String,
    ffmpeg_path: Option<PathBuf>,
    cancel_rx: Option<watch::Receiver<bool>>,
}

impl ChaturbateClient {
    /// Crea un cliente listo para operar con la API publica.
    /// # Errors
    /// - `InfrastructureError::ExternalService` si falla la configuracion HTTP.
    pub fn new() -> Result<Self, InfrastructureError> {
        let client = Client::builder()
            .user_agent(DEFAULT_USER_AGENT)
            .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build()
            .map_err(|e| {
                InfrastructureError::ExternalService(format!("Failed to create HTTP client: {}", e))
            })?;

        Ok(Self {
            client,
            base_url: "https://chaturbate.com".to_string(),
            ffmpeg_path: None,
            cancel_rx: None,
        })
    }

    /// Configura la ruta de ffmpeg.
    pub fn with_ffmpeg_path(mut self, path: PathBuf) -> Self {
        self.ffmpeg_path = Some(path);
        self
    }

    /// Configura un receptor de cancelacion global.
    pub fn with_cancel_receiver(mut self, cancel_rx: watch::Receiver<bool>) -> Self {
        self.cancel_rx = Some(cancel_rx);
        self
    }
}

#[async_trait]
impl StreamRepository for ChaturbateClient {
    type Error = InfrastructureError;

    async fn get_stream_url(
        &self,
        model_name: &ModelName,
    ) -> Result<Option<StreamUrl>, InfrastructureError> {
        let url = format!(
            "{}/api/chatvideocontext/{}/",
            self.base_url,
            model_name.as_str()
        );

        let mut intento = 0;
        let mut espera_ms = HTTP_RETRY_BASE_MS;

        loop {
            let response = self.client.get(&url).send().await.map_err(|e| {
                InfrastructureError::ExternalService(format!("HTTP request failed: {}", e))
            })?;

            let status = response.status();
            if status.as_u16() == 404 {
                return Err(InfrastructureError::Domain(
                    crate::domain::errors::DomainError::ModelNotFound(
                        model_name.as_str().to_string(),
                    ),
                ));
            }

            if status.as_u16() == 429 || status.is_server_error() {
                if intento < HTTP_RETRY_MAX {
                    intento += 1;
                    sleep(Duration::from_millis(espera_ms)).await;
                    espera_ms = espera_ms.saturating_mul(2);
                    continue;
                }
                return Err(InfrastructureError::ExternalService(format!(
                    "HTTP request failed with status: {}",
                    status
                )));
            }

            if !status.is_success() {
                return Err(InfrastructureError::ExternalService(format!(
                    "HTTP request failed with status: {}",
                    status
                )));
            }

            let context: ChatVideoContext = response.json().await.map_err(|e| {
                InfrastructureError::ExternalService(format!("Failed to parse response: {}", e))
            })?;

            return match context.hls_source {
                Some(url) => Ok(Some(StreamUrl::try_from(url.as_str())?)),
                None => Ok(None),
            };
        }
    }

    async fn download_stream(
        &self,
        stream_url: &StreamUrl,
        output_path: &Path,
        quality: &str,
    ) -> Result<(), InfrastructureError> {
        let stream_url = match self.resolver_variant_url(stream_url, quality).await {
            Ok(url) => url,
            Err(err) => {
                println!(
                    "[WARN] No se pudo resolver variante: {}. Usando master.",
                    err
                );
                stream_url.clone()
            }
        };

        println!(
            "Starting recording of {} to {}",
            stream_url.as_str(),
            output_path.display()
        );
        println!("Press Ctrl+C to stop recording");

        let ffmpeg_bin = self
            .ffmpeg_path
            .as_deref()
            .unwrap_or_else(|| Path::new("ffmpeg"));
        let mut command = tokio::process::Command::new(ffmpeg_bin);
        command.kill_on_drop(true);
        let mut child = command
            .arg("-nostdin")
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-i")
            .arg(stream_url.as_str())
            .arg("-c")
            .arg("copy")
            .arg("-y")
            .arg(output_path)
            .spawn()
            .map_err(|e| {
                InfrastructureError::RecordingError(format!("Failed to start ffmpeg: {}", e))
            })?;

        let pid = child.id();
        println!("FFmpeg process started (PID: {:?})", pid);

        if let Some(mut cancel_rx) = self.cancel_rx.clone() {
            if *cancel_rx.borrow() {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(InfrastructureError::RecordingCancelled);
            }

            tokio::select! {
                status = child.wait() => {
                    match status {
                        Ok(exit_status) => {
                            if !exit_status.success() {
                                return Err(InfrastructureError::RecordingError(
                                    format!("FFmpeg exited with status: {}", exit_status)
                                ));
                            }
                        }
                        Err(e) => {
                            return Err(InfrastructureError::RecordingError(
                                format!("Failed to wait for ffmpeg: {}", e)
                            ));
                        }
                    }
                }
                _ = cancel_rx.changed() => {
                    if *cancel_rx.borrow() {
                        println!("\n[WARN] Cancelacion solicitada, deteniendo grabacion...");
                        let _ = child.kill().await;
                        let _ = child.wait().await;
                        return Err(InfrastructureError::RecordingCancelled);
                    }
                }
            }
        } else {
            let status = child.wait().await.map_err(|e| {
                InfrastructureError::RecordingError(format!("Failed to wait for ffmpeg: {}", e))
            })?;
            if !status.success() {
                return Err(InfrastructureError::RecordingError(format!(
                    "FFmpeg exited with status: {}",
                    status
                )));
            }
        }

        let file_size = tokio::fs::metadata(output_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);

        if file_size < MIN_FILE_SIZE_BYTES {
            return Err(InfrastructureError::RecordingError(format!(
                "Recording failed: file size is only {} bytes",
                file_size
            )));
        }

        println!(
            "[OK] Recording completed: {} ({:.2} MB)",
            output_path.display(),
            file_size as f64 / BYTES_PER_MEGABYTE
        );
        Ok(())
    }
}

impl ChaturbateClient {
    pub async fn listar_calidades(
        &self,
        master_url: &StreamUrl,
    ) -> Result<Vec<CalidadDisponible>, InfrastructureError> {
        let variantes = self.obtener_variantes(master_url).await?;
        let mut calidades: Vec<CalidadDisponible> = variantes
            .into_iter()
            .map(|v| CalidadDisponible {
                height: v.height,
                bandwidth: v.bandwidth,
            })
            .collect();
        calidades.sort_by_key(|c| (c.height.unwrap_or(0), c.bandwidth.unwrap_or(0)));
        Ok(calidades)
    }

    async fn resolver_variant_url(
        &self,
        master_url: &StreamUrl,
        quality: &str,
    ) -> Result<StreamUrl, InfrastructureError> {
        let variantes = self.obtener_variantes(master_url).await?;

        if variantes.is_empty() {
            return Ok(master_url.clone());
        }

        let seleccion = seleccionar_variante(&variantes, quality)
            .or_else(|| seleccionar_variante(&variantes, "best"))
            .ok_or_else(|| {
                InfrastructureError::ExternalService(
                    "No se pudo seleccionar variante de calidad".to_string(),
                )
            })?;

        let url_final = resolver_url(master_url.as_str(), &seleccion.url)?;
        Ok(StreamUrl::try_from(url_final)?)
    }

    async fn obtener_variantes(
        &self,
        master_url: &StreamUrl,
    ) -> Result<Vec<VarianteStream>, InfrastructureError> {
        let contenido = self.obtener_playlist(master_url.as_str()).await?;
        parsear_variantes(&contenido)
    }

    async fn obtener_playlist(&self, url: &str) -> Result<String, InfrastructureError> {
        let mut intento = 0;
        let mut espera_ms = HTTP_RETRY_BASE_MS;

        loop {
            let response = self.client.get(url).send().await.map_err(|e| {
                InfrastructureError::ExternalService(format!("HTTP request failed: {}", e))
            })?;

            let status = response.status();
            if status.as_u16() == 429 || status.is_server_error() {
                if intento < HTTP_RETRY_MAX {
                    intento += 1;
                    sleep(Duration::from_millis(espera_ms)).await;
                    espera_ms = espera_ms.saturating_mul(2);
                    continue;
                }
                return Err(InfrastructureError::ExternalService(format!(
                    "HTTP request failed with status: {}",
                    status
                )));
            }

            if !status.is_success() {
                return Err(InfrastructureError::ExternalService(format!(
                    "HTTP request failed with status: {}",
                    status
                )));
            }

            return response.text().await.map_err(|e| {
                InfrastructureError::ExternalService(format!("Failed to read playlist: {}", e))
            });
        }
    }
}

#[derive(Clone, Debug)]
struct VarianteStream {
    url: String,
    bandwidth: Option<u64>,
    height: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct CalidadDisponible {
    pub height: Option<u32>,
    pub bandwidth: Option<u64>,
}

fn parsear_variantes(contenido: &str) -> Result<Vec<VarianteStream>, InfrastructureError> {
    let opciones = ParsingOptionsBuilder::new()
        .with_parsing_for_stream_inf()
        .build();
    let mut reader = Reader::from_str(contenido, opciones);
    let mut variantes = Vec::new();
    let mut pendiente: Option<VarianteStream> = None;

    loop {
        match reader.read_line() {
            Ok(Some(linea)) => match linea {
                HlsLine::KnownTag(KnownTag::Hls(hls::Tag::StreamInf(tag))) => {
                    let height = tag
                        .resolution()
                        .and_then(|res| u32::try_from(res.height).ok());
                    pendiente = Some(VarianteStream {
                        url: String::new(),
                        bandwidth: Some(tag.bandwidth()),
                        height,
                    });
                }
                HlsLine::Uri(uri) => {
                    if let Some(mut variante) = pendiente.take() {
                        let url = uri.trim();
                        if !url.is_empty() {
                            variante.url = url.to_string();
                            variantes.push(variante);
                        }
                    }
                }
                _ => {}
            },
            Ok(None) => break,
            Err(err) => {
                return Err(InfrastructureError::ExternalService(format!(
                    "Invalid playlist: {}",
                    err
                )))
            }
        }
    }

    Ok(variantes)
}

fn seleccionar_variante<'a>(
    variantes: &'a [VarianteStream],
    quality: &str,
) -> Option<&'a VarianteStream> {
    let objetivo = match quality {
        "1080p" => Some(1080),
        "720p" => Some(720),
        "480p" => Some(480),
        "240p" => Some(240),
        "best" => None,
        _ => None,
    };

    if let Some(objetivo) = objetivo {
        let mut menores: Vec<&VarianteStream> = variantes
            .iter()
            .filter(|v| v.height.is_some() && v.height.unwrap() <= objetivo)
            .collect();
        menores.sort_by_key(|v| (v.height.unwrap_or(0), v.bandwidth.unwrap_or(0)));
        if let Some(sel) = menores.last() {
            return Some(sel);
        }

        let mut mayores: Vec<&VarianteStream> = variantes
            .iter()
            .filter(|v| v.height.is_some() && v.height.unwrap() > objetivo)
            .collect();
        mayores.sort_by_key(|v| (v.height.unwrap_or(u32::MAX), v.bandwidth.unwrap_or(0)));
        if let Some(sel) = mayores.first() {
            return Some(sel);
        }
    }

    let mut por_resolucion: Vec<&VarianteStream> =
        variantes.iter().filter(|v| v.height.is_some()).collect();
    por_resolucion.sort_by_key(|v| (v.height.unwrap_or(0), v.bandwidth.unwrap_or(0)));
    if let Some(sel) = por_resolucion.last() {
        return Some(sel);
    }

    let mut por_bw: Vec<&VarianteStream> = variantes.iter().collect();
    por_bw.sort_by_key(|v| v.bandwidth.unwrap_or(0));
    por_bw.last().copied()
}

fn resolver_url(base: &str, relativa: &str) -> Result<String, InfrastructureError> {
    let base = reqwest::Url::parse(base)
        .map_err(|e| InfrastructureError::ExternalService(format!("Invalid base URL: {}", e)))?;
    let url = base
        .join(relativa)
        .map_err(|e| InfrastructureError::ExternalService(format!("Invalid URL: {}", e)))?;
    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsea_variantes_y_selecciona() {
        let playlist = "\
#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360
low.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=1400000,RESOLUTION=1280x720
mid.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2800000,RESOLUTION=1920x1080
hi.m3u8
";

        let variantes = parsear_variantes(playlist).expect("parse failed");
        assert_eq!(variantes.len(), 3);

        let sel_720 = seleccionar_variante(&variantes, "720p").unwrap();
        assert_eq!(sel_720.url, "mid.m3u8");

        let sel_best = seleccionar_variante(&variantes, "best").unwrap();
        assert_eq!(sel_best.url, "hi.m3u8");
    }
}
