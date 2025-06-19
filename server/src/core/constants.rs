// massive_game_server/server/src/core/constants.rs
use std::time::Duration;

pub const SERVER_TICK_RATE: u64 = 30;
pub const TICK_DURATION_MS: u64 = 1000 / SERVER_TICK_RATE;
pub const TICK_DURATION: Duration = Duration::from_millis(TICK_DURATION_MS);

// World constants
pub const WORLD_MIN_X: f32 = -800.0; // Example, adjust as needed
pub const WORLD_MAX_X: f32 = 800.0;  // Example
pub const WORLD_MIN_Y: f32 = -600.0; // Example
pub const WORLD_MAX_Y: f32 = 600.0;  // Example
pub const PARTITION_GRID_SIZE: usize = 8; 
pub const PARTITION_SIZE_X: f32 = (WORLD_MAX_X - WORLD_MIN_X) / PARTITION_GRID_SIZE as f32;
pub const PARTITION_SIZE_Y: f32 = (WORLD_MAX_Y - WORLD_MIN_Y) / PARTITION_GRID_SIZE as f32;
pub const BOUNDARY_ZONE_WIDTH: f32 = 100.0;

// Spatial Index constants
pub const SPATIAL_INDEX_CELL_SIZE: f32 = 400.0;

// Player constants
pub const PLAYER_SHARDS_COUNT: usize = 96; // Default, overridden by dev config
pub const PLAYER_RADIUS: f32 = 15.0; // Player hitbox radius
pub const PLAYER_BASE_SPEED: f32 = 150.0; // Base movement speed for players
pub const MIN_PLAYERS_TO_START: usize = 1; // Reduced to 1 so single player can start with bots

// Projectile constants
// (Add if needed, e.g., default projectile speed, lifetime)

// Pickup constants
pub const PICKUP_COLLECTION_RADIUS: f32 = 25.0;
pub const PICKUP_DEFAULT_RESPAWN_TIME_SECS: f32 = 10.0;

// Anti-cheat constants (example values)
pub const MAX_PLAYER_SPEED_MULTIPLIER: f32 = 1.5; // For speed boosts
pub const MAX_POSITION_DELTA_SLACK: f32 = 10.0; // Max allowed movement per tick if not moving via velocity
pub const MIN_SHOT_INTERVAL_SECONDS: f32 = 0.05; // Minimum interval between shots
pub const POSITION_VALIDATION_VIOLATION_THRESHOLD: u32 = 5;

// Weapon specific constants (can be moved to a dedicated module later)
pub const SHOTGUN_PELLET_COUNT: i32 = 8;
pub const SHOTGUN_SPREAD_ANGLE_RAD: f32 = 0.4;

// Other game constants
pub const DEFAULT_RESPAWN_DURATION_SECS: f32 = 5.0;
pub const MAX_INPUT_QUEUE_SIZE_PER_PLAYER: usize = 32;

pub const SAFE_SPAWN_RADIUS_FROM_ENEMY: f32 = 300.0; // Example value, adjust as needed



// Performance 
pub const TARGET_TICK_MS: u64      = 16;   // 60 Hz
pub const SLOW_TICK_LOG_MS: u64    = 12;   // warn if physics+logic exceed this
pub const NET_IO_TIMEOUT_MS: u64   = 10;   // drop network read if it blocks
pub const AI_TIMEOUT_MS: u64       = 10;   // fail-safe for runaway AI
pub const FAN_OUT_TIMEOUT_MS: u64  = 50;   // serialization + broadcast (increased for initial state)
pub const AI_UPDATE_STRIDE: u64    = 2;    // run AI every N frames (≈ 30 Hz) - more responsive bots



// Placeholder constants for projectile speeds (define these in your core::constants.rs)
pub const PISTOL_PROJECTILE_SPEED: f32 = 450.0;
pub const SHOTGUN_PROJECTILE_SPEED: f32 = 400.0;
pub const RIFLE_PROJECTILE_SPEED: f32 = 600.0;
pub const SNIPER_PROJECTILE_SPEED: f32 = 800.0;


pub const AOI_RADIUS: f32 = 600.0; 
pub const AOI_UPDATE_INTERVAL_SECS: f32 = 0.1;
