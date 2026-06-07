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
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: 60,
            poll_interval_idle_secs: 300,
            idle_threshold_mins: 30,
            max_simultaneous: 3,
            cooldown_tras_fallo_secs: 300,
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
            min_file_size: 262_144_000, // 250 MiB
            naming_template: "{year}.{month}.{day}_{hour}.{minute}.{second}_{model}.mp4"
                .to_string(),
            watch: WatchConfig::default(),
            auth: AuthConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        let mut config = Self::default();

        let ruta_config = ProjectDirs::from("", "", "cbrec")
            .map(|p| p.config_dir().join("config.toml"))
            .filter(|p| p.exists());

        if let Some(ruta_config) = ruta_config {
            if let Ok(contenido) = fs::read_to_string(&ruta_config) {
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
                    if let Some(watch) = file_config.watch {
                        if let Some(v) = watch.poll_interval_secs {
                            config.watch.poll_interval_secs = v;
                        }
                        if let Some(v) = watch.poll_interval_idle_secs {
                            config.watch.poll_interval_idle_secs = v;
                        }
                        if let Some(v) = watch.idle_threshold_mins {
                            config.watch.idle_threshold_mins = v;
                        }
                        if let Some(v) = watch.max_simultaneous {
                            config.watch.max_simultaneous = v;
                        }
                        if let Some(v) = watch.cooldown_tras_fallo_secs {
                            config.watch.cooldown_tras_fallo_secs = v;
                        }
                    }
                    if let Some(auth) = file_config.auth {
                        if let Some(cookie) = auth.session_cookie {
                            config.auth.session_cookie = Some(cookie);
                        }
                    }
                }
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
    }

    #[test]
    fn auth_config_por_defecto_sin_cookie() {
        let cfg = AuthConfig::default();
        assert!(cfg.session_cookie.is_none());
    }
}
