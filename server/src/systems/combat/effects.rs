// massive_game_server/server/src/systems/combat/effects.rs

use crate::core::types::{GameEvent, PlayerID, ServerWeaponType, Vec2};

pub fn build_damage_effect(
    target_id: PlayerID,
    attacker_id: Option<PlayerID>,
    damage: i32,
    weapon: ServerWeaponType,
    position: Vec2,
) -> GameEvent {
    GameEvent::PlayerDamaged {
        target_id,
        attacker_id,
        damage,
        weapon,
        position,
    }
}

pub fn build_kill_effect(
    victim_id: PlayerID,
    killer_id: PlayerID,
    weapon: ServerWeaponType,
    position: Vec2,
) -> GameEvent {
    GameEvent::PlayerKilled {
        victim_id,
        killer_id,
        weapon,
        position,
    }
}
