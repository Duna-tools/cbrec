//! Entrega segmentos completos a FFmpeg para evitar pérdidas por transferencias truncadas.

use super::chaturbate_client::{retry_with_backoff, RetryFailure};
use crate::infrastructure::InfrastructureError;
use lru::LruCache;
use reqwest::{Client, StatusCode, Url};
use std::{num::NonZeroUsize, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Mutex,
    task::{JoinHandle, JoinSet},
};

const MAX_BODY: usize = 32 * 1024 * 1024;
const MAX_PLAYLIST: usize = 1024 * 1024;
const MAX_RESOURCES: usize = 4096;

pub(super) struct HlsRelay {
    pub url: String,
    task: JoinHandle<()>,
    last_error: Arc<Mutex<Option<String>>>,
}

struct Resources {
    root: Url,
    prefix: String,
    next: u64,
    urls: LruCache<u64, Url>,
}

struct ResourceBody {
    status: StatusCode,
    content_type: String,
    content_range: Option<String>,
    final_url: Url,
    bytes: Vec<u8>,
}

fn error(message: &str) -> InfrastructureError {
    InfrastructureError::ExternalService(message.into())
}

impl HlsRelay {
    pub async fn start(client: Client, source: &str) -> Result<Self, InfrastructureError> {
        let root = Url::parse(source).map_err(|_| error("URL HLS inválida"))?;
        validate_url(&root)?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let mut secret = [0; 16];
        getrandom::fill(&mut secret)
            .map_err(|_| error("No se pudo proteger el relevo HLS local"))?;
        let token: String = secret.iter().map(|byte| format!("{byte:02x}")).collect();
        let prefix = format!("http://{}/{token}/", listener.local_addr()?);
        let url = format!("{prefix}0.m3u8");
        let resources = Arc::new(Mutex::new(Resources {
            root,
            prefix,
            next: 1,
            urls: LruCache::new(NonZeroUsize::new(MAX_RESOURCES).unwrap()),
        }));
        let path_prefix = format!("/{token}/");
        let last_error = Arc::new(Mutex::new(None));
        let errors = last_error.clone();
        let task = tokio::spawn(async move {
            let mut requests = JoinSet::new();
            loop {
                tokio::select! {
                    connection = listener.accept(), if requests.len() < 2 => {
                        let Ok((socket, _)) = connection else { break; };
                        let client = client.clone();
                        let resources = resources.clone();
                        let prefix = path_prefix.clone();
                        let errors = errors.clone();
                        requests.spawn(async move {
                            if let Err(error) = serve(socket, client, resources, prefix, errors.clone()).await {
                                let mut previous = errors.lock().await;
                                if previous.is_none() { *previous = Some(error.to_string()); }
                            }
                        });
                    }
                    Some(_) = requests.join_next(), if !requests.is_empty() => {}
                }
            }
        });
        Ok(Self {
            url,
            task,
            last_error,
        })
    }

    pub async fn last_error(&self) -> Option<String> {
        self.last_error.lock().await.clone()
    }
}

impl Drop for HlsRelay {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn validate_url(url: &Url) -> Result<(), InfrastructureError> {
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(error(
            "El recurso HLS debe usar HTTP(S) sin credenciales de usuario",
        ));
    }
    Ok(())
}

impl Resources {
    fn register(&mut self, base: &Url, uri: &str) -> Result<String, InfrastructureError> {
        if uri.len() > 8192 {
            return Err(error("URI HLS demasiado larga"));
        }
        let url = base.join(uri).map_err(|_| error("URI HLS inválida"))?;
        validate_url(&url)?;
        if url == self.root {
            return Ok(format!("{}0.m3u8", self.prefix));
        }
        let existing = self
            .urls
            .iter()
            .find(|(_, value)| **value == url)
            .map(|(id, _)| *id);
        let id = if let Some(id) = existing {
            self.urls.get(&id);
            id
        } else {
            let id = self.next;
            self.next = self
                .next
                .checked_add(1)
                .ok_or_else(|| error("Se agotaron los identificadores HLS"))?;
            self.urls.put(id, url.clone());
            id
        };
        let extension = url
            .path()
            .rsplit('/')
            .next()
            .and_then(|name| name.rsplit_once('.'))
            .map(|(_, ext)| ext)
            .filter(|ext| ext.len() <= 8 && ext.bytes().all(|b| b.is_ascii_alphanumeric()))
            .unwrap_or("ts");
        Ok(format!("{}{id}.{extension}", self.prefix))
    }

