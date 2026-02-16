// massive_game_server/server/src/entities/flag.rs

use crate::core::types::{PlayerID, Vec2};

#[derive(Debug, Clone)]
pub struct FlagEntity {
    pub team_id: u8,
    pub base_position: Vec2,
    pub current_position: Vec2,
    pub carrier_id: Option<PlayerID>,
}
