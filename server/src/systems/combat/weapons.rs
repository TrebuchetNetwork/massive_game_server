// massive_game_server/server/src/systems/combat/weapons.rs

use crate::core::types::ServerWeaponType;

#[derive(Debug, Clone, Copy)]
pub struct WeaponProfile {
    pub damage: i32,
    pub fire_rate_seconds: f32,
    pub max_ammo: i32,
}

pub fn profile(weapon: ServerWeaponType) -> WeaponProfile {
    match weapon {
        ServerWeaponType::Pistol => WeaponProfile {
            damage: 8,
            fire_rate_seconds: 0.6,
            max_ammo: 7,
        },
        ServerWeaponType::Shotgun => WeaponProfile {
            damage: 7,
            fire_rate_seconds: 0.8,
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
