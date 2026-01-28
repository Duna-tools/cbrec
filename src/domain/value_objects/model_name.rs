use crate::domain::errors::DomainError;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Nombre de modelo normalizado y validado.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelName(String);

impl ModelName {
    /// Crea un nombre de modelo valido y normalizado.
    /// # Arguments
    /// - `name`: nombre crudo del modelo.
    /// # Errors
    /// - `DomainError::InvalidModelName` si el formato es invalido.
    pub fn new(name: impl Into<String>) -> Result<Self, DomainError> {
        let name = name.into().trim().to_lowercase();

        if name.is_empty() {
            return Err(DomainError::InvalidModelName(
                "Model name cannot be empty".to_string(),
            ));
        }

        if name.len() > 100 {
            return Err(DomainError::InvalidModelName(
                "Model name too long".to_string(),
            ));
        }

        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(DomainError::InvalidModelName(
                "Model name contains invalid characters".to_string(),
            ));
        }

        Ok(Self(name))
    }

    /// Devuelve el nombre como `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for ModelName {
    type Error = DomainError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl TryFrom<String> for ModelName {
    type Error = DomainError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl fmt::Display for ModelName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_name_valid() {
        let result = ModelName::new("validmodel");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().as_str(), "validmodel");
    }

    #[test]
    fn test_model_name_with_underscore() {
        let result = ModelName::new("model_name_123");
        assert!(result.is_ok());
    }

    #[test]
    fn test_model_name_uppercase_converts_to_lowercase() {
        let result = ModelName::new("ModelName");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().as_str(), "modelname");
    }

    #[test]
    fn test_model_name_empty_fails() {
        let result = ModelName::new("");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            DomainError::InvalidModelName(_)
        ));
    }

    #[test]
    fn test_model_name_too_long_fails() {
        let long_name = "a".repeat(101);
        let result = ModelName::new(long_name);
        assert!(result.is_err());
    }

    #[test]
    fn test_model_name_invalid_characters_fails() {
        let invalid_names = vec!["model@name", "model name", "model.name"];

        for name in invalid_names {
            let result = ModelName::new(name);
            assert!(result.is_err(), "Expected {} to fail", name);
        }
    }
}
