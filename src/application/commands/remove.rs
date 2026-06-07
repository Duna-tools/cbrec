use crate::infrastructure::WatchedModels;
use colored::Colorize;

pub(crate) fn eliminar_modelos(modelos: Vec<String>) -> anyhow::Result<()> {
    let mut watched = WatchedModels::load();
    let mut hubo_cambio = false;

    for nombre in &modelos {
        if watched.remove(nombre) {
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
