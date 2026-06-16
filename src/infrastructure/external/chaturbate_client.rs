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
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use std::future;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Child;
use tokio::sync::watch;
use tokio::time::{Duration, Instant};

const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36";
const HTTP_TIMEOUT_SECS: u64 = 10;
const HTTP_RETRY_BASE_MS: u64 = 200;
const FFMPEG_SHUTDOWN_GRACE_SECS: u64 = 15;
const FFMPEG_STALL_TIMEOUT_SECS: u64 = 120;
const FFMPEG_STALL_CHECK_SECS: u64 = 5;

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
    room_status: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EstadoStream {
    Online { stream_url: StreamUrl },
    Offline,
    RequiereSesion { detalle: String },
    RateLimited,
    Bloqueado { detalle: String },
    RespuestaInesperada { detalle: String },
}

#[derive(Clone)]
pub struct ChaturbateClient {
    client: Client,
    base_url: String,
    ffmpeg_path: Option<PathBuf>,
    cancel_rx: Option<watch::Receiver<bool>>,
    session_cookie: Option<String>,
    max_duration_secs: Option<u64>,
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
            max_duration_secs: None,
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

    pub fn with_max_duration_secs(mut self, seconds: u64) -> Self {
        self.max_duration_secs = Some(seconds);
        self
    }

    fn get_request(&self, url: &str) -> reqwest::RequestBuilder {
        let mut req = self.client.get(url);
        if let Some(cookie) = &self.session_cookie {
            req = req.header(reqwest::header::COOKIE, cookie.as_str());
        }
        req
    }

    pub async fn consultar_estado(
        &self,
        model_name: &ModelName,
    ) -> Result<EstadoStream, InfrastructureError> {
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

            match clasificar_status_http(response.status()) {
                EstadoHttp::Ok => {}
                EstadoHttp::NoEncontrado => {
                    return Err(backoff::Error::Permanent(InfrastructureError::Domain(
                        crate::domain::errors::DomainError::ModelNotFound(
                            model_name.as_str().to_string(),
                        ),
                    )));
                }
                EstadoHttp::RateLimited => return Ok(EstadoStream::RateLimited),
                EstadoHttp::RequiereSesion => {
                    return Ok(EstadoStream::RequiereSesion {
                        detalle: format!("HTTP {}", response.status()),
                    });
                }
                EstadoHttp::Reintentable => {
                    return Err(backoff::Error::transient(error_status_http(
                        response.status(),
                    )));
                }
                EstadoHttp::Permanente => {
                    return Ok(EstadoStream::Bloqueado {
                        detalle: format!("HTTP {}", response.status()),
                    });
                }
            }

            let contenido = response.text().await.map_err(|e| {
                backoff::Error::Permanent(InfrastructureError::ExternalService(format!(
                    "Failed to read response: {}",
                    e
                )))
            })?;

            Ok(clasificar_chat_video_context(&contenido))
        })
        .await
    }
}

#[async_trait]
impl StreamRepository for ChaturbateClient {
    type Error = InfrastructureError;

    async fn get_stream_url(
        &self,
        model_name: &ModelName,
    ) -> Result<Option<StreamUrl>, InfrastructureError> {
        match self.consultar_estado(model_name).await? {
            EstadoStream::Online { stream_url } => Ok(Some(stream_url)),
            EstadoStream::Offline => Ok(None),
            EstadoStream::RateLimited => Err(InfrastructureError::HttpStatus(429)),
            EstadoStream::RequiereSesion { detalle } => Err(InfrastructureError::ExternalService(
                format!("stream requiere sesion o acceso privado: {detalle}"),
            )),
            EstadoStream::Bloqueado { detalle } => Err(InfrastructureError::ExternalService(
                format!("respuesta bloqueada por Chaturbate: {detalle}"),
            )),
            EstadoStream::RespuestaInesperada { detalle } => {
                Err(InfrastructureError::ExternalService(format!(
                    "respuesta inesperada de Chaturbate: {detalle}"
                )))
            }
        }
    }

