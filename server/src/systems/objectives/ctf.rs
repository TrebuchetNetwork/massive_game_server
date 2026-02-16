// massive_game_server/server/src/systems/objectives/ctf.rs

use crate::core::types::{PlayerID, Vec2};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagStatus {
    AtBase,
    Carried,
    Dropped,
}

#[derive(Debug, Clone)]
pub struct FlagObjective {
    pub team_id: u8,
    pub status: FlagStatus,
    pub base_position: Vec2,
    pub position: Vec2,
    pub carrier_id: Option<PlayerID>,
}

impl FlagObjective {
    pub fn new(team_id: u8, base_position: Vec2) -> Self {
        Self {
            team_id,
            status: FlagStatus::AtBase,
            base_position,
            position: base_position,
            carrier_id: None,
        }
    }

    pub fn pickup(&mut self, player_id: PlayerID) {
        self.status = FlagStatus::Carried;
        self.carrier_id = Some(player_id);
    }

    pub fn drop_at(&mut self, position: Vec2) {
        self.status = FlagStatus::Dropped;
        self.position = position;
        self.carrier_id = None;
    }

    pub fn reset(&mut self) {
        self.status = FlagStatus::AtBase;
        self.position = self.base_position;
        self.carrier_id = None;
    }
}
