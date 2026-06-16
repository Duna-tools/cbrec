use crate::application::utils::{normalizar_modelo, obtener_version_ffmpeg};
use crate::infrastructure::{AppConfig, WatchedModels};
use crate::presentation::Output;
use std::path::Path;

pub(crate) async fn ejecutar_doctor(
    config: &AppConfig,
    ffmpeg_path: &Path,
    ffmpeg_explicito: bool,
    raiz_salida: Option<std::path::PathBuf>,
    salida: &dyn Output,
) -> anyhow::Result<()> {
    let mut fallos = 0usize;
    let mut advertencias = 0usize;

    salida.doctor_inicio();

    match obtener_version_ffmpeg(ffmpeg_path, ffmpeg_explicito).await {
        Ok(version) => salida.doctor_ok(
            "ffmpeg",
            &format!("{} ({})", ffmpeg_path.display(), version),
        ),
        Err(e) => {
            fallos += 1;
            salida.doctor_error("ffmpeg", &e.to_string());
        }
    }

    match probar_salida(config, raiz_salida.as_deref()).await {
        Ok(ruta) => salida.doctor_ok("salida", &format!("escribible: {}", ruta.display())),
        Err(e) => {
            fallos += 1;
            salida.doctor_error("salida", &e.to_string());
        }
    }

    let ejemplo = config.get_output_path("alice", raiz_salida.as_deref());
    if ejemplo.file_name().is_some() {
        salida.doctor_ok("nombres", &format!("ejemplo: {}", ejemplo.display()));
    } else {
        fallos += 1;
        salida.doctor_error(
            "nombres",
            "la plantilla no produce un nombre de archivo valido",
        );
    }

    let watched = WatchedModels::load_with_warnings();
    advertencias += watched.warnings.len();
    for warning in watched.warnings {
        salida.doctor_warn("watchlist", &warning.to_string());
    }

    let mut vistos = std::collections::HashSet::new();
    let mut invalidos = 0usize;
    let mut duplicados = 0usize;
    for modelo in &watched.watched.models {
        match normalizar_modelo(modelo) {
            Ok(modelo) if vistos.insert(modelo.as_str().to_string()) => {}
            Ok(modelo) => {
                duplicados += 1;
                salida.doctor_warn("watchlist", &format!("duplicado: {}", modelo.as_str()));
            }
            Err(e) => {
                invalidos += 1;
                salida.doctor_warn("watchlist", &format!("modelo invalido '{}': {}", modelo, e));
            }
        }
    }
    advertencias += invalidos + duplicados;

    if invalidos == 0 && duplicados == 0 {
        salida.doctor_ok(
            "watchlist",
            &format!("{} modelo(s) guardado(s)", watched.watched.models.len()),
        );
    }

    if config.auth.session_cookie.is_some() {
        salida.doctor_ok("auth", "cookie configurada");
    } else {
        salida.doctor_warn("auth", "sin cookie de sesion configurada");
        advertencias += 1;
    }

    salida.doctor_resumen(fallos, advertencias);

    if fallos > 0 {
        anyhow::bail!("doctor encontro {} fallo(s)", fallos);
    }

    Ok(())
}

async fn probar_salida(
    config: &AppConfig,
    raiz_salida: Option<&Path>,
) -> anyhow::Result<std::path::PathBuf> {
    let ruta = config.get_output_path("alice", raiz_salida);
    let dir = ruta
        .parent()
        .ok_or_else(|| anyhow::anyhow!("ruta de salida sin directorio padre"))?;
    tokio::fs::create_dir_all(dir).await?;
    let probe = dir.join(".cbrec_doctor_write_test");
    tokio::fs::write(&probe, b"ok").await?;
    tokio::fs::remove_file(&probe).await?;
    Ok(dir.to_path_buf())
}
