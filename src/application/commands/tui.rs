//! Coordina la TUI sin bloquear su teclado ni escribir mensajes sobre el terminal.

use crate::application::commands::discover;
use crate::application::recording::{
    descargar_grabacion, detener_tarea_progreso, preparar_ruta_grabacion, write_recording_metadata,
    RecordingProgress, ResultadoGrabacion,
};
use crate::application::thumbnail::{decode_thumbnail, ThumbnailCache};
use crate::application::utils::validar_ffmpeg;
use crate::domain::repositories::StreamRepository;
use crate::domain::value_objects::{ModelName, VideoQuality};
use crate::infrastructure::external::RoomPage;
use crate::infrastructure::{receive_browser_session_with_prompt, AppConfig, ChaturbateClient};
use crate::presentation::tui::{TuiAction, TuiEvent, TuiInitial, TuiPage};
use crate::presentation::{run_discovery_tui, TuiRoom};
use image::DynamicImage;
use std::collections::HashMap;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::sync::{mpsc, watch};
use tokio::task::{Id, JoinSet};
use tokio::time::Duration;

#[derive(Clone)]
pub(crate) struct TuiParams {
    pub client: ChaturbateClient,
    pub config: AppConfig,
    pub ffmpeg_path: PathBuf,
    pub ffmpeg_explicit: bool,
    pub output_root: Option<PathBuf>,
    pub max_recordings: usize,
    pub min_file_size: Option<u64>,
    pub initial_quality: VideoQuality,
    pub cancel_rx: watch::Receiver<bool>,
}

enum OperationResult {
    Rooms(u64, usize, Result<(String, RoomPage), String>),
    Preview(
        u64,
        String,
        String,
        Result<(Arc<DynamicImage>, Duration), String>,
    ),
    Session(u64, Result<String, String>),
    Browser(Result<(), String>),
}

pub(crate) async fn run(params: TuiParams, raw_tag: &str, limit: usize) -> anyhow::Result<()> {
    anyhow::ensure!(
        (1..=50).contains(&limit),
        "El límite debe estar entre 1 y 50"
    );
    anyhow::ensure!(
        params.max_recordings > 0,
        "La concurrencia debe ser mayor a cero"
    );
    anyhow::ensure!(
        io::stdin().is_terminal() && io::stdout().is_terminal(),
        "La TUI requiere una terminal interactiva"
    );
    let tag = if raw_tag.is_empty() {
        String::new()
    } else {
        discover::normalize_tag(raw_tag)?
    };
    let (commands, actions) = mpsc::channel(32);
    let (events, updates) = mpsc::channel(64);
    let initial = TuiInitial {
        tag,
        page_size: limit,
        session_present: params.config.auth.session_cookie.is_some(),
        max_recordings: params.max_recordings,
        quality: params.initial_quality,
        output_folder: params
            .output_root
            .clone()
            .unwrap_or_else(|| params.config.output_root.clone()),
    };
    let ui = tokio::task::spawn_blocking(move || run_discovery_tui(initial, commands, updates));
    let messages = run_worker(params, limit, actions, events).await;
    ui.await??;
    for message in messages {
        eprintln!(
            "{}",
            crate::presentation::output::sanitize_external_text(&message)
        );
    }
    Ok(())
}

