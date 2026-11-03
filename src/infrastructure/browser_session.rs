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
const ALLOWED_COOKIE_NAMES: [&str; 3] = ["sessionid", "PHPSESSID", "chaturbatesid"];

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
    let cookie = receive_browser_session_with_prompt(|url| {
        eprintln!("Abre Chaturbate y ejecuta «Enviar sesión a cbrec» en Violentmonkey.");
        eprintln!("Pega esta dirección en el diálogo del userscript:");
        eprintln!("{url}");
        eprintln!(
            "La recepción caduca en {} segundos.",
            RECEIVE_TIMEOUT.as_secs()
        );
    })
    .await?;
    eprintln!("Sesión recibida para esta ejecución; no se guardó en disco.");
    Ok(cookie)
}

pub(crate) async fn receive_browser_session_with_prompt(
    prompt: impl FnOnce(&str),
) -> anyhow::Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("No se pudo abrir el receptor local de sesión")?;
    let address = listener
        .local_addr()
        .context("No se pudo obtener la dirección del receptor local")?;
    let code = generate_code()?;
    let path = format!("{SESSION_PATH_PREFIX}{code}");

    prompt(&format!("http://127.0.0.1:{}{}", address.port(), path));

    receive_session(listener, &path, Instant::now() + RECEIVE_TIMEOUT).await
}

