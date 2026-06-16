use colored::Colorize;
use std::path::Path;

pub trait Output: Send + Sync {
    fn is_verbose(&self) -> bool {
        false
    }
    fn is_quiet(&self) -> bool {
        false
    }
    fn advertir_limite_concurrencia(&self, recomendado: usize, solicitado: usize);
    fn mostrar_error_sin_modelo(&self);
    fn advertir_modelos_duplicados(&self, duplicados: usize);
    fn advertir_modelos_sobre_limite(&self, total: usize, limite: usize);
    fn advertir_no_se_pudo_guardar_lista(&self, error: &str);
    fn advertir_error_consulta_estado(&self, modelo: &str, error: &str);
    fn advertir_config(&self, warning: &str);
    fn modelo_agregado(&self, modelo: &str);
    fn modelo_ya_en_lista(&self, modelo: &str);
    fn modelo_eliminado(&self, modelo: &str);
    fn modelo_no_encontrado_en_lista(&self, modelo: &str);
    fn error_fallo_grabacion(&self, modelo: &str, error: &str);
    fn error_tarea_abortada(&self, error: &str);
    fn mostrar_inicio_detallado(&self, modelo: &str, calidad: &str);
    fn mostrar_inicio_resumido(&self, modelo: &str, calidad: &str);
    fn mostrar_verificando_disponibilidad(&self);
    fn mostrar_modelo_offline_detallado(&self, modelo: &str);
    fn mostrar_modelo_offline_resumido(&self, modelo: &str);
    fn mostrar_modelo_online_detallado(&self);
    fn mostrar_detalle_inicio_grabacion(&self, ruta: &Path);
    fn mostrar_cancelacion_detallada(&self);
    fn mostrar_cancelacion_resumida(&self, modelo: &str);
    fn mostrar_archivo_pequeno_detallado(&self, bytes: u64, destino: &Path);
    fn mostrar_archivo_pequeno_resumido(&self, modelo: &str, destino: &Path);
    fn mostrar_archivo_guardado_detallado(&self, ruta: &Path);
    fn mostrar_archivo_guardado_resumido(&self, modelo: &str, ruta: &Path);
    fn mostrar_inicio_verificacion(&self, modelo: &str);
    fn mostrar_estado_modelo(&self, modelo: &str, online: bool);
    fn mostrar_estado_modelo_detalle(&self, _modelo: &str, _estado: &str, _detalle: &str) {}
    fn mostrar_modelo_sin_variantes(&self, modelo: &str);
    fn mostrar_calidades(&self, modelo: &str, calidades: &[(Option<u32>, Option<u64>)]);
    fn mostrar_progreso_grabacion(&self, _modelo: &str, _bytes: u64) {}
    fn doctor_inicio(&self) {}
    fn doctor_ok(&self, _area: &str, _detalle: &str) {}
    fn doctor_warn(&self, _area: &str, _detalle: &str) {}
    fn doctor_error(&self, _area: &str, _detalle: &str) {}
    fn doctor_resumen(&self, _fallos: usize, _advertencias: usize) {}
    fn watch_inicio(&self, modelos: &[&str]);
    fn watch_tick_online(&self, modelo: &str);
    fn watch_tick_offline(&self, modelo: &str);
    fn watch_inicio_grabacion(&self, modelo: &str);
    fn watch_fin_grabacion(&self, modelo: &str, ruta: &Path);
    fn watch_modelo_omitido(&self, modelo: &str);
    fn watch_proximo_check(&self, secs: u64);
    fn watch_deteniendo(&self);
}

pub struct ConsoleOutput {
    verbose: bool,
    quiet: bool,
}

impl ConsoleOutput {
    pub fn new(verbose: bool, quiet: bool) -> Self {
        Self { verbose, quiet }
    }
}

impl Default for ConsoleOutput {
    fn default() -> Self {
        Self::new(false, false)
    }
}

impl Output for ConsoleOutput {
    fn is_verbose(&self) -> bool {
        self.verbose
    }
    fn is_quiet(&self) -> bool {
        self.quiet
    }

    fn advertir_limite_concurrencia(&self, recomendado: usize, solicitado: usize) {
        eprintln!(
            "{} El limite recomendado es {}. Se solicito {}",
            "[WARN]".yellow().bold(),
            recomendado,
            solicitado
        );
    }

    fn mostrar_error_sin_modelo(&self) {
        eprintln!(
            "{} Debes especificar un modelo o comando",
            "Error:".red().bold()
        );
        eprintln!("Uso: cbrec <nombremodelo> [<nombremodelo> ...]");
        eprintln!("     cbrec check <nombremodelo>");
    }

    fn advertir_modelos_duplicados(&self, duplicados: usize) {
        eprintln!(
            "{} Se omitieron {} modelo(s) duplicados",
            "[WARN]".yellow().bold(),
            duplicados
        );
    }

