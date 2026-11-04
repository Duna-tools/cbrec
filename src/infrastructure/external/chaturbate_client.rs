use crate::domain::repositories::StreamRepository;
use crate::domain::value_objects::VideoQuality;
use crate::domain::value_objects::{ModelName, StreamUrl};
use crate::infrastructure::external::ffmpeg_process::run_ffmpeg;
use crate::infrastructure::external::hls_relay::HlsRelay;
use crate::infrastructure::InfrastructureError;
use async_trait::async_trait;
use quick_m3u8::config::ParsingOptionsBuilder;
use quick_m3u8::tag::{hls, KnownTag};
use quick_m3u8::{HlsLine, Reader};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::path::{Path, PathBuf};
use tokio::sync::watch;
use tokio::time::{Duration, Instant};

const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36";
const HTTP_TIMEOUT_SECS: u64 = 10;
const HTTP_RETRY_BASE_MS: u64 = 200;
const HTTP_RETRY_MAX_INTERVAL_SECS: u64 = 2;
const HTTP_RETRY_MAX_ELAPSED_SECS: u64 = 10;
const MAX_RESPONSE_BODY_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
pub(super) enum RetryFailure<E> {
    Transient(E),
    Permanent(E),
}

fn retry_budget_exceeded(elapsed: Duration, delay: Duration, max_elapsed: Duration) -> bool {
    elapsed.saturating_add(delay) > max_elapsed
}

fn response_body_too_large() -> InfrastructureError {
    InfrastructureError::ExternalService(format!(
        "La respuesta HTTP excede el limite de {} bytes",
        MAX_RESPONSE_BODY_BYTES
    ))
}

fn append_body_chunk(body: &mut Vec<u8>, chunk: &[u8]) -> Result<(), InfrastructureError> {
    if chunk.len() > MAX_RESPONSE_BODY_BYTES.saturating_sub(body.len()) {
        return Err(response_body_too_large());
    }
    body.extend_from_slice(chunk);
    Ok(())
}

async fn read_limited_text(
    mut response: reqwest::Response,
) -> Result<String, RetryFailure<InfrastructureError>> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BODY_BYTES as u64)
    {
        return Err(RetryFailure::Permanent(response_body_too_large()));
    }

    let capacity = response
        .content_length()
        .unwrap_or_default()
        .min(MAX_RESPONSE_BODY_BYTES as u64) as usize;
    let mut body = Vec::with_capacity(capacity);
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        RetryFailure::Transient(InfrastructureError::ExternalService(format!(
            "No se pudo leer la respuesta HTTP: {}",
            error.without_url()
        )))
    })? {
        append_body_chunk(&mut body, &chunk).map_err(RetryFailure::Permanent)?;
    }

    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// Retries transient HTTP failures with bounded deterministic exponential delays.
