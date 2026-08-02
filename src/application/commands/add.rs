use crate::application::utils::normalizar_modelo;
use crate::domain::value_objects::ModelName;
use crate::infrastructure::WatchedModels;
use crate::presentation::Output;

pub(crate) fn agregar_modelos(modelos: Vec<String>, salida: &dyn Output) -> anyhow::Result<()> {
    let modelos = modelos
        .iter()
        .map(|m| normalizar_modelo(m))
        .collect::<Result<Vec<_>, _>>()?;

    let resultado = WatchedModels::update_with_warnings(|watched| {
        let changed = add_models(watched, &modelos, salida);
        ((), changed)
    })?;
    for warning in resultado.warnings {
        salida.advertir_config(&warning.to_string());
    }
    Ok(())
}

fn add_models(watched: &mut WatchedModels, models: &[ModelName], output: &dyn Output) -> bool {
    let mut changed = false;

    for model in models {
        if watched.add(model.as_str()) {
            output.modelo_agregado(model.as_str());
            changed = true;
        } else {
            output.modelo_ya_en_lista(model.as_str());
        }
    }

    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presentation::ConsoleOutput;

    #[test]
    fn adds_only_models_not_already_watched() {
        let mut watched = WatchedModels {
            models: vec!["alice".to_string()],
        };
        let models = [
            ModelName::try_from("alice").unwrap(),
            ModelName::try_from("bob").unwrap(),
        ];

        let changed = add_models(&mut watched, &models, &ConsoleOutput::new(false, true));

        assert!(changed);
        assert_eq!(watched.models, ["alice", "bob"]);
    }

    #[test]
    fn rejects_invalid_model_before_accessing_persistence() {
        let error = agregar_modelos(vec!["invalid model".to_string()], &ConsoleOutput::default())
            .expect_err("invalid model must fail");

        assert!(error.to_string().contains("Invalid model name"));
    }
}
