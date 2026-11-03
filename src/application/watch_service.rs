use crate::application::recording::{
    descargar_grabacion, detener_tarea_progreso, preparar_ruta_grabacion, write_recording_metadata,
    RecordingProgress, ResultadoGrabacion,
};
use crate::domain::errors::DomainError;
use crate::domain::repositories::StreamRepository;
use crate::domain::value_objects::{EstadoModelo, ModelName, StreamUrl, VideoQuality};
use crate::infrastructure::{AppConfig, ChaturbateClient, InfrastructureError, WatchConfig};
use crate::presentation::Output;
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch, Mutex, Semaphore};
use tokio::task::{Id, JoinSet};

const MAX_CONCURRENT_STATUS_CHECKS: usize = 5;
type RecordingCompletion = (String, Option<PathBuf>, bool);

pub(crate) struct WatchParams<R = ChaturbateClient> {
    pub client: Arc<R>,
    pub config: Arc<AppConfig>,
    pub modelos: Vec<ModelName>,
    pub ask: bool,
    pub raiz_salida: Option<PathBuf>,
    pub quality: VideoQuality,
    pub limite_concurrencia: usize,
    pub min_file_size: Option<u64>,
    pub cancel_rx: watch::Receiver<bool>,
    pub salida: Arc<dyn Output>,
    pub prompter: Arc<dyn WatchPrompter>,
}

#[async_trait]
#[allow(
    clippy::double_must_use,
    reason = "async_trait generates the redundant must_use attribute"
)]
pub(crate) trait WatchPrompter: Send + Sync {
    async fn confirmar_grabacion(&self, modelo: &str, cfg: &WatchConfig) -> bool;
}

#[derive(Default)]
pub(crate) struct ConsoleWatchPrompter {
    input: Mutex<Option<mpsc::Receiver<String>>>,
}

#[async_trait]
impl WatchPrompter for ConsoleWatchPrompter {
    async fn confirmar_grabacion(&self, modelo: &str, cfg: &WatchConfig) -> bool {
        let mut input = self.input.lock().await;
        let input = input.get_or_insert_with(|| {
            let (sender, receiver) = mpsc::channel(1);
            // A dedicated thread avoids Tokio stdin blocking runtime shutdown.
            std::thread::spawn(move || loop {
                let mut line = String::new();
                match std::io::stdin().read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if sender.blocking_send(line).is_err() {
                            break;
                        }
                    }
                }
            });
            receiver
        });
        preguntar_con_timeout(modelo, cfg, input).await
    }
}

pub(crate) async fn ejecutar_watch(params: WatchParams) -> anyhow::Result<()> {
    ejecutar_watch_con_repo(params).await
}

fn handle_recording_completion(
    result: Result<(Id, RecordingCompletion), tokio::task::JoinError>,
    task_models: &mut HashMap<Id, String>,
    states: &mut HashMap<String, EstadoModelo>,
    blocked_until: &mut HashMap<String, Instant>,
    failure_cooldown: Duration,
    output: &dyn Output,
) {
    let (model, final_path, failed) = match result {
        Ok((id, completion)) => {
            task_models.remove(&id);
            completion
        }
        Err(error) => {
            output.error_tarea_abortada(&error.to_string());
            let Some(model) = task_models.remove(&error.id()) else {
                return;
            };
            (model, None, true)
        }
    };

    if let Some(path) = final_path {
        output.watch_fin_grabacion(&model, &path);
    }
    if failed {
        blocked_until.insert(model.clone(), instante_tras(failure_cooldown));
    }
    states.insert(model, EstadoModelo::Offline);
}

