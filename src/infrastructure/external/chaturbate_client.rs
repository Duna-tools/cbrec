use crate::domain::repositories::StreamRepository;
use crate::domain::value_objects::VideoQuality;
use crate::domain::value_objects::{ModelName, StreamUrl};
use crate::infrastructure::InfrastructureError;
use async_trait::async_trait;
use backoff::future::retry;
use backoff::ExponentialBackoff;
use quick_m3u8::config::ParsingOptionsBuilder;
use quick_m3u8::tag::{hls, KnownTag};
use quick_m3u8::{HlsLine, Reader};
use reqwest::Client;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tokio::sync::watch;
use tokio::time::Duration;

const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36";
const HTTP_TIMEOUT_SECS: u64 = 10;
const HTTP_RETRY_BASE_MS: u64 = 200;

fn make_backoff() -> ExponentialBackoff {
    ExponentialBackoff {
        initial_interval: Duration::from_millis(HTTP_RETRY_BASE_MS),
        max_interval: Duration::from_secs(2),
        max_elapsed_time: Some(Duration::from_secs(10)),
        ..Default::default()
    }
}

#[derive(Debug, Deserialize)]
struct ChatVideoContext {
    hls_source: Option<String>,
}

#[derive(Clone)]
pub struct ChaturbateClient {
    client: Client,
    base_url: String,
    ffmpeg_path: Option<PathBuf>,
    cancel_rx: Option<watch::Receiver<bool>>,
    session_cookie: Option<String>,
}