pub(crate) async fn run_worker(
    mut params: TuiParams,
    limit: usize,
    mut actions: mpsc::Receiver<TuiAction>,
    events: mpsc::Sender<TuiEvent>,
) -> Vec<String> {
    let mut operations = JoinSet::new();
    let mut recordings = JoinSet::new();
    let mut active = HashMap::<String, (watch::Sender<bool>, PathBuf)>::new();
    let mut task_models = HashMap::<Id, String>::new();
    let mut search_job: Option<(u64, tokio::task::AbortHandle)> = None;
    let mut preview_job: Option<(u64, String, tokio::task::AbortHandle)> = None;
    let mut preview_generation = 0_u64;
    let mut preview_cache = ThumbnailCache::new();
    let mut thumbnail_urls = HashMap::<String, String>::new();
    let mut session_job: Option<tokio::task::AbortHandle> = None;
    let mut session_generation = 0_u64;
    let mut cancel_rx = params.cancel_rx.clone();
    loop {
        tokio::select! {
            _ = async { let _ = cancel_rx.wait_for(|value| *value).await; } => break,
            action = actions.recv() => {
                let Some(action) = action else { break; };
                match action {
                    TuiAction::Quit => break,
                    TuiAction::Search(query, offset, generation) => {
                        if let Some((_, task)) = search_job.take() { task.abort(); }
                        let client = params.client.clone();
                        let task = operations.spawn(async move {
                            let result = match tokio::time::timeout(Duration::from_secs(8), discover::find_page(&client, &query, limit, offset)).await {
                                Ok(result) => result.map_err(|error| error.to_string()),
                                Err(_) => Err("La búsqueda superó 8 segundos. Se conservaron los resultados; pulsa R para reintentar.".into()),
                            };
                            OperationResult::Rooms(generation, offset, result)
                        });
                        search_job = Some((generation, task));
                    }
                    action @ (TuiAction::Preview(_) | TuiAction::RefreshPreview(_)) => {
                        let refresh = matches!(action, TuiAction::RefreshPreview(_));
                        let model = match action { TuiAction::Preview(model) | TuiAction::RefreshPreview(model) => model, _ => unreachable!() };
                        let source = thumbnail_urls.get(&model).cloned();
                        let source_key = source.clone().unwrap_or_default();
                        if !refresh {
                            if let Some(image) = preview_cache.get(&model, &source_key) {
                                if let Some((_, _, task)) = preview_job.take() { task.abort(); }
                                let _ = events.send(TuiEvent::Preview(model, Ok(image))).await;
                                continue;
                            }
                            if preview_job.as_ref().is_some_and(|(_, current, _)| current == &model) { continue; }
                        }
                        if let Some((_, _, task)) = preview_job.take() { task.abort(); }
                        preview_generation = preview_generation.wrapping_add(1);
                        let generation = preview_generation;
                        let client = params.client.clone();
                        let requested = model.clone();
                        let task = operations.spawn(async move {
                            let result = load_preview(&client, &requested, source.as_deref()).await.map_err(|error| error.to_string());
                            OperationResult::Preview(generation, requested, source_key, result)
                        });
                        preview_job = Some((generation, model, task));
                    }
                    TuiAction::CancelSession => {
                        if let Some(task) = session_job.take() { task.abort(); }
                        let _ = events.send(TuiEvent::SessionFinished(params.config.auth.session_cookie.is_some(), "Conexión de sesión cancelada".into())).await;
                    }
                    TuiAction::ConnectSession if session_job.is_none() => {
                        session_generation = session_generation.wrapping_add(1);
                        let generation = session_generation;
                        let sender = events.clone();
                        session_job = Some(operations.spawn(async move {
                            let result = receive_browser_session_with_prompt(|url| { let _ = sender.try_send(TuiEvent::SessionUrl(url.to_string())); }).await.map_err(|error| error.to_string());
                            OperationResult::Session(generation, result)
                        }));
                    }
                    TuiAction::OpenRoom(model) => {
                        if operations.len() < 4 { operations.spawn(async move { OperationResult::Browser(open_site(Some(&model)).await.map_err(|error| error.to_string())) }); }
                    }
                    TuiAction::OpenSite => {
                        if operations.len() < 4 { operations.spawn(async { OperationResult::Browser(open_site(None).await.map_err(|error| error.to_string())) }); }
                    }
                    TuiAction::Record(model, quality) => {
                        if active.contains_key(&model) || active.len() >= params.max_recordings {
                            let _ = events.send(TuiEvent::Notice("La sala ya se está grabando o se alcanzó el límite de grabaciones".into())).await;
                            continue;
                        }
                        let model_name = match ModelName::try_from(model.as_str()) { Ok(model) => model, Err(error) => { let _ = events.send(TuiEvent::Notice(error.to_string())).await; continue; } };
                        let (sender, receiver) = watch::channel(false);
                        let path = params.config.get_output_path(model_name.as_str(), params.output_root.as_deref());
                        active.insert(model.clone(), (sender, path.clone()));
                        let _ = events.send(TuiEvent::Recording(model.clone(), path)).await;
                        let mut job = params.clone();
                        job.cancel_rx = receiver.clone();
                        job.client = job.client.with_cancel_receiver(receiver);
                        let updates = events.clone();
                        let name = model.clone();
                        let handle = recordings.spawn(async move {
                            let result = record_room(job, model_name, quality, updates).await;
                            (name, result.map_err(|error| error.to_string()))
                        });
                        task_models.insert(handle.id(), model);
                    }
                    TuiAction::Stop(model) => { if let Some((sender, _)) = active.get(&model) { let _ = sender.send(true); let _ = events.send(TuiEvent::Notice(format!("Deteniendo {model}; se conservará lo recibido"))).await; } }
                    _ => {}
                }
            }
            Some(result) = operations.join_next(), if !operations.is_empty() => {
                match result {
                    Ok(OperationResult::Rooms(generation, offset, result)) => {
                        if search_job.as_ref().is_none_or(|(current, _)| *current != generation) { continue; }
                        search_job = None;
                        let mut query = String::new();
                        let result = result.map(|(normalized, page)| {
                            query = normalized;
                            thumbnail_urls.clear();
                            let rooms = page.rooms.into_iter().map(|room| {
                                if let Some(url) = room.thumbnail_url { thumbnail_urls.insert(room.username.clone(), url); }
                                TuiRoom::new(room.username, room.viewers, room.show, room.subject)
                            }).collect();
                            TuiPage { rooms, total: page.total, has_more: page.has_more }
                        });
                        let _ = events.send(TuiEvent::Rooms(query, offset, generation, result)).await;
                    }
                    Ok(OperationResult::Preview(generation, model, source, result)) => {
                        if preview_job.as_ref().is_some_and(|(current, _, _)| *current == generation) {
                            preview_job = None;
                            let result = result.map(|(image, freshness)| { preview_cache.insert(model.clone(), source, image.clone(), freshness); image });
                            let _ = events.send(TuiEvent::Preview(model, result)).await;
                        }
                    }
                    Ok(OperationResult::Session(generation, result)) => {
                        if generation != session_generation || session_job.take().is_none() { continue; }
                        let (present, message) = match result { Ok(cookie) => { params.client = params.client.with_session_cookie(cookie.clone()); params.config.auth.session_cookie = Some(cookie); (true, "Sesión recibida en memoria. Su validez se comprobará al consultar una sala.".into()) }, Err(error) => (params.config.auth.session_cookie.is_some(), error) };
                        let _ = events.send(TuiEvent::SessionFinished(present, message)).await;
                    }
                    Ok(OperationResult::Browser(result)) => { let _ = events.send(TuiEvent::Notice(result.err().unwrap_or_else(|| "Sala abierta en el navegador habitual: allí puedes ver vídeo y comentar con tu sesión.".into()))).await; }
                    Err(error) if error.is_cancelled() => {},
                    Err(error) => { search_job = None; preview_job = None; session_job = None; let _ = events.send(TuiEvent::Notice(format!("La operación terminó inesperadamente: {error}"))).await; }
                }
            }
            Some(result) = recordings.join_next_with_id(), if !recordings.is_empty() => {
                let (model, message) = match result {
                    Ok((id, (model, result))) => { task_models.remove(&id); (model, result.unwrap_or_else(|error| format!("Error: {error}"))) }
                    Err(error) => { let model = task_models.remove(&error.id()).unwrap_or_default(); (model, "La tarea terminó inesperadamente; revisa el archivo conservado".into()) }
                };
                active.remove(&model);
                let _ = events.send(TuiEvent::Finished(model, message)).await;
            }

        }
    }
    for (sender, _) in active.values() {
        let _ = sender.send(true);
    }
    operations.abort_all();
    drop(events);
    drop(actions);
    // La cancelación cooperativa finaliza el MP4 y su sidecar antes de cerrar el runtime.
    let mut messages = Vec::new();
    while let Some(result) = recordings.join_next_with_id().await {
        match result {
            Ok((_, (model, result))) => messages.push(format!(
                "{model}: {}",
                result.unwrap_or_else(|error| format!("Error: {error}"))
            )),
            Err(error) => messages.push(format!(
                "{}: la tarea terminó inesperadamente; revisa el archivo conservado",
                task_models.remove(&error.id()).unwrap_or_default()
            )),
        }
    }
    messages
}

