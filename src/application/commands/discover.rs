//! Discovers public rooms by tag without changing account or recording state.

use crate::infrastructure::external::{DiscoveredRoom, EstadoStream, RoomPage};
use crate::infrastructure::ChaturbateClient;
use crate::presentation::Output;
use serde::Serialize;

const MAX_RESULTS: usize = 50;

/// Validated discovery response shared by terminal presentation adapters.
#[derive(Serialize)]
pub(crate) struct DiscoveryResult {
    /// Normalized tag without a leading hash.
    pub(crate) tag: String,
    /// Public rooms matching the requested tag and limit.
    pub(crate) rooms: Vec<DiscoveredRoom>,
}

pub(crate) async fn discover_rooms(
    client: &ChaturbateClient,
    output: &dyn Output,
    raw_tag: &str,
    limit: usize,
) -> anyhow::Result<()> {
    let result = find_rooms(client, raw_tag, limit).await?;
    if result.rooms.is_empty() {
        output.discovery_empty(&result.tag);
        return Ok(());
    }

    output.discovery_started(&result.tag, result.rooms.len());
    for room in result.rooms {
        output.discovery_room(&room.username, room.viewers, &room.show, &room.subject);
    }
    Ok(())
}

/// Returns one compact JSON document for a tag discovery query.
pub(crate) async fn discover_rooms_json(
    client: &ChaturbateClient,
    raw_tag: &str,
    limit: usize,
) -> anyhow::Result<String> {
    Ok(serde_json::to_string(
        &find_rooms(client, raw_tag, limit).await?,
    )?)
}

/// Validates the query and returns at most 50 matching public rooms.
pub(crate) async fn find_rooms(
    client: &ChaturbateClient,
    raw_tag: &str,
    limit: usize,
) -> anyhow::Result<DiscoveryResult> {
    let tag = normalize_tag(raw_tag)?;
    if !(1..=MAX_RESULTS).contains(&limit) {
        anyhow::bail!("El limite debe estar entre 1 y {MAX_RESULTS}");
    }

    let rooms = client.discover_rooms_by_tag(&tag, limit).await?;
    Ok(DiscoveryResult { tag, rooms })
}

pub(crate) fn normalize_tag(raw_tag: &str) -> anyhow::Result<String> {
    let tag = raw_tag.trim().strip_prefix('#').unwrap_or(raw_tag.trim());
    if tag.is_empty()
        || tag.len() > 50
        || !tag
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        anyhow::bail!("Tag invalido: usa 1-50 letras, numeros, '_' o '-'");
    }
    Ok(tag.to_ascii_lowercase())
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum DiscoveryQuery {
    Popular,
    Tag(String),
    Model(crate::domain::value_objects::ModelName),
}

pub(crate) fn parse_query(raw: &str) -> anyhow::Result<DiscoveryQuery> {
    use crate::domain::value_objects::ModelName;
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(DiscoveryQuery::Popular);
    }
    let model = if let Some(model) = raw.strip_prefix('@') {
        model.to_string()
    } else if raw.contains("://")
        || raw.starts_with("chaturbate.com/")
        || raw.starts_with("www.chaturbate.com/")
    {
        let url = url::Url::parse(&if raw.contains("://") {
            raw.into()
        } else {
            format!("https://{raw}")
        })?;
        let host = url.host_str().unwrap_or_default();
        anyhow::ensure!(
            matches!(url.scheme(), "https" | "http")
                && crate::application::utils::is_chaturbate_host(host)
                && url.username().is_empty()
                && url.password().is_none()
                && url.port().is_none(),
            "Usa una URL de perfil de chaturbate.com"
        );
        let path = url.path().trim_matches('/');
        anyhow::ensure!(
            !path.contains('/'),
            "La URL debe apuntar a un perfil, no a una sección"
        );
        path.into()
    } else {
        return Ok(DiscoveryQuery::Tag(normalize_tag(raw)?));
    };
    anyhow::ensure!(
        model.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
        "Nombre de usuario inválido"
    );
    Ok(DiscoveryQuery::Model(ModelName::try_from(model)?))
}

