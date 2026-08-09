//! Recepción efímera y local de una sesión enviada desde un userscript.
//!
//! El receptor solo acepta una petición `POST` con un código criptográficamente
//! aleatorio y conserva la cookie exclusivamente durante el proceso actual.

use anyhow::{anyhow, bail, ensure, Context};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{timeout, Duration, Instant};

const RECEIVE_TIMEOUT: Duration = Duration::from_secs(120);
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_HEADER_BYTES: usize = 4_096;
const MAX_BODY_BYTES: usize = 8_192;
const MAX_INVALID_REQUESTS: usize = 3;
const SESSION_PATH_PREFIX: &str = "/cbrec-session/";
const ALLOWED_COOKIE_NAMES: [&str; 2] = ["PHPSESSID", "chaturbatesid"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrowserSessionPayload {
    session_cookie: String,
}

/// Espera una cookie de sesión desde el userscript de Violentmonkey.
///
/// La dirección mostrada contiene un código de un solo uso y solo acepta
/// conexiones a `127.0.0.1`. La cookie resultante no se escribe en disco.
pub async fn receive_browser_session() -> anyhow::Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("No se pudo abrir el receptor local de sesión")?;
    let address = listener
        .local_addr()
        .context("No se pudo obtener la dirección del receptor local")?;
    let code = generate_code()?;
    let path = format!("{SESSION_PATH_PREFIX}{code}");

    eprintln!("Abre Chaturbate y ejecuta «Enviar sesión a cbrec» en Violentmonkey.");
    eprintln!("Pega esta dirección en el diálogo del userscript:");
    eprintln!("http://127.0.0.1:{}{}", address.port(), path);
    eprintln!(
        "La recepción caduca en {} segundos.",
        RECEIVE_TIMEOUT.as_secs()
    );

    let deadline = Instant::now() + RECEIVE_TIMEOUT;
    let mut invalid_requests = 0;

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            bail!("Se agotó el tiempo para recibir la sesión del navegador");
        }

        let (mut stream, _) = timeout(remaining, listener.accept())
            .await
            .map_err(|_| anyhow!("Se agotó el tiempo para recibir la sesión del navegador"))?
            .context("No se pudo aceptar la conexión local")?;

        match timeout(CONNECTION_TIMEOUT, receive_request(&mut stream, &path)).await {
            Ok(Ok(cookie)) => {
                let _ = write_response(&mut stream, "204 No Content").await;
                eprintln!("Sesión recibida para esta ejecución; no se guardó en disco.");
                return Ok(cookie);
            }
            Ok(Err(_)) => {
                let _ = write_response(&mut stream, "400 Bad Request").await;
            }
            Err(_) => {
                let _ = write_response(&mut stream, "408 Request Timeout").await;
            }
        }

        invalid_requests += 1;
        if invalid_requests >= MAX_INVALID_REQUESTS {
            bail!("Se recibieron demasiadas solicitudes locales inválidas");
        }
    }
}

