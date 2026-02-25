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
