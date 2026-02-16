// massive_game_server/server/src/state_sync/delta.rs

use crate::core::types::{PlayerID, PlayerState};
use std::collections::HashMap;

pub fn changed_players<'a>(
    previous: &HashMap<PlayerID, PlayerState>,
    current: &'a HashMap<PlayerID, PlayerState>,
) -> Vec<&'a PlayerState> {
    let mut changed = Vec::new();
    for (player_id, current_state) in current {
        if !previous.contains_key(player_id) || current_state.changed_fields != 0 {
            changed.push(current_state);
        }
    }
    changed
}

pub fn removed_player_ids(
    previous: &HashMap<PlayerID, PlayerState>,
    current: &HashMap<PlayerID, PlayerState>,
) -> Vec<PlayerID> {
    previous
        .keys()
        .filter(|player_id| !current.contains_key(*player_id))
        .cloned()
        .collect()
}