    fn advertir_modelos_sobre_limite(&self, total: usize, limite: usize) {
        eprintln!(
            "{} Se solicitaron {} modelos; el limite concurrente es {}",
            "[WARN]".yellow().bold(),
            total,
            limite
        );
    }

    fn advertir_no_se_pudo_guardar_lista(&self, error: &str) {
        eprintln!("[WARN] No se pudo guardar lista de modelos: {}", error);
    }

    fn advertir_error_consulta_estado(&self, modelo: &str, error: &str) {
        eprintln!("[WARN][{}] Error al consultar estado: {}", modelo, error);
    }

    fn advertir_config(&self, warning: &str) {
        eprintln!("{} {}", "[WARN]".yellow().bold(), warning);
    }

    fn modelo_agregado(&self, modelo: &str) {
        println!("{} Añadido: {}", "[OK]".green().bold(), modelo.cyan());
    }

    fn modelo_ya_en_lista(&self, modelo: &str) {
        println!(
            "{} Ya en lista: {}",
            "[WARN]".yellow().bold(),
            modelo.cyan()
        );
    }

    fn modelo_eliminado(&self, modelo: &str) {
        println!("{} Eliminado: {}", "[OK]".green().bold(), modelo.cyan());
    }

    fn modelo_no_encontrado_en_lista(&self, modelo: &str) {
        println!(
            "{} No encontrado: {}",
            "[WARN]".yellow().bold(),
            modelo.cyan()
        );
    }

    fn error_fallo_grabacion(&self, modelo: &str, error: &str) {
        eprintln!(
            "{} Fallo grabacion para {}: {}",
            "[ERROR]".red().bold(),
            modelo.cyan(),
            error
        );
    }

    fn error_tarea_abortada(&self, error: &str) {
        eprintln!("{} Tarea abortada: {}", "[ERROR]".red().bold(), error);
    }

    fn mostrar_inicio_detallado(&self, modelo: &str, calidad: &str) {
        if self.quiet {
            return;
        }
        println!("=== cbrec - Stream Recorder ===\n");
        println!("Modelo:  {}", modelo.cyan());
        println!("Calidad: {}", calidad);
    }

    fn mostrar_inicio_resumido(&self, modelo: &str, calidad: &str) {
        if self.quiet {
            return;
        }
        println!("[{}] Inicio grabacion (calidad {})", modelo.cyan(), calidad);
    }

    fn mostrar_verificando_disponibilidad(&self) {
        if self.quiet {
            return;
        }
        println!("Verificando disponibilidad...");
    }

    fn mostrar_modelo_offline_detallado(&self, modelo: &str) {
        println!(
            "\n{} El modelo '{}' no esta online o no se pudo obtener el stream",
            "[ERROR]".red().bold(),
            modelo.cyan()
        );
        println!("\nPuedes verificar el estado con: cbrec check {}", modelo);
    }

    fn mostrar_modelo_offline_resumido(&self, modelo: &str) {
        println!("[{}] {}", modelo.cyan(), "OFFLINE".yellow());
    }

    fn mostrar_modelo_online_detallado(&self) {
        if self.quiet {
            return;
        }
        println!("{} Modelo online\n", "[OK]".green().bold());
    }

    fn mostrar_detalle_inicio_grabacion(&self, ruta: &Path) {
        if self.quiet {
            return;
        }
        println!("Archivo: {}", ruta.display().to_string().bright_black());
        println!("\nIniciando grabacion...");
        println!("Presiona Ctrl+C para detener\n");
        println!("---");
    }

    fn mostrar_cancelacion_detallada(&self) {
        println!("\n{} Grabacion cancelada", "[WARN]".yellow().bold());
    }

    fn mostrar_cancelacion_resumida(&self, modelo: &str) {
        println!("[{}] Cancelada", modelo.cyan());
    }

    fn mostrar_archivo_pequeno_detallado(&self, bytes: u64, destino: &Path) {
        eprintln!(
            "\n{} Archivo muy pequeno ({} bytes), movido a {}",
            "[WARN]".yellow().bold(),
            bytes,
            destino.display().to_string().bright_black()
        );
    }

    fn mostrar_archivo_pequeno_resumido(&self, modelo: &str, destino: &Path) {
        eprintln!(
            "[{}] Archivo pequeno, movido a {}",
            modelo.cyan(),
            destino.display().to_string().bright_black()
        );
    }

    fn mostrar_archivo_guardado_detallado(&self, ruta: &Path) {
        println!(
            "\n{} Archivo guardado: {}",
            "[OK]".green().bold(),
            ruta.display().to_string().bright_black()
        );
    }

    fn mostrar_archivo_guardado_resumido(&self, modelo: &str, ruta: &Path) {
        println!(
            "[{}] Guardado: {}",
            modelo.cyan(),
            ruta.display().to_string().bright_black()
        );
    }

    fn mostrar_inicio_verificacion(&self, modelo: &str) {
        if self.quiet {
            return;
        }
        println!("Verificando estado de: {}", modelo.cyan());
    }

