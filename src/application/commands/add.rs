use crate::application::utils::normalizar_modelo;
use crate::infrastructure::WatchedModels;
use crate::presentation::Output;

pub(crate) fn agregar_modelos(modelos: Vec<String>, salida: &dyn Output) -> anyhow::Result<()> {
    let modelos = modelos
        .iter()
        .map(|m| normalizar_modelo(m))
        .collect::<Result<Vec<_>, _>>()?;

    let resultado = WatchedModels::update_with_warnings(|watched| {
        let mut hubo_cambio = false;

        for nombre in &modelos {
            if watched.add(nombre.as_str()) {
                salida.modelo_agregado(nombre.as_str());
                hubo_cambio = true;
            } else {
                salida.modelo_ya_en_lista(nombre.as_str());
            }
        }

        ((), hubo_cambio)
    })?;
    for warning in resultado.warnings {
        salida.advertir_config(&warning.to_string());
    }
    Ok(())
}
