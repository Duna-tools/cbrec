use crate::application::recording::{
    descargar_grabacion, detener_tarea_progreso, preparar_ruta_grabacion, ruta_parcial,
    ResultadoGrabacion,
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
use tokio::io::AsyncBufReadExt;
use tokio::sync::watch;
use tokio::task::JoinSet;

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
pub(crate) trait WatchPrompter: Send + Sync {
    async fn confirmar_grabacion(&self, modelo: &str, cfg: &WatchConfig) -> bool;
}

pub(crate) struct ConsoleWatchPrompter;

#[async_trait]
impl WatchPrompter for ConsoleWatchPrompter {
    async fn confirmar_grabacion(&self, modelo: &str, cfg: &WatchConfig) -> bool {
        preguntar_con_timeout(modelo, cfg).await
    }
}

pub(crate) async fn ejecutar_watch(params: WatchParams) -> anyhow::Result<()> {
    ejecutar_watch_con_repo(params).await
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
    let mut grabaciones: JoinSet<(String, Option<PathBuf>, bool)> = JoinSet::new();

    let mut ultima_actividad = Instant::now()
        .checked_sub(Duration::from_secs(
            config.watch.idle_threshold_mins * 60 + 1,
        ))
        .unwrap_or_else(Instant::now);

    loop {
        while let Some(Ok((modelo, ruta_final, hubo_error))) = grabaciones.try_join_next() {
            if let Some(ruta) = ruta_final {
                salida.watch_fin_grabacion(&modelo, &ruta);
            }
            if hubo_error {
                bloqueados_hasta.insert(
                    modelo.clone(),
                    instante_tras(Duration::from_secs(config.watch.cooldown_tras_fallo_secs)),
                );
            }
            estados.insert(modelo, EstadoModelo::Offline);
        }

        if *cancel_rx.borrow() {
            salida.watch_deteniendo();
            cancelar_grabaciones(&mut grabaciones).await;
            break;
        }

        let mut slots_disponibles = calcular_slots_disponibles(&estados, limite_concurrencia);
        let cooldown = Duration::from_secs(config.watch.cooldown_tras_fallo_secs);

        // Checks de estado en paralelo
        let mut checks: JoinSet<(String, Result<Option<StreamUrl>, InfrastructureError>)> =
            JoinSet::new();

        for modelo in &modelos {
            let nombre = modelo.as_str().to_string();
            if !debe_consultar_modelo(&nombre, &estados, &omitidos, &invalidos, &bloqueados_hasta) {
                continue;
            }
            let client_c = Arc::clone(&client);
            let m = modelo.clone();
            checks
                .spawn(async move { (m.as_str().to_string(), client_c.get_stream_url(&m).await) });
        }

        let mut online: Vec<(String, StreamUrl)> = Vec::new();
        while let Some(Ok((nombre, resultado))) = checks.join_next().await {
            match resultado {
                Ok(Some(url)) => {
                    ultima_actividad = Instant::now();
                    salida.watch_tick_online(&nombre);
                    online.push((nombre, url));
                }
                Ok(None) => salida.watch_tick_offline(&nombre),
                Err(InfrastructureError::Domain(DomainError::ModelNotFound(_))) => {
                    salida.error_fallo_grabacion(
                        &nombre,
                        "modelo no encontrado, eliminado de monitoreo",
                    );
                    invalidos.insert(nombre);
                }
                Err(e) => {
                    salida.advertir_error_consulta_estado(&nombre, &e.to_string());
                    bloqueados_hasta.insert(
                        nombre,
                        instante_tras(cooldown_para_error_consulta(&e, cooldown)),
                    );
                }
            }
        }

        // Decisiones de grabación (secuencial para manejar stdin/slots)
        for (nombre, stream_url) in online {
            if *cancel_rx.borrow() || slots_disponibles == 0 {
                break;
            }

            if ask && !prompter.confirmar_grabacion(&nombre, &config.watch).await {
                salida.watch_modelo_omitido(&nombre);
                omitidos.insert(nombre);
                continue;
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

            grabaciones.spawn(async move {
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
                let parcial_p = ruta_parcial(&ruta);
                let progress_task = tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        if let Ok(meta) = tokio::fs::metadata(&parcial_p).await {
                            salida_p.mostrar_progreso_grabacion(&nombre_p, meta.len());
                        }
                    }
                });

                let result = descargar_grabacion(
                    client_clone.as_ref(),
                    &stream_url,
                    ruta,
                    quality,
                    min_file_size,
                )
                .await;
                detener_tarea_progreso(progress_task).await;

                match result {
                    Ok(ResultadoGrabacion::Guardado(p)) => (nombre_clone, Some(p), false),
                    Ok(ResultadoGrabacion::Pequeno(p, _)) => (nombre_clone, Some(p), false),
                    Ok(ResultadoGrabacion::Cancelado) => (nombre_clone, None, false),
                    Err(e) => {
                        salida_clone.error_fallo_grabacion(&nombre_clone, &e.to_string());
                        (nombre_clone, None, true)
                    }
                }
            });
        }

        let tiempo_idle = ultima_actividad.elapsed();
        let umbral_idle = Duration::from_secs(config.watch.idle_threshold_mins * 60);
        let intervalo_secs = if tiempo_idle >= umbral_idle {
            config.watch.poll_interval_idle_secs
        } else {
            config.watch.poll_interval_secs
        };

        salida.watch_proximo_check(intervalo_secs);

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(intervalo_secs)) => {}
            _ = esperar_cancelacion(cancel_rx.clone()) => {
                salida.watch_deteniendo();
                cancelar_grabaciones(&mut grabaciones).await;
                break;
            }
        }
    }

    Ok(())
}

