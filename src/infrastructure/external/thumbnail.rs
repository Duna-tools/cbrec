//! Acceso acotado a las miniaturas públicas del CDN, sin cookies de cuenta.

use crate::domain::value_objects::ModelName;
use crate::infrastructure::InfrastructureError;
use reqwest::{Client, Response, Url};
use std::time::Duration;

pub(crate) struct ThumbnailResponse {
    pub bytes: Vec<u8>,
    pub freshness: Duration,
}

fn invalid_thumbnail() -> InfrastructureError {
    InfrastructureError::ExternalService("URL de miniatura no permitida".into())
}

pub(crate) fn thumbnail_url(
    model: &ModelName,
    source: Option<&str>,
) -> Result<Url, InfrastructureError> {
    let fallback = Url::parse(&format!("https://thumb.live.mmcdn.com/riw/{model}.jpg"))
        .map_err(|_| invalid_thumbnail())?;
    let Some(source) = source else {
        return Ok(fallback);
    };
    if source.len() > 2048 {
        return Err(invalid_thumbnail());
    }
    let url = Url::parse(source).map_err(|_| invalid_thumbnail())?;
    if url.scheme() != "https"
        || url.host_str() != fallback.host_str()
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != fallback.path()
        || url.fragment().is_some()
        || url.query().is_some_and(|query| {
            query.is_empty() || !query.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Err(invalid_thumbnail());
    }
    Ok(url)
}

pub(crate) async fn download(
    client: &Client,
    url: Url,
) -> Result<ThumbnailResponse, InfrastructureError> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let response = client
            .get(url)
            .header(reqwest::header::ACCEPT, "image/jpeg,image/png")
            .send()
            .await
            .map_err(|error| {
                InfrastructureError::ExternalService(error.without_url().to_string())
            })?;
        read_image_response(response).await
    })
    .await
    .map_err(|_| {
        InfrastructureError::ExternalService("La miniatura no respondió en 5 segundos".into())
    })?
}

async fn read_image_response(
    mut response: Response,
) -> Result<ThumbnailResponse, InfrastructureError> {
    const MAX_BYTES: usize = 1024 * 1024;
    if !response.status().is_success() {
        return Err(InfrastructureError::HttpStatus(response.status().as_u16()));
    }
    let kind = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim();
    if !matches!(kind, "image/jpeg" | "image/png") {
        return Err(InfrastructureError::ExternalService(
            "El CDN no devolvió una imagen JPEG/PNG".into(),
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BYTES as u64)
    {
        return Err(InfrastructureError::ExternalService(
            "La miniatura supera 1 MiB".into(),
        ));
    }
    let control = response
        .headers()
        .get(reqwest::header::CACHE_CONTROL)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let mut age_limit = 0;
    let mut no_cache = false;
    for part in control
        .split(',')
        .map(|part| part.trim().to_ascii_lowercase())
    {
        if matches!(part.as_str(), "no-store" | "no-cache" | "private") {
            no_cache = true;
        }
        if let Some(value) = part.strip_prefix("max-age=") {
            age_limit = value.parse::<u64>().unwrap_or(0).min(15);
        }
    }
    let age = response
        .headers()
        .get(reqwest::header::AGE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let freshness = Duration::from_secs(if no_cache {
        0
    } else {
        age_limit.saturating_sub(age)
    });
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| InfrastructureError::ExternalService(error.without_url().to_string()))?
    {
        if chunk.len() > MAX_BYTES.saturating_sub(bytes.len()) {
            return Err(InfrastructureError::ExternalService(
                "La miniatura supera 1 MiB".into(),
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(ThumbnailResponse { bytes, freshness })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn only_the_known_model_cdn_path_is_accepted() {
        let model = ModelName::try_from("alice").unwrap();
        assert_eq!(
            thumbnail_url(&model, None).unwrap().as_str(),
            "https://thumb.live.mmcdn.com/riw/alice.jpg"
        );
        assert!(thumbnail_url(
            &model,
            Some("https://thumb.live.mmcdn.com/riw/alice.jpg?1793720040")
        )
        .is_ok());
        for url in [
            "http://thumb.live.mmcdn.com/riw/alice.jpg",
            "https://evil.test/alice.jpg",
            "https://thumb.live.mmcdn.com/riw/bob.jpg",
            "https://user:secret@thumb.live.mmcdn.com/riw/alice.jpg",
            "https://thumb.live.mmcdn.com/riw/alice.jpg?url=http://localhost",
            "https://thumb.live.mmcdn.com:444/riw/alice.jpg",
        ] {
            assert!(thumbnail_url(&model, Some(url)).is_err());
        }
    }

    async fn response(headers: &str, body: &[u8]) -> Response {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let message = [
            format!("HTTP/1.1 200 OK\r\nConnection: close\r\n{headers}\r\n").into_bytes(),
            body.to_vec(),
        ]
        .concat();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            let _ = socket.read(&mut request).await;
            let _ = socket.write_all(&message).await;
        });
        Client::new()
            .get(format!("http://{address}"))
            .send()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn response_limits_type_size_and_uses_remaining_cdn_freshness() {
        let image = read_image_response(response("Content-Type: image/jpeg\r\nContent-Length: 3\r\nCache-Control: public, max-age=15\r\nAge: 8\r\n", b"jpg").await).await.unwrap();
        assert_eq!(image.bytes, b"jpg");
        assert_eq!(image.freshness, Duration::from_secs(7));
        let image = read_image_response(response("Content-Type: image/png\r\nContent-Length: 0\r\nCache-Control: No-Store, Max-Age=15\r\n", b"").await).await.unwrap();
        assert!(image.freshness.is_zero());
        assert!(read_image_response(
            response("Content-Type: text/html\r\nContent-Length: 0\r\n", b"").await
        )
        .await
        .is_err());
        assert!(read_image_response(
            response(
                "Content-Type: image/jpeg\r\nContent-Length: 1048577\r\n",
                b""
            )
            .await
        )
        .await
        .is_err());
        let large = vec![0; 1048577];
        assert!(
            read_image_response(response("Content-Type: image/jpeg\r\n", &large).await)
                .await
                .is_err()
        );
    }
}