///
/// ponytail: add jitter only if synchronized clients create measurable upstream load spikes.
pub(super) async fn retry_with_backoff<T, F, Fut>(
    mut operation: F,
) -> Result<T, InfrastructureError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, RetryFailure<InfrastructureError>>>,
{
    let started = Instant::now();
    let max_elapsed = Duration::from_secs(HTTP_RETRY_MAX_ELAPSED_SECS);
    let max_interval = Duration::from_secs(HTTP_RETRY_MAX_INTERVAL_SECS);
    let mut delay = Duration::from_millis(HTTP_RETRY_BASE_MS);

    loop {
        let result = tokio::time::timeout_at(started + max_elapsed, operation())
            .await
            .map_err(|_| {
                InfrastructureError::ExternalService(
                    "Se agotó el plazo total de la consulta HTTP".to_string(),
                )
            })?;
        match result {
            Ok(value) => return Ok(value),
            Err(RetryFailure::Permanent(error)) => return Err(error),
            Err(RetryFailure::Transient(error)) => {
                if retry_budget_exceeded(started.elapsed(), delay, max_elapsed) {
                    return Err(error);
                }
                tokio::time::sleep(delay).await;
                delay = delay.saturating_mul(2).min(max_interval);
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct ChatVideoContext {
    hls_source: Option<String>,
    room_status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RoomListResponse {
    #[serde(default)]
    rooms: Vec<RoomListEntry>,
    total_count: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct RoomListEntry {
    username: Option<String>,
    room_subject: Option<String>,
    tags: Option<Vec<String>>,
    num_users: Option<u64>,
    current_show: Option<String>,
    img: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct DiscoveredRoom {
    #[serde(skip)]
    pub(crate) thumbnail_url: Option<String>,
    pub(crate) username: String,
    pub(crate) subject: String,
    pub(crate) viewers: u64,
    pub(crate) show: String,
}

pub(crate) struct RoomPage {
    pub rooms: Vec<DiscoveredRoom>,
    pub total: Option<usize>,
    pub has_more: bool,
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
    min_free_space: u64,
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
            min_free_space: 0,
        })
    }

    async fn cancel_http<T>(
        &self,
        operation: impl Future<Output = Result<T, InfrastructureError>>,
    ) -> Result<T, InfrastructureError> {
        let Some(mut receiver) = self.cancel_rx.clone() else {
            return operation.await;
        };
        if *receiver.borrow() {
            return Err(InfrastructureError::RecordingCancelled);
        }
        tokio::select! {
            result = operation => result,
            _ = receiver.wait_for(|cancelled| *cancelled) => Err(InfrastructureError::RecordingCancelled),
        }
    }

    pub(crate) async fn thumbnail(
        &self,
        model: &ModelName,
        source: Option<&str>,
    ) -> Result<super::thumbnail::ThumbnailResponse, InfrastructureError> {
        let url = super::thumbnail::thumbnail_url(model, source)?;
        self.cancel_http(super::thumbnail::download(&self.client, url))
            .await
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

    pub fn with_min_free_space(mut self, bytes: u64) -> Self {
        self.min_free_space = bytes;
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

        self.cancel_http(retry_with_backoff(|| async {
            let response = self.get_request(&url).send().await.map_err(|e| {
                RetryFailure::Transient(InfrastructureError::ExternalService(format!(
                    "HTTP request failed: {}",
                    e.without_url()
                )))
            })?;

            match clasificar_status_http(response.status()) {
                EstadoHttp::Ok => {}
                EstadoHttp::NoEncontrado => {
                    return Err(RetryFailure::Permanent(InfrastructureError::Domain(
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
                    return Err(RetryFailure::Transient(error_status_http(
                        response.status(),
                    )));
                }
                EstadoHttp::Permanente => {
                    return Ok(EstadoStream::Bloqueado {
                        detalle: format!("HTTP {}", response.status()),
                    });
                }
            }

            let contenido = read_limited_text(response).await?;

            Ok(clasificar_chat_video_context(&contenido))
        }))
        .await
    }

    pub(crate) async fn discover_rooms_by_tag(
        &self,
        tag: &str,
        limit: usize,
    ) -> Result<Vec<DiscoveredRoom>, InfrastructureError> {
        Ok(self
            .discover_room_page(Some(tag), limit, 0)
            .await?
            .rooms
            .into_iter()
            .take(limit)
            .collect())
    }

    pub(crate) async fn discover_room_page(
        &self,
        tag: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<RoomPage, InfrastructureError> {
        let url = format!("{}/api/ts/roomlist/room-list/", self.base_url);

        self.cancel_http(retry_with_backoff(|| async {
            let mut request = self.client.get(&url);
            if let Some(tag) = tag {
                request = request.query(&[("hashtags", tag)]);
            }
            let response = request
                .query(&[("limit", limit), ("offset", offset)])
                .send()
                .await
                .map_err(|error| {
                    RetryFailure::Transient(InfrastructureError::ExternalService(format!(
                        "HTTP request failed: {}",
                        error.without_url()
                    )))
                })?;

            match clasificar_status_http(response.status()) {
                EstadoHttp::Ok => {}
                EstadoHttp::RateLimited | EstadoHttp::Reintentable => {
                    return Err(RetryFailure::Transient(error_status_http(
                        response.status(),
                    )));
                }
                EstadoHttp::NoEncontrado | EstadoHttp::RequiereSesion | EstadoHttp::Permanente => {
                    return Err(RetryFailure::Permanent(error_status_http(
                        response.status(),
                    )));
                }
            }

            let content = read_limited_text(response).await?;
            let response: RoomListResponse = serde_json::from_str(&content).map_err(|error| {
                RetryFailure::Permanent(InfrastructureError::ExternalService(format!(
                    "Invalid room list response: {error}"
                )))
            })?;

            let has_more = response
                .total_count
                .map_or(response.rooms.len() >= limit, |total| {
                    offset.saturating_add(limit) < total
                });
            let total = response.total_count;
            let rooms = response
                .rooms
                .into_iter()
                .filter_map(|room| {
                    let tags = room.tags.unwrap_or_default();
                    if tag.is_some_and(|tag| {
                        !tags.iter().any(|value| value.eq_ignore_ascii_case(tag))
                    }) {
                        return None;
                    }
                    let username = ModelName::try_from(room.username?).ok()?;
                    let thumbnail_url = room.img.filter(|source| {
                        super::thumbnail::thumbnail_url(&username, Some(source)).is_ok()
                    });
                    Some(DiscoveredRoom {
                        thumbnail_url,
                        username: username.as_str().to_string(),
                        subject: room.room_subject.unwrap_or_default(),
                        viewers: room.num_users.unwrap_or_default(),
                        show: room.current_show.unwrap_or_else(|| "unknown".to_string()),
                    })
                })
                .collect();
            Ok(RoomPage {
                rooms,
                total,
                has_more,
            })
        }))
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
        let stream_url = self.resolver_variant_url(stream_url, quality).await?;

        let ffmpeg_path = self
            .ffmpeg_path
            .as_deref()
            .unwrap_or_else(|| Path::new("ffmpeg"));
        let relay = HlsRelay::start(self.client.clone(), stream_url.as_str()).await?;
        let result = run_ffmpeg(
            ffmpeg_path,
            &relay.url,
            output_path,
            self.max_duration_secs,
            self.min_free_space,
            self.cancel_rx.clone(),
        )
        .await;
        match result {
            Err(InfrastructureError::RecordingCancelled) => {
                Err(InfrastructureError::RecordingCancelled)
            }
            Err(error) => match relay.last_error().await {
                Some(detail) => Err(InfrastructureError::RecordingError(format!(
                    "{error}; último error HLS: {detail}"
                ))),
                None => Err(error),
            },
            Ok(()) => Ok(()),
        }
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
        self.cancel_http(retry_with_backoff(|| async {
            // Media URLs carry their own authorization; never forward website cookies.
            let response = self.client.get(url).send().await.map_err(|e| {
                RetryFailure::Transient(InfrastructureError::ExternalService(format!(
                    "HTTP request failed: {}",
                    e.without_url()
                )))
            })?;

            match clasificar_status_http(response.status()) {
                EstadoHttp::Ok => {}
                EstadoHttp::RateLimited => {
                    return Err(RetryFailure::Transient(InfrastructureError::HttpStatus(
                        429,
                    )));
                }
                EstadoHttp::RequiereSesion => {
                    return Err(RetryFailure::Permanent(error_status_http(
                        response.status(),
                    )));
                }
                EstadoHttp::Reintentable => {
                    return Err(RetryFailure::Transient(error_status_http(
                        response.status(),
                    )));
                }
                EstadoHttp::NoEncontrado | EstadoHttp::Permanente => {
                    return Err(RetryFailure::Permanent(error_status_http(
                        response.status(),
                    )));
                }
            }

            read_limited_text(response).await
        }))
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

    #[tokio::test]
    async fn retry_deadline_includes_time_spent_in_requests() {
        let started = Instant::now();
        let attempts = std::sync::atomic::AtomicUsize::new(0);
        let result = retry_with_backoff(|| {
            let attempt = attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async move {
                tokio::time::sleep(Duration::from_secs(6)).await;
                if attempt == 0 {
                    Err(RetryFailure::Transient(InfrastructureError::HttpStatus(
                        503,
                    )))
                } else {
                    Ok(())
                }
            }
        })
        .await;
        assert!(
            result.is_err(),
            "El segundo intento no debe superar el presupuesto total"
        );
        assert!(started.elapsed() < Duration::from_secs(11));
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn retry_with_backoff_retries_transient_error() {
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let operation_attempts = attempts.clone();

        let result = retry_with_backoff(|| {
            let attempt = operation_attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async move {
                if attempt == 0 {
                    Err(RetryFailure::Transient(InfrastructureError::HttpStatus(
                        503,
                    )))
                } else {
                    Ok("ok")
                }
            }
        })
        .await;

        assert_eq!(result.unwrap(), "ok");
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn retry_with_backoff_does_not_retry_permanent_error() {
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let operation_attempts = attempts.clone();

        let result: Result<(), InfrastructureError> = retry_with_backoff(|| {
            operation_attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async {
                Err(RetryFailure::Permanent(InfrastructureError::HttpStatus(
                    403,
                )))
            }
        })
        .await;

        assert!(matches!(result, Err(InfrastructureError::HttpStatus(403))));
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn response_body_rejects_bytes_over_limit() {
        let mut body = Vec::new();
        append_body_chunk(&mut body, &vec![b'x'; MAX_RESPONSE_BODY_BYTES])
            .expect("the exact limit is accepted");

        let error = append_body_chunk(&mut body, b"y").expect_err("one extra byte is rejected");

        assert_eq!(body.len(), MAX_RESPONSE_BODY_BYTES);
        assert!(error.to_string().contains("excede el limite"));
    }

    #[tokio::test]
    async fn response_body_rejects_declared_oversize_before_reading() {
        let listener = match TcpListener::bind("127.0.0.1:0").await {
            Ok(listener) => listener,
            Err(error) => panic!("bind test server: {error}"),
        };
        let address = listener.local_addr().expect("test server address");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept request");
            let mut request = [0_u8; 1024];
            let _ = socket.read(&mut request).await.expect("read request");
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                MAX_RESPONSE_BODY_BYTES + 1
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write response headers");
        });
        let response = reqwest::Client::new()
            .get(format!("http://{address}"))
            .send()
            .await
            .expect("request test server");

        let error = read_limited_text(response)
            .await
            .expect_err("declared oversized body must fail");

        let RetryFailure::Permanent(error) = error else {
            panic!("un cuerpo excesivo no debe reintentarse");
        };
        assert!(error.to_string().contains("excede el limite"));
        server.await.expect("test server completes");
    }

    #[test]
    fn retry_budget_distinguishes_limit_boundary() {
        let max_elapsed = Duration::from_secs(10);

        assert!(!retry_budget_exceeded(
            Duration::from_secs(7),
            Duration::from_secs(2),
            max_elapsed
        ));
        assert!(!retry_budget_exceeded(
            Duration::from_secs(8),
            Duration::from_secs(2),
            max_elapsed
        ));
        assert!(retry_budget_exceeded(
            Duration::from_secs(9),
            Duration::from_secs(2),
            max_elapsed
        ));
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

    #[tokio::test]
    async fn consultar_estado_online_contrato_http() {
        let (base_url, request_task) = servidor_http_falso(
            200,
            r#"{"room_status":"public","hls_source":"https://example.com/live.m3u8"}"#,
        )
        .await;
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
        let (base_url, request_task) = servidor_http_falso(429, "{}").await;
        let mut client = ChaturbateClient::new().expect("crea cliente");
        client.base_url = base_url;
        let model = ModelName::try_from("alice").unwrap();

        let estado = client.consultar_estado(&model).await.expect("consulta");

        assert_eq!(estado, EstadoStream::RateLimited);
        let _ = request_task.await.expect("request task");
    }

    #[tokio::test]
    async fn consultar_estado_forbidden_requiere_sesion_contrato_http() {
        let (base_url, request_task) = servidor_http_falso(403, "{}").await;
        let mut client = ChaturbateClient::new().expect("crea cliente");
        client.base_url = base_url;
        let model = ModelName::try_from("alice").unwrap();

        let estado = client.consultar_estado(&model).await.expect("consulta");

        assert!(matches!(estado, EstadoStream::RequiereSesion { .. }));
        let _ = request_task.await.expect("request task");
    }

    #[tokio::test]
    async fn consultar_estado_html_ok_bloqueado_contrato_http() {
        let (base_url, request_task) = servidor_http_falso(200, "<html>access denied</html>").await;
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
        let (base_url, request_task) = servidor_http_falso(200, playlist).await;
        let client = ChaturbateClient::new()
            .expect("crea cliente")
            .with_session_cookie("sessionid=synthetic-secret".into());
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
        assert!(!request.to_ascii_lowercase().contains("cookie:"));
    }

    #[tokio::test]
    async fn invalid_playlist_does_not_start_ffmpeg_or_ignore_quality() {
        let (base_url, request) = servidor_http_falso(200, "<html>blocked</html>").await;
        let client = ChaturbateClient::new()
            .unwrap()
            .with_ffmpeg_path(PathBuf::from("missing-ffmpeg"));
        let stream = StreamUrl::new(base_url).unwrap();
        let error = client
            .download_stream(&stream, Path::new("unused.mp4"), VideoQuality::P720)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("Invalid playlist"), "{error}");
        request.await.unwrap();
    }

    #[tokio::test]
    async fn listar_calidades_rechaza_html_ok_contrato_http() {
        let (base_url, request_task) = servidor_http_falso(200, "<html>cloudflare</html>").await;
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
    async fn consultar_estado_envia_cookie_si_existe() {
        let (base_url, request_task) =
            servidor_http_falso(200, r#"{"room_status":"offline","hls_source":null}"#).await;
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

    #[tokio::test]
    async fn discover_rooms_filters_tag_and_omits_cookie() {
        let body = r#"{"rooms":[{"username":"Alice","img":"https://thumb.live.mmcdn.com/riw/alice.jpg?1793720040","room_subject":"hello","tags":["Gaming"],"num_users":42,"current_show":"public"},{"username":"bob","tags":["music"]}]}"#;
        let (base_url, request_task) = servidor_http_falso(200, body).await;
        let mut client = ChaturbateClient::new()
            .expect("crea cliente")
            .with_session_cookie("PHPSESSID=secret".to_string());
        client.base_url = base_url;

        let rooms = client
            .discover_rooms_by_tag("gaming", 2)
            .await
            .expect("discover rooms");

        assert_eq!(
            rooms,
            vec![DiscoveredRoom {
                thumbnail_url: Some("https://thumb.live.mmcdn.com/riw/alice.jpg?1793720040".into()),
                username: "alice".to_string(),
                subject: "hello".to_string(),
                viewers: 42,
                show: "public".to_string(),
            }]
        );
        assert!(serde_json::to_value(&rooms).unwrap()[0]
            .get("thumbnail_url")
            .is_none());
        let request = request_task.await.expect("request task");
        assert!(request.starts_with(
            "GET /api/ts/roomlist/room-list/?hashtags=gaming&limit=2&offset=0 HTTP/1.1"
        ));
        assert!(!request.to_ascii_lowercase().contains("cookie:"));
    }

    #[tokio::test]
    async fn discovery_pages_follow_offsets_totals_and_keep_promoted_entries_without_cookies() {
        for (tag, offset, total, more) in [(None, 0, 3, true), (Some("gaming"), 2, 3, false)] {
            let body = if offset == 0 {
                r#"{"total_count":3,"rooms":[{"username":"alice"},{"username":"bob"},{"username":"promoted"}]}"#
            } else {
                r#"{"total_count":3,"rooms":[{"username":"carol","tags":["gaming"]}]}"#
            };
            let (url, request) = servidor_http_falso(200, body).await;
            let mut client = ChaturbateClient::new()
                .unwrap()
                .with_session_cookie("sessionid=secret".into());
            client.base_url = url;
            let page = client.discover_room_page(tag, 2, offset).await.unwrap();
            assert_eq!(page.total, Some(total));
            assert_eq!(page.has_more, more);
            assert_eq!(page.rooms.len(), if offset == 0 { 3 } else { 1 });
            let request = request.await.unwrap();
            assert!(request.contains(&format!("limit=2&offset={offset}")));
            assert_eq!(request.contains("hashtags=gaming"), tag.is_some());
            assert!(!request.to_ascii_lowercase().contains("cookie:"));
        }
    }

    #[tokio::test]
    async fn direct_profile_query_uses_fixed_context_and_preserves_access_states() {
        use crate::application::commands::discover::find_page;
        for (status, body, expected) in [
            (200, r#"{"room_status":"offline"}"#, Some("offline")),
            (
                200,
                r#"{"room_status":"public","hls_source":"https://example.com/live.m3u8"}"#,
                Some("online"),
            ),
            (403, "{}", Some("requiere sesión o permisos")),
            (429, "{}", None),
            (200, "<html>blocked</html>", None),
        ] {
            let (url, request) = servidor_http_falso(status, body).await;
            let mut client = ChaturbateClient::new().unwrap();
            client.base_url = url;
            let result = find_page(
                &client,
                "https://chaturbate.com/Alice/?token=ignored",
                20,
                0,
            )
            .await;
            if let Some(show) = expected {
                let (query, page) = result.unwrap();
                assert_eq!(query, "@alice");
                assert_eq!(page.rooms[0].show, show);
                assert!(!page.has_more);
            } else {
                assert!(result.is_err());
            }
            let request = request.await.unwrap();
            assert!(request.starts_with("GET /api/chatvideocontext/alice/ HTTP/1.1"));
            assert!(!request.contains("token="));
        }
        let client = ChaturbateClient::new().unwrap();
        for (query, limit, offset) in [
            ("@alice", 20, 20),
            ("https://evil.example/alice", 20, 0),
            ("", 0, 0),
        ] {
            assert!(find_page(&client, query, limit, offset).await.is_err());
        }
    }

    #[tokio::test]
    async fn thumbnail_download_never_uses_the_account_cookie() {
        let (url, request_task) = servidor_http_falso(200, "synthetic").await;
        let client = ChaturbateClient::new()
            .unwrap()
            .with_session_cookie("sessionid=synthetic-secret".into());
        // El servidor local prueba el transporte; el validador del CDN se prueba aparte.
        assert!(super::super::thumbnail::download(
            &client.client,
            reqwest::Url::parse(&url).unwrap()
        )
        .await
        .is_err());
        let request = request_task.await.unwrap();
        assert!(!request.to_ascii_lowercase().contains("cookie:"));
        assert!(request
            .to_ascii_lowercase()
            .contains("accept: image/jpeg,image/png"));
    }

    struct HttpTestServer {
        url: String,
        requests: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        first_request: Option<tokio::sync::oneshot::Receiver<()>>,
        task: JoinHandle<()>,
    }

    impl HttpTestServer {
        async fn wait_for_request(&mut self) {
            tokio::time::timeout(Duration::from_secs(2), self.first_request.take().unwrap())
                .await
                .expect("el servidor debe recibir la solicitud")
                .expect("el servidor debe confirmar la solicitud");
        }

        fn request_count(&self) -> usize {
            self.requests.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl Drop for HttpTestServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn http_test_server(responses: Vec<String>, hold_connection: bool) -> HttpTestServer {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let request_count = requests.clone();
        let (sender, first_request) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let mut sender = Some(sender);
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                let mut chunk = [0_u8; 1024];
                while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                    let size = socket.read(&mut chunk).await.unwrap();
                    if size == 0 {
                        break;
                    }
                    request.extend_from_slice(&chunk[..size]);
                    assert!(request.len() <= 8192);
                }
                let index = request_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if !responses.is_empty() {
                    let response = &responses[index.min(responses.len() - 1)];
                    socket.write_all(response.as_bytes()).await.unwrap();
                }
                if let Some(sender) = sender.take() {
                    let _ = sender.send(());
                }
                if hold_connection {
                    std::future::pending::<()>().await;
                }
            }
        });
        HttpTestServer {
            url: format!("http://{address}"),
            requests,
            first_request: Some(first_request),
            task,
        }
    }

    #[tokio::test]
    async fn media_request_error_does_not_expose_url_secrets() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let mut client = ChaturbateClient::new().unwrap();
        client.client = Client::builder().no_proxy().build().unwrap();
        let url = StreamUrl::new(format!(
            "http://{address}/stream.m3u8?token=CANARY_MEDIA_TOKEN&key=CANARY_MEDIA_KEY"
        ))
        .unwrap();

        let error = client.listar_calidades(&url).await.unwrap_err().to_string();

        assert!(
            !error.contains("CANARY_MEDIA_TOKEN"),
            "el error expone el token"
        );
        assert!(
            !error.contains("CANARY_MEDIA_KEY"),
            "el error expone la clave"
        );
        assert!(!error.is_empty());
    }

    #[tokio::test]
    async fn cancellation_interrupts_pending_status_request() {
        let mut server = http_test_server(Vec::new(), true).await;
        let (sender, receiver) = watch::channel(false);
        let mut client = ChaturbateClient::new()
            .unwrap()
            .with_cancel_receiver(receiver);
        client.base_url = server.url.clone();
        let model = ModelName::new("alice").unwrap();
        let request = client.consultar_estado(&model);
        tokio::pin!(request);
        tokio::select! {
            _ = server.wait_for_request() => {}
            result = &mut request => panic!("la consulta termino antes de la cancelacion: {result:?}"),
        }
        sender.send(true).unwrap();

        let result = tokio::time::timeout(Duration::from_secs(1), request).await;

        assert!(matches!(
            result,
            Ok(Err(InfrastructureError::RecordingCancelled))
        ));
        assert_eq!(server.request_count(), 1);
    }

    #[tokio::test]
    async fn cancellation_interrupts_response_body_read() {
        let mut server = http_test_server(
            vec![
                "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n#EXTM3U\n"
                    .into(),
            ],
            true,
        )
        .await;
        let (sender, receiver) = watch::channel(false);
        let client = ChaturbateClient::new()
            .unwrap()
            .with_cancel_receiver(receiver);
        let url = StreamUrl::new(format!("{}/stream.m3u8", server.url)).unwrap();
        let request = client.listar_calidades(&url);
        tokio::pin!(request);
        tokio::select! {
            _ = server.wait_for_request() => {}
            result = &mut request => panic!("la lectura termino antes de la cancelacion: {result:?}"),
        }
        sender.send(true).unwrap();

        let result = tokio::time::timeout(Duration::from_secs(1), request).await;

        assert!(matches!(
            result,
            Ok(Err(InfrastructureError::RecordingCancelled))
        ));
        assert_eq!(server.request_count(), 1);
    }

    #[tokio::test]
    async fn cancellation_stops_retry_wait_without_another_request() {
        let mut server = http_test_server(
            vec!["HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into()],
            false,
        )
        .await;
        let (sender, receiver) = watch::channel(false);
        let client = ChaturbateClient::new()
            .unwrap()
            .with_cancel_receiver(receiver);
        let url = StreamUrl::new(format!("{}/stream.m3u8", server.url)).unwrap();
        let request = client.listar_calidades(&url);
        tokio::pin!(request);
        tokio::select! {
            _ = server.wait_for_request() => {}
            result = &mut request => panic!("la consulta termino antes del reintento: {result:?}"),
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
            result = &mut request => panic!("la consulta termino antes de cancelar el backoff: {result:?}"),
        }
        sender.send(true).unwrap();

        let result = tokio::time::timeout(Duration::from_secs(1), request).await;

        assert!(matches!(
            result,
            Ok(Err(InfrastructureError::RecordingCancelled))
        ));
        assert_eq!(server.request_count(), 1);
    }

    #[tokio::test]
    async fn truncated_playlist_response_is_retried_before_returning_quality() {
        let body = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360\nlow.m3u8\n";
        let server = http_test_server(
            vec![
                "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n#EXTM3U\n"
                    .into(),
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                ),
            ],
            false,
        )
        .await;
        let client = ChaturbateClient::new().unwrap();
        let url = StreamUrl::new(format!("{}/stream.m3u8", server.url)).unwrap();

        let result =
            tokio::time::timeout(Duration::from_secs(2), client.listar_calidades(&url)).await;

        let qualities = result
            .expect("el reintento debe ser acotado")
            .expect("la segunda respuesta debe recuperarse");
        assert_eq!(qualities.len(), 1);
        assert_eq!(qualities[0].height, Some(360));
        assert_eq!(server.request_count(), 2);
    }

    #[tokio::test]
    async fn oversized_playlist_response_is_not_retried() {
        let server = http_test_server(
            vec![format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                MAX_RESPONSE_BODY_BYTES + 1
            )],
            false,
        )
        .await;
        let client = ChaturbateClient::new().unwrap();
        let url = StreamUrl::new(format!("{}/stream.m3u8", server.url)).unwrap();

        let error = client.listar_calidades(&url).await.unwrap_err();

        assert!(error.to_string().contains("excede el limite"));
        assert_eq!(server.request_count(), 1);
    }

    #[tokio::test]
    async fn application_check_preserves_status_contracts_and_never_labels_access_errors_offline() {
        use crate::application::commands::check;
        use crate::presentation::ConsoleOutput;
        for (status, body, expected) in [
            (
                200,
                r#"{"room_status":"public","hls_source":"https://example.com/live.m3u8"}"#,
                "online",
            ),
            (200, r#"{"room_status":"offline"}"#, "offline"),
            (403, "{}", "requires_session"),
            (429, "{}", "rate_limited"),
            (200, "<html>challenge</html>", "blocked"),
            (200, "{}", "indeterminate"),
        ] {
            for json in [true, false] {
                let (base_url, request) = servidor_http_falso(status, body).await;
                let mut client = ChaturbateClient::new().unwrap();
                client.base_url = base_url;
                if json {
                    let result: serde_json::Value = serde_json::from_str(
                        &check::check_model_json(&client, "Alice").await.unwrap(),
                    )
                    .unwrap();
                    assert_eq!(result["model"], "alice");
                    assert_eq!(result["status"], expected);
                    assert_eq!(
                        result.get("detail").is_some(),
                        !matches!(expected, "online" | "offline")
                    );
                } else {
                    check::verificar_modelos(
                        &client,
                        &ConsoleOutput::default(),
                        vec!["Alice".into()],
                    )
                    .await
                    .unwrap();
                }
                assert!(request
                    .await
                    .unwrap()
                    .contains("/api/chatvideocontext/alice/"));
            }
        }
        let (base_url, request) = servidor_http_falso(404, "{}").await;
        let mut client = ChaturbateClient::new().unwrap();
        client.base_url = base_url;
        assert!(check::check_model_json(&client, "alice").await.is_err());
        request.await.unwrap();
    }

    #[tokio::test]
    async fn application_discovery_validates_input_and_preserves_json_and_terminal_results() {
        use crate::application::commands::discover;
        use crate::presentation::ConsoleOutput;
        let client = ChaturbateClient::new().unwrap();
        for (tag, limit) in [("bad tag", 1), ("gaming", 0), ("gaming", 51), ("", 1)] {
            assert!(discover::find_rooms(&client, tag, limit).await.is_err());
        }
        for body in [
            r#"{"rooms":[]}"#,
            r#"{"rooms":[{"username":"Alice","room_subject":"hello","tags":["gaming"],"num_users":42,"current_show":"public"}]}"#,
        ] {
            for json in [true, false] {
                let (base_url, request) = servidor_http_falso(200, body).await;
                let mut client = ChaturbateClient::new().unwrap();
                client.base_url = base_url;
                if json {
                    let result: serde_json::Value = serde_json::from_str(
                        &discover::discover_rooms_json(&client, " #Gaming ", 1)
                            .await
                            .unwrap(),
                    )
                    .unwrap();
                    assert_eq!(result["tag"], "gaming");
                    assert_eq!(
                        result["rooms"].as_array().unwrap().len(),
                        usize::from(body.contains("Alice"))
                    );
                    if body.contains("Alice") {
                        assert_eq!(result["rooms"][0]["username"], "alice");
                        assert_eq!(result["rooms"][0]["viewers"], 42);
                    }
                } else {
                    discover::discover_rooms(&client, &ConsoleOutput::default(), "#Gaming", 1)
                        .await
                        .unwrap();
                }
                assert!(request.await.unwrap().contains("hashtags=gaming&limit=1"));
            }
        }
    }

    #[tokio::test]
    async fn application_list_handles_offline_empty_and_variant_playlists() {
        use crate::application::commands::list;
        use crate::presentation::ConsoleOutput;
        let (base_url, request) = servidor_http_falso(200, r#"{"room_status":"offline"}"#).await;
        let mut client = ChaturbateClient::new().unwrap();
        client.base_url = base_url;
        list::listar_calidades_modelos(&client, &ConsoleOutput::default(), vec!["alice".into()])
            .await
            .unwrap();
        request.await.unwrap();
        for playlist in [
            "#EXTM3U\n#EXT-X-TARGETDURATION:5\n#EXTINF:5,\nsegment.ts\n",
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360\nlow.m3u8\n",
        ] {
            let (media_url, media_request) = servidor_http_falso(200, playlist).await;
            let body =
                format!(r#"{{"room_status":"public","hls_source":"{media_url}/master.m3u8"}}"#);
            let server = http_test_server(
                vec![format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )],
                false,
            )
            .await;
            let mut client = ChaturbateClient::new()
                .unwrap()
                .with_session_cookie("sessionid=synthetic".into());
            client.base_url = server.url.clone();
            list::listar_calidades_modelos(
                &client,
                &ConsoleOutput::default(),
                vec!["Alice".into()],
            )
            .await
            .unwrap();
            assert_eq!(server.request_count(), 1);
            let request = media_request.await.unwrap().to_ascii_lowercase();
            assert!(request.starts_with("get /master.m3u8"));
            assert!(!request.contains("cookie:"));
        }
    }

    #[tokio::test]
    async fn controller_query_modes_dispatch_without_ffmpeg_or_persistence() {
        use crate::application::ejecutar_cli;
        use crate::infrastructure::AppConfig;
        use crate::presentation::Cli;
        use clap::Parser;
        for arguments in [
            vec!["cbrec", "check", "alice", "--json"],
            vec!["cbrec", "check", "alice"],
            vec!["cbrec", "--check", "alice"],
            vec!["cbrec", "--list", "alice"],
            vec!["cbrec", "discover", "--tag", "gaming", "--json"],
            vec!["cbrec", "discover", "--tag", "gaming"],
        ] {
            let discovery = arguments.contains(&"discover");
            let (base_url, request) = servidor_http_falso(
                200,
                if discovery {
                    r#"{"rooms":[]}"#
                } else {
                    r#"{"room_status":"offline"}"#
                },
            )
            .await;
            let mut client = ChaturbateClient::new().unwrap();
            client.base_url = base_url;
            let cli = Cli::try_parse_from(arguments).unwrap();
            ejecutar_cli(cli, AppConfig::default(), Vec::new(), client)
                .await
                .unwrap();
            let request = request.await.unwrap();
            assert!(request.starts_with(if discovery {
                "GET /api/ts/roomlist/room-list/"
            } else {
                "GET /api/chatvideocontext/alice/"
            }));
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn controller_recording_preserves_bytes_and_metadata_on_success_small_and_failure() {
        use crate::application::ejecutar_cli;
        use crate::infrastructure::AppConfig;
        use crate::presentation::Cli;
        use clap::Parser;
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "cbrec_record_flow_{}_{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut video = Vec::new();
        for (kind, payload) in [
            (b"ftyp", b"isom0000".as_slice()),
            (b"moov", b"".as_slice()),
            (b"mdat", b"x".as_slice()),
        ] {
            video.extend_from_slice(&((8 + payload.len()) as u32).to_be_bytes());
            video.extend_from_slice(kind);
            video.extend_from_slice(payload);
        }
        for (index, (batch, classification)) in [
            (false, "saved"),
            (true, "saved"),
            (false, "small"),
            (true, "small"),
            (false, "interrupted"),
            (true, "interrupted"),
            (false, "empty"),
            (false, "offline"),
            (true, "offline"),
            (false, "private"),
            (false, "rate_limited"),
            (false, "blocked"),
            (false, "indeterminate"),
        ]
        .into_iter()
        .enumerate()
        {
            let directory = root.join(index.to_string());
            std::fs::create_dir_all(&directory).unwrap();
            let executable = directory.join("ffmpeg");
            std::fs::write(executable.with_extension("fixture"), &video).unwrap();
            let script = format!("#!/bin/sh\nif [ \"$1\" = \"-version\" ]; then echo 'ffmpeg version synthetic'; exit 0; fi\nfor output do :; done\n{}\nexit {}\n", if classification == "empty" { "" } else { "cp \"${0}.fixture\" \"$output\"" }, u8::from(classification == "interrupted"));
            std::fs::write(&executable, script).unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            let playlist = "#EXTM3U\n#EXT-X-TARGETDURATION:5\n#EXTINF:5,\nsegment.ts\n";
            let media = http_test_server(
                vec![format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{playlist}",
                    playlist.len()
                )],
                false,
            )
            .await;
            let has_media = matches!(classification, "saved" | "small" | "interrupted" | "empty");
            let context = if has_media {
                format!(
                    r#"{{"room_status":"public","hls_source":"{}/master.m3u8"}}"#,
                    media.url
                )
            } else {
                match classification {
                    "offline" => r#"{"room_status":"offline"}"#,
                    "private" => r#"{"room_status":"private"}"#,
                    "blocked" => "<html>challenge</html>",
                    _ => "{}",
                }
                .to_string()
            };
            let status = if classification == "rate_limited" {
                429
            } else {
                200
            };
            let server = http_test_server(
                vec![format!(
                    "HTTP/1.1 {status} Status\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{context}",
                    context.len()
                )],
                false,
            )
            .await;
            let mut client = ChaturbateClient::new().unwrap();
            client.base_url = server.url.clone();
            let mut arguments = vec!["cbrec".to_string()];
            if !batch {
                arguments.push("record".into());
            }
            arguments.extend([
                "alice".into(),
                "--output".into(),
                directory.to_str().unwrap().into(),
                "--ffmpeg-path".into(),
                executable.to_str().unwrap().into(),
                "--jobs".into(),
                "1".into(),
            ]);
            if batch {
                arguments.extend(["bob".into(), "Alice".into()]);
            }
            if classification != "small" {
                arguments.extend(["--duration".into(), "1".into()]);
            }
            let config = AppConfig {
                naming_template: "clip.mp4".into(),
                min_file_size: 1024,
                min_free_space: 0,
                ..AppConfig::default()
            };
            let result = ejecutar_cli(
                Cli::try_parse_from(arguments).unwrap(),
                config,
                Vec::new(),
                client,
            )
            .await;
            assert_eq!(
                result.is_err(),
                !matches!(classification, "saved" | "small" | "offline"),
                "{classification}: {result:?}"
            );
            let expected_count = if batch { 2 } else { 1 };
            assert_eq!(server.request_count(), expected_count);
            assert_eq!(
                media.request_count(),
                if has_media { expected_count } else { 0 }
            );
            for model in if batch {
                vec!["alice", "bob"]
            } else {
                vec!["alice"]
            } {
                let folder = directory.join("cb_rec").join(model);
                let path = if classification == "small" {
                    folder.join("small/clip.mp4")
                } else {
                    folder.join("clip.mp4")
                };
                if classification == "empty" || !has_media {
                    assert!(!path.exists());
                    assert!(!path.with_extension("mp4.json").exists());
                } else {
                    assert_eq!(std::fs::read(&path).unwrap(), video);
                    let metadata: serde_json::Value = serde_json::from_slice(
                        &std::fs::read(path.with_extension("mp4.json")).unwrap(),
                    )
                    .unwrap();
                    assert_eq!(metadata["classification"], classification);
                    assert_eq!(metadata["model"], model);
                    assert_eq!(metadata["file_size_bytes"], video.len());
                    assert!(!folder.join("clip.part.mp4").exists());
                }
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn tui_worker_cancels_recording_without_losing_bytes_or_sidecar() {
        use crate::application::commands::tui::{run_worker, TuiParams};
        use crate::infrastructure::AppConfig;
        use crate::presentation::tui::{TuiAction, TuiEvent};
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!(
            "cbrec_tui_worker_{}_{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let ffmpeg = root.join("ffmpeg");
        let video = [
            16_u32.to_be_bytes().as_slice(),
            b"ftypisom0000",
            8_u32.to_be_bytes().as_slice(),
            b"moov",
            9_u32.to_be_bytes().as_slice(),
            b"mdatx",
        ]
        .concat();
        std::fs::write(root.join("ffmpeg.fixture"), &video).unwrap();
        std::fs::write(&ffmpeg, "#!/bin/sh\nif [ \"$1\" = \"-version\" ]; then echo 'ffmpeg version synthetic'; exit 0; fi\nfor output do :; done\ncp \"$0.fixture\" \"$output\"\necho ready > \"$0.started\"\nread quit\n").unwrap();
        std::fs::set_permissions(&ffmpeg, std::fs::Permissions::from_mode(0o700)).unwrap();
        let playlist = "#EXTM3U\n#EXT-X-TARGETDURATION:5\n#EXTINF:5,\nsegment.ts\n";
        let media = http_test_server(
            vec![format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{playlist}",
                playlist.len()
            )],
            false,
        )
        .await;
        let context = format!(
            r#"{{"room_status":"public","hls_source":"{}/master.m3u8"}}"#,
            media.url
        );
        let server = http_test_server(
            vec![format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{context}",
                context.len()
            )],
            false,
        )
        .await;
        let mut client = ChaturbateClient::new().unwrap();
        client.base_url = server.url.clone();
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let (actions, incoming) = tokio::sync::mpsc::channel(32);
        let (updates, mut events) = tokio::sync::mpsc::channel(64);
        let params = TuiParams {
            client,
            config: AppConfig {
                naming_template: "clip.mp4".into(),
                min_free_space: 0,
                ..AppConfig::default()
            },
            ffmpeg_path: ffmpeg,
            ffmpeg_explicit: true,
            output_root: Some(root.clone()),
            max_recordings: 1,
            min_file_size: None,
            initial_quality: VideoQuality::Best,
            cancel_rx,
        };
        let worker = tokio::spawn(run_worker(params, 5, incoming, updates));
        actions
            .send(TuiAction::Record("alice".into(), VideoQuality::Best))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while !root.join("ffmpeg.started").exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        while let Ok(event) = events.try_recv() {
            assert!(
                !matches!(event, TuiEvent::Finished(_, _)),
                "La espera del proceso no debe cerrar stdin antes de solicitar la parada"
            );
        }
        actions
            .send(TuiAction::Record("alice".into(), VideoQuality::Best))
            .await
            .unwrap();
        actions
            .send(TuiAction::Record("bob".into(), VideoQuality::Best))
            .await
            .unwrap();
        let mut refusals = 0;
        tokio::time::timeout(Duration::from_secs(2), async {
            while refusals < 2 {
                if let Some(TuiEvent::Notice(message)) = events.recv().await {
                    assert!(message.contains("límite"));
                    refusals += 1;
                }
            }
        })
        .await
        .unwrap();
        actions.send(TuiAction::Stop("alice".into())).await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Some(TuiEvent::Finished(model, message)) = events.recv().await {
                    assert_eq!(model, "alice");
                    assert!(message.contains("Guardado"), "{message}");
                    break;
                }
            }
        })
        .await
        .unwrap();
        let path = root.join("cb_rec/alice/clip.mp4");
        assert_eq!(std::fs::read(&path).unwrap(), video);
        let metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path.with_extension("mp4.json")).unwrap())
                .unwrap();
        assert_eq!(metadata["classification"], "saved");
        assert_eq!(server.request_count(), 1);
        assert_eq!(media.request_count(), 1);
        assert!(!root.join("cb_rec/alice/clip.part.mp4").exists());
        std::fs::remove_file(root.join("ffmpeg.started")).unwrap();
        actions
            .send(TuiAction::Record("alice".into(), VideoQuality::Best))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while !root.join("ffmpeg.started").exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        actions.send(TuiAction::Quit).await.unwrap();
        let messages = tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap();
        assert!(messages.iter().any(|message| message.contains("Guardado")));
        assert_eq!(
            std::fs::read(root.join("cb_rec/alice/clip_001.mp4")).unwrap(),
            video
        );
        assert!(root.join("cb_rec/alice/clip_001.mp4.json").exists());
        drop(cancel_tx);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn tui_worker_keeps_search_errors_and_session_transfer_inside_events() {
        use crate::application::commands::tui::{run_worker, TuiParams};
        use crate::infrastructure::AppConfig;
        use crate::presentation::tui::{TuiAction, TuiEvent};
        let body = r#"{"rooms":[{"username":"alice","tags":["gaming"],"num_users":42,"current_show":"public"}]}"#;
        let (base_url, request) = servidor_http_falso(200, body).await;
        let mut client = ChaturbateClient::new().unwrap();
        client.base_url = base_url;
        let (_cancel, cancel_rx) = watch::channel(false);
        let (actions, incoming) = tokio::sync::mpsc::channel(32);
        let (updates, mut events) = tokio::sync::mpsc::channel(64);
        let params = TuiParams {
            client,
            config: AppConfig::default(),
            ffmpeg_path: "missing-ffmpeg".into(),
            ffmpeg_explicit: true,
            output_root: None,
            max_recordings: 1,
            min_file_size: None,
            initial_quality: VideoQuality::Best,
            cancel_rx,
        };
        let worker = tokio::spawn(run_worker(params, 5, incoming, updates));
        actions
            .send(TuiAction::Search("bad tag".into(), 0, 1))
            .await
            .unwrap();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(2), events.recv())
                .await
                .unwrap(),
            Some(TuiEvent::Rooms(_, _, _, Err(_)))
        ));
        actions
            .send(TuiAction::Search("#Gaming".into(), 0, 2))
            .await
            .unwrap();
        match tokio::time::timeout(Duration::from_secs(2), events.recv())
            .await
            .unwrap()
            .unwrap()
        {
            TuiEvent::Rooms(tag, 0, 2, Ok(page)) => {
                let rooms = page.rooms;
                assert_eq!(tag, "#gaming");
                assert_eq!(rooms.len(), 1);
                assert_eq!(rooms[0].username, "alice");
            }
            _ => panic!("Se esperaban resultados"),
        }
        request.await.unwrap();
        actions
            .send(TuiAction::Preview("bad/name".into()))
            .await
            .unwrap();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(2), events.recv())
                .await
                .unwrap(),
            Some(TuiEvent::Preview(_, Err(_)))
        ));
        actions
            .send(TuiAction::Record("bad/name".into(), VideoQuality::Best))
            .await
            .unwrap();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(2), events.recv())
                .await
                .unwrap(),
            Some(TuiEvent::Notice(_))
        ));
        actions
            .send(TuiAction::Record("alice".into(), VideoQuality::Best))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let Some(TuiEvent::Finished(_, message)) = events.recv().await {
                    assert!(message.contains("Error"));
                    break;
                }
            }
        })
        .await
        .unwrap();
        actions.send(TuiAction::ConnectSession).await.unwrap();
        let url = match tokio::time::timeout(Duration::from_secs(2), events.recv())
            .await
            .unwrap()
            .unwrap()
        {
            TuiEvent::SessionUrl(url) => reqwest::Url::parse(&url).unwrap(),
            _ => panic!("Se esperaba receptor efímero"),
        };
        let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", url.port().unwrap()))
            .await
            .unwrap();
        let body = r#"{"session_cookie":"sessionid=synthetic-tui-session"}"#;
        stream
            .write_all(
                format!(
                    "POST {} HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
                    url.path(),
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        assert!(response.starts_with(b"HTTP/1.1 204"));
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(2), events.recv())
                .await
                .unwrap(),
            Some(TuiEvent::SessionFinished(true, _))
        ));
        actions.send(TuiAction::Quit).await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn new_search_cancels_a_stalled_catalog_and_session_cancel_closes_the_receiver() {
        use crate::application::commands::tui::{run_worker, TuiParams};
        use crate::infrastructure::AppConfig;
        use crate::presentation::tui::{TuiAction, TuiEvent};
        let mut server = http_test_server(vec![], true).await;
        let mut client = ChaturbateClient::new().unwrap();
        client.base_url = server.url.clone();
        let (_cancel, cancel_rx) = watch::channel(false);
        let (actions, incoming) = tokio::sync::mpsc::channel(16);
        let (updates, mut events) = tokio::sync::mpsc::channel(16);
        let worker = tokio::spawn(run_worker(
            TuiParams {
                client,
                config: AppConfig::default(),
                ffmpeg_path: "missing".into(),
                ffmpeg_explicit: true,
                output_root: None,
                max_recordings: 1,
                min_file_size: None,
                initial_quality: VideoQuality::Best,
                cancel_rx,
            },
            20,
            incoming,
            updates,
        ));
        actions
            .send(TuiAction::Search("#gaming".into(), 0, 1))
            .await
            .unwrap();
        server.wait_for_request().await;
        actions
            .send(TuiAction::Search("bad tag".into(), 0, 2))
            .await
            .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(result, TuiEvent::Rooms(_, 0, 2, Err(_))));
        assert_eq!(server.request_count(), 1);
        actions
            .send(TuiAction::Search("#music".into(), 0, 3))
            .await
            .unwrap();
        let event = tokio::time::timeout(Duration::from_secs(9), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(event, TuiEvent::Rooms(_, 0, 3, Err(error)) if error.contains("8 segundos"))
        );
        actions.send(TuiAction::ConnectSession).await.unwrap();
        let result = tokio::time::timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap();
        let url = match result {
            TuiEvent::SessionUrl(url) => reqwest::Url::parse(&url).unwrap(),
            _ => panic!("Falta receptor"),
        };
        actions.send(TuiAction::CancelSession).await.unwrap();
        assert!(matches!(
            tokio::time::timeout(Duration::from_secs(1), events.recv())
                .await
                .unwrap(),
            Some(TuiEvent::SessionFinished(false, _))
        ));
        actions.send(TuiAction::Quit).await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), worker)
            .await
            .unwrap()
            .unwrap();
        assert!(
            tokio::net::TcpStream::connect(("127.0.0.1", url.port().unwrap()))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn selecting_another_thumbnail_cancels_a_stalled_request() {
        use crate::application::commands::tui::{run_worker, TuiParams};
        use crate::infrastructure::AppConfig;
        use crate::presentation::tui::{TuiAction, TuiEvent};
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (first, received) = tokio::sync::oneshot::channel();
        let proxy = tokio::spawn(async move {
            let (mut stalled, _) = listener.accept().await.unwrap();
            let mut buffer = [0; 4096];
            let size = stalled.read(&mut buffer).await.unwrap();
            assert!(String::from_utf8_lossy(&buffer[..size])
                .starts_with("CONNECT thumb.live.mmcdn.com:443"));
            first.send(()).unwrap();
            let (mut next, _) = listener.accept().await.unwrap();
            let _ = next.read(&mut buffer).await.unwrap();
            next.write_all(
                b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )
            .await
            .unwrap();
        });
        let mut client = ChaturbateClient::new().unwrap();
        client.client = Client::builder()
            .proxy(reqwest::Proxy::all(format!("http://{address}")).unwrap())
            .build()
            .unwrap();
        let (_cancel, cancel_rx) = watch::channel(false);
        let (actions, incoming) = tokio::sync::mpsc::channel(16);
        let (updates, mut events) = tokio::sync::mpsc::channel(16);
        let params = TuiParams {
            client,
            config: AppConfig::default(),
            ffmpeg_path: "missing".into(),
            ffmpeg_explicit: true,
            output_root: None,
            max_recordings: 1,
            min_file_size: None,
            initial_quality: VideoQuality::Best,
            cancel_rx,
        };
        let worker = tokio::spawn(run_worker(params, 5, incoming, updates));
        actions
            .send(TuiAction::Preview("alice".into()))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), received)
            .await
            .unwrap()
            .unwrap();
        actions
            .send(TuiAction::Preview("bob".into()))
            .await
            .unwrap();
        let event = tokio::time::timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(event, TuiEvent::Preview(model, Err(_)) if model == "bob"));
        actions.send(TuiAction::Quit).await.unwrap();
        tokio::time::timeout(Duration::from_secs(2), worker)
            .await
            .unwrap()
            .unwrap();
        proxy.await.unwrap();
    }

    async fn servidor_http_falso(status: u16, body: &'static str) -> (String, JoinHandle<String>) {
        let listener = match TcpListener::bind("127.0.0.1:0").await {
            Ok(listener) => listener,
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
        (format!("http://{}", addr), task)
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