impl ChaturbateClient {
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
            session_cookie: None,
        })
    }

    pub fn with_ffmpeg_path(mut self, path: PathBuf) -> Self {
        self.ffmpeg_path = Some(path);
        self
    }

    pub fn with_cancel_receiver(mut self, cancel_rx: watch::Receiver<bool>) -> Self {
        self.cancel_rx = Some(cancel_rx);
        self
    }

    pub fn with_session_cookie(mut self, cookie: String) -> Self {
        self.session_cookie = Some(cookie);
        self
    }

    fn get_request(&self, url: &str) -> reqwest::RequestBuilder {
        let mut req = self.client.get(url);
        if let Some(cookie) = &self.session_cookie {
            req = req.header(reqwest::header::COOKIE, cookie.as_str());
        }
        req
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

        let backoff = make_backoff();

        retry(backoff, || async {
            let response = self.get_request(&url).send().await.map_err(|e| {
                backoff::Error::transient(InfrastructureError::ExternalService(format!(
                    "HTTP request failed: {}",
                    e
                )))
            })?;

            let status = response.status();
            if status.as_u16() == 404 {
                return Err(backoff::Error::Permanent(InfrastructureError::Domain(
                    crate::domain::errors::DomainError::ModelNotFound(
                        model_name.as_str().to_string(),
                    ),
                )));
            }

            if status.as_u16() == 429 || status.is_server_error() {
                return Err(backoff::Error::transient(
                    InfrastructureError::ExternalService(format!(
                        "HTTP request failed with status: {}",
                        status
                    )),
                ));
            }

            if !status.is_success() {
                return Err(backoff::Error::Permanent(
                    InfrastructureError::ExternalService(format!(
                        "HTTP request failed with status: {}",
                        status
                    )),
                ));
            }

            let context: ChatVideoContext = response.json().await.map_err(|e| {
                backoff::Error::Permanent(InfrastructureError::ExternalService(format!(
                    "Failed to parse response: {}",
                    e
                )))
            })?;

            match context.hls_source {
                Some(url) => {
                    let stream_url = StreamUrl::try_from(url.as_str())
                        .map_err(|e| backoff::Error::Permanent(InfrastructureError::Domain(e)))?;
                    Ok(Some(stream_url))
                }
                None => Ok(None),
            }
        })
        .await
    }

    async fn download_stream(
        &self,
        stream_url: &StreamUrl,
        output_path: &Path,
        quality: VideoQuality,
    ) -> Result<(), InfrastructureError> {
        let stream_url = if quality == VideoQuality::Best {
            stream_url.clone()
        } else {
            match self.resolver_variant_url(stream_url, quality).await {
                Ok(url) => url,
                Err(_) => stream_url.clone(),
            }
        };

        let ffmpeg_bin = self
            .ffmpeg_path
            .as_deref()
            .unwrap_or_else(|| Path::new("ffmpeg"));
        let mut command = tokio::process::Command::new(ffmpeg_bin);
        command.kill_on_drop(true);
        command
            .arg("-nostdin")
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error");

        if let Some(cookie) = &self.session_cookie {
            command
                .arg("-headers")
                .arg(format!("Cookie: {}\r\n", cookie));
        }

        let mut child = command
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
                _ = async { cancel_rx.wait_for(|v| *v).await.ok(); } => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    return Err(InfrastructureError::RecordingCancelled);
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
        quality: VideoQuality,
    ) -> Result<StreamUrl, InfrastructureError> {
        let variantes = self.obtener_variantes(master_url).await?;

        if variantes.is_empty() {
            return Ok(master_url.clone());
        }

        let seleccion = seleccionar_variante(&variantes, quality)
            .or_else(|| seleccionar_variante(&variantes, VideoQuality::Best))
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
        let backoff = make_backoff();

        retry(backoff, || async {
            let response = self.get_request(url).send().await.map_err(|e| {
                backoff::Error::transient(InfrastructureError::ExternalService(format!(
                    "HTTP request failed: {}",
                    e
                )))
            })?;

            let status = response.status();
            if status.as_u16() == 429 || status.is_server_error() {
                return Err(backoff::Error::transient(
                    InfrastructureError::ExternalService(format!(
                        "HTTP request failed with status: {}",
                        status
                    )),
                ));
            }

            if !status.is_success() {
                return Err(backoff::Error::Permanent(
                    InfrastructureError::ExternalService(format!(
                        "HTTP request failed with status: {}",
                        status
                    )),
                ));
            }

            response.text().await.map_err(|e| {
                backoff::Error::Permanent(InfrastructureError::ExternalService(format!(
                    "Failed to read playlist: {}",
                    e
                )))
            })
        })
        .await
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

fn seleccionar_variante(
    variantes: &[VarianteStream],
    quality: VideoQuality,
) -> Option<&VarianteStream> {
    if variantes.is_empty() {
        return None;
    }

    if quality == VideoQuality::AudioOnly {
        // streams sin RESOLUTION son pistas de audio puro
        let audio = variantes
            .iter()
            .filter(|v| v.height.is_none())
            .min_by_key(|v| v.bandwidth.unwrap_or(0));
        if let Some(a) = audio {
            return Some(a);
        }
        // fallback: video de menor calidad
        return variantes
            .iter()
            .filter(|v| v.height.is_some())
            .min_by_key(|v| (v.height.unwrap_or(u32::MAX), v.bandwidth.unwrap_or(0)));
    }

    let objetivo = quality.target_height();

    if let Some(objetivo) = objetivo {
        // mejor variante en o por debajo del objetivo
        let bajo = variantes
            .iter()
            .filter(|v| v.height.is_some_and(|h| h <= objetivo))
            .max_by_key(|v| (v.height.unwrap_or(0), v.bandwidth.unwrap_or(0)));
        if let Some(sel) = bajo {
            return Some(sel);
        }
        // fallback: la más cercana por encima
        let sobre = variantes
            .iter()
            .filter(|v| v.height.is_some_and(|h| h > objetivo))
            .min_by_key(|v| (v.height.unwrap_or(u32::MAX), v.bandwidth.unwrap_or(0)));
        if let Some(sel) = sobre {
            return Some(sel);
        }
    }

    // Best o sin coincidencia: mayor resolución, luego mayor bandwidth
    variantes
        .iter()
        .filter(|v| v.height.is_some())
        .max_by_key(|v| (v.height.unwrap_or(0), v.bandwidth.unwrap_or(0)))
        .or_else(|| variantes.iter().max_by_key(|v| v.bandwidth.unwrap_or(0)))
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
    use crate::domain::value_objects::VideoQuality;

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

        let sel_720 = seleccionar_variante(&variantes, VideoQuality::P720).unwrap();
        assert_eq!(sel_720.url, "mid.m3u8");

        let sel_best = seleccionar_variante(&variantes, VideoQuality::Best).unwrap();
        assert_eq!(sel_best.url, "hi.m3u8");
    }

    #[test]
    fn audio_only_prefiere_stream_sin_resolucion() {
        let playlist = "\
#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=128000
audio.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360
low.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2800000,RESOLUTION=1920x1080
hi.m3u8
";
        let variantes = parsear_variantes(playlist).expect("parse failed");
        let sel = seleccionar_variante(&variantes, VideoQuality::AudioOnly).unwrap();
        assert_eq!(sel.url, "audio.m3u8");
    }

    #[test]
    fn audio_only_fallback_a_menor_video_si_no_hay_audio() {
        let playlist = "\
#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360
low.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2800000,RESOLUTION=1920x1080
hi.m3u8
";
        let variantes = parsear_variantes(playlist).expect("parse failed");
        let sel = seleccionar_variante(&variantes, VideoQuality::AudioOnly).unwrap();
        assert_eq!(sel.url, "low.m3u8");
    }
}
