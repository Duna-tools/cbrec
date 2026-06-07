use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "cbrec")]
#[command(author, version, about = "Grabador de streams simple y eficiente")]
pub struct Cli {
    /// Modelos a grabar (modo principal).
    #[arg(value_name = "MODEL", num_args = 0.., index = 1)]
    pub modelos: Vec<String>,

    /// Solo verificar si el modelo esta online.
    #[arg(short = 'c', long = "check", global = true)]
    pub verificar: bool,

    /// Listar resoluciones disponibles del stream.
    #[arg(short = 'l', long = "list", global = true)]
    pub listar: bool,

    /// Directorio base de salida (se crea `cb_rec/<modelo>`).
    #[arg(short, long)]
    pub output: Option<String>,

    /// Ruta a ffmpeg.
    #[arg(long, global = true)]
    pub ffmpeg_path: Option<String>,

    /// Calidad de video (240p, 480p, 720p, 1080p, best).
    #[arg(short, long, default_value = "best")]
    pub quality: String,

    /// Limite de grabaciones simultaneas.
    #[arg(short = 'j', long, default_value_t = 3, global = true)]
    pub jobs: usize,

    /// Cookie de sesion de Chaturbate (sobreescribe config).
    /// Ejemplo: "PHPSESSID=abc123; chaturbatesid=xyz"
    /// Obtenerla: DevTools (F12) → Application → Cookies → chaturbate.com
    #[arg(long, global = true, value_name = "COOKIE")]
    pub session_cookie: Option<String>,

    /// Suprime output informativo. Solo muestra errores y resultados finales.
    #[arg(long, global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    /// Muestra informacion detallada siempre.
    #[arg(short = 'v', long, global = true, conflicts_with = "quiet")]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Graba un modelo o URL.
    Record {
        /// Modelos o URLs.
        #[arg(value_name = "MODEL", num_args = 1..)]
        modelos: Vec<String>,
        /// Directorio base de salida.
        #[arg(short, long)]
        output: Option<String>,
        /// Calidad de video (240p, 480p, 720p, 1080p, best).
        #[arg(short, long, default_value = "best")]
        quality: String,
    },

    /// Verifica si un modelo esta online.
    Check {
        /// Nombre del modelo.
        model: String,
    },

    /// Monitoriza modelos y graba automaticamente cuando se conectan.
    Watch {
        /// Modelos a monitorizar.
        #[arg(value_name = "MODEL", num_args = 1..)]
        modelos: Vec<String>,
        /// Pedir confirmacion antes de grabar cada modelo.
        #[arg(long)]
        ask: bool,
        /// Directorio base de salida.
        #[arg(short, long)]
        output: Option<String>,
        /// Calidad de video (240p, 480p, 720p, 1080p, best).
        #[arg(short, long, default_value = "best")]
        quality: String,
    },
}