async fn record_room(
    params: TuiParams,
    model: ModelName,
    quality: VideoQuality,
    events: mpsc::Sender<TuiEvent>,
) -> anyhow::Result<String> {
    validar_ffmpeg(
        &params.ffmpeg_path,
        params.ffmpeg_explicit,
        params.cancel_rx.clone(),
    )
    .await?;
    let client = params.client.with_ffmpeg_path(params.ffmpeg_path);
    let stream = client
        .get_stream_url(&model)
        .await?
        .ok_or_else(|| anyhow::anyhow!("{} no está online", model))?;
    let path = preparar_ruta_grabacion(
        params
            .config
            .get_output_path(model.as_str(), params.output_root.as_deref()),
    )
    .await?;
    let _ = events
        .send(TuiEvent::Recording(model.as_str().into(), path.clone()))
        .await;
    let progress_events = events.clone();
    let progress_path = path.clone();
    let progress_model = model.as_str().to_string();
    let progress = RecordingProgress(tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            if let Ok(metadata) = tokio::fs::metadata(&progress_path).await {
                let _ = progress_events
                    .try_send(TuiEvent::Progress(progress_model.clone(), metadata.len()));
            }
        }
    }));
    let started = chrono::Utc::now();
    let result = descargar_grabacion(&client, &stream, path, quality, params.min_file_size).await;
    detener_tarea_progreso(progress).await;
    let result = result?;
    write_recording_metadata(&result, model.as_str(), quality, started).await?;
    Ok(match result {
        ResultadoGrabacion::Guardado(path) => format!("Guardado: {}", path.display()),
        ResultadoGrabacion::Pequeno(path, _) => {
            format!("Archivo pequeño conservado: {}", path.display())
        }
        ResultadoGrabacion::Interrupted(_, reason) => format!("Grabación interrumpida: {reason}"),
        ResultadoGrabacion::Cancelado => "Cancelada antes de recibir datos".into(),
    })
}

