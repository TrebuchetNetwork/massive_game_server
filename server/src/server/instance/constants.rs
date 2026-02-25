use std::sync::Arc;

use once_cell::sync::OnceCell;
use parking_lot::RwLock as ParkingLotRwLock;

use crate::core::types::Wall;

pub(super) const INITIAL_SNAPSHOT_MAX_PLAYERS: usize = 24;
pub(super) const INITIAL_SNAPSHOT_MAX_WALLS: usize = 128;
pub(super) const INITIAL_SNAPSHOT_MAX_PROJECTILES: usize = 200;
pub(super) const INITIAL_SNAPSHOT_MAX_PICKUPS: usize = 24;
pub(super) const INITIAL_SNAPSHOT_TAIL_MAX_PLAYERS: usize = 16;
pub(super) const INITIAL_SNAPSHOT_TAIL_MAX_WALLS: usize = 96;
pub(super) const INITIAL_SNAPSHOT_TAIL_MAX_PROJECTILES: usize = 120;
pub(super) const INITIAL_SNAPSHOT_TAIL_MAX_PICKUPS: usize = 16;
pub(super) const INITIAL_SNAPSHOT_TAIL_AGGRESSIVE_MAX_PLAYERS: usize = 12;
pub(super) const INITIAL_SNAPSHOT_TAIL_AGGRESSIVE_MAX_WALLS: usize = 72;
pub(super) const INITIAL_SNAPSHOT_TAIL_AGGRESSIVE_MAX_PROJECTILES: usize = 80;
pub(super) const INITIAL_SNAPSHOT_TAIL_AGGRESSIVE_MAX_PICKUPS: usize = 12;
pub(super) const INITIAL_SNAPSHOT_EXTREME_TAIL_MAX_PLAYERS: usize = 10;
pub(super) const INITIAL_SNAPSHOT_EXTREME_TAIL_MAX_WALLS: usize = 56;
pub(super) const INITIAL_SNAPSHOT_EXTREME_TAIL_MAX_PROJECTILES: usize = 56;
pub(super) const INITIAL_SNAPSHOT_EXTREME_TAIL_MAX_PICKUPS: usize = 10;
pub(super) const INITIAL_SNAPSHOT_SINGLE_MACHINE_BACKLOG_MAX_PLAYERS: usize = 14;
pub(super) const INITIAL_SNAPSHOT_SINGLE_MACHINE_BACKLOG_MAX_WALLS: usize = 84;
pub(super) const INITIAL_SNAPSHOT_SINGLE_MACHINE_BACKLOG_MAX_PROJECTILES: usize = 96;
pub(super) const INITIAL_SNAPSHOT_SINGLE_MACHINE_BACKLOG_MAX_PICKUPS: usize = 14;
pub(super) const MAX_CHAT_PER_BATCH: usize = 10;

pub(super) const MAX_KILL_FEED_HISTORY: usize = 10;
pub(super) const MAX_CHAT_MESSAGES_HISTORY: usize = 50;
pub(super) const MAX_MELEE_EVENTS_PER_TICK: usize = 200;

pub(super) const JOIN_STAGE_WAVES: [(&str, &str, u64, Option<u64>); 4] = [
    ("wave_1_24", "1-24", 1, Some(24)),
    ("wave_25_48", "25-48", 25, Some(48)),
    ("wave_49_72", "49-72", 49, Some(72)),
    ("wave_73_plus", "73+", 73, None),
];

#[derive(Clone, Copy, Debug)]
pub(super) struct InitialSnapshotCaps {
    pub(super) max_players: usize,
    pub(super) max_walls: usize,
    pub(super) max_projectiles: usize,
    pub(super) max_pickups: usize,
}

impl InitialSnapshotCaps {
    pub(super) const DEFAULT: Self = Self {
        max_players: INITIAL_SNAPSHOT_MAX_PLAYERS,
        max_walls: INITIAL_SNAPSHOT_MAX_WALLS,
        max_projectiles: INITIAL_SNAPSHOT_MAX_PROJECTILES,
        max_pickups: INITIAL_SNAPSHOT_MAX_PICKUPS,
    };

    pub(super) const TAIL: Self = Self {
        max_players: INITIAL_SNAPSHOT_TAIL_MAX_PLAYERS,
        max_walls: INITIAL_SNAPSHOT_TAIL_MAX_WALLS,
        max_projectiles: INITIAL_SNAPSHOT_TAIL_MAX_PROJECTILES,
        max_pickups: INITIAL_SNAPSHOT_TAIL_MAX_PICKUPS,
    };

    pub(super) const TAIL_AGGRESSIVE: Self = Self {
        max_players: INITIAL_SNAPSHOT_TAIL_AGGRESSIVE_MAX_PLAYERS,
        max_walls: INITIAL_SNAPSHOT_TAIL_AGGRESSIVE_MAX_WALLS,
        max_projectiles: INITIAL_SNAPSHOT_TAIL_AGGRESSIVE_MAX_PROJECTILES,
        max_pickups: INITIAL_SNAPSHOT_TAIL_AGGRESSIVE_MAX_PICKUPS,
    };

    pub(super) const EXTREME_TAIL: Self = Self {
        max_players: INITIAL_SNAPSHOT_EXTREME_TAIL_MAX_PLAYERS,
        max_walls: INITIAL_SNAPSHOT_EXTREME_TAIL_MAX_WALLS,
        max_projectiles: INITIAL_SNAPSHOT_EXTREME_TAIL_MAX_PROJECTILES,
        max_pickups: INITIAL_SNAPSHOT_EXTREME_TAIL_MAX_PICKUPS,
    };

    pub(super) const SINGLE_MACHINE_BACKLOG: Self = Self {
        max_players: INITIAL_SNAPSHOT_SINGLE_MACHINE_BACKLOG_MAX_PLAYERS,
        max_walls: INITIAL_SNAPSHOT_SINGLE_MACHINE_BACKLOG_MAX_WALLS,
        max_projectiles: INITIAL_SNAPSHOT_SINGLE_MACHINE_BACKLOG_MAX_PROJECTILES,
        max_pickups: INITIAL_SNAPSHOT_SINGLE_MACHINE_BACKLOG_MAX_PICKUPS,
    };
}

#[allow(clippy::type_complexity)]
pub(super) static CACHED_WALLS: OnceCell<Arc<ParkingLotRwLock<(u64, Vec<Wall>)>>> = OnceCell::new();