    async fn download_stream(
        &self,
        stream_url: &StreamUrl,
        output_path: &Path,
        quality: VideoQuality,
    ) -> Result<(), InfrastructureError> {
        let stream_url = match self.resolver_variant_url(stream_url, quality).await {
            Ok(url) => url,
            Err(_) => stream_url.clone(),
        };

        let ffmpeg_bin = self
            .ffmpeg_path
            .as_deref()
            .unwrap_or_else(|| Path::new("ffmpeg"));
        let mut command = tokio::process::Command::new(ffmpeg_bin);
        command.kill_on_drop(true);
        configurar_aislamiento_ffmpeg(&mut command);
        command
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        if let Some(cookie) = &self.session_cookie {
            command
                .arg("-headers")
                .arg(format!("Cookie: {}\r\n", cookie));
        }

        let mut child = command
            .args(duration_args(self.max_duration_secs))
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
        let mut stderr_task = child.stderr.take().map(|mut stderr| {
            tokio::spawn(async move {
                let mut buffer = Vec::new();
                let _ = stderr.read_to_end(&mut buffer).await;
                buffer
            })
        });

        if let Some(mut cancel_rx) = self.cancel_rx.clone() {
            if *cancel_rx.borrow() {
                cancelar_ffmpeg(&mut child, stderr_task.take()).await;
                return Err(InfrastructureError::RecordingCancelled);
            }

            tokio::select! {
                status = child.wait() => {
                    match status {
                        Ok(exit_status) => {
                            if !exit_status.success() {
                                let stderr = leer_stderr_ffmpeg(stderr_task.take()).await;
                                return Err(InfrastructureError::RecordingError(
                                    formatear_error_ffmpeg(exit_status, &stderr)
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
                    cancelar_ffmpeg(&mut child, stderr_task.take()).await;
                    return Err(InfrastructureError::RecordingCancelled);
                }
                _ = esperar_limite_grabacion(self.max_duration_secs) => {
                    cancelar_ffmpeg(&mut child, stderr_task.take()).await;
                    return Err(InfrastructureError::RecordingCancelled);
                }
                _ = esperar_sin_progreso(
                    output_path,
                    Duration::from_secs(FFMPEG_STALL_TIMEOUT_SECS),
                    Duration::from_secs(FFMPEG_STALL_CHECK_SECS),
                ) => {
                    cancelar_ffmpeg(&mut child, stderr_task.take()).await;
                    return Err(InfrastructureError::RecordingError(format!(
                        "FFmpeg no escribio datos nuevos durante {} segundos",
                        FFMPEG_STALL_TIMEOUT_SECS
                    )));
                }
            }
        } else {
            tokio::select! {
                status = child.wait() => {
                    let status = status.map_err(|e| {
                        InfrastructureError::RecordingError(format!("Failed to wait for ffmpeg: {}", e))
                    })?;
                    if !status.success() {
                        let stderr = leer_stderr_ffmpeg(stderr_task.take()).await;
                        return Err(InfrastructureError::RecordingError(formatear_error_ffmpeg(
                            status, &stderr,
                        )));
                    }
                }
                _ = esperar_limite_grabacion(self.max_duration_secs) => {
                    cancelar_ffmpeg(&mut child, stderr_task.take()).await;
                    return Err(InfrastructureError::RecordingCancelled);
                }
                _ = esperar_sin_progreso(
                    output_path,
                    Duration::from_secs(FFMPEG_STALL_TIMEOUT_SECS),
                    Duration::from_secs(FFMPEG_STALL_CHECK_SECS),
                ) => {
                    cancelar_ffmpeg(&mut child, stderr_task.take()).await;
                    return Err(InfrastructureError::RecordingError(format!(
                        "FFmpeg no escribio datos nuevos durante {} segundos",
                        FFMPEG_STALL_TIMEOUT_SECS
                    )));
                }
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

        resolver_url_variante(master_url, &variantes, quality)
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

            match clasificar_status_http(response.status()) {
                EstadoHttp::Ok => {}
                EstadoHttp::RateLimited => {
                    return Err(backoff::Error::transient(InfrastructureError::HttpStatus(
                        429,
                    )));
                }
                EstadoHttp::RequiereSesion => {
                    return Err(backoff::Error::Permanent(error_status_http(
                        response.status(),
                    )));
                }
                EstadoHttp::Reintentable => {
                    return Err(backoff::Error::transient(error_status_http(
                        response.status(),
                    )));
                }
                EstadoHttp::NoEncontrado | EstadoHttp::Permanente => {
                    return Err(backoff::Error::Permanent(error_status_http(
                        response.status(),
                    )));
                }
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

#[cfg(unix)]
fn configurar_aislamiento_ffmpeg(command: &mut tokio::process::Command) {
    command.process_group(0);
}

#[cfg(not(unix))]
fn configurar_aislamiento_ffmpeg(_command: &mut tokio::process::Command) {}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EstadoHttp {
    Ok,
    NoEncontrado,
    RateLimited,
    RequiereSesion,
    Reintentable,
    Permanente,
}

fn clasificar_status_http(status: StatusCode) -> EstadoHttp {
    if status.is_success() {
        return EstadoHttp::Ok;
    }
    if status == StatusCode::NOT_FOUND {
        return EstadoHttp::NoEncontrado;
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return EstadoHttp::RateLimited;
    }
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return EstadoHttp::RequiereSesion;
    }
    if status.is_server_error() {
        return EstadoHttp::Reintentable;
    }
    EstadoHttp::Permanente
}

fn error_status_http(status: StatusCode) -> InfrastructureError {
    InfrastructureError::HttpStatus(status.as_u16())
}

fn clasificar_chat_video_context(contenido: &str) -> EstadoStream {
    let context = match serde_json::from_str::<ChatVideoContext>(contenido) {
        Ok(context) => context,
        Err(e) if parece_html_o_bloqueo(contenido) => {
            return EstadoStream::Bloqueado {
                detalle: format!("respuesta no JSON: {}", e),
            };
        }
        Err(e) => {
            return EstadoStream::RespuestaInesperada {
                detalle: format!("respuesta no JSON: {}", e),
            };
        }
    };

    let room_status = context.room_status.as_deref().map(str::trim);

    if let Some(url) = context.hls_source {
        let url = url.trim();
        if !url.is_empty() {
            return match StreamUrl::try_from(url) {
                Ok(stream_url) => EstadoStream::Online { stream_url },
                Err(e) => EstadoStream::RespuestaInesperada {
                    detalle: format!("hls_source invalido: {}", e),
                },
            };
        }
    }

    match room_status.map(|status| status.to_ascii_lowercase()) {
        Some(status) if status == "offline" => EstadoStream::Offline,
        Some(status)
            if status.contains("private")
                || status.contains("fan")
                || status.contains("password")
                || status.contains("hidden") =>
        {
            EstadoStream::RequiereSesion {
                detalle: format!("room_status={status} sin hls_source"),
            }
        }
        Some(status) => EstadoStream::RespuestaInesperada {
            detalle: format!("room_status={status} sin hls_source"),
        },
        None => EstadoStream::RespuestaInesperada {
            detalle: "sin hls_source ni room_status".to_string(),
        },
    }
}

fn parece_html_o_bloqueo(contenido: &str) -> bool {
    let inicio = contenido.trim_start().to_ascii_lowercase();
    inicio.starts_with('<') || inicio.contains("cloudflare") || inicio.contains("access denied")
}

fn parsear_variantes(contenido: &str) -> Result<Vec<VarianteStream>, InfrastructureError> {
    if !contenido.trim_start().starts_with("#EXTM3U") {
        return Err(InfrastructureError::ExternalService(
            "Invalid playlist: missing #EXTM3U header".to_string(),
        ));
    }

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

fn resolver_url_variante(
    master_url: &StreamUrl,
    variantes: &[VarianteStream],
    quality: VideoQuality,
) -> Result<StreamUrl, InfrastructureError> {
    let seleccion = seleccionar_variante(variantes, quality)
        .or_else(|| seleccionar_variante(variantes, VideoQuality::Best))
        .ok_or_else(|| {
            InfrastructureError::ExternalService(
                "No se pudo seleccionar variante de calidad".to_string(),
            )
        })?;

    let url_final = resolver_url(master_url.as_str(), &seleccion.url)?;
    Ok(StreamUrl::try_from(url_final)?)
}

fn resolver_url(base: &str, relativa: &str) -> Result<String, InfrastructureError> {
    let base = reqwest::Url::parse(base)
        .map_err(|e| InfrastructureError::ExternalService(format!("Invalid base URL: {}", e)))?;
    let url = base
        .join(relativa)
        .map_err(|e| InfrastructureError::ExternalService(format!("Invalid URL: {}", e)))?;
    Ok(url.to_string())
}

fn duration_args(max_duration_secs: Option<u64>) -> Vec<String> {
    match max_duration_secs {
        Some(seconds) => vec!["-t".to_string(), seconds.to_string()],
        None => Vec::new(),
    }
}

async fn esperar_limite_grabacion(max_duration_secs: Option<u64>) {
    match max_duration_secs {
        Some(seconds) => tokio::time::sleep(Duration::from_secs(seconds.saturating_add(10))).await,
        None => future::pending::<()>().await,
    }
}

async fn leer_stderr_ffmpeg(stderr_task: Option<tokio::task::JoinHandle<Vec<u8>>>) -> Vec<u8> {
    match stderr_task {
        Some(task) => task.await.unwrap_or_default(),
        None => Vec::new(),
    }
}

async fn cancelar_ffmpeg(child: &mut Child, stderr_task: Option<tokio::task::JoinHandle<Vec<u8>>>) {
    if !solicitar_cierre_ffmpeg_por_stdin(child).await {
        solicitar_cierre_ffmpeg(child);
    }
    if tokio::time::timeout(
        Duration::from_secs(FFMPEG_SHUTDOWN_GRACE_SECS),
        child.wait(),
    )
    .await
    .is_err()
    {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    let _ = leer_stderr_ffmpeg(stderr_task).await;
}

async fn solicitar_cierre_ffmpeg_por_stdin(child: &mut Child) -> bool {
    let Some(mut stdin) = child.stdin.take() else {
        return false;
    };

    stdin.write_all(b"q\n").await.is_ok()
}

async fn esperar_sin_progreso(path: &Path, timeout: Duration, check_interval: Duration) {
    let mut ultimo_tamano = tamano_archivo(path).await;
    let mut ultimo_cambio = Instant::now();

    loop {
        tokio::time::sleep(check_interval).await;
        let tamano = tamano_archivo(path).await;
        if tamano > ultimo_tamano {
            ultimo_tamano = tamano;
            ultimo_cambio = Instant::now();
            continue;
        }

        if ultimo_cambio.elapsed() >= timeout {
            return;
        }
    }
}

async fn tamano_archivo(path: &Path) -> u64 {
    tokio::fs::metadata(path)
        .await
        .map(|meta| meta.len())
        .unwrap_or(0)
}

#[cfg(unix)]
fn solicitar_cierre_ffmpeg(child: &mut Child) {
    if let Some(pid) = child.id() {
        unsafe {
            libc::kill(pid as i32, libc::SIGINT);
        }
    } else {
        let _ = child.start_kill();
    }
}

#[cfg(not(unix))]
fn solicitar_cierre_ffmpeg(child: &mut Child) {
    let _ = child.start_kill();
}

fn formatear_error_ffmpeg(status: ExitStatus, stderr: &[u8]) -> String {
    match resumen_stderr(stderr) {
        Some(stderr) => format!("FFmpeg exited with status: {}. stderr: {}", status, stderr),
        None => format!("FFmpeg exited with status: {}", status),
    }
}

fn resumen_stderr(stderr: &[u8]) -> Option<String> {
    const MAX_CHARS: usize = 1200;
    const MAX_LINES: usize = 8;

    let texto = String::from_utf8_lossy(stderr);
    let mut resumen = Vec::new();

    for linea in texto.lines() {
        let linea = redactar_linea_sensible(linea.trim());
        if linea.is_empty() {
            continue;
        }
        resumen.push(linea);
        if resumen.len() >= MAX_LINES {
            break;
        }
    }

    let mut texto = resumen.join(" | ");
    if texto.is_empty() {
        return None;
    }
    if texto.chars().count() > MAX_CHARS {
        texto = texto.chars().take(MAX_CHARS).collect();
        texto.push_str("...");
    }
    Some(texto)
}

fn redactar_linea_sensible(linea: &str) -> String {
    if linea.to_ascii_lowercase().contains("cookie:") {
        "Cookie: [redacted]".to_string()
    } else {
        linea.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::value_objects::VideoQuality;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

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
    fn best_resuelve_url_de_variante_mas_alta() {
        let master = StreamUrl::try_from("https://example.com/hls/master.m3u8").unwrap();
        let variantes = vec![
            VarianteStream {
                url: "360/index.m3u8".to_string(),
                bandwidth: Some(800_000),
                height: Some(360),
            },
            VarianteStream {
                url: "1080/index.m3u8".to_string(),
                bandwidth: Some(5_128_000),
                height: Some(1080),
            },
        ];

        let url =
            resolver_url_variante(&master, &variantes, VideoQuality::Best).expect("resuelve best");

        assert_eq!(url.as_str(), "https://example.com/hls/1080/index.m3u8");
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

    #[test]
    fn parsear_variantes_rechaza_html_con_http_ok() {
        let err = parsear_variantes("<html>access denied</html>").expect_err("playlist invalida");

        assert!(err
            .to_string()
            .contains("Invalid playlist: missing #EXTM3U header"));
    }

    #[test]
    fn seleccionar_calidad_fallback_a_mas_cercana_superior() {
        let playlist = "\
#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=2800000,RESOLUTION=1920x1080
hi.m3u8
";
        let variantes = parsear_variantes(playlist).expect("parse failed");
        let sel = seleccionar_variante(&variantes, VideoQuality::P720).unwrap();

        assert_eq!(sel.url, "hi.m3u8");
    }

    #[test]
    fn clasificar_status_http_distingue_respuestas() {
        assert_eq!(clasificar_status_http(StatusCode::OK), EstadoHttp::Ok);
        assert_eq!(
            clasificar_status_http(StatusCode::NOT_FOUND),
            EstadoHttp::NoEncontrado
        );
        assert_eq!(
            clasificar_status_http(StatusCode::TOO_MANY_REQUESTS),
            EstadoHttp::RateLimited
        );
        assert_eq!(
            clasificar_status_http(StatusCode::FORBIDDEN),
            EstadoHttp::RequiereSesion
        );
        assert_eq!(
            clasificar_status_http(StatusCode::BAD_REQUEST),
            EstadoHttp::Permanente
        );
        assert_eq!(
            clasificar_status_http(StatusCode::BAD_GATEWAY),
            EstadoHttp::Reintentable
        );
    }

    #[test]
    fn duration_args_agrega_limite_de_ffmpeg() {
        assert_eq!(
            duration_args(Some(20)),
            vec!["-t".to_string(), "20".to_string()]
        );
        assert!(duration_args(None).is_empty());
    }

    #[test]
    fn clasificar_contexto_online_devuelve_stream_url() {
        let estado =
            clasificar_chat_video_context(r#"{"hls_source":"https://example.com/stream.m3u8"}"#);

        let EstadoStream::Online { stream_url } = estado else {
            panic!("se esperaba online");
        };
        assert_eq!(stream_url.as_str(), "https://example.com/stream.m3u8");
    }

    #[test]
    fn clasificar_contexto_null_es_offline() {
        let estado =
            clasificar_chat_video_context(r#"{"room_status":"offline","hls_source":null}"#);

        assert_eq!(estado, EstadoStream::Offline);
    }

    #[test]
    fn clasificar_contexto_vacio_es_offline() {
        let estado = clasificar_chat_video_context(r#"{"room_status":"offline","hls_source":""}"#);

        assert_eq!(estado, EstadoStream::Offline);
    }

    #[test]
    fn clasificar_contexto_vacio_sin_offline_es_inesperado() {
        let estado = clasificar_chat_video_context(r#"{"room_status":"public","hls_source":""}"#);

        assert!(matches!(estado, EstadoStream::RespuestaInesperada { .. }));
    }

    #[test]
    fn clasificar_contexto_privado_sin_hls_requiere_sesion() {
        let estado = clasificar_chat_video_context(r#"{"room_status":"private","hls_source":""}"#);

        assert!(matches!(estado, EstadoStream::RequiereSesion { .. }));
    }

    #[test]
    fn clasificar_contexto_fanclub_sin_hls_requiere_sesion() {
        let estado = clasificar_chat_video_context(r#"{"room_status":"fanclub","hls_source":""}"#);

        assert!(matches!(estado, EstadoStream::RequiereSesion { .. }));
    }

    #[test]
    fn clasificar_html_como_bloqueo() {
        let estado = clasificar_chat_video_context("<html>cloudflare</html>");

        assert!(matches!(estado, EstadoStream::Bloqueado { .. }));
    }

    #[test]
    fn clasificar_json_invalido_como_inesperado() {
        let estado = clasificar_chat_video_context("{");

        assert!(matches!(estado, EstadoStream::RespuestaInesperada { .. }));
    }

    #[test]
    fn clasificar_hls_invalido_como_inesperado() {
        let estado = clasificar_chat_video_context(r#"{"hls_source":"not-a-url"}"#);

        assert!(matches!(estado, EstadoStream::RespuestaInesperada { .. }));
    }

    #[test]
    fn resumen_stderr_omite_salida_vacia() {
        assert_eq!(resumen_stderr(b"\n  \n"), None);
    }

    #[test]
    fn resumen_stderr_redacta_cookie() {
        let resumen = resumen_stderr(b"Cookie: PHPSESSID=secret; other=value\nfallo").unwrap();

        assert_eq!(resumen, "Cookie: [redacted] | fallo");
    }

    #[test]
    fn resumen_stderr_limita_lineas() {
        let stderr = b"1\n2\n3\n4\n5\n6\n7\n8\n9\n";
        let resumen = resumen_stderr(stderr).unwrap();

        assert_eq!(resumen, "1 | 2 | 3 | 4 | 5 | 6 | 7 | 8");
    }

    #[tokio::test]
    async fn consultar_estado_online_contrato_http() {
        let Some((base_url, request_task)) = servidor_http_falso(
            200,
            r#"{"room_status":"public","hls_source":"https://example.com/live.m3u8"}"#,
        )
        .await
        else {
            return;
        };
        let mut client = ChaturbateClient::new().expect("crea cliente");
        client.base_url = base_url;
        let model = ModelName::try_from("alice").unwrap();

        let estado = client.consultar_estado(&model).await.expect("consulta");

        let EstadoStream::Online { stream_url } = estado else {
            panic!("se esperaba online");
        };
        assert_eq!(stream_url.as_str(), "https://example.com/live.m3u8");
        let request = request_task.await.expect("request task");
        assert!(request.starts_with("GET /api/chatvideocontext/alice/ HTTP/1.1"));
    }

    #[tokio::test]
    async fn consultar_estado_rate_limit_contrato_http() {
        let Some((base_url, request_task)) = servidor_http_falso(429, "{}").await else {
            return;
        };
        let mut client = ChaturbateClient::new().expect("crea cliente");
        client.base_url = base_url;
        let model = ModelName::try_from("alice").unwrap();

        let estado = client.consultar_estado(&model).await.expect("consulta");

        assert_eq!(estado, EstadoStream::RateLimited);
        let _ = request_task.await.expect("request task");
    }

    #[tokio::test]
    async fn consultar_estado_forbidden_requiere_sesion_contrato_http() {
        let Some((base_url, request_task)) = servidor_http_falso(403, "{}").await else {
            return;
        };
        let mut client = ChaturbateClient::new().expect("crea cliente");
        client.base_url = base_url;
        let model = ModelName::try_from("alice").unwrap();

        let estado = client.consultar_estado(&model).await.expect("consulta");

        assert!(matches!(estado, EstadoStream::RequiereSesion { .. }));
        let _ = request_task.await.expect("request task");
    }

    #[tokio::test]
    async fn consultar_estado_html_ok_bloqueado_contrato_http() {
        let Some((base_url, request_task)) =
            servidor_http_falso(200, "<html>access denied</html>").await
        else {
            return;
        };
        let mut client = ChaturbateClient::new().expect("crea cliente");
        client.base_url = base_url;
        let model = ModelName::try_from("alice").unwrap();

        let estado = client.consultar_estado(&model).await.expect("consulta");

        assert!(matches!(estado, EstadoStream::Bloqueado { .. }));
        let _ = request_task.await.expect("request task");
    }

    #[tokio::test]
    async fn listar_calidades_parsea_playlist_contrato_http() {
        let playlist = "\
#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360
low.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2800000,RESOLUTION=1920x1080
hi.m3u8
";
        let Some((base_url, request_task)) = servidor_http_falso(200, playlist).await else {
            return;
        };
        let client = ChaturbateClient::new().expect("crea cliente");
        let master = StreamUrl::try_from(format!("{base_url}/hls/master.m3u8")).unwrap();

        let calidades = client
            .listar_calidades(&master)
            .await
            .expect("lista calidades");

        assert_eq!(calidades.len(), 2);
        assert_eq!(calidades[0].height, Some(360));
        assert_eq!(calidades[1].height, Some(1080));
        let request = request_task.await.expect("request task");
        assert!(request.starts_with("GET /hls/master.m3u8 HTTP/1.1"));
    }

    #[tokio::test]
    async fn listar_calidades_rechaza_html_ok_contrato_http() {
        let Some((base_url, request_task)) =
            servidor_http_falso(200, "<html>cloudflare</html>").await
        else {
            return;
        };
        let client = ChaturbateClient::new().expect("crea cliente");
        let master = StreamUrl::try_from(format!("{base_url}/hls/master.m3u8")).unwrap();

        let err = client
            .listar_calidades(&master)
            .await
            .expect_err("playlist html debe fallar");

        assert!(err
            .to_string()
            .contains("Invalid playlist: missing #EXTM3U header"));
        let _ = request_task.await.expect("request task");
    }

    #[tokio::test]
    async fn esperar_sin_progreso_detecta_archivo_estancado() {
        let path = std::env::temp_dir().join(format!(
            "cbrec_stall_{}.part.mp4",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        tokio::fs::write(&path, b"datos")
            .await
            .expect("crea parcial");

        esperar_sin_progreso(&path, Duration::from_millis(20), Duration::from_millis(5)).await;

        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn consultar_estado_envia_cookie_si_existe() {
        let Some((base_url, request_task)) =
            servidor_http_falso(200, r#"{"room_status":"offline","hls_source":null}"#).await
        else {
            return;
        };
        let mut client = ChaturbateClient::new()
            .expect("crea cliente")
            .with_session_cookie("PHPSESSID=abc; chaturbatesid=xyz".to_string());
        client.base_url = base_url;
        let model = ModelName::try_from("alice").unwrap();

        let estado = client.consultar_estado(&model).await.expect("consulta");

        assert_eq!(estado, EstadoStream::Offline);
        let request = request_task.await.expect("request task");
        assert!(request
            .to_ascii_lowercase()
            .contains("cookie: phpsessid=abc; chaturbatesid=xyz"));
    }

    async fn servidor_http_falso(
        status: u16,
        body: &'static str,
    ) -> Option<(String, JoinHandle<String>)> {
        let listener = match TcpListener::bind("127.0.0.1:0").await {
            Ok(listener) => listener,
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return None,
            Err(e) => panic!("bind test server: {e}"),
        };
        let addr = listener.local_addr().expect("addr test server");
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept request");
            let mut buffer = vec![0_u8; 4096];
            let n = socket.read(&mut buffer).await.expect("read request");
            let request = String::from_utf8_lossy(&buffer[..n]).to_string();
            let response = format!(
                "HTTP/1.1 {} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                status,
                razon_http(status),
                body.len(),
                body
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write response");
            request
        });
        Some((format!("http://{}", addr), task))
    }

    fn razon_http(status: u16) -> &'static str {
        match status {
            200 => "OK",
            403 => "Forbidden",
            429 => "Too Many Requests",
            _ => "Status",
        }
    }
}
