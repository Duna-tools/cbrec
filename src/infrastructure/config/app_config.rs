use directories::UserDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Configuracion de salida y reglas de nombrado.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub output_root: PathBuf,
    pub min_file_size: u64,
    pub naming_template: String,
}

const OUTPUT_FOLDER: &str = "cb_rec";

impl Default for AppConfig {
    fn default() -> Self {
        let output_root = if let Some(dirs) = UserDirs::new() {
            dirs.video_dir()
                .map(PathBuf::from)
                .unwrap_or_else(|| dirs.home_dir().to_path_buf())
        } else {
            PathBuf::from(".")
        };
        Self {
            output_root,
            min_file_size: 1024,
            naming_template: "{year}.{month}.{day}_{hour}.{minute}.{second}_{model}.mp4"
                .to_string(),
        }
    }
}

impl AppConfig {
    /// Carga la configuracion desde `config/default.toml` si existe.
    /// # Notas
    /// - Si el archivo no existe, usa valores por defecto.
    pub fn load() -> Self {
        let mut config = Self::default();
        let ruta_config = Path::new("config/default.toml");

        if let Ok(contenido) = fs::read_to_string(ruta_config) {
            if let Ok(file_config) = toml::from_str::<FileConfig>(&contenido) {
                if let Some(general) = file_config.general {
                    if let Some(output_root) = general.output_root {
                        config.output_root = expandir_tilde(&output_root);
                    }
                    if let Some(min_file_size) = general.min_file_size {
                        config.min_file_size = min_file_size;
                    }
                }
                if let Some(naming) = file_config.naming {
                    if let Some(template) = naming.template {
                        config.naming_template = template;
                    }
                }
            }
        }

        config
    }

    /// Construye la ruta de salida con `cb_rec/<modelo>` y la fecha actual.
    /// # Arguments
    /// - `model_name`: nombre del modelo.
    /// - `output_root_override`: raiz opcional de salida.
    /// # Returns
    /// - Ruta completa del archivo de salida.
    pub fn get_output_path(
        &self,
        model_name: &str,
        output_root_override: Option<&Path>,
    ) -> PathBuf {
        let now = chrono::Local::now();
        let filename = self
            .naming_template
            .replace("{year}", &now.format("%Y").to_string())
            .replace("{month}", &now.format("%m").to_string())
            .replace("{day}", &now.format("%d").to_string())
            .replace("{hour}", &now.format("%H").to_string())
            .replace("{minute}", &now.format("%M").to_string())
            .replace("{second}", &now.format("%S").to_string())
            .replace("{model}", model_name);

        let output_root = output_root_override.unwrap_or(self.output_root.as_path());
        let base_dir = if output_root.ends_with(OUTPUT_FOLDER) {
            output_root.to_path_buf()
        } else {
            output_root.join(OUTPUT_FOLDER)
        };

        base_dir.join(model_name).join(filename)
    }
}

#[derive(Debug, Deserialize)]
struct FileConfig {
    general: Option<GeneralConfig>,
    naming: Option<NamingConfig>,
}

#[derive(Debug, Deserialize)]
struct GeneralConfig {
    output_root: Option<String>,
    min_file_size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct NamingConfig {
    template: Option<String>,
}

fn expandir_tilde(ruta: &str) -> PathBuf {
    let ruta_normalizada = ruta.trim();
    if let Some(resto) = ruta_normalizada.strip_prefix("~/") {
        if let Some(home) = obtener_home_dir() {
            return home.join(resto);
        }
    }
    if let Some(resto) = ruta_normalizada.strip_prefix("~\\") {
        if let Some(home) = obtener_home_dir() {
            return home.join(resto);
        }
    }

    PathBuf::from(ruta_normalizada)
}

fn obtener_home_dir() -> Option<PathBuf> {
    UserDirs::new().map(|dirs| dirs.home_dir().to_path_buf())
}
