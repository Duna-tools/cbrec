use crate::domain::repositories::StreamRepository;
use crate::domain::value_objects::ModelName;
use crate::infrastructure::ChaturbateClient;
use crate::presentation::Output;

pub(crate) async fn verificar_modelo(
    client: &ChaturbateClient,
    salida: &dyn Output,
    model: &str,
) -> anyhow::Result<()> {
    let model_name = ModelName::try_from(model)?;
    salida.mostrar_inicio_verificacion(model_name.as_str());

    match client.get_stream_url(&model_name).await? {
        Some(_) => salida.mostrar_estado_modelo(model_name.as_str(), true),
        None => salida.mostrar_estado_modelo(model_name.as_str(), false),
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
