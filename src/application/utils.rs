use crate::domain::errors::DomainError;
use crate::domain::value_objects::ModelName;
use crate::infrastructure::{expandir_tilde, ChaturbateClient};
use std::path::{Path, PathBuf};

pub(crate) const FFMPEG_ENV: &str = "CBREC_FFMPEG";
pub(crate) const SESSION_COOKIE_ENV: &str = "CBREC_SESSION_COOKIE";

pub(crate) struct ParametrosGrabacion {
    pub raiz_salida: Option<PathBuf>,
    pub quality: crate::domain::value_objects::VideoQuality,
    pub limite_concurrencia: usize,
    pub min_file_size: Option<u64>,
    pub cancel_rx: tokio::sync::watch::Receiver<bool>,
    pub salida: std::sync::Arc<dyn crate::presentation::Output>,
}

pub(crate) fn resolver_ruta_opcional(ruta: Option<String>) -> Option<PathBuf> {
    ruta.map(|r| expandir_tilde(&r))
}

pub(crate) fn resolver_ffmpeg_path(ruta: Option<PathBuf>) -> PathBuf {
    if let Some(ruta) = ruta {
        return ruta;
    }

    if let Ok(ruta) = std::env::var(FFMPEG_ENV) {
        let ruta = ruta.trim();
        if !ruta.is_empty() {
            return expandir_tilde(ruta);
        }
    }

    buscar_ffmpeg_empaquetado().unwrap_or_else(|| PathBuf::from("ffmpeg"))
}

pub(crate) fn aplicar_ffmpeg_path(client: ChaturbateClient, ruta: PathBuf) -> ChaturbateClient {
    client.with_ffmpeg_path(ruta)
}

pub(crate) async fn validar_ffmpeg(ruta: &Path, requiere_existencia: bool) -> anyhow::Result<()> {
    obtener_version_ffmpeg(ruta, requiere_existencia)
        .await
        .map(|_| ())
}