async fn ejecutar_watch_con_repo<R>(params: WatchParams<R>) -> anyhow::Result<()>
where
    R: StreamRepository<Error = InfrastructureError> + 'static,
{
    let WatchParams {
        client,
        config,
        modelos,
        ask,
        raiz_salida,
        quality,
        limite_concurrencia,
        min_file_size,
        cancel_rx,
        salida,
        prompter,
    } = params;

    let nombres: Vec<&str> = modelos.iter().map(|m| m.as_str()).collect();
    salida.watch_inicio(&nombres);

    let mut estados: HashMap<String, EstadoModelo> = modelos
        .iter()
        .map(|m| (m.as_str().to_string(), EstadoModelo::Offline))
        .collect();

    let mut omitidos: HashSet<String> = HashSet::new();
    let mut invalidos: HashSet<String> = HashSet::new();
    let mut bloqueados_hasta: HashMap<String, Instant> = HashMap::new();
    let mut waiting: HashSet<String> = HashSet::new();
    let mut grabaciones: JoinSet<(String, Option<PathBuf>, bool)> = JoinSet::new();
    let mut task_models = HashMap::new();

    let mut ultima_actividad = Instant::now()
        .checked_sub(Duration::from_secs(
            config.watch.idle_threshold_mins * 60 + 1,
        ))
        .unwrap_or_else(Instant::now);

    'watch: loop {
        while let Some(result) = grabaciones.try_join_next_with_id() {
            handle_recording_completion(
                result,
                &mut task_models,
                &mut estados,
                &mut bloqueados_hasta,
                Duration::from_secs(config.watch.cooldown_tras_fallo_secs),
                salida.as_ref(),
            );
        }

        if *cancel_rx.borrow() {
            break;
        }

        let mut slots_disponibles = calcular_slots_disponibles(&estados, limite_concurrencia);
        let cooldown = Duration::from_secs(config.watch.cooldown_tras_fallo_secs);
        let prioritize_waiting = slots_disponibles > 0
            && waiting.iter().any(|name| {
                debe_consultar_modelo(name, &estados, &omitidos, &invalidos, &bloqueados_hasta)
            });

        // Checks de estado en paralelo acotados por semaforo
        let semaforo = Arc::new(Semaphore::new(MAX_CONCURRENT_STATUS_CHECKS));
        let mut checks: JoinSet<(String, Result<Option<StreamUrl>, InfrastructureError>)> =
            JoinSet::new();

        for modelo in &modelos {
            let nombre = modelo.as_str().to_string();
            // Se guardan candidatos, nunca URLs que podrían caducar mientras esperan.
            if prioritize_waiting && !waiting.contains(&nombre) {
                continue;
            }
            if !debe_consultar_modelo(&nombre, &estados, &omitidos, &invalidos, &bloqueados_hasta) {
                continue;
            }
            let client_c = Arc::clone(&client);
            let sem = Arc::clone(&semaforo);
            let m = modelo.clone();
            checks.spawn(async move {
                let _permit = sem.acquire().await.ok();
                (m.as_str().to_string(), client_c.get_stream_url(&m).await)
            });
        }

        loop {
            let completed = tokio::select! {
                result = checks.join_next() => result,
                completed = grabaciones.join_next_with_id(), if !grabaciones.is_empty() => {
                    if let Some(result) = completed {
                        handle_recording_completion(
                            result,
                            &mut task_models,
                            &mut estados,
                            &mut bloqueados_hasta,
                            cooldown,
                            salida.as_ref(),
                        );
                        slots_disponibles = calcular_slots_disponibles(&estados, limite_concurrencia);
                    }
                    continue;
                }
                _ = esperar_cancelacion(cancel_rx.clone()) => break 'watch,
            };
            let Some(completed) = completed else { break };
            let (nombre, resultado) = match completed {
                Ok(result) => result,
                Err(error) => {
                    salida.error_tarea_abortada(&error.to_string());
                    continue;
                }
            };
            waiting.remove(&nombre);
            let stream_url = match resultado {
                Ok(Some(url)) => {
                    ultima_actividad = Instant::now();
                    salida.watch_tick_online(&nombre);
                    url
                }
                Ok(None) => {
                    salida.watch_tick_offline(&nombre);
                    continue;
                }
                Err(InfrastructureError::Domain(DomainError::ModelNotFound(_))) => {
                    salida.error_fallo_grabacion(
                        &nombre,
                        "modelo no encontrado, eliminado de monitoreo",
                    );
                    invalidos.insert(nombre);
                    continue;
                }
                Err(e) => {
                    salida.advertir_error_consulta_estado(&nombre, &e.to_string());
                    bloqueados_hasta.insert(
                        nombre,
                        instante_tras(cooldown_para_error_consulta(&e, cooldown)),
                    );
                    continue;
                }
            };

            // Cada modelo puede iniciar sin esperar las demás consultas.
            if *cancel_rx.borrow() {
                break 'watch;
            }
            if slots_disponibles == 0 {
                waiting.insert(nombre);
                continue;
            }

            if ask {
                let accepted = tokio::select! {
                    answer = prompter.confirmar_grabacion(&nombre, &config.watch) => answer,
                    _ = esperar_cancelacion(cancel_rx.clone()) => break 'watch,
                };
                if !accepted {
                    salida.watch_modelo_omitido(&nombre);
                    omitidos.insert(nombre);
                    continue;
                }
            }

            salida.watch_inicio_grabacion(&nombre);
            estados.insert(nombre.clone(), EstadoModelo::Grabando);
            slots_disponibles = slots_disponibles.saturating_sub(1);

            let client_clone = Arc::clone(&client);
            let config_clone = Arc::clone(&config);
            let salida_clone = Arc::clone(&salida);
            let raiz_clone = raiz_salida.clone();
            let cancel_clone = cancel_rx.clone();
            let nombre_clone = nombre.clone();

            let task = grabaciones.spawn(async move {
                if *cancel_clone.borrow() {
                    return (nombre_clone, None, false);
                }

                let ruta_base =
                    config_clone.get_output_path(nombre_clone.as_str(), raiz_clone.as_deref());
                let ruta = match preparar_ruta_grabacion(ruta_base).await {
                    Ok(ruta) => ruta,
                    Err(e) => {
                        salida_clone.error_fallo_grabacion(&nombre_clone, &e.to_string());
                        return (nombre_clone, None, true);
                    }
                };

                let salida_p = Arc::clone(&salida_clone);
                let nombre_p = nombre_clone.clone();
                let output_path = ruta.clone();
                let progress_task = RecordingProgress(tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        if let Ok(meta) = tokio::fs::metadata(&output_path).await {
                            salida_p.mostrar_progreso_grabacion(&nombre_p, meta.len());
                        }
                    }
                }));

                let started_at = chrono::Utc::now();
                let result = descargar_grabacion(
                    client_clone.as_ref(),
                    &stream_url,
                    ruta,
                    quality,
                    min_file_size,
                )
                .await;
                detener_tarea_progreso(progress_task).await;

                if let Ok(recording) = &result {
                    if let Err(e) =
                        write_recording_metadata(recording, &nombre_clone, quality, started_at)
                            .await
                    {
                        salida_clone.error_fallo_grabacion(&nombre_clone, &e.to_string());
                        return (nombre_clone, None, true);
                    }
                }

                match result {
                    Ok(ResultadoGrabacion::Guardado(p)) => (nombre_clone, Some(p), false),
                    Ok(ResultadoGrabacion::Pequeno(p, _)) => (nombre_clone, Some(p), false),
                    Ok(ResultadoGrabacion::Interrupted(_, error)) => {
                        salida_clone.error_fallo_grabacion(&nombre_clone, &error);
                        (nombre_clone, None, true)
                    }
                    Ok(ResultadoGrabacion::Cancelado) => (nombre_clone, None, false),
                    Err(e) => {
                        salida_clone.error_fallo_grabacion(&nombre_clone, &e.to_string());
                        (nombre_clone, None, true)
                    }
                }
            });
            task_models.insert(task.id(), nombre);
        }

        if prioritize_waiting && slots_disponibles > 0 {
            continue 'watch;
        }

        let tiempo_idle = ultima_actividad.elapsed();
        let umbral_idle = Duration::from_secs(config.watch.idle_threshold_mins * 60);
        let intervalo_secs = if tiempo_idle >= umbral_idle {
            config.watch.poll_interval_idle_secs
        } else {
            config.watch.poll_interval_secs
        };

        salida.watch_proximo_check(intervalo_secs);

        let poll_delay = tokio::time::sleep(Duration::from_secs(intervalo_secs));
        tokio::pin!(poll_delay);
        loop {
            tokio::select! {
                _ = &mut poll_delay => break,
                completed = grabaciones.join_next_with_id(), if !grabaciones.is_empty() => {
                    if let Some(result) = completed {
                        handle_recording_completion(
                            result,
                            &mut task_models,
                            &mut estados,
                            &mut bloqueados_hasta,
                            Duration::from_secs(config.watch.cooldown_tras_fallo_secs),
                            salida.as_ref(),
                        );
                        if waiting.iter().any(|name| debe_consultar_modelo(name, &estados, &omitidos, &invalidos, &bloqueados_hasta)) {
                            break;
                        }
                    }
                }
                _ = esperar_cancelacion(cancel_rx.clone()) => {
                    break 'watch;
                }
            }
        }
    }

    salida.watch_deteniendo();
    finalizar_grabaciones(&mut grabaciones, salida.as_ref()).await;
    Ok(())
}

