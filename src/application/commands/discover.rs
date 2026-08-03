//! Discovers public rooms by tag without changing account or recording state.

use crate::infrastructure::external::DiscoveredRoom;
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

fn normalize_tag(raw_tag: &str) -> anyhow::Result<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
