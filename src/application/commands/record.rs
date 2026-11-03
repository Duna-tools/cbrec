use crate::application::recording::{
    descargar_grabacion, detener_tarea_progreso, preparar_ruta_grabacion, write_recording_metadata,
    RecordingProgress, ResultadoGrabacion,
};
use crate::application::utils::{normalizar_modelos, ParametrosGrabacion};
use crate::domain::value_objects::{ModelName, VideoQuality};
use crate::infrastructure::{AppConfig, ChaturbateClient, EstadoStream};
use crate::presentation::Output;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

#[derive(Clone, Copy)]
struct OpcionesGrabacionModelo<'a> {
    raiz_salida_override: Option<&'a Path>,
    quality: VideoQuality,
    min_file_size: Option<u64>,
    modo_detallado: bool,
}

fn model_queue_capacity(model_count: usize) -> usize {
    model_count.max(1)
}

pub(crate) async fn grabar_modelos(
    client: ChaturbateClient,
    config: AppConfig,
    modelos: Vec<String>,
    parametros: ParametrosGrabacion,
) -> anyhow::Result<()> {
    let ParametrosGrabacion {
        raiz_salida,
        quality,
        limite_concurrencia,
        min_file_size,
        cancel_rx,
        salida,
    } = parametros;
    let client = Arc::new(client);
    let config = Arc::new(config);

    let (modelos, duplicados) = normalizar_modelos(modelos)?;
    if duplicados > 0 {
        salida.advertir_modelos_duplicados(duplicados);
    }

    let modo_detallado = modelos.len() <= 1 || salida.is_verbose();

    if modelos.len() > limite_concurrencia {
        salida.advertir_modelos_sobre_limite(modelos.len(), limite_concurrencia);
    }

    let (tx, rx) = mpsc::channel::<ModelName>(model_queue_capacity(modelos.len()));
    let rx = Arc::new(tokio::sync::Mutex::new(rx));
    let mut tareas = JoinSet::new();

    let worker_count = limite_concurrencia.min(modelos.len());
    for modelo in modelos {
        if tx.send(modelo).await.is_err() {
            break;
        }
    }
    drop(tx);

    for _ in 0..worker_count {
        let client = Arc::clone(&client);
        let config = Arc::clone(&config);
        let raiz_salida = raiz_salida.clone();
        let rx = Arc::clone(&rx);
        let cancel_rx = cancel_rx.clone();
        let salida = Arc::clone(&salida);

        tareas.spawn(async move {
            let mut errores = Vec::new();
            loop {
                if *cancel_rx.borrow() {
                    break;
                }

                let modelo = {
                    let mut guard = rx.lock().await;
                    guard.recv().await
                };

                let Some(modelo) = modelo else {
                    break;
                };

                if *cancel_rx.borrow() {
                    break;
                }

                let resultado = grabar_modelo(
                    client.as_ref(),
                    config.as_ref(),
                    &modelo,
                    OpcionesGrabacionModelo {
                        raiz_salida_override: raiz_salida.as_deref(),
                        quality,
                        min_file_size,
                        modo_detallado,
                    },
                    Arc::clone(&salida),
                )
                .await;

                if let Err(err) = resultado {
                    salida.error_fallo_grabacion(modelo.as_str(), &err.to_string());
                    errores.push(modelo.as_str().to_string());
                }
            }
            errores
        });
    }

    let mut errores = Vec::new();
    while let Some(resultado) = tareas.join_next().await {
        match resultado {
            Ok(errores_worker) => errores.extend(errores_worker),
            Err(err) => {
                salida.error_tarea_abortada(&err.to_string());
                errores.push("tarea".to_string());
            }
        }
    }

    if !errores.is_empty() {
        anyhow::bail!("Fallo la grabacion en {} modelo(s)", errores.len());
    }

    Ok(())
}

