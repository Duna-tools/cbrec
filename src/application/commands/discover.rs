//! Discovers public rooms by tag without changing account or recording state.

use crate::infrastructure::ChaturbateClient;
use crate::presentation::Output;

const MAX_RESULTS: usize = 50;

pub(crate) async fn discover_rooms(
    client: &ChaturbateClient,
    output: &dyn Output,
    raw_tag: &str,
    limit: usize,
) -> anyhow::Result<()> {
    let tag = normalize_tag(raw_tag)?;
    if !(1..=MAX_RESULTS).contains(&limit) {
        anyhow::bail!("El limite debe estar entre 1 y {MAX_RESULTS}");
    }

    let rooms = client.discover_rooms_by_tag(&tag, limit).await?;
    if rooms.is_empty() {
        output.discovery_empty(&tag);
        return Ok(());
    }

    output.discovery_started(&tag, rooms.len());
    for room in rooms {
        output.discovery_room(&room.username, room.viewers, &room.show, &room.subject);
    }
    Ok(())
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
}