    fn rewrite(&mut self, base: &Url, text: &str) -> Result<Vec<u8>, InfrastructureError> {
        if text.len() > MAX_PLAYLIST {
            return Err(error("Playlist HLS demasiado grande"));
        }
        let mut output = String::new();
        let mut count = 0;
        for line in text.lines() {
            let line = line.trim();
            if !line.is_empty() && !line.starts_with('#') {
                count += 1;
                output.push_str(&self.register(base, line)?);
            } else {
                let mut rest = line;
                while let Some(index) = rest.find("URI=\"") {
                    count += 1;
                    output.push_str(&rest[..index + 5]);
                    rest = &rest[index + 5..];
                    let end = rest
                        .find('"')
                        .ok_or_else(|| error("Atributo URI HLS sin cerrar"))?;
                    output.push_str(&self.register(base, &rest[..end])?);
                    output.push('"');
                    rest = &rest[end + 1..];
                }
                output.push_str(rest);
            }
            if count > MAX_RESOURCES / 2 {
                return Err(error("Demasiados recursos en la playlist HLS"));
            }
            output.push('\n');
        }
        Ok(output.into_bytes())
    }
}

async fn download(
    client: &Client,
    url: &Url,
    range: Option<&str>,
) -> Result<ResourceBody, InfrastructureError> {
    retry_with_backoff(|| async {
        let mut request = client
            .get(url.clone())
            .header(reqwest::header::ACCEPT_ENCODING, "identity");
        if let Some(range) = range {
            request = request.header(reqwest::header::RANGE, range);
        }
        let mut response = request
            .send()
            .await
            .map_err(|e| RetryFailure::Transient(error(&e.without_url().to_string())))?;
        let status = response.status();
        if !status.is_success() {
            let error = InfrastructureError::HttpStatus(status.as_u16());
            return Err(
                if status.is_server_error() || matches!(status.as_u16(), 408 | 429 | 404) {
                    RetryFailure::Transient(error)
                } else {
                    RetryFailure::Permanent(error)
                },
            );
        }
        let length = response.content_length();
        if length.is_some_and(|n| n > MAX_BODY as u64) {
            return Err(RetryFailure::Permanent(error(
                "El recurso HLS supera 32 MiB",
            )));
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();
        let content_range = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let final_url = response.url().clone();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| RetryFailure::Transient(error(&e.without_url().to_string())))?
        {
            if chunk.len() > MAX_BODY.saturating_sub(bytes.len()) {
                return Err(RetryFailure::Permanent(error(
                    "El recurso HLS supera 32 MiB",
                )));
            }
            bytes.extend_from_slice(&chunk);
        }
        if length.is_some_and(|n| n != bytes.len() as u64) {
            return Err(RetryFailure::Transient(error(
                "Transferencia HLS incompleta",
            )));
        }
        Ok(ResourceBody {
            status,
            content_type,
            content_range,
            final_url,
            bytes,
        })
    })
    .await
}

fn parse_request<'a>(request: &'a str, prefix: &str) -> Option<(u64, Option<&'a str>)> {
    let mut lines = request.split("\r\n");
    let mut first = lines.next()?.split_whitespace();
    if first.next()? != "GET" {
        return None;
    }
    let path = first.next()?.strip_prefix(prefix)?;
    if path.contains(['/', '?', '#']) || first.next()? != "HTTP/1.1" {
        return None;
    }
    let id = path.split_once('.')?.0.parse().ok()?;
    let mut range = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("range") {
                if range.is_some() {
                    return None;
                }
                let value = value.trim();
                let (start, end) = value.strip_prefix("bytes=")?.split_once('-')?;
                if start.is_empty()
                    || !start.bytes().all(|b| b.is_ascii_digit())
                    || !end.bytes().all(|b| b.is_ascii_digit())
                {
                    return None;
                }
                range = Some(value);
            }
        }
    }
    Some((id, range))
}