pub(crate) async fn find_page(
    client: &ChaturbateClient,
    raw: &str,
    limit: usize,
    offset: usize,
) -> anyhow::Result<(String, RoomPage)> {
    anyhow::ensure!(
        (1..=MAX_RESULTS).contains(&limit),
        "El límite debe estar entre 1 y {MAX_RESULTS}"
    );
    match parse_query(raw)? {
        DiscoveryQuery::Popular => Ok((
            String::new(),
            client.discover_room_page(None, limit, offset).await?,
        )),
        DiscoveryQuery::Tag(tag) => {
            let page = client.discover_room_page(Some(&tag), limit, offset).await?;
            Ok((format!("#{tag}"), page))
        }
        DiscoveryQuery::Model(model) => {
            anyhow::ensure!(offset == 0, "Un perfil no tiene páginas adicionales");
            let show = match client.consultar_estado(&model).await? {
                EstadoStream::Online { .. } => "online",
                EstadoStream::Offline => "offline",
                EstadoStream::RequiereSesion { .. } => "requiere sesión o permisos",
                EstadoStream::RateLimited => {
                    anyhow::bail!("El sitio limitó las consultas; espera antes de reintentar")
                }
                EstadoStream::Bloqueado { .. } => {
                    anyhow::bail!("El sitio bloqueó la consulta del perfil")
                }
                EstadoStream::RespuestaInesperada { .. } => {
                    anyhow::bail!("El sitio devolvió una respuesta de perfil no reconocida")
                }
            };
            Ok((format!("@{model}"), RoomPage {
                rooms: vec![DiscoveredRoom { username: model.to_string(), thumbnail_url: None, subject: "Perfil consultado directamente. Abre la sala para ver vídeo y chat; grabar requiere una emisión accesible.".into(), viewers: 0, show: show.into() }],
                total: Some(1), has_more: false,
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_distinguishes_catalog_tags_and_profile_urls() {
        assert_eq!(parse_query(" ").unwrap(), DiscoveryQuery::Popular);
        assert_eq!(
            parse_query("Gaming").unwrap(),
            DiscoveryQuery::Tag("gaming".into())
        );
        assert_eq!(
            parse_query("#Gaming").unwrap(),
            DiscoveryQuery::Tag("gaming".into())
        );
        for raw in [
            "@Alice_1",
            "https://chaturbate.com/Alice_1/",
            "https://es.chaturbate.com/alice_1/?source=test#bio",
            "chaturbate.com/alice_1/",
        ] {
            assert!(
                matches!(parse_query(raw).unwrap(), DiscoveryQuery::Model(model) if model.as_str() == "alice_1"),
                "{raw}"
            );
        }
        for raw in [
            "https://evil.example/alice",
            "https://chaturbate.com.evil.example/alice",
            "https://chaturbate.com@evil.example/alice",
            "https://u:p@chaturbate.com/alice",
            "https://chaturbate.com:8443/alice",
            "file:///alice",
            "https://chaturbate.com/",
            "https://chaturbate.com/tags/gaming",
            "@",
            "@bad/name",
            "@álice",
            "#bad tag",
        ] {
            assert!(parse_query(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn normalize_tag_accepts_hash_and_rejects_unsafe_input() {
        assert_eq!(normalize_tag(" #Gaming ").unwrap(), "gaming");
        assert!(normalize_tag("bad tag").is_err());
        assert!(normalize_tag("\u{1b}[31m").is_err());
    }

    #[test]
    fn discovery_json_has_tag_and_rooms() {
        let result = DiscoveryResult {
            tag: "gaming".to_string(),
            rooms: vec![DiscoveredRoom {
                thumbnail_url: None,
                username: "alice".to_string(),
                subject: "hello".to_string(),
                viewers: 42,
                show: "public".to_string(),
            }],
        };

        assert_eq!(
            serde_json::to_string(&result).unwrap(),
            r#"{"tag":"gaming","rooms":[{"username":"alice","subject":"hello","viewers":42,"show":"public"}]}"#
        );
    }
}
