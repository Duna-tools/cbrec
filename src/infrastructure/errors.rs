use crate::domain::errors::DomainError;
use thiserror::Error;

/// Errores de infraestructura.
#[derive(Error, Debug)]
pub enum InfrastructureError {
    #[error(transparent)]
    Domain(#[from] DomainError),

    #[error("External service error: {0}")]
    ExternalService(String),

    #[error("Recording error: {0}")]
    RecordingError(String),

    #[error("Recording cancelled")]
    RecordingCancelled,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