async fn read_header(socket: &mut TcpStream) -> Result<Vec<u8>, InfrastructureError> {
    let mut request = Vec::new();
    while !request.ends_with(b"\r\n\r\n") {
        let mut buffer = [0; 1024];
        let count = socket.read(&mut buffer).await?;
        if count == 0 || request.len() + count > 8192 {
            return Err(error("Petición local HLS inválida"));
        }
        request.extend_from_slice(&buffer[..count]);
    }
    Ok(request)
}

async fn serve(
    mut socket: TcpStream,
    client: Client,
    resources: Arc<Mutex<Resources>>,
    prefix: String,
    last_error: Arc<Mutex<Option<String>>>,
) -> Result<(), InfrastructureError> {
    let request = tokio::time::timeout(Duration::from_secs(2), read_header(&mut socket))
        .await
        .map_err(|_| error("Petición local HLS caducada"))??;
    let text = std::str::from_utf8(&request).map_err(|_| error("Petición local HLS inválida"))?;
    let Some((id, range)) = parse_request(text, &prefix) else {
        return respond(
            &mut socket,
            "403 Forbidden",
            "text/plain",
            None,
            b"Recurso no autorizado",
        )
        .await;
    };
    let source = {
        let mut map = resources.lock().await;
        if id == 0 {
            Some(map.root.clone())
        } else {
            map.urls.get(&id).cloned()
        }
    };
    let Some(source) = source else {
        return respond(
            &mut socket,
            "404 Not Found",
            "text/plain",
            None,
            b"Recurso caducado",
        )
        .await;
    };
    // No se envían cabeceras ni cookies del navegador al CDN; solo un rango validado.
    let mut body = match download(&client, &source, range).await {
        Ok(body) => body,
        Err(error) => {
            *last_error.lock().await = Some(error.to_string());
            let status = if let InfrastructureError::HttpStatus(code) = error {
                StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_GATEWAY)
            } else {
                StatusCode::BAD_GATEWAY
            };
            let status = format!(
                "{} {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or("Upstream Error")
            );
            return respond(
                &mut socket,
                &status,
                "text/plain",
                None,
                b"No se pudo completar el recurso HLS",
            )
            .await;
        }
    };
    if body.bytes.starts_with(b"#EXTM3U") {
        let text = std::str::from_utf8(&body.bytes).map_err(|_| error("Playlist HLS no válida"))?;
        body.bytes = resources.lock().await.rewrite(&body.final_url, text)?;
        body.content_type = "application/vnd.apple.mpegurl".into();
        body.content_range = None;
        body.status = StatusCode::OK;
    }
    respond(
        &mut socket,
        if body.status == StatusCode::PARTIAL_CONTENT {
            "206 Partial Content"
        } else {
            "200 OK"
        },
        &body.content_type,
        body.content_range.as_deref(),
        &body.bytes,
    )
    .await
}

