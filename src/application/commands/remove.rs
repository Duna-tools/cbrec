use crate::application::utils::normalizar_modelo;
use crate::domain::value_objects::ModelName;
use crate::infrastructure::WatchedModels;
use crate::presentation::Output;

pub(crate) fn eliminar_modelos(modelos: Vec<String>, salida: &dyn Output) -> anyhow::Result<()> {
    let modelos = modelos
        .iter()
        .map(|m| normalizar_modelo(m))
        .collect::<Result<Vec<_>, _>>()?;

    let resultado = WatchedModels::update_with_warnings(|watched| {
        let changed = remove_models(watched, &modelos, salida);
        ((), changed)
    })?;
    for warning in resultado.warnings {
        salida.advertir_config(&warning.to_string());
    }
    Ok(())
}

fn remove_models(watched: &mut WatchedModels, models: &[ModelName], output: &dyn Output) -> bool {
    let mut changed = false;

    for model in models {
        if watched.remove(model.as_str()) {
            output.modelo_eliminado(model.as_str());
            changed = true;
        } else {
            output.modelo_no_encontrado_en_lista(model.as_str());
        }
    }

    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presentation::ConsoleOutput;

    #[test]
    fn removes_existing_models_and_ignores_missing_ones() {
        let mut watched = WatchedModels {
            models: vec!["alice".to_string(), "bob".to_string()],
        };
        let models = [
            ModelName::try_from("alice").unwrap(),
            ModelName::try_from("carol").unwrap(),
        ];

        let changed = remove_models(&mut watched, &models, &ConsoleOutput::new(false, true));

        assert!(changed);
        assert_eq!(watched.models, ["bob"]);
    }

    #[test]
    fn rejects_invalid_model_before_accessing_persistence() {
        let error = eliminar_modelos(vec!["invalid model".to_string()], &ConsoleOutput::default())
            .expect_err("invalid model must fail");

        assert!(error.to_string().contains("Invalid model name"));
    }
}