async fn receive_session(
    listener: TcpListener,
    path: &str,
    deadline: Instant,
) -> anyhow::Result<String> {
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

        match tokio::time::timeout_at(
            deadline.min(Instant::now() + CONNECTION_TIMEOUT),
            receive_request(&mut stream, path),
        )
        .await
        {
            Ok(Ok(cookie)) => {
                let _ = tokio::time::timeout_at(
                    deadline,
                    write_response(&mut stream, "204 No Content"),
                )
                .await;
                return Ok(cookie);
            }
            Ok(Err(_)) => {
                let _ = tokio::time::timeout_at(
                    deadline,
                    write_response(&mut stream, "400 Bad Request"),
                )
                .await;
            }
            Err(_) => {
                let _ = tokio::time::timeout_at(
                    deadline,
                    write_response(&mut stream, "408 Request Timeout"),
                )
                .await;
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

        ensure!(
            header_end <= MAX_HEADER_BYTES,
            "Los encabezados exceden el límite"
        );
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
    ensure!(
        !header
            .split("\r\n")
            .skip(1)
            .filter_map(|line| line.split_once(':'))
            .any(|(name, _)| name.eq_ignore_ascii_case("transfer-encoding")),
        "Transfer-Encoding no está permitido"
    );
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
        ensure!(!value.is_empty(), "El valor de la cookie está vacío");
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
        let body = r#"{"session_cookie":"sessionid=current; PHPSESSID=abc; chaturbatesid=xyz"}"#;
        let result =
            parse_session_request(&request("/cbrec-session/code", body), "/cbrec-session/code");

        assert_eq!(
            result.unwrap(),
            "sessionid=current; PHPSESSID=abc; chaturbatesid=xyz"
        );
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
    #[tokio::test]
    async fn receiver_rejects_large_headers_even_with_complete_body() {
        let body = r#"{"session_cookie":"sessionid=abc"}"#;
        let raw = format!(
            "POST /code HTTP/1.1\r\nX-Padding: {}\r\nContent-Length: {}\r\n\r\n{body}",
            "x".repeat(MAX_HEADER_BYTES),
            body.len()
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut sender = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        sender.write_all(raw.as_bytes()).await.unwrap();
        sender.shutdown().await.unwrap();
        let (mut stream, _) = listener.accept().await.unwrap();
        assert!(receive_request(&mut stream, "/code").await.is_err());
    }

    #[test]
    fn session_rejects_empty_cookie_values() {
        assert!(normalize_session_cookie("sessionid=").is_err());
    }

    #[test]
    fn session_rejects_ambiguous_http_framing() {
        let raw = b"Content-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n";
        let header = [b"POST /code HTTP/1.1\r\n".as_slice(), raw].concat();
        assert!(content_length(&header).is_err());
    }
    #[tokio::test]
    async fn receiver_recovers_after_wrong_code_and_accepts_session_once() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let receiver = tokio::spawn(receive_session(
            listener,
            "/code",
            Instant::now() + Duration::from_secs(2),
        ));
        for (path, expected) in [("/wrong", "400 Bad Request"), ("/code", "204 No Content")] {
            let mut stream = TcpStream::connect(address).await.unwrap();
            let raw = request(path, r#"{"session_cookie":"sessionid=abc"}"#);
            for chunk in raw.chunks(7) {
                stream.write_all(chunk).await.unwrap();
                tokio::task::yield_now().await;
            }
            let mut response = String::new();
            stream.read_to_string(&mut response).await.unwrap();
            assert!(response.starts_with(&format!("HTTP/1.1 {expected}")));
        }
        assert_eq!(receiver.await.unwrap().unwrap(), "sessionid=abc");
        assert!(TcpStream::connect(address).await.is_err());
    }

    #[tokio::test]
    async fn receiver_bounds_invalid_attempts_and_total_wait() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let receiver = tokio::spawn(receive_session(
            listener,
            "/code",
            Instant::now() + Duration::from_secs(2),
        ));
        for _ in 0..MAX_INVALID_REQUESTS {
            let mut stream = TcpStream::connect(address).await.unwrap();
            stream.write_all(&request("/wrong", "{}")).await.unwrap();
            let mut response = Vec::new();
            stream.read_to_end(&mut response).await.unwrap();
            assert!(response.starts_with(b"HTTP/1.1 400"));
        }
        assert!(receiver
            .await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("demasiadas"));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let _stalled = TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let result = timeout(
            Duration::from_secs(1),
            receive_session(
                listener,
                "/code",
                Instant::now() + Duration::from_millis(30),
            ),
        )
        .await;
        assert!(result.unwrap().is_err());
    }

    #[test]
    fn session_parser_rejects_malformed_and_conflicting_input() {
        for cookie in [
            "",
            "sessionid",
            "tracking=abc",
            "sessionid=a; sessionid=b",
            "sessionid=a b",
            "sessionid=a\r\nInjected:x",
        ] {
            assert!(normalize_session_cookie(cookie).is_err(), "{cookie:?}");
        }
        for header in [
            b"POST / HTTP/1.1\r\n\r\n".as_slice(),
            b"POST / HTTP/1.1\r\nContent-Length: x\r\n\r\n",
            b"POST / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\n",
            b"\xff",
        ] {
            assert!(content_length(header).is_err());
        }
        for body in [
            r#"{"session_cookie":"sessionid=abc","extra":1}"#,
            "{",
            r#"{"session_cookie":42}"#,
        ] {
            assert!(parse_session_request(&request("/code", body), "/code").is_err());
        }
        let code = generate_code().unwrap();
        assert_eq!(code.len(), 32);
        assert!(code.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[tokio::test]
    async fn receiver_rejects_oversized_truncated_and_extra_body() {
        let cases = [
            b"POST /code HTTP/1.1\r\nContent-Length: 8193\r\n\r\n".to_vec(),
            b"POST /code HTTP/1.1\r\nContent-Length: 4\r\n\r\n{}".to_vec(),
            request("/code", r#"{"session_cookie":"sessionid=abc"}"#)
                .into_iter()
                .chain(b"extra".iter().copied())
                .collect(),
        ];
        for raw in cases {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let mut sender = TcpStream::connect(listener.local_addr().unwrap())
                .await
                .unwrap();
            sender.write_all(&raw).await.unwrap();
            sender.shutdown().await.unwrap();
            let (mut stream, _) = listener.accept().await.unwrap();
            assert!(receive_request(&mut stream, "/code").await.is_err());
        }
    }
    #[tokio::test]
    async fn late_connection_cannot_extend_the_session_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let receiver = tokio::spawn(receive_session(
            listener,
            "/code",
            Instant::now() + Duration::from_millis(300),
        ));
        tokio::time::sleep(Duration::from_millis(200)).await;
        let _stalled = TcpStream::connect(address).await;
        let mut receiver = receiver;
        let result = timeout(Duration::from_millis(200), &mut receiver).await;
        if result.is_err() {
            receiver.abort();
            let _ = receiver.await;
        }
        assert!(
            result.is_ok(),
            "Una conexión tardía no debe reiniciar el tiempo de espera"
        );
        assert!(result.unwrap().unwrap().is_err());
    }
}
