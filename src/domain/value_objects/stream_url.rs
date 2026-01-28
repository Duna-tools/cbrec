use crate::domain::errors::DomainError;
use serde::{Deserialize, Serialize};
use std::fmt;

/// URL de stream validada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamUrl(String);

impl StreamUrl {
    /// Crea una URL de stream valida.
    /// # Arguments
    /// - `url`: URL cruda del stream.
    /// # Errors
    /// - `DomainError::InvalidStreamUrl` si el formato es invalido.
    pub fn new(url: impl Into<String>) -> Result<Self, DomainError> {
        let url = url.into().trim().to_string();

        if url.is_empty() {
            return Err(DomainError::InvalidStreamUrl(
                "URL cannot be empty".to_string(),
            ));
        }

        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(DomainError::InvalidStreamUrl(
                "URL must start with http:// or https://".to_string(),
            ));
        }

        Ok(Self(url))
    }

    /// Devuelve la URL como `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for StreamUrl {
    type Error = DomainError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<String> for StreamUrl {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl fmt::Display for StreamUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_url_valid_https() {
        let result = StreamUrl::new("https://example.com/stream.m3u8");
        assert!(result.is_ok());
    }

    #[test]
    fn test_stream_url_valid_http() {
        let result = StreamUrl::new("http://example.com/stream");
        assert!(result.is_ok());
    }

    #[test]
    fn test_stream_url_empty_fails() {
        let result = StreamUrl::new("");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DomainError::InvalidStreamUrl(_)
        ));
    }

    #[test]
    fn test_stream_url_no_protocol_fails() {
        let result = StreamUrl::new("example.com/stream");
        assert!(result.is_err());
    }

    #[test]
    fn test_stream_url_invalid_protocol_fails() {
        let result = StreamUrl::new("ftp://example.com/stream");
        assert!(result.is_err());
    }

    #[test]
    fn test_stream_url_try_from() {
        let result = StreamUrl::try_from("https://test.com/video.m3u8");
        assert!(result.is_ok());
    }
}
