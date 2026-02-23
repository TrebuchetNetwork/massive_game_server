// massive_game_server/server/src/systems/combat/damage.rs

use crate::core::types::{PlayerState, ServerWeaponType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageResult {
    pub health_damage: i32,
    pub shield_damage: i32,
    pub killed: bool,
}

pub fn apply_damage(
    target: &mut PlayerState,
    _weapon: ServerWeaponType,
    amount: i32,
) -> DamageResult {
    if target.invulnerable_remaining > 0.0 {
        return DamageResult {
            health_damage: 0,
            shield_damage: 0,
            killed: false,
        };
    }
    let mut pending = amount.max(0);
    let mut shield_damage = 0;
    if target.shield_current > 0 {
        shield_damage = pending.min(target.shield_current);
        target.shield_current -= shield_damage;
        pending -= shield_damage;
    }

    let health_damage = pending.min(target.health.max(0));
    target.health -= health_damage;
    if target.health <= 0 {
        target.alive = false;
    }

    DamageResult {
        health_damage,
        shield_damage,
        killed: !target.alive,
    }
}
