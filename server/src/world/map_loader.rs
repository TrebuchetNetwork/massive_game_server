// massive_game_server/server/src/world/map_loader.rs

use crate::core::types::{CorePickupType, Pickup, Wall};
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize)]
struct MapFile {
    walls: Vec<MapWall>,
    pickups: Vec<MapPickup>,
}

#[derive(Debug, Deserialize)]
struct MapWall {
    id: u64,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    is_destructible: Option<bool>,
    current_health: Option<i32>,
    max_health: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct MapPickup {
    id: u64,
    x: f32,
    y: f32,
    pickup_type: String,
}

#[derive(Debug)]
pub struct LoadedMap {
    pub walls: Vec<Wall>,
    pub pickups: Vec<Pickup>,
}

pub fn load_map_from_json(path: &str) -> Result<LoadedMap> {
    let raw = fs::read_to_string(path).with_context(|| format!("failed reading map '{}'", path))?;
    let parsed: MapFile = serde_json::from_str(&raw)
        .with_context(|| format!("failed parsing map json '{}'", path))?;

    let walls = parsed
        .walls
        .into_iter()
        .map(|wall| Wall {
            id: wall.id,
            x: wall.x,
            y: wall.y,
            width: wall.width,
            height: wall.height,
            is_destructible: wall.is_destructible.unwrap_or(false),
            current_health: wall.current_health.unwrap_or(100),
            max_health: wall.max_health.unwrap_or(100),
        })
        .collect();

    let mut pickups = Vec::new();
    for pickup in parsed.pickups {
        let pickup_type = parse_pickup_type(&pickup.pickup_type)?;
        pickups.push(Pickup::new(pickup.id, pickup.x, pickup.y, pickup_type));
    }

    Ok(LoadedMap { walls, pickups })
}

fn parse_pickup_type(raw: &str) -> Result<CorePickupType> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "health" => Ok(CorePickupType::Health),
        "ammo" => Ok(CorePickupType::Ammo),
        "speedboost" | "speed_boost" => Ok(CorePickupType::SpeedBoost),
        "damageboost" | "damage_boost" => Ok(CorePickupType::DamageBoost),
        "shield" => Ok(CorePickupType::Shield),
        "weaponcrate_pistol" => Ok(CorePickupType::WeaponCrate(
            crate::core::types::ServerWeaponType::Pistol,
        )),
        "weaponcrate_shotgun" => Ok(CorePickupType::WeaponCrate(
            crate::core::types::ServerWeaponType::Shotgun,
        )),
        "weaponcrate_rifle" => Ok(CorePickupType::WeaponCrate(
            crate::core::types::ServerWeaponType::Rifle,
        )),
        "weaponcrate_sniper" => Ok(CorePickupType::WeaponCrate(
            crate::core::types::ServerWeaponType::Sniper,
        )),
        other => Err(anyhow!("unsupported pickup_type '{}'", other)),
    }
}
