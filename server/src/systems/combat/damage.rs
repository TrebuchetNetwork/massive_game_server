// massive_game_server/server/src/systems/combat/damage.rs

use crate::core::types::PlayerState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageResult {
    pub health_damage: i32,
    pub shield_damage: i32,
    pub overkill_damage: i32,
    pub killed: bool,
}

pub fn apply_damage(target: &mut PlayerState, amount: i32) -> DamageResult {
    if target.invulnerable_remaining > 0.0 {
        return DamageResult {
            health_damage: 0,
            shield_damage: 0,
            overkill_damage: 0,
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

    let current_health = target.health.max(0);
    let health_damage = pending.min(current_health);
    let overkill_damage = (pending - current_health).max(0);
    target.health = target.health.saturating_sub(pending).max(0);
    if target.health <= 0 {
        target.alive = false;
    }

    DamageResult {
        health_damage,
        shield_damage,
        overkill_damage,
        killed: !target.alive,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_damage_tracks_overkill_without_counting_extra_health_damage() {
        let mut target = PlayerState::new("target".into(), "Target".to_string(), 0.0, 0.0);
        target.health = 25;
        target.shield_current = 0;
        target.alive = true;

        let result = apply_damage(&mut target, 40);
        assert_eq!(result.health_damage, 25);
        assert_eq!(result.overkill_damage, 15);
        assert_eq!(result.shield_damage, 0);
        assert!(result.killed);
        assert!(!target.alive);
    }

    #[test]
    fn apply_damage_consumes_shield_before_health() {
        let mut target = PlayerState::new("target".into(), "Target".to_string(), 0.0, 0.0);
        target.health = 100;
        target.shield_current = 20;
        target.alive = true;

        let result = apply_damage(&mut target, 35);
        assert_eq!(result.shield_damage, 20);
        assert_eq!(result.health_damage, 15);
        assert_eq!(result.overkill_damage, 0);
        assert!(!result.killed);
        assert!(target.alive);
        assert_eq!(target.shield_current, 0);
        assert_eq!(target.health, 85);
    }
}
