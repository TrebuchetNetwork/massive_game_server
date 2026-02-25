// massive_game_server/server/src/systems/combat/weapons.rs

use crate::core::constants::*;
use crate::core::types::ServerWeaponType;

#[derive(Debug, Clone, Copy)]
pub struct WeaponProfile {
    pub damage: i32,
    pub fire_rate_seconds: f32,
    pub max_ammo: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct WeaponFalloffProfile {
    pub falloff_start: f32,
    pub max_range: f32,
    pub min_multiplier: f32,
}

pub fn profile(weapon: ServerWeaponType) -> WeaponProfile {
    match weapon {
        ServerWeaponType::Pistol => WeaponProfile {
            damage: PISTOL_DAMAGE,
            fire_rate_seconds: PISTOL_FIRE_RATE_SECS,
            max_ammo: PISTOL_MAX_AMMO,
        },
        ServerWeaponType::Shotgun => WeaponProfile {
            damage: SHOTGUN_DAMAGE,
            fire_rate_seconds: SHOTGUN_FIRE_RATE_SECS,
            max_ammo: SHOTGUN_MAX_AMMO,
        },
        ServerWeaponType::Rifle => WeaponProfile {
            damage: RIFLE_DAMAGE,
            fire_rate_seconds: RIFLE_FIRE_RATE_SECS,
            max_ammo: RIFLE_MAX_AMMO,
        },
        ServerWeaponType::Sniper => WeaponProfile {
            damage: SNIPER_DAMAGE,
            fire_rate_seconds: SNIPER_FIRE_RATE_SECS,
            max_ammo: SNIPER_MAX_AMMO,
        },
        ServerWeaponType::Melee => WeaponProfile {
            damage: MELEE_DAMAGE,
            fire_rate_seconds: MELEE_FIRE_RATE_SECS,
            max_ammo: MELEE_MAX_AMMO,
        },
    }
}

pub fn falloff_profile(weapon: ServerWeaponType) -> WeaponFalloffProfile {
    match weapon {
        ServerWeaponType::Pistol => WeaponFalloffProfile {
            falloff_start: PISTOL_FALLOFF_START,
            max_range: PISTOL_MAX_RANGE,
            min_multiplier: PISTOL_MIN_MULTIPLIER,
        },
        ServerWeaponType::Shotgun => WeaponFalloffProfile {
            falloff_start: SHOTGUN_FALLOFF_START,
            max_range: SHOTGUN_MAX_RANGE,
            min_multiplier: SHOTGUN_MIN_MULTIPLIER,
        },
        ServerWeaponType::Rifle => WeaponFalloffProfile {
            falloff_start: RIFLE_FALLOFF_START,
            max_range: RIFLE_MAX_RANGE,
            min_multiplier: RIFLE_MIN_MULTIPLIER,
        },
        ServerWeaponType::Sniper => WeaponFalloffProfile {
            falloff_start: SNIPER_FALLOFF_START,
            max_range: SNIPER_MAX_RANGE,
            min_multiplier: SNIPER_MIN_MULTIPLIER,
        },
        ServerWeaponType::Melee => WeaponFalloffProfile {
            falloff_start: MELEE_FALLOFF_START,
            max_range: MELEE_MAX_RANGE,
            min_multiplier: MELEE_MIN_MULTIPLIER,
        },
    }
}

pub fn distance_damage_multiplier(weapon: ServerWeaponType, distance: f32) -> f32 {
    let profile = falloff_profile(weapon);
    if distance <= profile.falloff_start || profile.max_range <= profile.falloff_start {
        return 1.0;
    }

    let t = ((distance - profile.falloff_start) / (profile.max_range - profile.falloff_start))
        .clamp(0.0, 1.0);
    (1.0 - t).max(profile.min_multiplier)
}

pub fn apply_distance_falloff(weapon: ServerWeaponType, base_damage: i32, distance: f32) -> i32 {
    let clamped_damage = base_damage.max(0) as f32;
    let scaled = clamped_damage * distance_damage_multiplier(weapon, distance.max(0.0));
    scaled.round().max(0.0) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::ServerWeaponType;

    // ── Pistol falloff (start=150, max=300, min_mult=0.60) ─────────

    #[test]
    fn pistol_no_falloff_at_zero() {
        let mult = distance_damage_multiplier(ServerWeaponType::Pistol, 0.0);
        assert!((mult - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn pistol_no_falloff_at_start() {
        let mult = distance_damage_multiplier(ServerWeaponType::Pistol, PISTOL_FALLOFF_START);
        assert!((mult - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn pistol_midpoint_falloff() {
        // At midpoint between start (150) and max (300), t = 0.5, mult = 1 - 0.5 = 0.5
        // But min_mult is 0.60, so max(0.5, 0.60) = 0.60
        let mid = (PISTOL_FALLOFF_START + PISTOL_MAX_RANGE) / 2.0;
        let mult = distance_damage_multiplier(ServerWeaponType::Pistol, mid);
        assert!((mult - 0.60).abs() < 0.01, "pistol midpoint mult={}", mult);
    }

    #[test]
    fn pistol_at_max_range() {
        let mult = distance_damage_multiplier(ServerWeaponType::Pistol, PISTOL_MAX_RANGE);
        assert!((mult - PISTOL_MIN_MULTIPLIER).abs() < f32::EPSILON);
    }

    #[test]
    fn pistol_beyond_max_range_clamped() {
        let mult = distance_damage_multiplier(ServerWeaponType::Pistol, PISTOL_MAX_RANGE + 500.0);
        assert!((mult - PISTOL_MIN_MULTIPLIER).abs() < f32::EPSILON);
    }

    // ── Shotgun falloff (start=40, max=160, min_mult=0.10) ────────

    #[test]
    fn shotgun_full_damage_at_close_range() {
        let mult = distance_damage_multiplier(ServerWeaponType::Shotgun, 30.0);
        assert!((mult - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn shotgun_at_max_range() {
        let mult = distance_damage_multiplier(ServerWeaponType::Shotgun, SHOTGUN_MAX_RANGE);
        assert!((mult - SHOTGUN_MIN_MULTIPLIER).abs() < f32::EPSILON);
    }

    #[test]
    fn shotgun_beyond_max_range() {
        let mult = distance_damage_multiplier(ServerWeaponType::Shotgun, 500.0);
        assert!((mult - SHOTGUN_MIN_MULTIPLIER).abs() < f32::EPSILON);
    }

    // ── Rifle falloff (start=200, max=500, min_mult=0.15) ─────────

    #[test]
    fn rifle_no_falloff_at_start() {
        let mult = distance_damage_multiplier(ServerWeaponType::Rifle, RIFLE_FALLOFF_START);
        assert!((mult - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn rifle_at_max_range() {
        let mult = distance_damage_multiplier(ServerWeaponType::Rifle, RIFLE_MAX_RANGE);
        assert!((mult - RIFLE_MIN_MULTIPLIER).abs() < f32::EPSILON);
    }

    #[test]
    fn rifle_gradual_falloff_midpoint() {
        // At midpoint (350), t = (350-200)/(500-200) = 150/300 = 0.5
        // mult = max(1 - 0.5, 0.15) = 0.5
        let mid = (RIFLE_FALLOFF_START + RIFLE_MAX_RANGE) / 2.0;
        let mult = distance_damage_multiplier(ServerWeaponType::Rifle, mid);
        assert!((mult - 0.5).abs() < 0.01, "rifle midpoint mult={}", mult);
    }

    // ── Sniper falloff (start=600, max=1200, min_mult=0.80) ──────

    #[test]
    fn sniper_full_damage_within_start() {
        let mult = distance_damage_multiplier(ServerWeaponType::Sniper, 400.0);
        assert!((mult - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn sniper_at_max_range() {
        let mult = distance_damage_multiplier(ServerWeaponType::Sniper, SNIPER_MAX_RANGE);
        assert!((mult - SNIPER_MIN_MULTIPLIER).abs() < f32::EPSILON);
    }

    #[test]
    fn sniper_minimal_falloff_midpoint() {
        // At midpoint (900), t = (900-600)/(1200-600) = 0.5
        // mult = max(1 - 0.5, 0.80) = 0.80
        let mid = (SNIPER_FALLOFF_START + SNIPER_MAX_RANGE) / 2.0;
        let mult = distance_damage_multiplier(ServerWeaponType::Sniper, mid);
        assert!((mult - 0.80).abs() < 0.01, "sniper midpoint mult={}", mult);
    }

    // ── Melee falloff (start=0, max=30, min_mult=1.0) ────────────

    #[test]
    fn melee_always_full_damage() {
        // Melee: falloff_start=0, max_range=30, min_mult=1.0
        // Since max_range > falloff_start, but min_mult is 1.0, result is always >= 1.0
        let mult = distance_damage_multiplier(ServerWeaponType::Melee, 15.0);
        assert!((mult - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn melee_at_max_range() {
        let mult = distance_damage_multiplier(ServerWeaponType::Melee, MELEE_MAX_RANGE);
        assert!((mult - MELEE_MIN_MULTIPLIER).abs() < f32::EPSILON);
    }

    // ── apply_distance_falloff integration tests ─────────────────

    #[test]
    fn apply_falloff_pistol_close_range() {
        let dmg = apply_distance_falloff(ServerWeaponType::Pistol, PISTOL_DAMAGE, 50.0);
        assert_eq!(dmg, PISTOL_DAMAGE); // no falloff within start range
    }

    #[test]
    fn apply_falloff_shotgun_at_max_range() {
        let dmg = apply_distance_falloff(ServerWeaponType::Shotgun, SHOTGUN_DAMAGE, SHOTGUN_MAX_RANGE);
        let expected = (SHOTGUN_DAMAGE as f32 * SHOTGUN_MIN_MULTIPLIER).round() as i32;
        assert_eq!(dmg, expected);
    }

    #[test]
    fn apply_falloff_zero_base_damage() {
        let dmg = apply_distance_falloff(ServerWeaponType::Rifle, 0, 100.0);
        assert_eq!(dmg, 0);
    }

    #[test]
    fn apply_falloff_negative_distance_treated_as_zero() {
        let dmg = apply_distance_falloff(ServerWeaponType::Pistol, PISTOL_DAMAGE, -50.0);
        assert_eq!(dmg, PISTOL_DAMAGE); // distance clamped to 0, no falloff
    }
}