async fn grabar_modelo(
    client: &ChaturbateClient,
    config: &AppConfig,
    model_name: &ModelName,
    opciones: OpcionesGrabacionModelo<'_>,
    salida: Arc<dyn Output>,
) -> anyhow::Result<()> {
    let OpcionesGrabacionModelo {
        raiz_salida_override,
        quality,
        min_file_size,
        modo_detallado,
    } = opciones;

    if modo_detallado {
        salida.mostrar_inicio_detallado(model_name.as_str(), &quality.to_string());
    } else {
        salida.mostrar_inicio_resumido(model_name.as_str(), &quality.to_string());
    }

    if modo_detallado {
        salida.mostrar_verificando_disponibilidad();
    }

    let stream_url = match client.consultar_estado(model_name).await? {
        EstadoStream::Online { stream_url } => stream_url,
        EstadoStream::Offline => {
            if modo_detallado {
                salida.mostrar_modelo_offline_detallado(model_name.as_str());
            } else {
                salida.mostrar_modelo_offline_resumido(model_name.as_str());
            }
            return Ok(());
        }
        EstadoStream::RequiereSesion { detalle } => {
            anyhow::bail!(
                "{} requiere sesion o acceso privado: {}",
                model_name,
                detalle
            );
        }
        EstadoStream::RateLimited => {
            anyhow::bail!("Chaturbate limito las consultas; reintenta mas tarde");
        }
        EstadoStream::Bloqueado { detalle } => {
            anyhow::bail!("Respuesta bloqueada por Chaturbate: {}", detalle);
        }
        EstadoStream::RespuestaInesperada { detalle } => {
            anyhow::bail!("Respuesta inesperada de Chaturbate: {}", detalle);
        }
    };

    if modo_detallado {
        salida.mostrar_modelo_online_detallado();
    }

    let ruta_base = config.get_output_path(model_name.as_str(), raiz_salida_override);
    let ruta = preparar_ruta_grabacion(ruta_base).await?;

    if modo_detallado {
        salida.mostrar_detalle_inicio_grabacion(&ruta);
    }

    let salida_p = Arc::clone(&salida);
    let nombre_p = model_name.as_str().to_string();
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
    let result = descargar_grabacion(client, &stream_url, ruta, quality, min_file_size).await;
    detener_tarea_progreso(progress_task).await;
    if let Ok(recording) = &result {
        write_recording_metadata(recording, model_name.as_str(), quality, started_at).await?;
    }

    match result {
        Ok(ResultadoGrabacion::Guardado(ruta)) => {
            if modo_detallado {
                salida.mostrar_archivo_guardado_detallado(&ruta);
            } else {
                salida.mostrar_archivo_guardado_resumido(model_name.as_str(), &ruta);
            }
        }
        Ok(ResultadoGrabacion::Pequeno(ruta, bytes)) => {
            if modo_detallado {
                salida.mostrar_archivo_pequeno_detallado(bytes, &ruta);
            } else {
                salida.mostrar_archivo_pequeno_resumido(model_name.as_str(), &ruta);
            }
        }
        Ok(ResultadoGrabacion::Cancelado) => {
            if modo_detallado {
                salida.mostrar_cancelacion_detallada();
            } else {
                salida.mostrar_cancelacion_resumida(model_name.as_str());
            }
        }
        Ok(ResultadoGrabacion::Interrupted(_, reason)) => anyhow::bail!(reason),
        Err(e) => return Err(e.into()),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presentation::ConsoleOutput;

    #[tokio::test]
    async fn pre_cancelled_batch_does_not_start_recording() {
        let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(true);
        let parameters = ParametrosGrabacion {
            raiz_salida: None,
            quality: VideoQuality::Best,
            limite_concurrencia: 4096,
            min_file_size: None,
            cancel_rx,
            salida: Arc::new(ConsoleOutput::new(false, true)),
        };

        let result = grabar_modelos(
            ChaturbateClient::new().expect("create client"),
            AppConfig::default(),
            vec!["alice".to_string()],
            parameters,
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn queue_accepts_all_models_before_workers_start() {
        let models = [
            "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf",
        ]
        .into_iter()
        .map(|name| ModelName::try_from(name).expect("valid model name"))
        .collect::<Vec<_>>();
        let (sender, mut receiver) = mpsc::channel::<ModelName>(model_queue_capacity(models.len()));

        for model in models {
            tokio::time::timeout(Duration::from_millis(50), sender.send(model))
                .await
                .expect("the complete finite input must fit before workers start")
                .expect("the receiver remains open");
        }
        drop(sender);

        let mut received = 0;
        while receiver.recv().await.is_some() {
            received += 1;
        }
        assert_eq!(received, 7);
    }
}
