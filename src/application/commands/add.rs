use crate::application::utils::extraer_nombre;
use crate::infrastructure::WatchedModels;
use crate::presentation::Output;

pub(crate) fn agregar_modelos(modelos: Vec<String>, salida: &dyn Output) -> anyhow::Result<()> {
    let resultado = WatchedModels::update_with_warnings(|watched| {
        let mut hubo_cambio = false;

        for input in &modelos {
            let nombre = extraer_nombre(input);
            if watched.add(&nombre) {
                salida.modelo_agregado(&nombre);
                hubo_cambio = true;
            } else {
                salida.modelo_ya_en_lista(&nombre);
            }
        }

        ((), hubo_cambio)
    })?;
    for warning in resultado.warnings {
        salida.advertir_config(&warning.to_string());
    }
    Ok(())
}
