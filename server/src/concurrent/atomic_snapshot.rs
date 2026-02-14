// massive_game_server/server/src/concurrent/atomic_snapshot.rs
//
// Lock-free player snapshots used by AOI/broadcast read paths.

use crate::core::types::{PlayerID, PlayerState};
use arc_swap::ArcSwap;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct PlayerSoASnapshot {
    ids: Vec<PlayerID>,
    pub xs: Vec<f32>,
    pub ys: Vec<f32>,
    pub alive: Vec<bool>,
    pub team_ids: Vec<u8>,
    pub changed_fields: Vec<u16>,
    states: Vec<PlayerState>,
    index_by_id: HashMap<PlayerID, usize>,
}

impl PlayerSoASnapshot {
    pub fn from_player_states_map(player_states: &HashMap<PlayerID, PlayerState>) -> Self {
        let mut snapshot = Self::with_capacity(player_states.len());
        for (player_id, player_state) in player_states {
            let idx = snapshot.ids.len();
            snapshot.index_by_id.insert(player_id.clone(), idx);
            snapshot.ids.push(player_id.clone());
            snapshot.xs.push(player_state.x);
            snapshot.ys.push(player_state.y);
            snapshot.alive.push(player_state.alive);
            snapshot.team_ids.push(player_state.team_id);
            snapshot.changed_fields.push(player_state.changed_fields);
            snapshot.states.push(player_state.clone());
        }
        snapshot
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    #[inline]
    pub fn get_state(&self, player_id: &PlayerID) -> Option<&PlayerState> {
        self.index_by_id
            .get(player_id)
            .and_then(|idx| self.states.get(*idx))
    }

    #[inline]
    pub fn get_position(&self, player_id: &PlayerID) -> Option<(f32, f32)> {
        self.index_by_id.get(player_id).and_then(|idx| {
            let x = *self.xs.get(*idx)?;
            let y = *self.ys.get(*idx)?;
            Some((x, y))
        })
    }

    #[inline]
    fn with_capacity(capacity: usize) -> Self {
        Self {
            ids: Vec::with_capacity(capacity),
            xs: Vec::with_capacity(capacity),
            ys: Vec::with_capacity(capacity),
            alive: Vec::with_capacity(capacity),
            team_ids: Vec::with_capacity(capacity),
            changed_fields: Vec::with_capacity(capacity),
            states: Vec::with_capacity(capacity),
            index_by_id: HashMap::with_capacity(capacity),
        }
    }
}

pub struct AtomicPlayerSnapshot {
    current: ArcSwap<PlayerSoASnapshot>,
}

impl AtomicPlayerSnapshot {
    pub fn new() -> Self {
        Self {
            current: ArcSwap::from_pointee(PlayerSoASnapshot::default()),
        }
    }

    #[inline]
    pub fn load(&self) -> Arc<PlayerSoASnapshot> {
        self.current.load_full()
    }

    #[inline]
    pub fn publish(&self, snapshot: PlayerSoASnapshot) {
        self.current.store(Arc::new(snapshot));
    }

    #[inline]
    pub fn publish_arc(&self, snapshot: Arc<PlayerSoASnapshot>) {
        self.current.store(snapshot);
    }
}

impl Default for AtomicPlayerSnapshot {
    fn default() -> Self {
        Self::new()
    }
}