fn generate_code() -> anyhow::Result<String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes)
        .map_err(|error| anyhow!("No se pudo generar el código temporal de sesión: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

async fn receive_request(stream: &mut TcpStream, expected_path: &str) -> anyhow::Result<String> {
    let request = read_request(stream).await?;
    parse_session_request(&request, expected_path)
}

async fn read_request(stream: &mut TcpStream) -> anyhow::Result<Vec<u8>> {
    let mut request = Vec::with_capacity(MAX_HEADER_BYTES);
    let mut chunk = [0_u8; 1_024];

    loop {
        let read = stream.read(&mut chunk).await?;
        ensure!(read > 0, "La solicitud local terminó antes de completarse");
        request.extend_from_slice(&chunk[..read]);

        let Some(header_end) = header_end(&request) else {
            ensure!(
                request.len() <= MAX_HEADER_BYTES,
                "Los encabezados exceden el límite"
            );
            continue;
        };

        let content_length = content_length(&request[..header_end])?;
        ensure!(
            content_length <= MAX_BODY_BYTES,
            "El cuerpo excede el límite"
        );
        let expected_len = header_end + content_length;
        ensure!(
            expected_len <= MAX_HEADER_BYTES + MAX_BODY_BYTES,
            "La solicitud excede el límite"
        );

        while request.len() < expected_len {
            let read = stream.read(&mut chunk).await?;
            ensure!(read > 0, "La solicitud local terminó antes de completarse");
            request.extend_from_slice(&chunk[..read]);
            ensure!(
                request.len() <= expected_len,
                "La solicitud contiene datos adicionales"
            );
        }

        return Ok(request);
    }
}

fn parse_session_request(request: &[u8], expected_path: &str) -> anyhow::Result<String> {
    let header_end = header_end(request).ok_or_else(|| anyhow!("Faltan encabezados HTTP"))?;
    let header = std::str::from_utf8(&request[..header_end - 4])
        .context("Los encabezados HTTP no son texto válido")?;
    let mut lines = header.split("\r\n");
    let request_line = lines.next().ok_or_else(|| anyhow!("Falta la línea HTTP"))?;
    ensure!(
        request_line == format!("POST {expected_path} HTTP/1.1"),
        "La ruta o el método HTTP no son válidos"
    );

    let content_length = content_length(&request[..header_end])?;
    let body = &request[header_end..];
    ensure!(
        body.len() == content_length,
        "El cuerpo HTTP está incompleto"
    );

    let payload: BrowserSessionPayload =
        serde_json::from_slice(body).context("El cuerpo no contiene una sesión válida")?;
    normalize_session_cookie(&payload.session_cookie)
}

fn header_end(request: &[u8]) -> Option<usize> {
    request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
}

fn content_length(header_bytes: &[u8]) -> anyhow::Result<usize> {
    let header =
        std::str::from_utf8(header_bytes).context("Los encabezados HTTP no son texto válido")?;
    let mut lengths = header
        .split("\r\n")
        .skip(1)
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| value.trim().parse::<usize>());

    let length = lengths
        .next()
        .ok_or_else(|| anyhow!("Falta Content-Length"))?;
    let length = length.context("Content-Length no es válido")?;
    ensure!(lengths.next().is_none(), "Content-Length está repetido");
    Ok(length)
}

fn normalize_session_cookie(cookie: &str) -> anyhow::Result<String> {
    let mut normalized = Vec::new();

    for item in cookie.split(';') {
        let (name, value) = item
            .trim()
            .split_once('=')
            .ok_or_else(|| anyhow!("La cookie no tiene el formato nombre=valor"))?;
        ensure!(
            ALLOWED_COOKIE_NAMES.contains(&name),
            "La cookie no está permitida"
        );
        ensure!(
            value
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && byte != b';'),
            "El valor de la cookie no es válido"
        );
        ensure!(
            !normalized
                .iter()
                .any(|entry: &String| entry.starts_with(&format!("{name}="))),
            "La cookie está repetida"
        );
        normalized.push(format!("{name}={value}"));
    }

    ensure!(!normalized.is_empty(), "No se recibió ninguna cookie");
    Ok(normalized.join("; "))
}

async fn write_response(stream: &mut TcpStream, status: &str) -> std::io::Result<()> {
    stream
        .write_all(
            format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await?;
    stream.shutdown().await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(path: &str, body: &str) -> Vec<u8> {
        format!(
            "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    #[test]
    fn parse_session_request_accepts_only_allowed_cookie_names() {
        let body = r#"{"session_cookie":"PHPSESSID=abc; chaturbatesid=xyz"}"#;
        let result =
            parse_session_request(&request("/cbrec-session/code", body), "/cbrec-session/code");

        assert_eq!(result.unwrap(), "PHPSESSID=abc; chaturbatesid=xyz");
    }

    #[test]
    fn parse_session_request_rejects_an_unexpected_cookie() {
        let body = r#"{"session_cookie":"PHPSESSID=abc; tracking=value"}"#;
        let result =
            parse_session_request(&request("/cbrec-session/code", body), "/cbrec-session/code");

        assert!(result.is_err());
    }

    #[test]
    fn parse_session_request_rejects_an_incorrect_code() {
        let body = r#"{"session_cookie":"PHPSESSID=abc"}"#;
        let result = parse_session_request(
            &request("/cbrec-session/other", body),
            "/cbrec-session/code",
        );

        assert!(result.is_err());
    }
}
