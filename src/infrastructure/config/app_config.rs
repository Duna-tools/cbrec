use directories::{ProjectDirs, UserDirs};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchConfig {
    pub poll_interval_secs: u64,
    pub poll_interval_idle_secs: u64,
    pub idle_threshold_mins: u64,
    pub max_simultaneous: usize,
    pub cooldown_tras_fallo_secs: u64,
    pub ask_timeout_secs: u64,
    pub desktop_notify: bool,
    pub notif_titulo: String,
    pub notif_cuerpo: String,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: 60,
            poll_interval_idle_secs: 300,
            idle_threshold_mins: 30,
            max_simultaneous: 3,
            cooldown_tras_fallo_secs: 300,
            ask_timeout_secs: 5,
            desktop_notify: true,
            notif_titulo: "cbrec".to_string(),
            notif_cuerpo: "{modelo}".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AuthConfig {
    pub session_cookie: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub output_root: PathBuf,
    pub min_file_size: u64,
    pub naming_template: String,
    pub watch: WatchConfig,
    pub auth: AuthConfig,
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
            min_file_size: 262_144_000,
            naming_template: "{year}.{month}.{day}_{hour}.{minute}.{second}_{model}.mp4"
                .to_string(),
            watch: WatchConfig::default(),
            auth: AuthConfig::default(),
        }
    }
}

fn config_dir() -> Option<PathBuf> {
    ProjectDirs::from("", "", "cbrec").map(|p| p.config_dir().to_path_buf())
}

impl AppConfig {
    pub fn load() -> Self {
        let mut config = Self::default();

        let ruta_config = config_dir()
            .map(|d| d.join("config.toml"))
            .filter(|p| p.exists());

        if let Some(ruta_config) = ruta_config {
            match fs::read_to_string(&ruta_config) {
                Err(e) => eprintln!("[WARN] No se pudo leer config.toml: {}", e),
                Ok(contenido) => match toml::from_str::<FileConfig>(&contenido) {
                    Err(e) => eprintln!("[WARN] config.toml inválido: {}", e),
                    Ok(file_config) => {
                        if let Some(general) = file_config.general {
                            if let Some(v) = general.output_root {
                                config.output_root = expandir_tilde(&v);
                            }
                            if let Some(v) = general.min_file_size {
                                config.min_file_size = v;
                            }
                        }
                        if let Some(naming) = file_config.naming {
                            if let Some(v) = naming.template {
                                config.naming_template = v;
                            }
                        }
                        if let Some(w) = file_config.watch {
                            if let Some(v) = w.poll_interval_secs {
                                config.watch.poll_interval_secs = v;
                            }
                            if let Some(v) = w.poll_interval_idle_secs {
                                config.watch.poll_interval_idle_secs = v;
                            }
                            if let Some(v) = w.idle_threshold_mins {
                                config.watch.idle_threshold_mins = v;
                            }
                            if let Some(v) = w.max_simultaneous {
                                config.watch.max_simultaneous = v;
                            }
                            if let Some(v) = w.cooldown_tras_fallo_secs {
                                config.watch.cooldown_tras_fallo_secs = v;
                            }
                            if let Some(v) = w.ask_timeout_secs {
                                config.watch.ask_timeout_secs = v;
                            }
                            if let Some(v) = w.desktop_notify {
                                config.watch.desktop_notify = v;
                            }
                            if let Some(v) = w.notif_titulo {
                                config.watch.notif_titulo = v;
                            }
                            if let Some(v) = w.notif_cuerpo {
                                config.watch.notif_cuerpo = v;
                            }
                        }
                        if let Some(auth) = file_config.auth {
                            if let Some(v) = auth.session_cookie {
                                config.auth.session_cookie = Some(v);
                            }
                        }
                    }
                },
            }
        }

        config
    }

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

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct WatchedModels {
    pub models: Vec<String>,
}

impl WatchedModels {
    fn path() -> Option<PathBuf> {
        config_dir().map(|d| d.join("watched.toml"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let Ok(contenido) = fs::read_to_string(&path) else {
            return Self::default();
        };
        match toml::from_str::<WatchedModels>(&contenido) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("[WARN] watched.toml inválido: {}", e);
                Self::default()
            }
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "Sin directorio de config")
        })?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let contenido = toml::to_string(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(&path, contenido)
    }

    pub fn add(&mut self, modelo: &str) -> bool {
        if self.models.iter().any(|m| m == modelo) {
            return false;
        }
        self.models.push(modelo.to_owned());
        true
    }

    pub fn remove(&mut self, modelo: &str) -> bool {
        let antes = self.models.len();
        self.models.retain(|m| m != modelo);
        self.models.len() < antes
    }
}

#[derive(Debug, Deserialize)]
struct FileConfig {
    general: Option<GeneralConfig>,
    naming: Option<NamingConfig>,
    watch: Option<WatchFileConfig>,
    auth: Option<AuthFileConfig>,
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

#[derive(Debug, Deserialize)]
struct WatchFileConfig {
    poll_interval_secs: Option<u64>,
    poll_interval_idle_secs: Option<u64>,
    idle_threshold_mins: Option<u64>,
    max_simultaneous: Option<usize>,
    cooldown_tras_fallo_secs: Option<u64>,
    ask_timeout_secs: Option<u64>,
    desktop_notify: Option<bool>,
    notif_titulo: Option<String>,
    notif_cuerpo: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AuthFileConfig {
    session_cookie: Option<String>,
}

pub(crate) fn expandir_tilde(ruta: &str) -> PathBuf {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_config_defaults_son_razonables() {
        let cfg = WatchConfig::default();
        assert_eq!(cfg.poll_interval_secs, 60);
        assert_eq!(cfg.poll_interval_idle_secs, 300);
        assert_eq!(cfg.idle_threshold_mins, 30);
        assert_eq!(cfg.max_simultaneous, 3);
        assert_eq!(cfg.ask_timeout_secs, 5);
        assert!(cfg.desktop_notify);
        assert_eq!(cfg.notif_titulo, "cbrec");
        assert_eq!(cfg.notif_cuerpo, "{modelo}");
    }

    #[test]
    fn auth_config_por_defecto_sin_cookie() {
        let cfg = AuthConfig::default();
        assert!(cfg.session_cookie.is_none());
    }

    #[test]
    fn watched_models_add_dedup() {
        let mut w = WatchedModels::default();
        assert!(w.add("alice"));
        assert!(!w.add("alice"));
        assert_eq!(w.models.len(), 1);
    }

    #[test]
    fn watched_models_remove() {
        let mut w = WatchedModels {
            models: vec!["alice".into(), "bob".into()],
        };
        assert!(w.remove("alice"));
        assert!(!w.remove("carol"));
        assert_eq!(w.models, vec!["bob"]);
    }
}
