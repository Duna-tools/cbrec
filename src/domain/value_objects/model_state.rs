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
