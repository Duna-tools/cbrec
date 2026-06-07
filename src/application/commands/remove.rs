use crate::application::utils::extraer_nombre;
use crate::infrastructure::WatchedModels;
use colored::Colorize;

pub(crate) fn eliminar_modelos(modelos: Vec<String>) -> anyhow::Result<()> {
    let mut watched = WatchedModels::load();
    let mut hubo_cambio = false;

    for input in &modelos {
        let nombre = extraer_nombre(input);
        if watched.remove(&nombre) {
            println!("{} Eliminado: {}", "[OK]".green().bold(), nombre.cyan());
            hubo_cambio = true;
        } else {
            println!(
                "{} No encontrado: {}",
                "[WARN]".yellow().bold(),
                nombre.cyan()
            );
        }
    }

    if hubo_cambio {
        watched.save()?;
    }

    Ok(())
}
