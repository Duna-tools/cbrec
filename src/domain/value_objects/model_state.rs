#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EstadoModelo {
    Offline,
    Grabando,
}

impl std::fmt::Display for EstadoModelo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EstadoModelo::Offline => write!(f, "offline"),
            EstadoModelo::Grabando => write!(f, "grabando"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_stable_state_names() {
        assert_eq!(EstadoModelo::Offline.to_string(), "offline");
        assert_eq!(EstadoModelo::Grabando.to_string(), "grabando");
    }
}