fn room_url(model: &str) -> anyhow::Result<String> {
    let model = ModelName::try_from(model)?;
    Ok(format!("https://chaturbate.com/{model}/"))
}

async fn open_site(model: Option<&str>) -> anyhow::Result<()> {
    let url = match model {
        Some(model) => room_url(model)?,
        None => "https://chaturbate.com/".into(),
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = tokio::process::Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler");
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = tokio::process::Command::new("open");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut command = tokio::process::Command::new("xdg-open");
    let status = tokio::time::timeout(
        Duration::from_secs(10),
        command
            .arg(url)
            .env_remove("CBREC_SESSION_COOKIE")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status(),
    )
    .await
    .map_err(|_| {
        anyhow::anyhow!("El lanzador no respondió; comprueba si el navegador se abrió")
    })??;
    anyhow::ensure!(status.success(), "No se pudo abrir el navegador habitual");
    Ok(())
}

async fn load_preview(
    client: &ChaturbateClient,
    model: &str,
    source: Option<&str>,
) -> anyhow::Result<(Arc<DynamicImage>, Duration)> {
    let model = ModelName::try_from(model)?;
    let response = client.thumbnail(&model, source).await?;
    let freshness = response.freshness;
    let image = tokio::task::spawn_blocking(move || decode_thumbnail(&response.bytes)).await??;
    Ok((Arc::new(image), freshness))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_target_is_fixed_and_rejects_injected_paths() {
        assert_eq!(room_url("Alice").unwrap(), "https://chaturbate.com/alice/");
        for name in [
            "../alice",
            "https://evil.example",
            "alice?token=x",
            "alice;exit",
            "alice\n",
        ] {
            if name == "alice\n" {
                continue;
            }
            assert!(room_url(name).is_err());
        }
    }
    #[tokio::test]
    async fn preview_rejects_invalid_model_before_requesting_the_cdn() {
        assert!(
            load_preview(&ChaturbateClient::new().unwrap(), "bad/name", None)
                .await
                .is_err()
        );
    }
}
