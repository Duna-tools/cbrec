use crate::domain::repositories::StreamRepository;
use crate::domain::value_objects::ModelName;
use crate::infrastructure::{AppConfig, ChaturbateClient, InfrastructureError};
use crate::presentation::{Cli, Commands, ConsoleOutput};
use directories::UserDirs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinSet;

const LIMITE_CONCURRENCIA_DEFECTO: usize = 3;

/// Orquesta la ejecucion de la CLI.
pub async fn ejecutar_cli(
    cli: Cli,
    config: AppConfig,
    client: ChaturbateClient,
) -> anyhow::Result<()> {
    let salida = Arc::new(ConsoleOutput::new());
    let Cli {
        modelos: modelos_principales,
        verificar,
        listar,
        output: salida_principal,
        quality: calidad_principal,
        jobs,
        ffmpeg_path,
        command,
    } = cli;

    let limite_concurrencia = jobs;
    if limite_concurrencia == 0 {
        anyhow::bail!("El limite de concurrencia debe ser mayor a 0");
    }
    if limite_concurrencia > LIMITE_CONCURRENCIA_DEFECTO {
        salida.advertir_limite_concurrencia(LIMITE_CONCURRENCIA_DEFECTO, limite_concurrencia);
    }

    let ruta_ffmpeg = resolver_ruta_opcional(ffmpeg_path);
    let (cancel_tx, cancel_rx) = watch::channel(false);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            let _ = cancel_tx.send(true);
        }
    });
    let cancel_rx_worker = cancel_rx.clone();
    let client = client.with_cancel_receiver(cancel_rx);

    match command {
        Some(Commands::Record {
            modelos,
            output,
            quality,
        }) => {
            validar_ffmpeg(ruta_ffmpeg.as_deref()).await?;
            let client = aplicar_ffmpeg_path(client, ruta_ffmpeg);
            let raiz_salida = resolver_ruta_opcional(output);
            grabar_modelos(
                client,
                config,
                modelos,
                raiz_salida,
                quality,
                limite_concurrencia,
                cancel_rx_worker.clone(),
                Arc::clone(&salida),
            )
            .await
        }
        Some(Commands::Check { model }) => verificar_modelo(&client, salida.as_ref(), &model).await,
        None => {
            if modelos_principales.is_empty() {
                salida.mostrar_error_sin_modelo();
                std::process::exit(1);
            }

            validar_ffmpeg(ruta_ffmpeg.as_deref()).await?;
            let client = aplicar_ffmpeg_path(client, ruta_ffmpeg);
            let raiz_salida = resolver_ruta_opcional(salida_principal);
            if listar {
                listar_calidades_modelos(&client, &salida, modelos_principales).await
            } else if verificar {
                verificar_modelos(&client, &salida, modelos_principales).await
            } else {
                grabar_modelos(
                    client,
                    config,
                    modelos_principales,
                    raiz_salida,
                    calidad_principal,
                    limite_concurrencia,
                    cancel_rx_worker.clone(),
                    Arc::clone(&salida),
                )
                .await
            }
        }
    }
}

async fn grabar_modelos(
    client: ChaturbateClient,
    config: AppConfig,
    modelos: Vec<String>,
    raiz_salida: Option<PathBuf>,
    quality: String,
    limite_concurrencia: usize,
    cancel_rx: watch::Receiver<bool>,
    salida: Arc<ConsoleOutput>,
) -> anyhow::Result<()> {
    let client = Arc::new(client);
    let config = Arc::new(config);

    let (modelos, duplicados) = deduplicar_modelos(modelos);
    if duplicados > 0 {
        salida.advertir_modelos_duplicados(duplicados);
    }

    let modo_detallado = modelos.len() <= 1;

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
        let quality = quality.clone();
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
                    &quality,
                    modo_detallado,
                    salida.as_ref(),
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
    quality: &str,
    modo_detallado: bool,
    salida: &ConsoleOutput,
) -> anyhow::Result<()> {
    if modo_detallado {
        salida.mostrar_inicio_detallado(target, quality);
    } else {
        salida.mostrar_inicio_resumido(target, quality);
    }

    let model_name = ModelName::try_from(target)?;

    if modo_detallado {
        salida.mostrar_verificando_disponibilidad();
    }
    let stream_url = client.get_stream_url(&model_name).await?;

    let stream_url = match stream_url {
        Some(url) => url,
        None => {
            if modo_detallado {
                salida.mostrar_modelo_offline_detallado(model_name.as_str());
            } else {
                salida.mostrar_modelo_offline_resumido(model_name.as_str());
            }
            return Ok(());
        }
    };

    if modo_detallado {
        salida.mostrar_modelo_online_detallado();
    }

    let ruta_salida = config.get_output_path(model_name.as_str(), raiz_salida_override);

    if let Some(parent) = ruta_salida.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    if modo_detallado {
        salida.mostrar_detalle_inicio_grabacion(&ruta_salida);
    }

    match client
        .download_stream(&stream_url, &ruta_salida, quality)
        .await
    {
        Ok(()) => {}
        Err(InfrastructureError::RecordingCancelled) => {
            if modo_detallado {
                salida.mostrar_cancelacion_detallada();
            } else {
                salida.mostrar_cancelacion_resumida(target);
            }
            return Ok(());
        }
        Err(err) => return Err(err.into()),
    }

    let metadata = tokio::fs::metadata(&ruta_salida).await?;
    if metadata.len() < config.min_file_size {
        if modo_detallado {
            salida.mostrar_archivo_pequeno_detallado(metadata.len());
        } else {
            salida.mostrar_archivo_pequeno_resumido(target);
        }
        tokio::fs::remove_file(&ruta_salida).await?;
    } else {
        if modo_detallado {
            salida.mostrar_archivo_guardado_detallado(&ruta_salida);
        } else {
            salida.mostrar_archivo_guardado_resumido(target, &ruta_salida);
        }
    }

    Ok(())
}

