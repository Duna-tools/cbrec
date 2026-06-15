use crate::application::utils::extraer_nombre;
use crate::domain::value_objects::ModelName;
use crate::infrastructure::{ChaturbateClient, EstadoStream};
use crate::presentation::Output;

pub(crate) async fn verificar_modelo(
    client: &ChaturbateClient,
    salida: &dyn Output,
    model: &str,
) -> anyhow::Result<()> {
    let nombre = extraer_nombre(model);
    let model_name = ModelName::try_from(nombre.as_str())?;
    salida.mostrar_inicio_verificacion(model_name.as_str());

    match client.consultar_estado(&model_name).await? {
        EstadoStream::Online { .. } => salida.mostrar_estado_modelo(model_name.as_str(), true),
        EstadoStream::Offline => salida.mostrar_estado_modelo(model_name.as_str(), false),
        EstadoStream::RequiereSesion { detalle } => salida.mostrar_estado_modelo_detalle(
            model_name.as_str(),
            "SESION",
            &format!("requiere sesion o acceso privado ({detalle})"),
        ),
        EstadoStream::RateLimited => salida.mostrar_estado_modelo_detalle(
            model_name.as_str(),
            "RATE LIMIT",
            "Chaturbate limito las consultas; reintenta mas tarde",
        ),
        EstadoStream::Bloqueado { detalle } => salida.mostrar_estado_modelo_detalle(
            model_name.as_str(),
            "BLOQUEADO",
            &format!("respuesta bloqueada o challenge ({detalle})"),
        ),
        EstadoStream::RespuestaInesperada { detalle } => salida.mostrar_estado_modelo_detalle(
            model_name.as_str(),
            "INDETERMINADO",
            &format!("respuesta inesperada del API ({detalle})"),
        ),
    }

    Ok(())
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
