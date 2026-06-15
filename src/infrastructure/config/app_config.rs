use directories::{ProjectDirs, UserDirs};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

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

#[derive(Debug, Clone)]
pub enum ConfigWarning {
    ConfigNoLeida {
        path: PathBuf,
        error: String,
    },
    ConfigInvalida {
        path: PathBuf,
        error: String,
    },
    WatchedInvalidoRespaldado {
        path: PathBuf,
        backup_path: PathBuf,
        error: String,
    },
    WatchedInvalidoSinRespaldo {
        path: PathBuf,
        error: String,
        backup_error: String,
    },
}

impl std::fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConfigNoLeida { path, error } => {
                write!(f, "No se pudo leer {}: {}", path.display(), error)
            }
            Self::ConfigInvalida { path, error } => {
                write!(f, "{} invalido: {}", path.display(), error)
            }
            Self::WatchedInvalidoRespaldado {
                path,
                backup_path,
                error,
            } => write!(
                f,
                "{} invalido: {}. Respaldo creado en {}",
                path.display(),
                error,
                backup_path.display()
            ),
            Self::WatchedInvalidoSinRespaldo {
                path,
                error,
                backup_error,
            } => write!(
                f,
                "{} invalido: {}. No se pudo crear respaldo: {}",
                path.display(),
                error,
                backup_error
            ),
        }
    }
}

