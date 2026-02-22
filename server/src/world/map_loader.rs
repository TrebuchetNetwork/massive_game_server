// massive_game_server/server/src/world/map_loader.rs

use crate::core::constants::{WORLD_MAX_X, WORLD_MAX_Y, WORLD_MIN_X, WORLD_MIN_Y};
use crate::core::types::{CorePickupType, Pickup, Wall};
use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;

const DEFAULT_MAX_MAP_WALLS: usize = 4096;
const DEFAULT_MAX_MAP_PICKUPS: usize = 2048;
const MAX_MAP_COORD_ABS: f32 = 100_000.0;
const MIN_WALL_DIMENSION: f32 = 1.0;
const MAX_WALL_DIMENSION: f32 = 10_000.0;
const DEFAULT_WALL_MAX_HEALTH: i32 = 100;

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
    validate_map_file(&parsed)?;

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
            current_health: wall
                .current_health
                .unwrap_or_else(|| wall.max_health.unwrap_or(DEFAULT_WALL_MAX_HEALTH)),
            max_health: wall.max_health.unwrap_or(DEFAULT_WALL_MAX_HEALTH),
        })
        .collect();

    let mut pickups = Vec::new();
    for pickup in parsed.pickups {
        let pickup_type = parse_pickup_type(&pickup.pickup_type)?;
        pickups.push(Pickup::new(pickup.id, pickup.x, pickup.y, pickup_type));
    }

    Ok(LoadedMap { walls, pickups })
}

