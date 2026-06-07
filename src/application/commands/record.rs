use crate::application::recording::{descargar_grabacion, ruta_parcial, ResultadoGrabacion};
use crate::application::utils::{deduplicar_modelos, ParametrosGrabacion};
use crate::domain::repositories::StreamRepository;
use crate::domain::value_objects::{ModelName, VideoQuality};
use crate::infrastructure::{AppConfig, ChaturbateClient};
use crate::presentation::Output;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinSet;

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
        cancel_rx,
        salida,
    } = parametros;
    let client = Arc::new(client);
    let config = Arc::new(config);

    let (modelos, duplicados) = deduplicar_modelos(modelos);
    if duplicados > 0 {
        salida.advertir_modelos_duplicados(duplicados);
    }

    let modo_detallado = modelos.len() <= 1 || salida.is_verbose();

    if modelos.len() > limite_concurrencia {
        salida.advertir_modelos_sobre_limite(modelos.len(), limite_concurrencia);
    }

    let (tx, rx) = mpsc::channel::<String>(limite_concurrencia.saturating_mul(2).max(1));
    let rx = Arc::new(tokio::sync::Mutex::new(rx));
    let mut tareas = JoinSet::new();

    for modelo in modelos {
        if tx.send(modelo).await.is_err() {
            break;
        }
    }
    drop(tx);

    for _ in 0..limite_concurrencia {
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
                    raiz_salida.as_deref(),
                    quality,
                    modo_detallado,
                    Arc::clone(&salida),
                )
                .await;

                if let Err(err) = resultado {
                    salida.error_fallo_grabacion(&modelo, &err.to_string());
                    errores.push(modelo);
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
    target: &str,
    raiz_salida_override: Option<&Path>,
    quality: VideoQuality,
    modo_detallado: bool,
    salida: Arc<dyn Output>,
) -> anyhow::Result<()> {
    if modo_detallado {
        salida.mostrar_inicio_detallado(target, &quality.to_string());
    } else {
        salida.mostrar_inicio_resumido(target, &quality.to_string());
    }

    let model_name = ModelName::try_from(target)?;

    if modo_detallado {
        salida.mostrar_verificando_disponibilidad();
    }

    let stream_url = client.get_stream_url(&model_name).await?;
    let Some(stream_url) = stream_url else {
        if modo_detallado {
            salida.mostrar_modelo_offline_detallado(model_name.as_str());
        } else {
            salida.mostrar_modelo_offline_resumido(model_name.as_str());
        }
        return Ok(());
    };

    if modo_detallado {
        salida.mostrar_modelo_online_detallado();
    }

    let ruta = config.get_output_path(model_name.as_str(), raiz_salida_override);

    if modo_detallado {
        salida.mostrar_detalle_inicio_grabacion(&ruta);
    }

    let parcial = ruta_parcial(&ruta);
    let salida_p = Arc::clone(&salida);
    let nombre_p = target.to_string();
    let parcial_p = parcial.clone();
    let progress_task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            if let Ok(meta) = tokio::fs::metadata(&parcial_p).await {
                salida_p.mostrar_progreso_grabacion(&nombre_p, meta.len());
            }
        }
    });

    let result =
        descargar_grabacion(client, &stream_url, ruta, quality, config.min_file_size).await;
    progress_task.abort();

    match result {
        Ok(ResultadoGrabacion::Guardado(ruta)) => {
            if modo_detallado {
                salida.mostrar_archivo_guardado_detallado(&ruta);
            } else {
                salida.mostrar_archivo_guardado_resumido(target, &ruta);
            }
        }
        Ok(ResultadoGrabacion::Pequeno(ruta, bytes)) => {
            if modo_detallado {
                salida.mostrar_archivo_pequeno_detallado(bytes, &ruta);
            } else {
                salida.mostrar_archivo_pequeno_resumido(target, &ruta);
            }
        }
        Ok(ResultadoGrabacion::Cancelado) => {
            if modo_detallado {
                salida.mostrar_cancelacion_detallada();
            } else {
                salida.mostrar_cancelacion_resumida(target);
            }
        }
        Err(e) => return Err(e.into()),
    }

    Ok(())
}
