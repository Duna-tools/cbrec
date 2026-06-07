use crate::application::commands::{add, check, list, record, remove};
use crate::application::utils::{
    aplicar_ffmpeg_path, extraer_nombre, resolver_ruta_opcional, validar_ffmpeg,
    ParametrosGrabacion,
};
use crate::application::watch_service;
use crate::domain::value_objects::{ModelName, VideoQuality};
use crate::infrastructure::{AppConfig, ChaturbateClient, WatchedModels};
use crate::presentation::{Cli, Commands, ConsoleOutput, Output};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::watch;

const LIMITE_CONCURRENCIA_DEFECTO: usize = 3;

pub async fn ejecutar_cli(
    cli: Cli,
    mut config: AppConfig,
    client: ChaturbateClient,
) -> anyhow::Result<()> {
    let Cli {
        modelos: modelos_principales,
        verificar,
        listar,
        output: salida_principal,
        quality: calidad_principal,
        jobs,
        ffmpeg_path,
        session_cookie: cookie_cli,
        quiet,
        verbose,
        command,
    } = cli;
    let salida: Arc<dyn Output> = Arc::new(ConsoleOutput::new(verbose, quiet));

    let session_cookie_final = cookie_cli.or_else(|| config.auth.session_cookie.clone());

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

    let client = if let Some(cookie) = session_cookie_final {
        client.with_session_cookie(cookie)
    } else {
        client
    };
    let client = client.with_cancel_receiver(cancel_rx);

    match command {
        Some(Commands::Record {
            modelos,
            output,
            quality,
        }) => {
            validar_ffmpeg(ruta_ffmpeg.as_deref()).await?;
            let client = aplicar_ffmpeg_path(client, ruta_ffmpeg);
            let v_quality = VideoQuality::from_str(&quality).map_err(|e| anyhow::anyhow!(e))?;
            let parametros = ParametrosGrabacion {
                raiz_salida: resolver_ruta_opcional(output),
                quality: v_quality,
                limite_concurrencia,
                cancel_rx: cancel_rx_worker,
                salida: Arc::clone(&salida),
            };
            record::grabar_modelos(client, config, modelos, parametros).await
        }
        Some(Commands::Check { model }) => {
            check::verificar_modelo(&client, salida.as_ref(), &model).await
        }
        Some(Commands::Watch {
            modelos,
            ask,
            timeout,
            output,
            quality,
        }) => {
            // Override timeout from CLI si se especificó
            if let Some(t) = timeout {
                config.watch.ask_timeout_secs = t;
            }

            // Normalizar nombres (extrae username de URLs)
            let nombres: Vec<String> = if modelos.is_empty() {
                let watched = WatchedModels::load();
                if watched.models.is_empty() {
                    anyhow::bail!(
                        "Sin modelos. Usa 'cbrec add <modelo>' o especifica modelos en el comando."
                    );
                }
                watched.models
            } else {
                modelos.iter().map(|m| extraer_nombre(m)).collect()
            };

            // Validar todos los nombres antes de guardar en watched.toml
            let modelos_vobj = nombres
                .iter()
                .map(|m| ModelName::try_from(m.as_str()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| anyhow::anyhow!(e))?;

            // Guardar solo si vienen de CLI (no de watched.toml)
            if !modelos.is_empty() {
                let mut watched = WatchedModels::load();
                let cambio = modelos_vobj
                    .iter()
                    .fold(false, |acc, m| watched.add(m.as_str()) || acc);
                if cambio {
                    if let Err(e) = watched.save() {
                        eprintln!("[WARN] No se pudo guardar lista de modelos: {}", e);
                    }
                }
            }

            let v_quality = VideoQuality::from_str(&quality).map_err(|e| anyhow::anyhow!(e))?;
            let raiz_salida = resolver_ruta_opcional(output);

            validar_ffmpeg(ruta_ffmpeg.as_deref()).await?;
            let client = aplicar_ffmpeg_path(client, ruta_ffmpeg);

            watch_service::ejecutar_watch(
                Arc::new(client),
                Arc::new(config),
                modelos_vobj,
                ask,
                raiz_salida,
                v_quality,
                cancel_rx_worker,
                salida,
            )
            .await
        }
        Some(Commands::Add { models }) => add::agregar_modelos(models),
        Some(Commands::Remove { models }) => remove::eliminar_modelos(models),
        None => {
            if modelos_principales.is_empty() {
                salida.mostrar_error_sin_modelo();
                std::process::exit(1);
            }

            validar_ffmpeg(ruta_ffmpeg.as_deref()).await?;
            let client = aplicar_ffmpeg_path(client, ruta_ffmpeg);

            if listar {
                list::listar_calidades_modelos(&client, salida.as_ref(), modelos_principales).await
            } else if verificar {
                check::verificar_modelos(&client, salida.as_ref(), modelos_principales).await
            } else {
                let v_quality =
                    VideoQuality::from_str(&calidad_principal).map_err(|e| anyhow::anyhow!(e))?;
                let parametros = ParametrosGrabacion {
                    raiz_salida: resolver_ruta_opcional(salida_principal),
                    quality: v_quality,
                    limite_concurrencia,
                    cancel_rx: cancel_rx_worker,
                    salida: Arc::clone(&salida),
                };
                record::grabar_modelos(client, config, modelos_principales, parametros).await
            }
        }
    }
}