    fn mostrar_estado_modelo(&self, modelo: &str, online: bool) {
        if online {
            println!(
                "{} {} esta {}",
                "[OK]".green().bold(),
                modelo.cyan(),
                "ONLINE".green()
            );
        } else {
            println!(
                "{} {} esta {}",
                "[OFFLINE]".yellow(),
                modelo.cyan(),
                "OFFLINE".yellow()
            );
        }
    }

    fn mostrar_estado_modelo_detalle(&self, modelo: &str, estado: &str, detalle: &str) {
        println!(
            "[{}] {}: {}",
            estado.yellow().bold(),
            modelo.cyan(),
            detalle
        );
    }

    fn mostrar_modelo_sin_variantes(&self, modelo: &str) {
        println!("[{}] Sin variantes en playlist", modelo.cyan());
    }

    fn mostrar_calidades(&self, modelo: &str, calidades: &[(Option<u32>, Option<u64>)]) {
        println!("[{}] {}", modelo.cyan(), formatear_calidades(calidades));
    }

    fn mostrar_progreso_grabacion(&self, modelo: &str, bytes: u64) {
        if self.quiet {
            return;
        }
        let mb = bytes as f64 / 1_048_576.0;
        println!(
            "[{}][{}] Grabando... {}",
            ahora().bright_black(),
            modelo.cyan(),
            format!("{:.1} MB", mb).bright_blue()
        );
    }

    fn doctor_inicio(&self) {
        if self.quiet {
            return;
        }
        println!("=== cbrec doctor ===");
    }

    fn doctor_ok(&self, area: &str, detalle: &str) {
        if self.quiet {
            return;
        }
        println!("{} {}: {}", "[OK]".green().bold(), area.cyan(), detalle);
    }

    fn doctor_warn(&self, area: &str, detalle: &str) {
        eprintln!("{} {}: {}", "[WARN]".yellow().bold(), area.cyan(), detalle);
    }

    fn doctor_error(&self, area: &str, detalle: &str) {
        eprintln!("{} {}: {}", "[ERROR]".red().bold(), area.cyan(), detalle);
    }

    fn doctor_resumen(&self, fallos: usize, advertencias: usize) {
        if fallos == 0 {
            println!(
                "{} doctor completo ({} advertencia(s))",
                "[OK]".green().bold(),
                advertencias
            );
        } else {
            eprintln!(
                "{} doctor encontro {} fallo(s) y {} advertencia(s)",
                "[ERROR]".red().bold(),
                fallos,
                advertencias
            );
        }
    }

    fn watch_inicio(&self, modelos: &[&str]) {
        if self.quiet {
            return;
        }
        println!("=== cbrec watch iniciado ===");
        println!(
            "Monitorizando: {}",
            modelos
                .iter()
                .map(|m| m.cyan().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        println!("Presiona Ctrl+C para detener\n");
    }

    fn watch_tick_online(&self, modelo: &str) {
        println!(
            "[{}][{}] {}",
            ahora().bright_black(),
            modelo.cyan(),
            "ONLINE detectado".green()
        );
    }

    fn watch_tick_offline(&self, modelo: &str) {
        if self.quiet {
            return;
        }
        println!(
            "[{}][{}] {}",
            ahora().bright_black(),
            modelo.cyan(),
            "offline".yellow()
        );
    }

    fn watch_inicio_grabacion(&self, modelo: &str) {
        println!(
            "[{}][{}] Iniciando grabacion...",
            ahora().bright_black(),
            modelo.cyan()
        );
    }

    fn watch_fin_grabacion(&self, modelo: &str, ruta: &Path) {
        println!(
            "[{}][{}] {} {}",
            ahora().bright_black(),
            modelo.cyan(),
            "Grabacion finalizada →".green(),
            ruta.display().to_string().bright_black()
        );
    }

    fn watch_modelo_omitido(&self, modelo: &str) {
        println!(
            "[{}][{}] Omitido por el usuario",
            ahora().bright_black(),
            modelo.cyan()
        );
    }

    fn watch_proximo_check(&self, secs: u64) {
        if self.quiet {
            return;
        }
        println!("[{}] Próximo ciclo en {} s", ahora().bright_black(), secs);
    }

    fn watch_deteniendo(&self) {
        println!(
            "\n[{}] {}",
            ahora().bright_black(),
            "Deteniendo daemon watch...".yellow()
        );
    }
}

fn formatear_calidades(calidades: &[(Option<u32>, Option<u64>)]) -> String {
    let mut items: Vec<String> = calidades
        .iter()
        .map(|(height, bandwidth)| match (height, bandwidth) {
            (Some(h), Some(bw)) => format!("{}p({}kbps)", h, bw / 1000),
            (Some(h), None) => format!("{}p", h),
            (None, Some(bw)) => format!("{}kbps", bw / 1000),
            (None, None) => "desconocida".to_string(),
        })
        .collect();
    items.sort();
    items.join(", ")
}

fn ahora() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}