async fn preguntar_con_timeout(
    modelo: &str,
    cfg: &WatchConfig,
    input: &mut mpsc::Receiver<String>,
) -> bool {
    if cfg.desktop_notify {
        let cuerpo = cfg.notif_cuerpo.replace("{modelo}", modelo);
        let _ = tokio::process::Command::new("notify-send")
            .args(["--urgency=low", &cfg.notif_titulo, &cuerpo])
            .spawn();
    }

    print!(
        "[{}] ¿Grabar? [S/n] (auto en {}s): ",
        modelo, cfg.ask_timeout_secs
    );
    let _ = std::io::Write::flush(&mut std::io::stdout());

    match tokio::time::timeout(Duration::from_secs(cfg.ask_timeout_secs), input.recv()).await {
        Err(_) => true,
        Ok(Some(line)) => !line.trim().eq_ignore_ascii_case("n"),
        Ok(None) => false,
    }
}

async fn esperar_cancelacion(mut rx: watch::Receiver<bool>) {
    let _ = rx.wait_for(|v| *v).await;
}

async fn finalizar_grabaciones(
    grabaciones: &mut JoinSet<(String, Option<PathBuf>, bool)>,
    salida: &dyn Output,
) {
    // The shared cancellation receiver stops FFmpeg; let each task finalize its file.
    while let Some(result) = grabaciones.join_next().await {
        match result {
            Ok((model, Some(path), _)) => salida.watch_fin_grabacion(&model, &path),
            Err(error) => salida.error_tarea_abortada(&error.to_string()),
            _ => {}
        }
    }
}

fn calcular_slots_disponibles(
    estados: &HashMap<String, EstadoModelo>,
    limite_concurrencia: usize,
) -> usize {
    let grabando_ahora = estados
        .values()
        .filter(|e| **e == EstadoModelo::Grabando)
        .count();
    limite_concurrencia.saturating_sub(grabando_ahora)
}

fn debe_consultar_modelo(
    nombre: &str,
    estados: &HashMap<String, EstadoModelo>,
    omitidos: &HashSet<String>,
    invalidos: &HashSet<String>,
    bloqueados_hasta: &HashMap<String, Instant>,
) -> bool {
    if estados.get(nombre) == Some(&EstadoModelo::Grabando) {
        return false;
    }
    if omitidos.contains(nombre) || invalidos.contains(nombre) {
        return false;
    }
    if bloqueados_hasta
        .get(nombre)
        .is_some_and(|hasta| Instant::now() < *hasta)
    {
        return false;
    }
    true
}

fn cooldown_para_error_consulta(error: &InfrastructureError, base: Duration) -> Duration {
    match error {
        InfrastructureError::HttpStatus(429) => base.saturating_mul(4).max(Duration::from_secs(60)),
        InfrastructureError::HttpStatus(status) if *status >= 500 => {
            base.saturating_mul(2).max(Duration::from_secs(30))
        }
        InfrastructureError::ExternalService(mensaje)
            if mensaje.starts_with("HTTP request failed") =>
        {
            base.saturating_mul(2).max(Duration::from_secs(30))
        }
        _ => base,
    }
}

