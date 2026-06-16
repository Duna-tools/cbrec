use crate::application::utils::normalizar_modelo;
use crate::domain::repositories::StreamRepository;
use crate::infrastructure::ChaturbateClient;
use crate::presentation::Output;

pub(crate) async fn listar_calidades_modelos(
    client: &ChaturbateClient,
    salida: &dyn Output,
    modelos: Vec<String>,
) -> anyhow::Result<()> {
    for modelo in modelos {
        let model_name = normalizar_modelo(&modelo)?;
        let stream_url = client.get_stream_url(&model_name).await?;
        let Some(stream_url) = stream_url else {
            salida.mostrar_estado_modelo(model_name.as_str(), false);
            continue;
        };

        let calidades = client.listar_calidades(&stream_url).await?;
        if calidades.is_empty() {
            salida.mostrar_modelo_sin_variantes(model_name.as_str());
            continue;
        }

        let calidades_formato: Vec<(Option<u32>, Option<u64>)> =
            calidades.iter().map(|c| (c.height, c.bandwidth)).collect();
        salida.mostrar_calidades(model_name.as_str(), &calidades_formato);
    }

    Ok(())
}
