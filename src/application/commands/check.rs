use crate::application::utils::normalizar_modelo;
use crate::domain::value_objects::ModelName;
use crate::infrastructure::{ChaturbateClient, EstadoStream};
use crate::presentation::Output;
use serde::Serialize;

#[derive(Serialize)]
struct ModelCheck {
    model: String,
    status: CheckStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum CheckStatus {
    Online,
    Offline,
    RequiresSession,
    RateLimited,
    Blocked,
    Indeterminate,
}

pub(crate) async fn verificar_modelo(
    client: &ChaturbateClient,
    salida: &dyn Output,
    model: &str,
) -> anyhow::Result<()> {
    let model = normalizar_modelo(model)?;
    salida.mostrar_inicio_verificacion(model.as_str());
    let result = check_model(client, &model).await?;

    match result.status {
        CheckStatus::Online => salida.mostrar_estado_modelo(&result.model, true),
        CheckStatus::Offline => salida.mostrar_estado_modelo(&result.model, false),
        CheckStatus::RequiresSession => salida.mostrar_estado_modelo_detalle(
            &result.model,
            "SESION",
            result.detail.as_deref().unwrap_or_default(),
        ),
        CheckStatus::RateLimited => salida.mostrar_estado_modelo_detalle(
            &result.model,
            "RATE LIMIT",
            result.detail.as_deref().unwrap_or_default(),
        ),
        CheckStatus::Blocked => salida.mostrar_estado_modelo_detalle(
            &result.model,
            "BLOQUEADO",
            result.detail.as_deref().unwrap_or_default(),
        ),
        CheckStatus::Indeterminate => salida.mostrar_estado_modelo_detalle(
            &result.model,
            "INDETERMINADO",
            result.detail.as_deref().unwrap_or_default(),
        ),
    }

    Ok(())
}

/// Returns one compact JSON document for a model status query.
pub(crate) async fn check_model_json(
    client: &ChaturbateClient,
    model: &str,
) -> anyhow::Result<String> {
    let model = normalizar_modelo(model)?;
    Ok(serde_json::to_string(&check_model(client, &model).await?)?)
}

async fn check_model(client: &ChaturbateClient, model: &ModelName) -> anyhow::Result<ModelCheck> {
    let (status, detail) = match client.consultar_estado(model).await? {
        EstadoStream::Online { .. } => (CheckStatus::Online, None),
        EstadoStream::Offline => (CheckStatus::Offline, None),
        EstadoStream::RequiereSesion { detalle } => (
            CheckStatus::RequiresSession,
            Some(format!("requiere sesion o acceso privado ({detalle})")),
        ),
        EstadoStream::RateLimited => (
            CheckStatus::RateLimited,
            Some("Chaturbate limito las consultas; reintenta mas tarde".to_string()),
        ),
        EstadoStream::Bloqueado { detalle } => (
            CheckStatus::Blocked,
            Some(format!("respuesta bloqueada o challenge ({detalle})")),
        ),
        EstadoStream::RespuestaInesperada { detalle } => (
            CheckStatus::Indeterminate,
            Some(format!("respuesta inesperada del API ({detalle})")),
        ),
    };
    Ok(ModelCheck {
        model: model.as_str().to_string(),
        status,
        detail,
    })
}

pub(crate) async fn verificar_modelos(
    client: &ChaturbateClient,
    salida: &dyn Output,
    modelos: Vec<String>,
) -> anyhow::Result<()> {
    for modelo in modelos {
        verificar_modelo(client, salida, &modelo).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_check_json_uses_stable_status_names() {
        let result = ModelCheck {
            model: "alice".to_string(),
            status: CheckStatus::RequiresSession,
            detail: Some("private".to_string()),
        };

        assert_eq!(
            serde_json::to_string(&result).unwrap(),
            r#"{"model":"alice","status":"requires_session","detail":"private"}"#
        );
    }
}