async fn respond(
    socket: &mut TcpStream,
    status: &str,
    content_type: &str,
    content_range: Option<&str>,
    body: &[u8],
) -> Result<(), InfrastructureError> {
    let range = content_range
        .map(|v| format!("Content-Range: {v}\r\n"))
        .unwrap_or_default();
    let header = format!("HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\n{range}Content-Length: {}\r\nConnection: close\r\n\r\n", body.len());
    socket.write_all(header.as_bytes()).await?;
    socket.write_all(body).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn resources() -> Resources {
        Resources {
            root: Url::parse("https://media.test/live/index.m3u8?secret=private").unwrap(),
            prefix: "http://127.0.0.1:1234/token/".into(),
            next: 1,
            urls: LruCache::new(NonZeroUsize::new(4).unwrap()),
        }
    }

    async fn read_request(socket: &mut TcpStream) -> String {
        String::from_utf8(read_header(socket).await.unwrap()).unwrap()
    }

    #[test]
    fn playlist_rewriting_preserves_media_tags_ranges_keys_and_signed_queries() {
        let mut map = resources();
        let base = map.root.clone();
        let text = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"key?secret=private\",IV=0x0000\n#EXT-X-MAP:URI=\"init.mp4\",BYTERANGE=\"50@0\"\n#EXT-X-BYTERANGE:100@50\nseg.ts?secret=private\n";
        let rewritten = String::from_utf8(map.rewrite(&base, text).unwrap()).unwrap();
        assert!(!rewritten.contains("private"));
        assert!(rewritten.contains("IV=0x0000"));
        assert!(rewritten.contains("BYTERANGE=\"50@0\""));
        assert!(rewritten.contains("#EXT-X-BYTERANGE:100@50"));
        assert!(rewritten.contains("/token/3.ts"));
        assert_eq!(
            map.urls.get(&3).unwrap().as_str(),
            "https://media.test/live/seg.ts?secret=private"
        );
        assert_eq!(
            map.register(&base, "seg.ts?secret=private").unwrap(),
            "http://127.0.0.1:1234/token/3.ts"
        );
        assert!(map
            .register(&base, base.as_str())
            .unwrap()
            .ends_with("/0.m3u8"));
        for bad in [
            "file:///etc/shadow",
            "https://user:pass@media.test/seg.ts",
            "data:secret",
        ] {
            assert!(map.register(&base, bad).is_err());
        }
        assert!(map
            .rewrite(&base, "#EXTM3U\n#EXT-X-KEY:URI=\"unterminated")
            .is_err());
        assert!(map.rewrite(&base, &"x".repeat(MAX_PLAYLIST + 1)).is_err());
        assert!(map.register(&base, &"x".repeat(8193)).is_err());
        for i in 0..10 {
            map.register(&base, &format!("seg{i}.ts")).unwrap();
        }
        assert_eq!(map.urls.len(), 4);
    }

    #[test]
    fn only_authorized_gets_and_single_numeric_ranges_are_accepted() {
        let prefix = "/secret/";
        assert_eq!(
            parse_request(
                "GET /secret/1.ts HTTP/1.1\r\nRange: bytes=10-20\r\n\r\n",
                prefix
            ),
            Some((1, Some("bytes=10-20")))
        );
        for request in [
            "POST /secret/1.ts HTTP/1.1\r\n\r\n",
            "GET /other/1.ts HTTP/1.1\r\n\r\n",
            "GET /secret/http://evil.test HTTP/1.1\r\n\r\n",
            "GET /secret/1.ts?url=evil HTTP/1.1\r\n\r\n",
            "GET /secret/1.ts HTTP/1.1\r\nRange: bytes=-10\r\n\r\n",
            "GET /secret/1.ts HTTP/1.1\r\nRange: bytes=1-2,3-4\r\n\r\n",
            "GET /secret/1.ts HTTP/1.1\r\nRange: bytes=0-\r\nRange: bytes=1-\r\n\r\n",
        ] {
            assert!(parse_request(request, prefix).is_none(), "{request}");
        }
    }

    #[tokio::test]
    async fn relay_never_exposes_a_truncated_body_or_browser_cookie() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let source = format!("http://{}/index.m3u8", listener.local_addr().unwrap());
        let attempts = Arc::new(AtomicUsize::new(0));
        let count = attempts.clone();
        let remote = tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let request = read_request(&mut socket).await;
                assert!(!request.to_ascii_lowercase().contains("cookie:"));
                let (body, truncated) = if request.starts_with("GET /index.m3u8 ") {
                    (
                        b"#EXTM3U\n#EXTINF:1,\nseg.ts\n#EXT-X-ENDLIST\n".as_slice(),
                        false,
                    )
                } else {
                    (
                        b"0123456789".as_slice(),
                        count.fetch_add(1, Ordering::SeqCst) == 0,
                    )
                };
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                socket.write_all(header.as_bytes()).await.unwrap();
                socket
                    .write_all(if truncated { &body[..3] } else { body })
                    .await
                    .unwrap();
            }
        });
        let client = Client::new();
        let relay = HlsRelay::start(client.clone(), &source).await.unwrap();
        let playlist = client
            .get(&relay.url)
            .header("Cookie", "sessionid=must-not-forward")
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        let segment = playlist
            .lines()
            .find(|line| line.starts_with("http://"))
            .unwrap();
        let body = client
            .get(segment)
            .header("Cookie", "sessionid=must-not-forward")
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert_eq!(body.as_ref(), b"0123456789");
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        let unauthorized = Url::parse(&relay.url)
            .unwrap()
            .join("/wrong/0.m3u8")
            .unwrap();
        assert_eq!(
            client.get(unauthorized).send().await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        remote.abort();
    }

    async fn upstream_once(response: Vec<u8>) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/resource", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let request = read_request(&mut socket).await;
            socket.write_all(&response).await.unwrap();
            request
        });
        (url, task)
    }

    #[tokio::test]
    async fn ranges_and_access_errors_keep_their_http_contract() {
        let (url, request) = upstream_once(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 3-5/10\r\nConnection: close\r\n\r\n345".to_vec()).await;
        let client = Client::new();
        let relay = HlsRelay::start(client.clone(), &url).await.unwrap();
        let response = client
            .get(&relay.url)
            .header("Range", "bytes=3-5")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            response.headers()[reqwest::header::CONTENT_RANGE],
            "bytes 3-5/10"
        );
        assert_eq!(response.bytes().await.unwrap().as_ref(), b"345");
        let request = request.await.unwrap().to_ascii_lowercase();
        assert!(request.contains("range: bytes=3-5"));
        assert!(request.contains("accept-encoding: identity"));
        let missing = relay.url.replace("0.m3u8", "9999.ts");
        assert_eq!(
            client.get(missing).send().await.unwrap().status(),
            StatusCode::NOT_FOUND
        );
        let (url, request) = upstream_once(
            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        )
        .await;
        let relay = HlsRelay::start(client.clone(), &url).await.unwrap();
        assert_eq!(
            client.get(&relay.url).send().await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        request.await.unwrap();
        assert!(relay.last_error().await.unwrap().contains("403"));
    }

    #[tokio::test]
    async fn oversized_resources_are_rejected_before_reading_the_body() {
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            MAX_BODY + 1
        );
        let (url, request) = upstream_once(response.into_bytes()).await;
        let result = download(&Client::new(), &Url::parse(&url).unwrap(), None).await;
        assert!(
            matches!(result, Err(InfrastructureError::ExternalService(message)) if message.contains("32 MiB"))
        );
        request.await.unwrap();
    }

    #[tokio::test]
    async fn incomplete_chunked_bodies_are_retried_before_delivery() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/resource", listener.local_addr().unwrap());
        let remote = tokio::spawn(async move {
            for body in [
                b"5\r\nabc".as_slice(),
                b"5\r\nabcde\r\n0\r\n\r\n".as_slice(),
            ] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let _ = read_request(&mut socket).await;
                socket.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").await.unwrap();
                socket.write_all(body).await.unwrap();
            }
        });
        let result = download(&Client::new(), &Url::parse(&url).unwrap(), None)
            .await
            .unwrap();
        assert_eq!(result.bytes, b"abcde");
        remote.await.unwrap();
    }

    #[tokio::test]
    async fn dropping_relay_cancels_an_incomplete_transfer() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let source = format!("http://{}/index.m3u8", listener.local_addr().unwrap());
        let (started, received) = tokio::sync::oneshot::channel();
        let remote = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let _ = read_request(&mut socket).await;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nx")
                .await
                .unwrap();
            started.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        let client = Client::new();
        let relay = HlsRelay::start(client.clone(), &source).await.unwrap();
        let url = relay.url.clone();
        let request = tokio::spawn(async move { client.get(url).send().await });
        tokio::time::timeout(Duration::from_secs(2), received)
            .await
            .unwrap()
            .unwrap();
        drop(relay);
        assert!(tokio::time::timeout(Duration::from_secs(2), request)
            .await
            .unwrap()
            .unwrap()
            .is_err());
        remote.abort();
    }
}
