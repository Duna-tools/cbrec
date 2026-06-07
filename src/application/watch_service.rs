use crate::application::recording::{descargar_grabacion, ruta_parcial, ResultadoGrabacion};
use crate::domain::repositories::StreamRepository;
use crate::domain::value_objects::{EstadoModelo, ModelName, StreamUrl, VideoQuality};
use crate::infrastructure::{AppConfig, ChaturbateClient};
use crate::presentation::Output;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tokio::task::JoinSet;

#[allow(clippy::too_many_arguments)]
pub async fn ejecutar_watch(
    client: Arc<ChaturbateClient>,
    config: Arc<AppConfig>,
    modelos: Vec<ModelName>,
    ask: bool,
    raiz_salida: Option<PathBuf>,
    quality: VideoQuality,
    cancel_rx: watch::Receiver<bool>,
    salida: Arc<dyn Output>,
) -> anyhow::Result<()> {
    let nombres: Vec<&str> = modelos.iter().map(|m| m.as_str()).collect();
    salida.watch_inicio(&nombres);

    let mut estados: HashMap<String, EstadoModelo> = modelos
        .iter()
        .map(|m| (m.as_str().to_string(), EstadoModelo::Offline))
        .collect();

    let mut omitidos: HashSet<String> = HashSet::new();
    let mut fallos: HashMap<String, Instant> = HashMap::new();
    let mut grabaciones: JoinSet<(String, Option<PathBuf>, bool)> = JoinSet::new();

    let mut ultima_actividad: Instant = Instant::now()
        .checked_sub(Duration::from_secs(
            config.watch.idle_threshold_mins * 60 + 1,
        ))
        .unwrap_or(Instant::now());

    loop {
        while let Some(Ok((modelo, ruta_final, hubo_error))) = grabaciones.try_join_next() {
            if let Some(ruta) = ruta_final {
                salida.watch_fin_grabacion(&modelo, &ruta);
            }
            if hubo_error {
                fallos.insert(modelo.clone(), Instant::now());
            }
            estados.insert(modelo, EstadoModelo::Offline);
        }

        if *cancel_rx.borrow() {
            salida.watch_deteniendo();
            grabaciones.abort_all();
            break;
        }

        let grabando_ahora = estados
            .values()
            .filter(|e| **e == EstadoModelo::Grabando)
            .count();
        let mut slots_disponibles = config.watch.max_simultaneous.saturating_sub(grabando_ahora);

        let cooldown = Duration::from_secs(config.watch.cooldown_tras_fallo_secs);

        for modelo in &modelos {
            if *cancel_rx.borrow() {
                break;
            }

            let nombre = modelo.as_str().to_string();

            if estados.get(&nombre) == Some(&EstadoModelo::Grabando) {
                continue;
            }
            if omitidos.contains(&nombre) {
                continue;
            }
            if fallos.get(&nombre).is_some_and(|t| t.elapsed() < cooldown) {
                continue;
            }

            let stream_url: Option<StreamUrl> = match client.get_stream_url(modelo).await {
                Ok(opt) => opt,
                Err(err) => {
                    eprintln!("[WARN][{}] Error al consultar estado: {}", nombre, err);
                    None
                }
            };

            if let Some(stream_url) = stream_url {
                ultima_actividad = Instant::now();
                salida.watch_tick_online(&nombre);

                if slots_disponibles == 0 {
                    continue;
                }

                if ask {
                    let confirmar = salida.watch_pregunta_grabar(&nombre);
                    if !confirmar {
                        salida.watch_modelo_omitido(&nombre);
                        omitidos.insert(nombre.clone());
                        continue;
                    }
                }

                salida.watch_inicio_grabacion(&nombre);
                estados.insert(nombre.clone(), EstadoModelo::Grabando);

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

                    let ruta =
                        config_clone.get_output_path(nombre_clone.as_str(), raiz_clone.as_deref());

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
                        &client_clone,
                        &stream_url,
                        ruta,
                        quality,
                        config_clone.min_file_size,
                    )
                    .await;
                    progress_task.abort();

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
                slots_disponibles = slots_disponibles.saturating_sub(1);
            } else {
                salida.watch_tick_offline(&nombre);
            }
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
                grabaciones.abort_all();
                break;
            }
        }
    }

    Ok(())
}

async fn esperar_cancelacion(mut rx: watch::Receiver<bool>) {
    let _ = rx.wait_for(|v| *v).await;
}