fn map_entity_limits() -> (usize, usize) {
    let max_walls = std::env::var("MGS_MAP_MAX_WALLS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_MAP_WALLS);
    let max_pickups = std::env::var("MGS_MAP_MAX_PICKUPS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_MAP_PICKUPS);
    (max_walls, max_pickups)
}

fn validate_map_file(parsed: &MapFile) -> Result<()> {
    let (max_walls, max_pickups) = map_entity_limits();
    if parsed.walls.len() > max_walls {
        return Err(anyhow!(
            "map contains {} walls but limit is {}",
            parsed.walls.len(),
            max_walls
        ));
    }
    if parsed.pickups.len() > max_pickups {
        return Err(anyhow!(
            "map contains {} pickups but limit is {}",
            parsed.pickups.len(),
            max_pickups
        ));
    }

    let mut seen_ids = HashSet::with_capacity(parsed.walls.len() + parsed.pickups.len());
    for wall in &parsed.walls {
        if !seen_ids.insert(wall.id) {
            return Err(anyhow!("duplicate entity id {} in map", wall.id));
        }
        validate_wall(wall)?;
    }
    for pickup in &parsed.pickups {
        if !seen_ids.insert(pickup.id) {
            return Err(anyhow!("duplicate entity id {} in map", pickup.id));
        }
        validate_pickup(pickup)?;
    }
    Ok(())
}

fn validate_wall(wall: &MapWall) -> Result<()> {
    validate_finite_coord(wall.x, "wall.x", wall.id)?;
    validate_finite_coord(wall.y, "wall.y", wall.id)?;
    validate_finite_coord(wall.width, "wall.width", wall.id)?;
    validate_finite_coord(wall.height, "wall.height", wall.id)?;

    if wall.width < MIN_WALL_DIMENSION
        || wall.height < MIN_WALL_DIMENSION
        || wall.width > MAX_WALL_DIMENSION
        || wall.height > MAX_WALL_DIMENSION
    {
        return Err(anyhow!(
            "wall {} dimensions out of range: width={} height={}",
            wall.id,
            wall.width,
            wall.height
        ));
    }

    let wall_min_x = wall.x;
    let wall_min_y = wall.y;
    let wall_max_x = wall.x + wall.width;
    let wall_max_y = wall.y + wall.height;
    if wall_min_x < WORLD_MIN_X
        || wall_min_y < WORLD_MIN_Y
        || wall_max_x > WORLD_MAX_X
        || wall_max_y > WORLD_MAX_Y
    {
        return Err(anyhow!(
            "wall {} is out of world bounds: ({}, {}) to ({}, {}) not within [{}, {}]x[{}, {}]",
            wall.id,
            wall_min_x,
            wall_min_y,
            wall_max_x,
            wall_max_y,
            WORLD_MIN_X,
            WORLD_MAX_X,
            WORLD_MIN_Y,
            WORLD_MAX_Y
        ));
    }

    let max_health = wall.max_health.unwrap_or(DEFAULT_WALL_MAX_HEALTH);
    if max_health <= 0 {
        return Err(anyhow!(
            "wall {} has invalid max_health {}; expected > 0",
            wall.id,
            max_health
        ));
    }
    let current_health = wall.current_health.unwrap_or(max_health);
    if current_health < 0 || current_health > max_health {
        return Err(anyhow!(
            "wall {} has invalid current_health {}; expected 0..={}",
            wall.id,
            current_health,
            max_health
        ));
    }
    Ok(())
}

fn validate_pickup(pickup: &MapPickup) -> Result<()> {
    validate_finite_coord(pickup.x, "pickup.x", pickup.id)?;
    validate_finite_coord(pickup.y, "pickup.y", pickup.id)?;
    if pickup.x < WORLD_MIN_X
        || pickup.x > WORLD_MAX_X
        || pickup.y < WORLD_MIN_Y
        || pickup.y > WORLD_MAX_Y
    {
        return Err(anyhow!(
            "pickup {} is out of world bounds: ({}, {}) not within [{}, {}]x[{}, {}]",
            pickup.id,
            pickup.x,
            pickup.y,
            WORLD_MIN_X,
            WORLD_MAX_X,
            WORLD_MIN_Y,
            WORLD_MAX_Y
        ));
    }
    let _ = parse_pickup_type(&pickup.pickup_type)?;
    Ok(())
}

fn validate_finite_coord(value: f32, field: &str, entity_id: u64) -> Result<()> {
    if !value.is_finite() || value.abs() > MAX_MAP_COORD_ABS {
        return Err(anyhow!(
            "{} for entity {} is invalid: {}",
            field,
            entity_id,
            value
        ));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_duplicate_entity_ids() {
        let map = MapFile {
            walls: vec![MapWall {
                id: 5,
                x: -100.0,
                y: -50.0,
                width: 40.0,
                height: 40.0,
                is_destructible: Some(false),
                current_health: Some(100),
                max_health: Some(100),
            }],
            pickups: vec![MapPickup {
                id: 5,
                x: 0.0,
                y: 0.0,
                pickup_type: "health".to_string(),
            }],
        };
        let err = validate_map_file(&map).expect_err("duplicate ids should be rejected");
        assert!(err.to_string().contains("duplicate entity id"));
    }

    #[test]
    fn rejects_out_of_bounds_wall() {
        let map = MapFile {
            walls: vec![MapWall {
                id: 1,
                x: WORLD_MAX_X - 10.0,
                y: WORLD_MAX_Y - 10.0,
                width: 64.0,
                height: 64.0,
                is_destructible: Some(false),
                current_health: Some(100),
                max_health: Some(100),
            }],
            pickups: vec![],
        };
        let err = validate_map_file(&map).expect_err("out-of-bounds walls should be rejected");
        assert!(err.to_string().contains("out of world bounds"));
    }

    #[test]
    fn rejects_invalid_wall_health() {
        let map = MapFile {
            walls: vec![MapWall {
                id: 2,
                x: -100.0,
                y: -100.0,
                width: 40.0,
                height: 40.0,
                is_destructible: Some(true),
                current_health: Some(200),
                max_health: Some(100),
            }],
            pickups: vec![],
        };
        let err = validate_map_file(&map).expect_err("invalid health range should fail");
        assert!(err.to_string().contains("invalid current_health"));
    }

    #[test]
    fn accepts_valid_map() {
        let map = MapFile {
            walls: vec![MapWall {
                id: 11,
                x: -200.0,
                y: -150.0,
                width: 80.0,
                height: 60.0,
                is_destructible: Some(true),
                current_health: Some(100),
                max_health: Some(120),
            }],
            pickups: vec![MapPickup {
                id: 12,
                x: 0.0,
                y: 0.0,
                pickup_type: "ammo".to_string(),
            }],
        };
        validate_map_file(&map).expect("valid map should pass validation");
    }
}