pub(crate) async fn obtener_version_ffmpeg(
    ruta: &Path,
    requiere_existencia: bool,
) -> anyhow::Result<String> {
    if requiere_existencia && !ruta.exists() {
        anyhow::bail!("Ruta de ffmpeg invalida: {}", ruta.display());
    }

    let salida = tokio::process::Command::new(ruta)
        .arg("-version")
        .output()
        .await;

    match salida {
        Ok(output) => {
            if output.status.success() {
                Ok(version_ffmpeg(&output.stdout)
                    .unwrap_or_else(|| "version desconocida".to_string()))
            } else {
                let detalle = resumen_salida_ffmpeg(&output.stderr)
                    .or_else(|| resumen_salida_ffmpeg(&output.stdout));
                if let Some(detalle) = detalle {
                    anyhow::bail!("ffmpeg respondio con error: {}", detalle);
                }
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

fn buscar_ffmpeg_empaquetado() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    candidatos_ffmpeg_empaquetado(dir)
        .into_iter()
        .find(|ruta| ruta.is_file())
}

fn candidatos_ffmpeg_empaquetado(dir: &Path) -> Vec<PathBuf> {
    let nombre = nombre_binario_ffmpeg();
    vec![
        dir.join(nombre),
        dir.join("bin").join(nombre),
        dir.join("ffmpeg").join("bin").join(nombre),
    ]
}

fn nombre_binario_ffmpeg() -> &'static str {
    if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

fn resumen_salida_ffmpeg(salida: &[u8]) -> Option<String> {
    const MAX_CHARS: usize = 600;

    let texto = String::from_utf8_lossy(salida);
    let mut lineas = Vec::new();

    for linea in texto.lines() {
        let linea = linea.trim();
        if linea.is_empty() {
            continue;
        }
        lineas.push(linea.to_string());
        if lineas.len() >= 4 {
            break;
        }
    }

    let mut resumen = lineas.join(" | ");
    if resumen.is_empty() {
        return None;
    }
    if resumen.chars().count() > MAX_CHARS {
        resumen = resumen.chars().take(MAX_CHARS).collect();
        resumen.push_str("...");
    }
    Some(resumen)
}

fn version_ffmpeg(salida: &[u8]) -> Option<String> {
    const MAX_CHARS: usize = 160;

    let texto = String::from_utf8_lossy(salida);
    let linea = texto
        .lines()
        .map(str::trim)
        .find(|linea| !linea.is_empty())?;
    let mut version: String = linea.chars().take(MAX_CHARS).collect();
    if linea.chars().count() > MAX_CHARS {
        version.push_str("...");
    }
    Some(version)
}

fn is_chaturbate_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "chaturbate.com" || host.ends_with(".chaturbate.com")
}

fn extract_model_name(input: &str) -> Result<String, DomainError> {
    let input = input.trim();
    let is_http_url = input.split_once("://").is_some_and(|(scheme, _)| {
        matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https")
    });
    if !is_http_url {
        return Ok(input.to_owned());
    }

    let url = url::Url::parse(input).map_err(|_| {
        DomainError::InvalidModelName("Model URL has an invalid format".to_string())
    })?;
    let host = url
        .host_str()
        .ok_or_else(|| DomainError::InvalidModelName("Model URL must have a host".to_string()))?;
    if !is_chaturbate_host(host) {
        return Err(DomainError::InvalidModelName(
            "Model URL must use chaturbate.com".to_string(),
        ));
    }

    url.path_segments()
        .and_then(|mut segments| segments.find(|segment| !segment.is_empty()))
        .map(str::to_owned)
        .ok_or_else(|| DomainError::InvalidModelName("Model URL has no model name".to_string()))
}

pub(crate) fn normalizar_modelo(input: &str) -> Result<ModelName, DomainError> {
    let nombre = extract_model_name(input)?;
    ModelName::try_from(nombre.as_str())
}

pub(crate) fn normalizar_modelos(
    modelos: Vec<String>,
) -> Result<(Vec<ModelName>, usize), DomainError> {
    let mut vistos = std::collections::HashSet::new();
    let mut unicos = Vec::new();
    let mut duplicados = 0;

    for modelo in modelos {
        let normalizado = normalizar_modelo(&modelo)?;
        if vistos.insert(normalizado.as_str().to_string()) {
            unicos.push(normalizado);
        } else {
            duplicados += 1;
        }
    }

    Ok((unicos, duplicados))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraer_nombre_acepta_url_de_chaturbate() {
        let nombre = extract_model_name("https://chaturbate.com/_sofy_smith_/").unwrap();

        assert_eq!(nombre, "_sofy_smith_");
    }

    #[test]
    fn normalizar_modelo_acepta_url_y_lowercase() {
        let nombre = normalizar_modelo("https://chaturbate.com/Alice_123/").unwrap();

        assert_eq!(nombre.as_str(), "alice_123");
    }

    #[test]
    fn normalizar_modelo_acepta_subdominio_de_chaturbate() {
        let nombre = normalizar_modelo("HTTPS://en.chaturbate.com/Alice_123/").unwrap();

        assert_eq!(nombre.as_str(), "alice_123");
    }

    #[test]
    fn normalizar_modelos_deduplica_despues_de_normalizar() {
        let (modelos, duplicados) = normalizar_modelos(vec![
            "Alice".into(),
            "https://chaturbate.com/alice/".into(),
            "bob".into(),
        ])
        .unwrap();

        let nombres: Vec<&str> = modelos.iter().map(|m| m.as_str()).collect();
        assert_eq!(nombres, vec!["alice", "bob"]);
        assert_eq!(duplicados, 1);
    }

    #[test]
    fn normalizar_modelo_rechaza_url_sin_modelo_valido() {
        let resultado = normalizar_modelo("https://chaturbate.com/");

        assert!(resultado.is_err());
    }

    #[test]
    fn normalizar_modelo_rechaza_url_de_otro_dominio() {
        let resultado = normalizar_modelo("https://example.com/alice/");

        assert!(resultado.is_err());
    }

    #[test]
    fn normalizar_modelo_rechaza_dominio_parecido() {
        let resultado = normalizar_modelo("https://notchaturbate.com/alice/");

        assert!(resultado.is_err());
    }

    #[test]
    fn resolver_ffmpeg_path_prefiere_ruta_explicita() {
        let ruta = resolver_ffmpeg_path(Some(PathBuf::from("/tmp/ffmpeg-custom")));

        assert_eq!(ruta, PathBuf::from("/tmp/ffmpeg-custom"));
    }

    #[test]
    fn candidatos_ffmpeg_empaquetado_busca_junto_al_binario() {
        let dir = Path::new("/opt/cbrec");
        let nombre = nombre_binario_ffmpeg();
        let candidatos = candidatos_ffmpeg_empaquetado(dir);

        assert_eq!(candidatos[0], dir.join(nombre));
        assert_eq!(candidatos[1], dir.join("bin").join(nombre));
        assert_eq!(candidatos[2], dir.join("ffmpeg").join("bin").join(nombre));
    }

    #[test]
    fn resumen_salida_ffmpeg_omite_salida_vacia() {
        assert_eq!(resumen_salida_ffmpeg(b"\n  \n"), None);
    }

    #[test]
    fn resumen_salida_ffmpeg_limita_lineas() {
        let resumen = resumen_salida_ffmpeg(b"1\n2\n3\n4\n5\n").unwrap();
        assert_eq!(resumen, "1 | 2 | 3 | 4");
    }

    #[test]
    fn version_ffmpeg_usa_primera_linea() {
        let version = version_ffmpeg(b"ffmpeg version 6.1 Copyright\nbuilt with gcc\n").unwrap();

        assert_eq!(version, "ffmpeg version 6.1 Copyright");
    }

    #[test]
    fn version_ffmpeg_omite_salida_vacia() {
        assert_eq!(version_ffmpeg(b"\n\n"), None);
    }
}
