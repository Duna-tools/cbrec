use crate::domain::errors::DomainError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum InfrastructureError {
    #[error(transparent)]
    Domain(#[from] DomainError),

    #[error("External service error: {0}")]
    ExternalService(String),

    #[error("HTTP request failed with status: {0}")]
    HttpStatus(u16),

    #[error("Recording error: {0}")]
    RecordingError(String),

    #[error("Recording cancelled")]
    RecordingCancelled,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
