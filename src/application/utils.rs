use crate::infrastructure::{expandir_tilde, ChaturbateClient};
use std::path::{Path, PathBuf};

pub(crate) struct ParametrosGrabacion {
    pub raiz_salida: Option<PathBuf>,
    pub quality: crate::domain::value_objects::VideoQuality,
    pub limite_concurrencia: usize,
    pub cancel_rx: tokio::sync::watch::Receiver<bool>,
    pub salida: std::sync::Arc<dyn crate::presentation::Output>,
}

pub(crate) fn resolver_ruta_opcional(ruta: Option<String>) -> Option<PathBuf> {
    ruta.map(|r| expandir_tilde(&r))
}

pub(crate) fn aplicar_ffmpeg_path(
    client: ChaturbateClient,
    ruta: Option<PathBuf>,
) -> ChaturbateClient {
    match ruta {
        Some(path) => client.with_ffmpeg_path(path),
        None => client,
    }
}

pub(crate) async fn validar_ffmpeg(ruta: Option<&Path>) -> anyhow::Result<()> {
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

pub(crate) fn deduplicar_modelos(modelos: Vec<String>) -> (Vec<String>, usize) {
    let mut vistos = std::collections::HashSet::new();
    let mut unicos = Vec::new();
    let mut duplicados = 0;

    for modelo in modelos {
        let clave = modelo.trim().to_lowercase();
        if clave.is_empty() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deduplicar_descarta_vacios() {
        let (unicos, dups) = deduplicar_modelos(vec!["".into(), "alice".into()]);
        assert_eq!(unicos, vec!["alice"]);
        assert_eq!(dups, 0);
    }

    #[test]
    fn deduplicar_cuenta_duplicados_ignora_vacios() {
        let (unicos, dups) = deduplicar_modelos(vec!["alice".into(), "".into(), "alice".into()]);
        assert_eq!(unicos, vec!["alice"]);
        assert_eq!(dups, 1);
    }

    #[test]
    fn deduplicar_case_insensitive() {
        let (unicos, dups) = deduplicar_modelos(vec!["Alice".into(), "alice".into()]);
        assert_eq!(unicos.len(), 1);
        assert_eq!(dups, 1);
    }
}