async fn preguntar_con_timeout(modelo: &str, cfg: &WatchConfig) -> bool {
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

    let readline = async {
        let mut reader = tokio::io::BufReader::new(tokio::io::stdin());
        let mut line = String::new();
        let _ = reader.read_line(&mut line).await;
        line
    };

    match tokio::time::timeout(Duration::from_secs(cfg.ask_timeout_secs), readline).await {
        Err(_) => true,
        Ok(line) => !line.trim().eq_ignore_ascii_case("n"),
    }
}

async fn esperar_cancelacion(mut rx: watch::Receiver<bool>) {
    let _ = rx.wait_for(|v| *v).await;
}

async fn cancelar_grabaciones(grabaciones: &mut JoinSet<(String, Option<PathBuf>, bool)>) {
    grabaciones.abort_all();
    while grabaciones.join_next().await.is_some() {}
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

    enum RespuestaRepo {
        Online,
        NoEncontrado,
    }

    struct RepoFake {
        respuesta: RespuestaRepo,
        consultas: AtomicUsize,
    }

    impl RepoFake {
        fn online() -> Self {
            Self {
                respuesta: RespuestaRepo::Online,
                consultas: AtomicUsize::new(0),
            }
        }

        fn no_encontrado() -> Self {
            Self {
                respuesta: RespuestaRepo::NoEncontrado,
                consultas: AtomicUsize::new(0),
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
            tokio::fs::write(output_path, b"video").await?;
            Ok(())
        }
    }

    struct OutputFake {
        eventos: Mutex<Vec<String>>,
        cancel_tx: watch::Sender<bool>,
    }

    impl OutputFake {
        fn new(cancel_tx: watch::Sender<bool>) -> Self {
            Self {
                eventos: Mutex::new(Vec::new()),
                cancel_tx,
            }
        }

        fn eventos(&self) -> Vec<String> {
            self.eventos.lock().unwrap().clone()
        }

        fn evento(&self, evento: impl Into<String>) {
            self.eventos.lock().unwrap().push(evento.into());
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
        fn error_tarea_abortada(&self, _error: &str) {}
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
            let _ = self.cancel_tx.send(true);
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

    #[tokio::test]
    async fn ejecutar_watch_graba_modelo_online_y_finaliza() {
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let repo = Arc::new(RepoFake::online());
        let salida = Arc::new(OutputFake::new(cancel_tx));
        let salida_trait: Arc<dyn Output> = salida.clone();
        let config = config_test();
        let output_root = config.output_root.clone();

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
        })
        .await
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