async fn verificar_modelo(
    client: &ChaturbateClient,
    salida: &ConsoleOutput,
    model: &str,
) -> anyhow::Result<()> {
    let model_name = ModelName::try_from(model)?;

    salida.mostrar_inicio_verificacion(model_name.as_str());

    match client.get_stream_url(&model_name).await? {
        Some(_) => {
            salida.mostrar_estado_modelo(model_name.as_str(), true);
        }
        None => {
            salida.mostrar_estado_modelo(model_name.as_str(), false);
        }
    }

    Ok(())
}

async fn verificar_modelos(
    client: &ChaturbateClient,
    salida: &ConsoleOutput,
    modelos: Vec<String>,
) -> anyhow::Result<()> {
    for modelo in modelos {
        verificar_modelo(client, salida, &modelo).await?;
    }
    Ok(())
}

async fn listar_calidades_modelos(
    client: &ChaturbateClient,
    salida: &ConsoleOutput,
    modelos: Vec<String>,
) -> anyhow::Result<()> {
    for modelo in modelos {
        let model_name = ModelName::try_from(modelo.as_str())?;
        let stream_url = client.get_stream_url(&model_name).await?;
        let Some(stream_url) = stream_url else {
            salida.mostrar_estado_modelo(model_name.as_str(), false);
            continue;
        };

        let calidades = client.listar_calidades(&stream_url).await?;
        if calidades.is_empty() {
            salida.mostrar_modelo_sin_variantes(model_name.as_str());
            continue;
        }

        let calidades_formato: Vec<(Option<u32>, Option<u64>)> =
            calidades.iter().map(|c| (c.height, c.bandwidth)).collect();
        salida.mostrar_calidades(model_name.as_str(), &calidades_formato);
    }

    Ok(())
}

fn resolver_ruta_opcional(ruta: Option<String>) -> Option<PathBuf> {
    ruta.map(|ruta| expandir_tilde(&ruta))
}

fn expandir_tilde(ruta: &str) -> PathBuf {
    let ruta_normalizada = ruta.trim();
    if let Some(resto) = ruta_normalizada.strip_prefix("~/") {
        if let Some(home) = obtener_home_dir() {
            return home.join(resto);
        }
    }
    if let Some(resto) = ruta_normalizada.strip_prefix("~\\") {
        if let Some(home) = obtener_home_dir() {
            return home.join(resto);
        }
    }

    PathBuf::from(ruta_normalizada)
}

fn obtener_home_dir() -> Option<PathBuf> {
    UserDirs::new().map(|dirs| dirs.home_dir().to_path_buf())
}

fn deduplicar_modelos(modelos: Vec<String>) -> (Vec<String>, usize) {
    let mut vistos = std::collections::HashSet::new();
    let mut unicos = Vec::new();
    let mut duplicados = 0;

    for modelo in modelos {
        let clave = modelo.trim().to_lowercase();
        if clave.is_empty() {
            unicos.push(modelo);
            continue;
        }
        if vistos.insert(clave) {
            unicos.push(modelo);
        } else {
            duplicados += 1;
        }
    }

    (unicos, duplicados)
}

fn aplicar_ffmpeg_path(client: ChaturbateClient, ruta: Option<PathBuf>) -> ChaturbateClient {
    match ruta {
        Some(path) => client.with_ffmpeg_path(path),
        None => client,
    }
}

async fn validar_ffmpeg(ruta: Option<&Path>) -> anyhow::Result<()> {
    if let Some(ruta) = ruta {
        if !ruta.exists() {
            anyhow::bail!("Ruta de ffmpeg invalida: {}", ruta.display());
        }
    }

    let bin = ruta.unwrap_or_else(|| Path::new("ffmpeg"));
    let salida = tokio::process::Command::new(bin)
        .arg("-version")
        .output()
        .await;

    match salida {
        Ok(output) => {
            if output.status.success() {
                Ok(())
            } else {
                anyhow::bail!("ffmpeg respondio con error");
            }
        }
        Err(err) => {
            if err.kind() == std::io::ErrorKind::NotFound {
                anyhow::bail!("ffmpeg no encontrado. Usa --ffmpeg-path");
            }
            anyhow::bail!("No se pudo ejecutar ffmpeg: {}", err);
        }
    }
}