fn instante_tras(duration: Duration) -> Instant {
    Instant::now()
        .checked_add(duration)
        .unwrap_or_else(Instant::now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::repositories::StreamRepository;
    use crate::presentation::Output;
    use async_trait::async_trait;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn recording_panic_releases_slot_and_applies_cooldown() {
        let mut recordings: JoinSet<(String, Option<PathBuf>, bool)> = JoinSet::new();
        let task = recordings.spawn(async { panic!("recording failed") });
        let mut task_models = HashMap::from([(task.id(), "alice".to_string())]);
        let result = recordings.join_next_with_id().await.unwrap();
        assert!(result.as_ref().unwrap_err().is_panic());
        let mut states = estados(&[
            ("alice", EstadoModelo::Grabando),
            ("bob", EstadoModelo::Grabando),
        ]);
        let mut blocked_until = HashMap::new();

        handle_recording_completion(
            result,
            &mut task_models,
            &mut states,
            &mut blocked_until,
            Duration::from_secs(30),
            &crate::presentation::ConsoleOutput::default(),
        );

        assert_eq!(states.get("alice"), Some(&EstadoModelo::Offline));
        assert_eq!(states.get("bob"), Some(&EstadoModelo::Grabando));
        assert_eq!(calcular_slots_disponibles(&states, 2), 1);
        assert!(blocked_until["alice"] > Instant::now());
        assert!(!blocked_until.contains_key("bob"));
        assert!(task_models.is_empty());
    }

    #[tokio::test]
    async fn shutdown_waits_for_recording_finalization() {
        let finalized = Arc::new(AtomicUsize::new(0));
        let marker = finalized.clone();
        let mut recordings = JoinSet::new();
        recordings.spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            marker.store(1, Ordering::SeqCst);
            ("alice".into(), None, false)
        });
        finalizar_grabaciones(
            &mut recordings,
            &crate::presentation::ConsoleOutput::default(),
        )
        .await;
        assert_eq!(finalized.load(Ordering::SeqCst), 1);
    }

    enum RespuestaRepo {
        Online,
        NoEncontrado,
    }

    struct RepoFake {
        respuesta: RespuestaRepo,
        consultas: AtomicUsize,
        cancel_on_download: Option<watch::Sender<bool>>,
        fail_download: bool,
        panic_download: bool,
    }

    impl RepoFake {
        fn online() -> Self {
            Self {
                respuesta: RespuestaRepo::Online,
                consultas: AtomicUsize::new(0),
                cancel_on_download: None,
                fail_download: false,
                panic_download: false,
            }
        }

        fn no_encontrado() -> Self {
            Self {
                respuesta: RespuestaRepo::NoEncontrado,
                consultas: AtomicUsize::new(0),
                cancel_on_download: None,
                fail_download: false,
                panic_download: false,
            }
        }

        fn consultas(&self) -> usize {
            self.consultas.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl StreamRepository for RepoFake {
        type Error = InfrastructureError;

        async fn get_stream_url(
            &self,
            model_name: &ModelName,
        ) -> Result<Option<StreamUrl>, Self::Error> {
            self.consultas.fetch_add(1, Ordering::SeqCst);
            match self.respuesta {
                RespuestaRepo::Online => Ok(Some(
                    StreamUrl::try_from("https://example.com/stream.m3u8").unwrap(),
                )),
                RespuestaRepo::NoEncontrado => Err(InfrastructureError::Domain(
                    DomainError::ModelNotFound(model_name.as_str().to_string()),
                )),
            }
        }

        async fn download_stream(
            &self,
            _stream_url: &StreamUrl,
            output_path: &Path,
            _quality: VideoQuality,
        ) -> Result<(), Self::Error> {
            if let Some(cancel) = &self.cancel_on_download {
                cancel.send(true).unwrap();
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            tokio::fs::write(output_path, mp4_minimo_valido()).await?;
            if self.panic_download {
                panic!("fallo de grabación simulado");
            }
            if self.fail_download {
                return Err(InfrastructureError::RecordingError(
                    "corte de red simulado".to_string(),
                ));
            }
            Ok(())
        }
    }

    struct OutputFake {
        eventos: Mutex<Vec<String>>,
        cancel_tx: watch::Sender<bool>,
        cancel_on_finish: bool,
        event_changed: tokio::sync::Notify,
    }

    impl OutputFake {
        fn new(cancel_tx: watch::Sender<bool>) -> Self {
            Self {
                eventos: Mutex::new(Vec::new()),
                cancel_tx,
                cancel_on_finish: true,
                event_changed: tokio::sync::Notify::new(),
            }
        }

        fn eventos(&self) -> Vec<String> {
            self.eventos.lock().unwrap().clone()
        }

        fn evento(&self, evento: impl Into<String>) {
            self.eventos.lock().unwrap().push(evento.into());
            self.event_changed.notify_waiters();
        }
    }

    async fn wait_for_event(output: &OutputFake, event: &str) {
        loop {
            let changed = output.event_changed.notified();
            if output.eventos().iter().any(|recorded| recorded == event) {
                return;
            }
            changed.await;
        }
    }

    impl Output for OutputFake {
        fn advertir_limite_concurrencia(&self, _recomendado: usize, _solicitado: usize) {}
        fn mostrar_error_sin_modelo(&self) {}
        fn advertir_modelos_duplicados(&self, _duplicados: usize) {}
        fn advertir_modelos_sobre_limite(&self, _total: usize, _limite: usize) {}
        fn advertir_no_se_pudo_guardar_lista(&self, _error: &str) {}
        fn advertir_error_consulta_estado(&self, modelo: &str, _error: &str) {
            self.evento(format!("warn:{modelo}"));
        }
        fn advertir_config(&self, warning: &str) {
            self.evento(format!("config:{warning}"));
        }
        fn modelo_agregado(&self, modelo: &str) {
            self.evento(format!("agregado:{modelo}"));
        }
        fn modelo_ya_en_lista(&self, modelo: &str) {
            self.evento(format!("ya_en_lista:{modelo}"));
        }
        fn modelo_eliminado(&self, modelo: &str) {
            self.evento(format!("eliminado:{modelo}"));
        }
        fn modelo_no_encontrado_en_lista(&self, modelo: &str) {
            self.evento(format!("no_encontrado:{modelo}"));
        }
        fn error_fallo_grabacion(&self, modelo: &str, _error: &str) {
            self.evento(format!("error:{modelo}"));
            let _ = self.cancel_tx.send(true);
        }
        fn error_tarea_abortada(&self, _error: &str) {
            self.evento("tarea_abortada");
        }
        fn mostrar_progreso_grabacion(&self, model: &str, _bytes: u64) {
            self.evento(format!("progreso:{model}"));
        }
        fn mostrar_inicio_detallado(&self, _modelo: &str, _calidad: &str) {}
        fn mostrar_inicio_resumido(&self, _modelo: &str, _calidad: &str) {}
        fn mostrar_verificando_disponibilidad(&self) {}
        fn mostrar_modelo_offline_detallado(&self, _modelo: &str) {}
        fn mostrar_modelo_offline_resumido(&self, _modelo: &str) {}
        fn mostrar_modelo_online_detallado(&self) {}
        fn mostrar_detalle_inicio_grabacion(&self, _ruta: &Path) {}
        fn mostrar_cancelacion_detallada(&self) {}
        fn mostrar_cancelacion_resumida(&self, _modelo: &str) {}
        fn mostrar_archivo_pequeno_detallado(&self, _bytes: u64, _destino: &Path) {}
        fn mostrar_archivo_pequeno_resumido(&self, _modelo: &str, _destino: &Path) {}
        fn mostrar_archivo_guardado_detallado(&self, _ruta: &Path) {}
        fn mostrar_archivo_guardado_resumido(&self, _modelo: &str, _ruta: &Path) {}
        fn mostrar_inicio_verificacion(&self, _modelo: &str) {}
        fn mostrar_estado_modelo(&self, _modelo: &str, _online: bool) {}
        fn mostrar_modelo_sin_variantes(&self, _modelo: &str) {}
        fn mostrar_calidades(&self, _modelo: &str, _calidades: &[(Option<u32>, Option<u64>)]) {}
        fn watch_inicio(&self, _modelos: &[&str]) {
            self.evento("inicio");
        }
        fn watch_tick_online(&self, modelo: &str) {
            self.evento(format!("online:{modelo}"));
        }
        fn watch_tick_offline(&self, modelo: &str) {
            self.evento(format!("offline:{modelo}"));
        }
        fn watch_inicio_grabacion(&self, modelo: &str) {
            self.evento(format!("grabando:{modelo}"));
        }
        fn watch_fin_grabacion(&self, modelo: &str, _ruta: &Path) {
            self.evento(format!("fin:{modelo}"));
            if self.cancel_on_finish {
                let _ = self.cancel_tx.send(true);
            }
        }
        fn watch_modelo_omitido(&self, modelo: &str) {
            self.evento(format!("omitido:{modelo}"));
            let _ = self.cancel_tx.send(true);
        }
        fn watch_proximo_check(&self, _secs: u64) {}
        fn watch_deteniendo(&self) {
            self.evento("deteniendo");
        }
    }

    struct PrompterFake {
        respuesta: bool,
        llamadas: AtomicUsize,
    }

    impl PrompterFake {
        fn new(respuesta: bool) -> Self {
            Self {
                respuesta,
                llamadas: AtomicUsize::new(0),
            }
        }

        fn llamadas(&self) -> usize {
            self.llamadas.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl WatchPrompter for PrompterFake {
        async fn confirmar_grabacion(&self, _modelo: &str, _cfg: &WatchConfig) -> bool {
            self.llamadas.fetch_add(1, Ordering::SeqCst);
            self.respuesta
        }
    }

    fn config_test() -> AppConfig {
        AppConfig {
            output_root: ruta_temporal("salida"),
            min_file_size: 1,
            watch: WatchConfig {
                poll_interval_secs: 0,
                poll_interval_idle_secs: 0,
                idle_threshold_mins: 30,
                cooldown_tras_fallo_secs: 300,
                ..WatchConfig::default()
            },
            ..AppConfig::default()
        }
    }

    fn ruta_temporal(nombre: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        std::env::temp_dir().join(format!("cbrec_watch_test_{}_{}", nombre, nanos))
    }

    fn modelo(nombre: &str) -> ModelName {
        ModelName::try_from(nombre).unwrap()
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

    #[tokio::test]
    async fn online_model_starts_before_slow_status_check_finishes() {
        struct GatedRepository {
            recording_started: tokio::sync::Notify,
            downloads: AtomicUsize,
        }

        #[async_trait]
        impl StreamRepository for GatedRepository {
            type Error = InfrastructureError;

            async fn get_stream_url(
                &self,
                model: &ModelName,
            ) -> Result<Option<StreamUrl>, Self::Error> {
                if model.as_str() == "bob" {
                    self.recording_started.notified().await;
                    return Ok(None);
                }
                Ok(Some(StreamUrl::new("https://example.com/stream.m3u8")?))
            }

            async fn download_stream(
                &self,
                _url: &StreamUrl,
                path: &Path,
                _quality: VideoQuality,
            ) -> Result<(), Self::Error> {
                self.downloads.fetch_add(1, Ordering::SeqCst);
                self.recording_started.notify_one();
                tokio::fs::write(path, mp4_minimo_valido()).await?;
                Ok(())
            }
        }

        let (cancel_tx, cancel_rx) = watch::channel(false);
        let repository = Arc::new(GatedRepository {
            recording_started: tokio::sync::Notify::new(),
            downloads: AtomicUsize::new(0),
        });
        let config = config_test();
        let root = config.output_root.clone();
        let output = Arc::new(OutputFake::new(cancel_tx));
        let result = tokio::time::timeout(
            Duration::from_millis(500),
            ejecutar_watch_con_repo(WatchParams {
                client: repository.clone(),
                config: Arc::new(config),
                modelos: vec![modelo("alice"), modelo("bob")],
                ask: false,
                raiz_salida: None,
                quality: VideoQuality::Best,
                limite_concurrencia: 1,
                min_file_size: Some(1),
                cancel_rx,
                salida: output.clone(),
                prompter: Arc::new(PrompterFake::new(true)),
            }),
        )
        .await;
        let _ = tokio::fs::remove_dir_all(root).await;
        result
            .expect("alice debe grabar sin esperar la consulta lenta de bob")
            .unwrap();
        assert_eq!(repository.downloads.load(Ordering::SeqCst), 1);
        assert!(output.eventos().contains(&"grabando:alice".to_string()));
    }

    #[tokio::test]
    async fn recording_panic_stops_progress_updates() {
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let mut repository = RepoFake::online();
        repository.panic_download = true;
        let mut config = config_test();
        config.watch.poll_interval_secs = 60;
        config.watch.poll_interval_idle_secs = 60;
        let root = config.output_root.clone();
        let output = Arc::new(OutputFake::new(cancel_tx.clone()));
        let watcher = tokio::spawn(ejecutar_watch_con_repo(WatchParams {
            client: Arc::new(repository),
            config: Arc::new(config),
            modelos: vec![modelo("alice")],
            ask: false,
            raiz_salida: None,
            quality: VideoQuality::Best,
            limite_concurrencia: 1,
            min_file_size: Some(1),
            cancel_rx,
            salida: output.clone(),
            prompter: Arc::new(PrompterFake::new(true)),
        }));
        let aborted = tokio::time::timeout(
            Duration::from_secs(2),
            wait_for_event(&output, "tarea_abortada"),
        )
        .await;
        tokio::time::sleep(Duration::from_millis(5_100)).await;
        cancel_tx.send(true).unwrap();
        tokio::time::timeout(Duration::from_secs(2), watcher)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let events = output.eventos();
        tokio::fs::remove_dir_all(root).await.unwrap();
        aborted.expect("debe observarse el panic de la grabación");
        assert!(
            !events.contains(&"progreso:alice".to_string()),
            "el progreso debe detenerse al fallar la tarea: {events:?}"
        );
    }

    #[tokio::test]
    async fn waiting_online_model_starts_when_recording_slot_is_released() {
        struct QueuedRepository {
            alice_started: tokio::sync::Notify,
            bob_checks: AtomicUsize,
            finish_alice: tokio::sync::Notify,
            bob_started: tokio::sync::Notify,
        }

        #[async_trait]
        impl StreamRepository for QueuedRepository {
            type Error = InfrastructureError;

            async fn get_stream_url(
                &self,
                model: &ModelName,
            ) -> Result<Option<StreamUrl>, Self::Error> {
                if model.as_str() == "bob" {
                    let check = self.bob_checks.fetch_add(1, Ordering::SeqCst);
                    if check == 0 {
                        self.alice_started.notified().await;
                    }
                    return Ok(Some(StreamUrl::new(format!(
                        "https://example.com/bob?revision={check}"
                    ))?));
                }
                Ok(Some(StreamUrl::new(format!(
                    "https://example.com/{}",
                    model.as_str()
                ))?))
            }

            async fn download_stream(
                &self,
                url: &StreamUrl,
                path: &Path,
                _quality: VideoQuality,
            ) -> Result<(), Self::Error> {
                if url.as_str().ends_with("/alice") {
                    self.alice_started.notify_one();
                    self.finish_alice.notified().await;
                } else {
                    assert!(
                        url.as_str().ends_with("?revision=1"),
                        "debe consultarse una URL nueva al liberar la plaza"
                    );
                    self.bob_started.notify_one();
                }
                tokio::fs::write(path, mp4_minimo_valido()).await?;
                Ok(())
            }
        }

        let (cancel_tx, cancel_rx) = watch::channel(false);
        let repository = Arc::new(QueuedRepository {
            alice_started: tokio::sync::Notify::new(),
            bob_checks: AtomicUsize::new(0),
            finish_alice: tokio::sync::Notify::new(),
            bob_started: tokio::sync::Notify::new(),
        });
        let mut config = config_test();
        config.watch.poll_interval_secs = 60;
        config.watch.poll_interval_idle_secs = 60;
        let root = config.output_root.clone();
        let mut output = OutputFake::new(cancel_tx.clone());
        output.cancel_on_finish = false;
        let output = Arc::new(output);
        let watcher = tokio::spawn(ejecutar_watch_con_repo(WatchParams {
            client: repository.clone(),
            config: Arc::new(config),
            modelos: vec![modelo("alice"), modelo("bob")],
            ask: false,
            raiz_salida: None,
            quality: VideoQuality::Best,
            limite_concurrencia: 1,
            min_file_size: Some(1),
            cancel_rx,
            salida: output.clone(),
            prompter: Arc::new(PrompterFake::new(true)),
        }));
        let online = tokio::time::timeout(
            Duration::from_secs(2),
            wait_for_event(&output, "online:bob"),
        )
        .await;
        repository.finish_alice.notify_one();
        let started = tokio::time::timeout(
            Duration::from_millis(500),
            repository.bob_started.notified(),
        )
        .await;
        cancel_tx.send(true).unwrap();
        tokio::time::timeout(Duration::from_secs(2), watcher)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        tokio::fs::remove_dir_all(root).await.unwrap();
        online.expect("bob debe detectarse online mientras alice ocupa la plaza");
        started
            .expect("bob debe iniciar al liberarse la plaza, sin esperar el sondeo de 60 segundos");
        assert!(output.eventos().contains(&"fin:alice".to_string()));
    }

    #[tokio::test]
    async fn watch_interruption_keeps_mp4_and_metadata_without_reporting_success() {
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let mut repository = RepoFake::online();
        repository.fail_download = true;
        let config = config_test();
        let root = config.output_root.clone();
        let output = Arc::new(OutputFake::new(cancel_tx));
        tokio::time::timeout(
            Duration::from_secs(2),
            ejecutar_watch_con_repo(WatchParams {
                client: Arc::new(repository),
                config: Arc::new(config),
                modelos: vec![modelo("alice")],
                ask: false,
                raiz_salida: None,
                quality: VideoQuality::Best,
                limite_concurrencia: 1,
                min_file_size: Some(u64::MAX),
                cancel_rx,
                salida: output.clone(),
                prompter: Arc::new(PrompterFake::new(true)),
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let files: Vec<_> = std::fs::read_dir(root.join("cb_rec/alice"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(files.len(), 2);
        let media = files
            .iter()
            .find(|path| path.extension().is_some_and(|extension| extension == "mp4"))
            .unwrap();
        assert_eq!(tokio::fs::read(media).await.unwrap(), mp4_minimo_valido());
        let sidecar = files
            .iter()
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .unwrap();
        let metadata: serde_json::Value =
            serde_json::from_slice(&tokio::fs::read(sidecar).await.unwrap()).unwrap();
        assert_eq!(metadata["classification"], "interrupted");
        assert!(output.eventos().contains(&"error:alice".to_string()));
        assert!(!output.eventos().contains(&"fin:alice".to_string()));
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn watch_cancellation_preserves_final_file_and_metadata() {
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let mut repo = RepoFake::online();
        repo.cancel_on_download = Some(cancel_tx.clone());
        let config = config_test();
        let output_root = config.output_root.clone();
        let output = Arc::new(OutputFake::new(cancel_tx));
        tokio::time::timeout(
            Duration::from_secs(2),
            ejecutar_watch_con_repo(WatchParams {
                client: Arc::new(repo),
                config: Arc::new(config),
                modelos: vec![modelo("alice")],
                ask: false,
                raiz_salida: None,
                quality: VideoQuality::Best,
                limite_concurrencia: 1,
                min_file_size: Some(1),
                cancel_rx,
                salida: output.clone(),
                prompter: Arc::new(PrompterFake::new(true)),
            }),
        )
        .await
        .unwrap()
        .unwrap();
        let files = std::fs::read_dir(output_root.join("cb_rec/alice"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(
            files.len(),
            2,
            "Expected final media and sidecar: {files:?}"
        );
        assert!(files.iter().any(|path| path.extension().unwrap() == "mp4"));
        assert!(files.iter().any(|path| path.extension().unwrap() == "json"));
        assert!(!files
            .iter()
            .any(|path| path.to_string_lossy().contains(".part.")));
        assert!(output.eventos().contains(&"fin:alice".to_string()));
        tokio::fs::remove_dir_all(output_root).await.unwrap();
    }

    #[tokio::test]
    async fn prompt_handles_no_and_closed_input() {
        let (sender, mut receiver) = mpsc::channel(1);
        let config = WatchConfig {
            desktop_notify: false,
            ..WatchConfig::default()
        };
        sender.send("n\n".into()).await.unwrap();
        assert!(!preguntar_con_timeout("alice", &config, &mut receiver).await);
        drop(sender);
        assert!(!preguntar_con_timeout("alice", &config, &mut receiver).await);
    }

    #[tokio::test]
    async fn ejecutar_watch_graba_modelo_online_y_finaliza() {
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let repo = Arc::new(RepoFake::online());
        let salida = Arc::new(OutputFake::new(cancel_tx));
        let salida_trait: Arc<dyn Output> = salida.clone();
        let mut config = config_test();
        config.watch.poll_interval_secs = 3_600;
        config.watch.poll_interval_idle_secs = 3_600;
        let output_root = config.output_root.clone();

        tokio::time::timeout(
            Duration::from_millis(500),
            ejecutar_watch_con_repo(WatchParams {
                client: Arc::clone(&repo),
                config: Arc::new(config),
                modelos: vec![modelo("alice")],
                ask: false,
                raiz_salida: None,
                quality: VideoQuality::Best,
                limite_concurrencia: 1,
                min_file_size: Some(1),
                cancel_rx,
                salida: salida_trait,
                prompter: Arc::new(PrompterFake::new(true)),
            }),
        )
        .await
        .expect("recording completion must wake watch before the poll deadline")
        .unwrap();

        let eventos = salida.eventos();
        assert!(eventos.contains(&"online:alice".to_string()));
        assert!(eventos.contains(&"grabando:alice".to_string()));
        assert!(eventos.contains(&"fin:alice".to_string()));
        assert_eq!(repo.consultas(), 1);
        let _ = tokio::fs::remove_dir_all(output_root).await;
    }

    #[tokio::test]
    async fn ejecutar_watch_modelo_no_encontrado_no_reintenta() {
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let repo = Arc::new(RepoFake::no_encontrado());
        let salida = Arc::new(OutputFake::new(cancel_tx));
        let salida_trait: Arc<dyn Output> = salida.clone();

        ejecutar_watch_con_repo(WatchParams {
            client: Arc::clone(&repo),
            config: Arc::new(config_test()),
            modelos: vec![modelo("alice")],
            ask: false,
            raiz_salida: None,
            quality: VideoQuality::Best,
            limite_concurrencia: 1,
            min_file_size: Some(1),
            cancel_rx,
            salida: salida_trait,
            prompter: Arc::new(PrompterFake::new(true)),
        })
        .await
        .unwrap();

        assert!(salida.eventos().contains(&"error:alice".to_string()));
        assert_eq!(repo.consultas(), 1);
    }

    #[tokio::test]
    async fn ejecutar_watch_ask_rechaza_y_omite_modelo() {
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let repo = Arc::new(RepoFake::online());
        let salida = Arc::new(OutputFake::new(cancel_tx));
        let salida_trait: Arc<dyn Output> = salida.clone();
        let prompter = Arc::new(PrompterFake::new(false));
        let prompter_trait: Arc<dyn WatchPrompter> = prompter.clone();
        let config = config_test();
        let output_root = config.output_root.clone();

        ejecutar_watch_con_repo(WatchParams {
            client: Arc::clone(&repo),
            config: Arc::new(config),
            modelos: vec![modelo("alice")],
            ask: true,
            raiz_salida: None,
            quality: VideoQuality::Best,
            limite_concurrencia: 1,
            min_file_size: Some(1),
            cancel_rx,
            salida: salida_trait,
            prompter: prompter_trait,
        })
        .await
        .unwrap();

        let eventos = salida.eventos();
        assert!(eventos.contains(&"online:alice".to_string()));
        assert!(eventos.contains(&"omitido:alice".to_string()));
        assert!(!eventos.contains(&"grabando:alice".to_string()));
        assert_eq!(prompter.llamadas(), 1);
        assert_eq!(repo.consultas(), 1);
        let _ = tokio::fs::remove_dir_all(output_root).await;
    }

    fn estados(items: &[(&str, EstadoModelo)]) -> HashMap<String, EstadoModelo> {
        items
            .iter()
            .map(|(nombre, estado)| ((*nombre).to_string(), estado.clone()))
            .collect()
    }

    #[test]
    fn calcular_slots_disponibles_respeta_limite() {
        let estados = estados(&[
            ("alice", EstadoModelo::Grabando),
            ("bob", EstadoModelo::Offline),
        ]);

        assert_eq!(calcular_slots_disponibles(&estados, 2), 1);
    }

    #[test]
    fn calcular_slots_disponibles_no_baja_de_cero() {
        let estados = estados(&[
            ("alice", EstadoModelo::Grabando),
            ("bob", EstadoModelo::Grabando),
        ]);

        assert_eq!(calcular_slots_disponibles(&estados, 1), 0);
    }

    #[test]
    fn debe_consultar_modelo_offline_sin_bloqueos() {
        let estados = estados(&[("alice", EstadoModelo::Offline)]);
        let omitidos = HashSet::new();
        let invalidos = HashSet::new();
        let bloqueados_hasta = HashMap::new();

        assert!(debe_consultar_modelo(
            "alice",
            &estados,
            &omitidos,
            &invalidos,
            &bloqueados_hasta,
        ));
    }

    #[test]
    fn debe_consultar_modelo_ignora_si_esta_grabando() {
        let estados = estados(&[("alice", EstadoModelo::Grabando)]);
        let omitidos = HashSet::new();
        let invalidos = HashSet::new();
        let bloqueados_hasta = HashMap::new();

        assert!(!debe_consultar_modelo(
            "alice",
            &estados,
            &omitidos,
            &invalidos,
            &bloqueados_hasta,
        ));
    }

    #[test]
    fn debe_consultar_modelo_ignora_omitidos_e_invalidos() {
        let estados = estados(&[
            ("alice", EstadoModelo::Offline),
            ("bob", EstadoModelo::Offline),
        ]);
        let omitidos = HashSet::from(["alice".to_string()]);
        let invalidos = HashSet::from(["bob".to_string()]);
        let bloqueados_hasta = HashMap::new();

        assert!(!debe_consultar_modelo(
            "alice",
            &estados,
            &omitidos,
            &invalidos,
            &bloqueados_hasta,
        ));
        assert!(!debe_consultar_modelo(
            "bob",
            &estados,
            &omitidos,
            &invalidos,
            &bloqueados_hasta,
        ));
    }

    #[test]
    fn debe_consultar_modelo_respeta_cooldown_de_fallo() {
        let estados = estados(&[("alice", EstadoModelo::Offline)]);
        let omitidos = HashSet::new();
        let invalidos = HashSet::new();
        let bloqueados_hasta = HashMap::from([(
            "alice".to_string(),
            Instant::now()
                .checked_add(Duration::from_secs(300))
                .unwrap_or_else(Instant::now),
        )]);

        assert!(!debe_consultar_modelo(
            "alice",
            &estados,
            &omitidos,
            &invalidos,
            &bloqueados_hasta,
        ));
    }

    #[test]
    fn debe_consultar_modelo_reintenta_despues_del_cooldown() {
        let estados = estados(&[("alice", EstadoModelo::Offline)]);
        let omitidos = HashSet::new();
        let invalidos = HashSet::new();
        let bloqueados_hasta = HashMap::from([(
            "alice".to_string(),
            Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
        )]);

        assert!(debe_consultar_modelo(
            "alice",
            &estados,
            &omitidos,
            &invalidos,
            &bloqueados_hasta,
        ));
    }

    #[test]
    fn cooldown_para_error_consulta_extiende_rate_limit() {
        let cooldown = cooldown_para_error_consulta(
            &InfrastructureError::HttpStatus(429),
            Duration::from_secs(10),
        );

        assert_eq!(cooldown, Duration::from_secs(60));
    }

    #[test]
    fn cooldown_para_error_consulta_extiende_errores_temporales() {
        let cooldown = cooldown_para_error_consulta(
            &InfrastructureError::HttpStatus(503),
            Duration::from_secs(20),
        );

        assert_eq!(cooldown, Duration::from_secs(40));
    }
}
