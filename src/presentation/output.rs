use std::path::Path;

pub struct ConsoleOutput;

impl ConsoleOutput {
    pub fn new() -> Self {
        Self
    }

    pub fn advertir_limite_concurrencia(&self, recomendado: usize, solicitado: usize) {
        println!(
            "[WARN] El limite recomendado es {}. Se solicito {}",
            recomendado, solicitado
        );
    }

    pub fn mostrar_error_sin_modelo(&self) {
        println!("Error: Debes especificar un modelo o comando");
        println!("Uso: cbrec <nombremodelo> [<nombremodelo> ...]");
        println!("     cbrec check <nombremodelo>");
    }

    pub fn advertir_modelos_duplicados(&self, duplicados: usize) {
        println!("[WARN] Se omitieron {} modelo(s) duplicados", duplicados);
    }

    pub fn advertir_modelos_sobre_limite(&self, total: usize, limite: usize) {
        println!(
            "[WARN] Se solicitaron {} modelos; el limite concurrente es {}",
            total, limite
        );
    }

    pub fn error_fallo_grabacion(&self, modelo: &str, error: &str) {
        println!("[ERROR] Fallo grabacion para {}: {}", modelo, error);
    }

    pub fn error_tarea_abortada(&self, error: &str) {
        println!("[ERROR] Tarea abortada: {}", error);
    }

    pub fn mostrar_inicio_detallado(&self, modelo: &str, calidad: &str) {
        println!("=== cbrec - Stream Recorder ===\n");
        println!("Modelo: {}", modelo);
        println!("Calidad: {}", calidad);
    }

    pub fn mostrar_inicio_resumido(&self, modelo: &str, calidad: &str) {
        println!("[{}] Inicio grabacion (calidad {})", modelo, calidad);
    }

    pub fn mostrar_verificando_disponibilidad(&self) {
        println!("Verificando disponibilidad...");
    }

    pub fn mostrar_modelo_offline_detallado(&self, modelo: &str) {
        println!(
            "\n[ERROR] El modelo '{}' no esta online o no se pudo obtener el stream",
            modelo
        );
        println!("\nPuedes verificar el estado con: cbrec check {}", modelo);
    }

    pub fn mostrar_modelo_offline_resumido(&self, modelo: &str) {
        println!("[{}] OFFLINE", modelo);
    }

    pub fn mostrar_modelo_online_detallado(&self) {
        println!("[OK] Modelo online\n");
    }

    pub fn mostrar_detalle_inicio_grabacion(&self, ruta: &Path) {
        println!("Archivo: {}", ruta.display());
        println!("\nIniciando grabacion...");
        println!("Presiona Ctrl+C para detener\n");
        println!("---");
    }

    pub fn mostrar_cancelacion_detallada(&self) {
        println!("\n[WARN] Grabacion cancelada");
    }

    pub fn mostrar_cancelacion_resumida(&self, modelo: &str) {
        println!("[{}] Cancelada", modelo);
    }

    pub fn mostrar_archivo_pequeno_detallado(&self, bytes: u64) {
        println!("\n[WARN] Archivo muy pequeno ({} bytes), eliminando", bytes);
    }

    pub fn mostrar_archivo_pequeno_resumido(&self, modelo: &str) {
        println!("[{}] Archivo muy pequeno, eliminando", modelo);
    }

    pub fn mostrar_archivo_guardado_detallado(&self, ruta: &Path) {
        println!("\n[OK] Archivo guardado: {}", ruta.display());
    }

    pub fn mostrar_archivo_guardado_resumido(&self, modelo: &str, ruta: &Path) {
        println!("[{}] Guardado: {}", modelo, ruta.display());
    }

    pub fn mostrar_inicio_verificacion(&self, modelo: &str) {
        println!("Verificando estado de: {}", modelo);
    }

    pub fn mostrar_estado_modelo(&self, modelo: &str, online: bool) {
        if online {
            println!("[OK] {} esta ONLINE", modelo);
        } else {
            println!("[OFFLINE] {} esta OFFLINE", modelo);
        }
    }

    pub fn mostrar_modelo_sin_variantes(&self, modelo: &str) {
        println!("[{}] Sin variantes en playlist", modelo);
    }

    pub fn mostrar_calidades(&self, modelo: &str, calidades: &[(Option<u32>, Option<u64>)]) {
        println!("[{}] {}", modelo, formatear_calidades(calidades));
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
