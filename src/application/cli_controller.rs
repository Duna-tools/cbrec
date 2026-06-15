use crate::application::commands::{add, check, list, record, remove};
use crate::application::utils::{
    aplicar_ffmpeg_path, extraer_nombre, resolver_ffmpeg_path, resolver_ruta_opcional,
    validar_ffmpeg, ParametrosGrabacion, FFMPEG_ENV,
};
use crate::application::watch_service::{self, ConsoleWatchPrompter, WatchParams};
use crate::domain::value_objects::{ModelName, VideoQuality};
use crate::infrastructure::{AppConfig, ChaturbateClient, ConfigWarning, WatchedModels};
use crate::presentation::{Cli, Commands, ConsoleOutput, Output};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::watch;

const LIMITE_CONCURRENCIA_DEFECTO: usize = 3;

pub async fn ejecutar_cli(
    cli: Cli,
    mut config: AppConfig,
    config_warnings: Vec<ConfigWarning>,
    client: ChaturbateClient,
) -> anyhow::Result<()> {
    let Cli {
        modelos: modelos_principales,
        verificar,
        listar,
        output: salida_principal,
        quality: calidad_principal,
        jobs,
        duration,
        ffmpeg_path,
        session_cookie: cookie_cli,
        quiet,
        verbose,
        command,
    } = cli;
    let salida: Arc<dyn Output> = Arc::new(ConsoleOutput::new(verbose, quiet));
    mostrar_config_warnings(salida.as_ref(), &config_warnings);

    let session_cookie_final = cookie_cli.or_else(|| config.auth.session_cookie.clone());

    if jobs == Some(0) {
        anyhow::bail!("El limite de concurrencia debe ser mayor a 0");
    }
    if duration == Some(0) {
        anyhow::bail!("La duracion debe ser mayor a 0");
    }
    if let Some(jobs) = jobs {
        if jobs > LIMITE_CONCURRENCIA_DEFECTO {
            salida.advertir_limite_concurrencia(LIMITE_CONCURRENCIA_DEFECTO, jobs);
        }
    }

    let ruta_ffmpeg_cli = resolver_ruta_opcional(ffmpeg_path);
    let ffmpeg_env_explicito = std::env::var(FFMPEG_ENV)
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    let ffmpeg_explicito = ruta_ffmpeg_cli.is_some() || ffmpeg_env_explicito;
    let ruta_ffmpeg = resolver_ffmpeg_path(ruta_ffmpeg_cli);
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
    let client = if let Some(duration) = duration {
        client.with_max_duration_secs(duration)
    } else {
        client
    };
    let client = client.with_cancel_receiver(cancel_rx);
    let min_file_size = if duration.is_some() {
        None
    } else {
        Some(config.min_file_size)
    };

    match command {
        Some(Commands::Record {
            modelos,
            output,
            quality,
        }) => {
            let limite_concurrencia = jobs.unwrap_or(LIMITE_CONCURRENCIA_DEFECTO);
            validar_ffmpeg(&ruta_ffmpeg, ffmpeg_explicito).await?;
            let client = aplicar_ffmpeg_path(client, ruta_ffmpeg);
            let v_quality = VideoQuality::from_str(&quality).map_err(|e| anyhow::anyhow!(e))?;
            let parametros = ParametrosGrabacion {
                raiz_salida: resolver_ruta_opcional(output),
                quality: v_quality,
                limite_concurrencia,
                min_file_size,
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
            let limite_concurrencia = jobs.unwrap_or(config.watch.max_simultaneous);
            // Override timeout from CLI si se especificó
            if let Some(t) = timeout {
                config.watch.ask_timeout_secs = t;
            }

            // Normalizar nombres (extrae username de URLs)
            let nombres: Vec<String> = if modelos.is_empty() {
                let watched = WatchedModels::load_with_warnings();
                mostrar_config_warnings(salida.as_ref(), &watched.warnings);
                if watched.watched.models.is_empty() {
                    anyhow::bail!(
                        "Sin modelos. Usa 'cbrec add <modelo>' o especifica modelos en el comando."
                    );
                }
                watched.watched.models
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
                let resultado = WatchedModels::update(|watched| {
                    let mut cambio = false;
                    for m in &modelos_vobj {
                        cambio |= watched.add(m.as_str());
                    }
                    ((), cambio)
                });
                if let Err(e) = resultado {
                    salida.advertir_no_se_pudo_guardar_lista(&e.to_string());
                }
            }

            let v_quality = VideoQuality::from_str(&quality).map_err(|e| anyhow::anyhow!(e))?;
            let raiz_salida = resolver_ruta_opcional(output);

            validar_ffmpeg(&ruta_ffmpeg, ffmpeg_explicito).await?;
            let client = aplicar_ffmpeg_path(client, ruta_ffmpeg);

            watch_service::ejecutar_watch(WatchParams {
                client: Arc::new(client),
                config: Arc::new(config),
                modelos: modelos_vobj,
                ask,
                raiz_salida,
                quality: v_quality,
                limite_concurrencia,
                min_file_size,
                cancel_rx: cancel_rx_worker,
                salida,
                prompter: Arc::new(ConsoleWatchPrompter),
            })
            .await
        }
        Some(Commands::Add { models }) => add::agregar_modelos(models, salida.as_ref()),
        Some(Commands::Remove { models }) => remove::eliminar_modelos(models, salida.as_ref()),
        None => {
            if modelos_principales.is_empty() {
                salida.mostrar_error_sin_modelo();
                anyhow::bail!("sin modelo");
            }

            let limite_concurrencia = jobs.unwrap_or(LIMITE_CONCURRENCIA_DEFECTO);
            validar_ffmpeg(&ruta_ffmpeg, ffmpeg_explicito).await?;
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
                    min_file_size,
                    cancel_rx: cancel_rx_worker,
                    salida: Arc::clone(&salida),
                };
                record::grabar_modelos(client, config, modelos_principales, parametros).await
            }
        }
    }
}

fn mostrar_config_warnings(salida: &dyn Output, warnings: &[ConfigWarning]) {
    for warning in warnings {
        salida.advertir_config(&warning.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[tokio::test]
    async fn ejecutar_cli_sin_modelos_devuelve_error_sin_salir_del_proceso() {
        let cli = Cli::parse_from(["cbrec"]);
        let client = ChaturbateClient::new().expect("crea cliente");

        let resultado = ejecutar_cli(cli, AppConfig::default(), Vec::new(), client).await;

        let error = resultado.expect_err("sin modelos debe fallar");
        assert_eq!(error.to_string(), "sin modelo");
    }

    #[tokio::test]
    async fn ejecutar_cli_rechaza_duracion_cero() {
        let cli = Cli::parse_from(["cbrec", "--duration", "0", "alice"]);
        let client = ChaturbateClient::new().expect("crea cliente");

        let resultado = ejecutar_cli(cli, AppConfig::default(), Vec::new(), client).await;

        let error = resultado.expect_err("duracion cero debe fallar");
        assert_eq!(error.to_string(), "La duracion debe ser mayor a 0");
    }
}