const OUTPUT_FOLDER: &str = "cb_rec";
const LOCK_TIMEOUT: Duration = Duration::from_secs(10);
const LOCK_VENCIDO_TRAS: Duration = Duration::from_secs(300);
const LOCK_REINTENTO: Duration = Duration::from_millis(50);

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
        Self::load_with_warnings().config
    }

    pub fn load_with_warnings() -> LoadedAppConfig {
        let ruta_config = config_dir()
            .map(|d| d.join("config.toml"))
            .filter(|p| p.exists());
        Self::load_from_path(ruta_config)
    }

    fn load_from_path(ruta_config: Option<PathBuf>) -> LoadedAppConfig {
        let mut config = Self::default();
        let mut warnings = Vec::new();

        if let Some(ruta_config) = ruta_config {
            match fs::read_to_string(&ruta_config) {
                Err(e) => warnings.push(ConfigWarning::ConfigNoLeida {
                    path: ruta_config,
                    error: e.to_string(),
                }),
                Ok(contenido) => {
                    if let Err(e) = config.aplicar_toml(&contenido) {
                        warnings.push(ConfigWarning::ConfigInvalida {
                            path: ruta_config,
                            error: e.to_string(),
                        });
                    }
                }
            }
        }

        LoadedAppConfig { config, warnings }
    }

    fn aplicar_toml(&mut self, contenido: &str) -> Result<(), toml::de::Error> {
        let file_config = toml::from_str::<FileConfig>(contenido)?;
        self.aplicar_file_config(file_config);
        Ok(())
    }

    fn aplicar_file_config(&mut self, file_config: FileConfig) {
        if let Some(general) = file_config.general {
            if let Some(v) = general.output_root {
                self.output_root = expandir_tilde(&v);
            }
            if let Some(v) = general.min_file_size {
                self.min_file_size = v;
            }
        }
        if let Some(naming) = file_config.naming {
            if let Some(v) = naming.template {
                self.naming_template = v;
            }
        }
        if let Some(w) = file_config.watch {
            if let Some(v) = w.poll_interval_secs {
                self.watch.poll_interval_secs = v;
            }
            if let Some(v) = w.poll_interval_idle_secs {
                self.watch.poll_interval_idle_secs = v;
            }
            if let Some(v) = w.idle_threshold_mins {
                self.watch.idle_threshold_mins = v;
            }
            if let Some(v) = w.max_simultaneous {
                self.watch.max_simultaneous = v;
            }
            if let Some(v) = w.cooldown_tras_fallo_secs {
                self.watch.cooldown_tras_fallo_secs = v;
            }
            if let Some(v) = w.ask_timeout_secs {
                self.watch.ask_timeout_secs = v;
            }
            if let Some(v) = w.desktop_notify {
                self.watch.desktop_notify = v;
            }
            if let Some(v) = w.notif_titulo {
                self.watch.notif_titulo = v;
            }
            if let Some(v) = w.notif_cuerpo {
                self.watch.notif_cuerpo = v;
            }
        }
        if let Some(auth) = file_config.auth {
            if let Some(v) = auth.session_cookie {
                self.auth.session_cookie = Some(v);
            }
        }
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

#[derive(Debug, Clone)]
pub struct LoadedAppConfig {
    pub config: AppConfig,
    pub warnings: Vec<ConfigWarning>,
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
        Self::load_with_warnings().watched
    }

    pub fn load_with_warnings() -> LoadedWatchedModels {
        let Some(path) = Self::path() else {
            return LoadedWatchedModels::default();
        };
        Self::load_from_path(&path)
    }

    fn load_from_path(path: &Path) -> LoadedWatchedModels {
        let Ok(contenido) = fs::read_to_string(path) else {
            return LoadedWatchedModels::default();
        };
        match toml::from_str::<WatchedModels>(&contenido) {
            Ok(models) => LoadedWatchedModels {
                watched: models,
                warnings: Vec::new(),
            },
            Err(e) => LoadedWatchedModels {
                watched: Self::default(),
                warnings: vec![respaldar_watched_invalido(path, &e.to_string())],
            },
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "Sin directorio de config")
        })?;
        self.save_to_path(&path)
    }

    pub fn update<F, T>(mutador: F) -> std::io::Result<T>
    where
        F: FnOnce(&mut WatchedModels) -> (T, bool),
    {
        Ok(Self::update_with_warnings(mutador)?.resultado)
    }

    pub fn update_with_warnings<F, T>(mutador: F) -> std::io::Result<UpdatedWatchedModels<T>>
    where
        F: FnOnce(&mut WatchedModels) -> (T, bool),
    {
        let path = Self::path().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "Sin directorio de config")
        })?;
        Self::update_in_path(&path, mutador)
    }

    fn update_in_path<F, T>(path: &Path, mutador: F) -> std::io::Result<UpdatedWatchedModels<T>>
    where
        F: FnOnce(&mut WatchedModels) -> (T, bool),
    {
        let _lock = WatchedLock::acquire(path)?;
        let LoadedWatchedModels {
            watched: mut models,
            warnings,
        } = Self::load_from_path(path);
        let (resultado, cambio) = mutador(&mut models);
        if cambio {
            models.save_to_path(path)?;
        }
        Ok(UpdatedWatchedModels {
            resultado,
            warnings,
        })
    }

    fn save_to_path(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let contenido = toml::to_string(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let tmp_path = ruta_temporal_para(path);
        fs::write(&tmp_path, contenido)?;
        fs::rename(&tmp_path, path)
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

#[derive(Debug, Default)]
pub struct LoadedWatchedModels {
    pub watched: WatchedModels,
    pub warnings: Vec<ConfigWarning>,
}

#[derive(Debug)]
pub struct UpdatedWatchedModels<T> {
    pub resultado: T,
    pub warnings: Vec<ConfigWarning>,
}

#[derive(Debug)]
struct WatchedLock {
    path: PathBuf,
}

impl WatchedLock {
    fn acquire(path: &Path) -> std::io::Result<Self> {
        Self::acquire_with(path, LOCK_TIMEOUT, LOCK_VENCIDO_TRAS)
    }

    fn acquire_with(
        path: &Path,
        timeout: Duration,
        lock_vencido_tras: Duration,
    ) -> std::io::Result<Self> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let lock_path = ruta_lock_para(path);
        let inicio = Instant::now();

        loop {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(mut file) => {
                    escribir_lock(&mut file)?;
                    return Ok(Self { path: lock_path });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if liberar_lock_si_vencido(&lock_path, lock_vencido_tras)? {
                        continue;
                    }
                    if inicio.elapsed() >= timeout {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::WouldBlock,
                            format!("watched.toml esta bloqueado: {}", lock_path.display()),
                        ));
                    }
                    std::thread::sleep(LOCK_REINTENTO);
                }
                Err(e) => return Err(e),
            }
        }
    }
}

impl Drop for WatchedLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn ruta_lock_para(path: &Path) -> PathBuf {
    path.with_extension("toml.lock")
}

fn liberar_lock_si_vencido(path: &Path, lock_vencido_tras: Duration) -> std::io::Result<bool> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(_) => return Ok(false),
    };
    let Ok(modificado) = metadata.modified() else {
        return Ok(false);
    };
    if !lock_vencido(modificado, SystemTime::now(), lock_vencido_tras) {
        return Ok(false);
    }
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(e) => Err(e),
    }
}

fn lock_vencido(modificado: SystemTime, ahora: SystemTime, max_age: Duration) -> bool {
    match ahora.duration_since(modificado) {
        Ok(edad) => edad >= max_age,
        Err(_) => false,
    }
}

