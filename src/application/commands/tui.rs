//! Loads public discovery data before handing control to the TUI adapter.

use crate::application::commands::discover;
use crate::infrastructure::ChaturbateClient;
use crate::presentation::{run_discovery_tui, TuiRoom};

/// Loads a validated discovery snapshot and opens its interactive view.
pub(crate) async fn run(
    client: &ChaturbateClient,
    raw_tag: &str,
    limit: usize,
) -> anyhow::Result<()> {
    let result = discover::find_rooms(client, raw_tag, limit).await?;
    let rooms = result
        .rooms
        .into_iter()
        .map(|room| TuiRoom::new(room.username, room.viewers, room.show, room.subject))
        .collect();
    run_discovery_tui(result.tag, rooms)
}
