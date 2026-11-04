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

pub(crate) async fn validar_ffmpeg(
    ruta: &Path,
    requiere_existencia: bool,
    cancel_rx: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<()> {
    obtener_version_ffmpeg(ruta, requiere_existencia, cancel_rx)
        .await
        .map(|_| ())
}

pub(crate) async fn obtener_version_ffmpeg(
    ruta: &Path,
    requiere_existencia: bool,
    mut cancel_rx: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<String> {
    if requiere_existencia && !ruta.exists() {
        anyhow::bail!("Ruta de ffmpeg invalida: {}", ruta.display());
    }

    if *cancel_rx.borrow() {
        anyhow::bail!("validacion de FFmpeg cancelada");
    }
    let mut child = tokio::process::Command::new(ruta)
        .env_remove(SESSION_COOKIE_ENV)
        .kill_on_drop(true)
        .arg("-version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!("ffmpeg no encontrado. Usa --ffmpeg-path")
            } else {
                anyhow::anyhow!("No se pudo ejecutar ffmpeg: {error}")
            }
        })?;
    let stdout = child.stdout.take().expect("stdout configurado");
    let stderr = child.stderr.take().expect("stderr configurado");
    let result = tokio::select! {
        result = async { tokio::try_join!(child.wait(), read_process_output(stdout), read_process_output(stderr)) } => result.map_err(anyhow::Error::from),
        _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => Err(anyhow::anyhow!("La validacion de FFmpeg excedio cinco segundos")),
        _ = cancel_rx.wait_for(|cancelled| *cancelled) => Err(anyhow::anyhow!("validacion de FFmpeg cancelada")),
    };
    let (status, stdout, stderr) = match result {
        Ok(output) => output,
        Err(error) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(error);
        }
    };
    if !status.success() {
        if let Some(detail) =
            resumen_salida_ffmpeg(&stderr).or_else(|| resumen_salida_ffmpeg(&stdout))
        {
            anyhow::bail!("ffmpeg respondio con error: {detail}");
        }
        anyhow::bail!("ffmpeg respondio con error");
    }
    Ok(version_ffmpeg(&stdout).unwrap_or_else(|| "version desconocida".to_string()))
}

async fn read_process_output(
    mut reader: impl tokio::io::AsyncRead + Unpin,
) -> std::io::Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    const LIMIT: usize = 64 * 1024;
    let mut retained = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            return Ok(retained);
        }
        let keep = count.min(LIMIT.saturating_sub(retained.len()));
        retained.extend_from_slice(&buffer[..keep]);
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

pub(crate) fn is_chaturbate_host(host: &str) -> bool {
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

    #[tokio::test]
    async fn process_output_is_drained_but_retained_bytes_are_bounded() {
        let data = vec![b'x'; 200_000];
        let mut reader = data.as_slice();
        let output = read_process_output(&mut reader).await.unwrap();
        assert_eq!(output.len(), 64 * 1024);
        assert!(reader.is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ffmpeg_probe_cancellation_reaps_the_child() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!(
            "cbrec_cancel_probe_{}_{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::write(&path, "#!/bin/sh\necho $$ > \"$0.pid\"\nexec sleep 6\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let marker = PathBuf::from(format!("{}.pid", path.display()));
        let (sender, receiver) = tokio::sync::watch::channel(false);
        let operation = obtener_version_ffmpeg(&path, true, receiver);
        tokio::pin!(operation);
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !marker.exists() {
                tokio::select! {
                    result = &mut operation => panic!("el proceso termino antes de cancelar: {result:?}"),
                    _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {}
                }
            }
        }).await.unwrap();
        let pid = std::fs::read_to_string(&marker).unwrap();
        sender.send(true).unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), &mut operation)
            .await
            .unwrap();
        assert!(result.unwrap_err().to_string().contains("cancelada"));
        #[cfg(target_os = "linux")]
        assert!(!Path::new("/proc").join(pid.trim()).exists());
        #[cfg(not(target_os = "linux"))]
        let _ = pid;
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(marker).unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn ffmpeg_probe_has_a_deadline() {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::temp_dir().join(format!(
            "cbrec_probe_{}_{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::write(&path, "#!/bin/sh\nexec sleep 6\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        let (_sender, receiver) = tokio::sync::watch::channel(false);
        let result = obtener_version_ffmpeg(&path, true, receiver).await;
        std::fs::remove_file(&path).unwrap();
        assert!(
            result.is_err(),
            "la validacion debe agotar su plazo antes de seis segundos"
        );
    }

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
