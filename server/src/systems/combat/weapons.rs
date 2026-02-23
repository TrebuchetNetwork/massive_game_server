// massive_game_server/server/src/systems/combat/weapons.rs

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
            damage: 8,
            fire_rate_seconds: 0.6,
            max_ammo: 7,
        },
        ServerWeaponType::Shotgun => WeaponProfile {
            damage: 12,
            fire_rate_seconds: 0.6,
            max_ammo: 5,
        },
        ServerWeaponType::Rifle => WeaponProfile {
            damage: 10,
            fire_rate_seconds: 0.1,
            max_ammo: 30,
        },
        ServerWeaponType::Sniper => WeaponProfile {
            damage: 50,
            fire_rate_seconds: 1.2,
            max_ammo: 5,
        },
        ServerWeaponType::Melee => WeaponProfile {
            damage: 30,
            fire_rate_seconds: 0.5,
            max_ammo: 0,
        },
    }
}

pub fn falloff_profile(weapon: ServerWeaponType) -> WeaponFalloffProfile {
    match weapon {
        ServerWeaponType::Pistol => WeaponFalloffProfile {
            falloff_start: 150.0,
            max_range: 300.0,
            min_multiplier: 0.60,
        },
        ServerWeaponType::Shotgun => WeaponFalloffProfile {
            falloff_start: 40.0,
            max_range: 160.0,
            min_multiplier: 0.10,
        },
        ServerWeaponType::Rifle => WeaponFalloffProfile {
            falloff_start: 200.0,
            max_range: 500.0,
            min_multiplier: 0.40,
        },
        ServerWeaponType::Sniper => WeaponFalloffProfile {
            falloff_start: 600.0,
            max_range: 1_200.0,
            min_multiplier: 0.80,
        },
        ServerWeaponType::Melee => WeaponFalloffProfile {
            falloff_start: 0.0,
            max_range: 30.0,
            min_multiplier: 1.0,
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