fn respaldar_watched_invalido(path: &Path, error: &str) -> ConfigWarning {
    let backup_path = ruta_invalida_para(path);
    match fs::rename(path, &backup_path) {
        Ok(()) => ConfigWarning::WatchedInvalidoRespaldado {
            path: path.to_path_buf(),
            backup_path,
            error: error.to_string(),
        },
        Err(e) => ConfigWarning::WatchedInvalidoSinRespaldo {
            path: path.to_path_buf(),
            error: error.to_string(),
            backup_error: e.to_string(),
        },
    }
}

fn ruta_invalida_para(path: &Path) -> PathBuf {
    let nombre = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("watched.toml");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    path.with_file_name(format!(
        "{}.invalid.{}.{}",
        nombre,
        std::process::id(),
        nanos
    ))
}

fn escribir_lock(file: &mut File) -> std::io::Result<()> {
    writeln!(file, "pid={}", std::process::id())
}

fn ruta_temporal_para(path: &Path) -> PathBuf {
    let nombre = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("watched.toml");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let tmp_nombre = format!(".{}.{}.{}.tmp", nombre, std::process::id(), nanos);
    path.with_file_name(tmp_nombre)
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
    fn app_config_parcial_mantiene_defaults() {
        let mut cfg = AppConfig::default();
        let min_file_size_default = cfg.min_file_size;
        let poll_idle_default = cfg.watch.poll_interval_idle_secs;

        cfg.aplicar_toml(
            r#"
[watch]
poll_interval_secs = 15

[auth]
session_cookie = "PHPSESSID=abc"
"#,
        )
        .expect("config válida");

        assert_eq!(cfg.watch.poll_interval_secs, 15);
        assert_eq!(cfg.watch.poll_interval_idle_secs, poll_idle_default);
        assert_eq!(cfg.min_file_size, min_file_size_default);
        assert_eq!(cfg.auth.session_cookie.as_deref(), Some("PHPSESSID=abc"));
    }

    #[test]
    fn app_config_aplica_valores_del_archivo() {
        let mut cfg = AppConfig::default();

        cfg.aplicar_toml(
            r#"
[general]
output_root = "/tmp/cbrec-videos"
min_file_size = 1024

[naming]
template = "{model}.mp4"

[watch]
poll_interval_secs = 10
poll_interval_idle_secs = 120
idle_threshold_mins = 5
max_simultaneous = 2
cooldown_tras_fallo_secs = 30
ask_timeout_secs = 8
desktop_notify = false
notif_titulo = "titulo"
notif_cuerpo = "cuerpo {modelo}"
"#,
        )
        .expect("config válida");

        assert_eq!(cfg.output_root, PathBuf::from("/tmp/cbrec-videos"));
        assert_eq!(cfg.min_file_size, 1024);
        assert_eq!(cfg.naming_template, "{model}.mp4");
        assert_eq!(cfg.watch.poll_interval_secs, 10);
        assert_eq!(cfg.watch.poll_interval_idle_secs, 120);
        assert_eq!(cfg.watch.idle_threshold_mins, 5);
        assert_eq!(cfg.watch.max_simultaneous, 2);
        assert_eq!(cfg.watch.cooldown_tras_fallo_secs, 30);
        assert_eq!(cfg.watch.ask_timeout_secs, 8);
        assert!(!cfg.watch.desktop_notify);
        assert_eq!(cfg.watch.notif_titulo, "titulo");
        assert_eq!(cfg.watch.notif_cuerpo, "cuerpo {modelo}");
    }

    #[test]
    fn app_config_toml_invalido_no_modifica_config() {
        let mut cfg = AppConfig::default();
        let min_file_size_default = cfg.min_file_size;
        let poll_default = cfg.watch.poll_interval_secs;

        assert!(cfg.aplicar_toml("[general").is_err());

        assert_eq!(cfg.min_file_size, min_file_size_default);
        assert_eq!(cfg.watch.poll_interval_secs, poll_default);
    }

    #[test]
    fn app_config_load_from_path_reporta_config_invalida() {
        let path = ruta_temporal("config.toml");
        fs::write(&path, "[general").expect("crea config invalida");

        let loaded = AppConfig::load_from_path(Some(path.clone()));

        assert_eq!(
            loaded.config.min_file_size,
            AppConfig::default().min_file_size
        );
        assert!(matches!(
            loaded.warnings.as_slice(),
            [ConfigWarning::ConfigInvalida { .. }]
        ));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn expandir_tilde_deja_rutas_normales_igual() {
        assert_eq!(
            expandir_tilde("/tmp/cbrec-videos"),
            PathBuf::from("/tmp/cbrec-videos")
        );
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

    #[test]
    fn watched_models_serializa_y_deserializa() {
        let watched = WatchedModels {
            models: vec!["alice".into(), "bob".into()],
        };

        let contenido = toml::to_string(&watched).expect("serializa watched.toml");
        let parsed = toml::from_str::<WatchedModels>(&contenido).expect("parsea watched.toml");

        assert_eq!(parsed.models, vec!["alice", "bob"]);
    }

    #[test]
    fn watched_models_save_to_path_escribe_archivo_valido() {
        let path = ruta_temporal("watched.toml");
        let watched = WatchedModels {
            models: vec!["alice".into()],
        };

        watched.save_to_path(&path).expect("guarda watched.toml");
        let contenido = fs::read_to_string(&path).expect("lee watched.toml");
        let parsed = toml::from_str::<WatchedModels>(&contenido).expect("parsea watched.toml");

        assert_eq!(parsed.models, vec!["alice"]);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn watched_models_save_to_path_reemplaza_archivo_existente() {
        let path = ruta_temporal("watched.toml");
        fs::write(&path, "models = [\"viejo\"]\n").expect("crea archivo anterior");
        let watched = WatchedModels {
            models: vec!["nuevo".into()],
        };

        watched.save_to_path(&path).expect("reemplaza watched.toml");
        let contenido = fs::read_to_string(&path).expect("lee watched.toml");

        assert!(contenido.contains("nuevo"));
        assert!(!contenido.contains("viejo"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn watched_models_update_in_path_guarda_con_lock() {
        let path = ruta_temporal("watched.toml");

        WatchedModels::update_in_path(&path, |watched| {
            let cambio = watched.add("alice");
            ((), cambio)
        })
        .expect("actualiza watched.toml");

        let contenido = fs::read_to_string(&path).expect("lee watched.toml");
        assert!(contenido.contains("alice"));
        assert!(!ruta_lock_para(&path).exists());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn watched_lock_bloqueado_falla_sin_esperar_en_tests() {
        let path = ruta_temporal("watched.toml");
        let lock_path = ruta_lock_para(&path);
        fs::write(&lock_path, "pid=123\n").expect("crea lock");

        let error = WatchedLock::acquire_with(&path, Duration::ZERO, Duration::from_secs(300))
            .expect_err("lock activo bloquea");

        assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
        let _ = fs::remove_file(lock_path);
    }

    #[test]
    fn watched_lock_vencido_se_recupera() {
        let path = ruta_temporal("watched.toml");
        let lock_path = ruta_lock_para(&path);
        fs::write(&lock_path, "pid=123\n").expect("crea lock");

        let lock = WatchedLock::acquire_with(&path, Duration::from_secs(1), Duration::ZERO)
            .expect("recupera lock vencido");

        assert!(lock_path.exists());
        drop(lock);
        assert!(!lock_path.exists());
    }

    #[test]
    fn lock_vencido_respeta_edad_y_reloj_futuro() {
        let ahora = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let viejo = ahora - Duration::from_secs(301);
        let reciente = ahora - Duration::from_secs(299);
        let futuro = ahora + Duration::from_secs(1);

        assert!(lock_vencido(viejo, ahora, Duration::from_secs(300)));
        assert!(!lock_vencido(reciente, ahora, Duration::from_secs(300)));
        assert!(!lock_vencido(futuro, ahora, Duration::from_secs(300)));
    }

    #[test]
    fn watched_models_load_from_path_respalda_archivo_invalido() {
        let path = ruta_temporal("watched.toml");
        fs::write(&path, "models = [").expect("crea watched invalido");

        let loaded = WatchedModels::load_from_path(&path);

        assert!(loaded.watched.models.is_empty());
        assert!(!path.exists());
        let backup_path = match loaded.warnings.as_slice() {
            [ConfigWarning::WatchedInvalidoRespaldado { backup_path, .. }] => backup_path.clone(),
            other => panic!("warning inesperado: {other:?}"),
        };
        assert!(backup_path.exists());
        let _ = fs::remove_file(backup_path);
    }

    fn ruta_temporal(nombre: &str) -> PathBuf {
        let nombre = nombre.replace('.', "_");
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        std::env::temp_dir().join(format!("cbrec_config_test_{}_{}", nombre, nanos))
    }
}
