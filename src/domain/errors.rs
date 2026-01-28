use thiserror::Error;

/// Errores del dominio.
#[derive(Error, Debug)]
pub enum DomainError {
    #[error("Invalid model name: {0}")]
    InvalidModelName(String),

    #[error("Invalid stream URL: {0}")]
    InvalidStreamUrl(String),

    #[error("Model not found: {0}")]
    ModelNotFound(String),

    #[error("Model is offline")]
    ModelOffline,
}
