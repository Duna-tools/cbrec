use crate::application::utils::extraer_nombre;
use crate::infrastructure::WatchedModels;
use colored::Colorize;

pub(crate) fn agregar_modelos(modelos: Vec<String>) -> anyhow::Result<()> {
    let mut watched = WatchedModels::load();
    let mut hubo_cambio = false;

    for input in &modelos {
        let nombre = extraer_nombre(input);
        if watched.add(&nombre) {
            println!("{} Añadido: {}", "[OK]".green().bold(), nombre.cyan());
            hubo_cambio = true;
        } else {
            println!(
                "{} Ya en lista: {}",
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
