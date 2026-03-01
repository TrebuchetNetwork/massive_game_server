// massive_game_server/server/src/core/constants.rs
use std::sync::OnceLock;
use std::time::Duration;

pub const SERVER_TICK_RATE: u64 = 60;
pub const TICK_DURATION_MS: u64 = 1000 / SERVER_TICK_RATE;
pub const TICK_DURATION: Duration = Duration::from_millis(TICK_DURATION_MS);

// World constants
pub const WORLD_MIN_X: f32 = -800.0; // Example, adjust as needed
pub const WORLD_MAX_X: f32 = 800.0; // Example
pub const WORLD_MIN_Y: f32 = -600.0; // Example
pub const WORLD_MAX_Y: f32 = 600.0; // Example
pub const PARTITION_GRID_SIZE: usize = 8;

// Compile-time assertion: PARTITION_GRID_SIZE must be at least 1 to avoid division by zero
// in PARTITION_SIZE_X / PARTITION_SIZE_Y calculations below.
const _: () = assert!(PARTITION_GRID_SIZE >= 1, "PARTITION_GRID_SIZE must be >= 1");

pub const PARTITION_SIZE_X: f32 = (WORLD_MAX_X - WORLD_MIN_X) / PARTITION_GRID_SIZE as f32;
pub const PARTITION_SIZE_Y: f32 = (WORLD_MAX_Y - WORLD_MIN_Y) / PARTITION_GRID_SIZE as f32;
pub const BOUNDARY_ZONE_WIDTH: f32 = 100.0;

// Spatial Index constants
pub const SPATIAL_INDEX_CELL_SIZE: f32 = 400.0;

// Player constants
pub const PLAYER_SHARDS_COUNT: usize = 32; // Default, overridden by dev config
pub const PLAYER_RADIUS: f32 = 15.0; // Player hitbox radius
pub const PLAYER_BASE_SPEED: f32 = 150.0; // Base movement speed for players
pub const MIN_PLAYERS_TO_START: usize = 1; // Reduced to 1 so single player can start with bots

// Projectile constants
// (Add if needed, e.g., default projectile speed, lifetime)

// Pickup constants
pub const PICKUP_COLLECTION_RADIUS: f32 = 25.0;
pub const PICKUP_DEFAULT_RESPAWN_TIME_SECS: f32 = 10.0;

// Anti-cheat constants – tightened from V6 defaults (dash/dodge are server-side
// so client speed should never legitimately exceed 1.08× base).
// Override at runtime via MGS_SPEED_HACK_TOLERANCE env var (e.g. "1.10").
pub const MAX_PLAYER_SPEED_MULTIPLIER: f32 = 1.08;
pub const MAX_POSITION_DELTA_SLACK: f32 = 3.0;
pub const MIN_SHOT_INTERVAL_SECONDS: f32 = 0.05; // Minimum interval between shots
pub const FIRE_RATE_JITTER_TOLERANCE_SECS: f32 = 0.050; // 50ms tolerance for network jitter on fire rate checks
pub const POSITION_VALIDATION_VIOLATION_THRESHOLD: u32 = 3;

// Acceleration-based speed hack detection: maximum allowed velocity change per tick.
// Legitimate sources of acceleration: input direction reversal (2× base speed change
// per tick) plus small tolerance for knockback settling and boost edges.
// Anything exceeding this is flagged as suspicious.
pub const MAX_ACCELERATION_PER_TICK: f32 = PLAYER_BASE_SPEED * 3.5; // ~525 units/s per tick
pub const ACCELERATION_VIOLATION_THRESHOLD: u32 = 3;

/// Read the speed-hack tolerance multiplier from the `MGS_SPEED_HACK_TOLERANCE`
/// environment variable, falling back to `MAX_PLAYER_SPEED_MULTIPLIER` if unset
/// or unparseable.
pub fn speed_hack_tolerance() -> f32 {
    static SPEED_HACK_TOLERANCE: OnceLock<f32> = OnceLock::new();
    *SPEED_HACK_TOLERANCE.get_or_init(|| {
        std::env::var("MGS_SPEED_HACK_TOLERANCE")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .filter(|&v| v > 1.0 && v < 2.0)
            .unwrap_or(MAX_PLAYER_SPEED_MULTIPLIER)
    })
}

// ── Weapon tuning constants ──────────────────────────────────────────
// All weapon balance values centralized here for easy iteration.
pub const PISTOL_DAMAGE: i32 = 10;
pub const PISTOL_FIRE_RATE_SECS: f32 = 0.30; // snappier sidearm cadence
pub const PISTOL_MAX_AMMO: i32 = 10;
pub const PISTOL_RELOAD_SECS: f32 = 1.5;
pub const PISTOL_FALLOFF_START: f32 = 150.0;
pub const PISTOL_MAX_RANGE: f32 = 300.0;
pub const PISTOL_MIN_MULTIPLIER: f32 = 0.60;

pub const SHOTGUN_DAMAGE: i32 = 18; // was 12 – viable CQC (144 total)
pub const SHOTGUN_FIRE_RATE_SECS: f32 = 0.6;
pub const SHOTGUN_MAX_AMMO: i32 = 5;
pub const SHOTGUN_RELOAD_SECS: f32 = 2.5;
pub const SHOTGUN_PELLET_COUNT: i32 = 8;
pub const SHOTGUN_SPREAD_ANGLE_RAD: f32 = 0.25;
pub const SHOTGUN_FALLOFF_START: f32 = 40.0;
pub const SHOTGUN_MAX_RANGE: f32 = 160.0;
pub const SHOTGUN_MIN_MULTIPLIER: f32 = 0.10;

pub const RIFLE_DAMAGE: i32 = 10;
pub const RIFLE_FIRE_RATE_SECS: f32 = 0.1;
pub const RIFLE_MAX_AMMO: i32 = 30;
pub const RIFLE_RELOAD_SECS: f32 = 2.0;
pub const RIFLE_FALLOFF_START: f32 = 200.0;
pub const RIFLE_MAX_RANGE: f32 = 500.0;
pub const RIFLE_MIN_MULTIPLIER: f32 = 0.15; // was 0.40 – kills Rifle dominance at range

pub const SNIPER_DAMAGE: i32 = 50;
pub const SNIPER_FIRE_RATE_SECS: f32 = 1.2;
pub const SNIPER_MAX_AMMO: i32 = 5;
pub const SNIPER_RELOAD_SECS: f32 = 3.0;
pub const SNIPER_FALLOFF_START: f32 = 600.0;
pub const SNIPER_MAX_RANGE: f32 = 1200.0;
pub const SNIPER_MIN_MULTIPLIER: f32 = 0.80;

pub const MELEE_DAMAGE: i32 = 30;
pub const MELEE_FIRE_RATE_SECS: f32 = 0.5;
pub const MELEE_MAX_AMMO: i32 = 0;
pub const MELEE_RELOAD_SECS: f32 = 0.0;
pub const MELEE_FALLOFF_START: f32 = 0.0;
pub const MELEE_MAX_RANGE: f32 = 30.0;
pub const MELEE_MIN_MULTIPLIER: f32 = 1.0;
pub const MELEE_CONE_HALF_ANGLE_RAD: f32 = std::f32::consts::FRAC_PI_6; // π/6 (60° cone)
pub const MELEE_LUNGE_DISTANCE: f32 = 10.0; // extends effective reach to ~40u without widening the cone

pub const SPEED_BOOST_MULTIPLIER: f32 = 1.15; // Speed boost powerup multiplier (separate from anti-cheat tolerance)
pub const DAMAGE_BOOST_MULTIPLIER: f32 = 1.5;

pub const WEAPON_SWAP_DURATION_SECS: f32 = 0.3;

// ── Ability tuning ──────────────────────────────────────────────────
pub const ABILITY_DASH_COOLDOWN_SECS: f32 = 6.0; // was 8 – more outplay moments
pub const ABILITY_DASH_DURATION_SECS: f32 = 0.2;
pub const ABILITY_DASH_SPEED_MULTIPLIER: f32 = 2.0;
pub const ABILITY_DODGE_COOLDOWN_SECS: f32 = 9.0; // was 12 – more outplay moments
pub const ABILITY_DODGE_DURATION_SECS: f32 = 0.3;
pub const ABILITY_DODGE_SPEED_MULTIPLIER: f32 = 1.6;
pub const TEAM_PING_COOLDOWN_SECS: f32 = 3.0;

// ── Zone tuning ─────────────────────────────────────────────────────
pub const ZONE_SLOW_MULTIPLIER: f32 = 0.6;
pub const ZONE_DAMAGE_PER_SEC: f32 = 25.0; // zone becomes a real threat (4s kill on 100hp)
pub const ZONE_BOOST_DURATION_SECS: f32 = 0.5;
pub const ZONE_BOOST_SPEED_MULTIPLIER: f32 = 2.0;
pub const ZONE_BOOST_RETRIGGER_COOLDOWN_SECS: f32 = 0.8;

// ── Killstreak tuning ───────────────────────────────────────────────
pub const KILLSTREAK_DAMAGE_BOOST_THRESHOLD: u32 = 3;
pub const KILLSTREAK_DAMAGE_BOOST_MULTIPLIER: f32 = 1.10; // +10% damage
pub const KILLSTREAK_DAMAGE_BOOST_DURATION_SECS: f32 = 30.0;
pub const KILLSTREAK_SPEED_BOOST_THRESHOLD: u32 = 5;
pub const KILLSTREAK_SPEED_BOOST_DURATION_SECS: f32 = 15.0;
pub const KILLSTREAK_DOMINATING_THRESHOLD: u32 = 7;

// ── Objective scoring ───────────────────────────────────────────────
pub const POINTS_PER_KILL: i32 = 10;
pub const POINTS_FLAG_CAPTURE: i32 = 100;
pub const POINTS_FLAG_RETURN: i32 = 50;
pub const POINTS_ASSIST: i32 = 3; // 25% of kill = ~2.5, round to 3
pub const ASSIST_WINDOW_SECS: f32 = 5.0;
pub const LOSING_TEAM_RESPAWN_REDUCTION_PER_5PTS: f32 = 0.5;

// ── Projectile knockback ────────────────────────────────────────────
pub const KNOCKBACK_FORCE_PER_DAMAGE: f32 = 0.4; // reduced shove intensity for better close-range control
pub const KNOCKBACK_MAX_VELOCITY: f32 = 350.0; // lower cap prevents exaggerated launch trajectories

// ── Commander constants ────────────────────────────────────────────
pub const COMMANDER_MAX_WAYPOINTS_PER_TEAM: usize = 3;
pub const COMMANDER_WAYPOINT_TTL_MS: u64 = 20_000;
pub const COMMANDER_SUPPLY_DROP_COOLDOWN_MS: u64 = 60_000;
pub const COMMANDER_SUPPLY_DROP_PICKUPS: usize = 6;

// ── Progressive destructible wall constants ────────────────────────
pub const PROGRESSIVE_WALL_STAGE1_HEALTH_RATIO: f32 = 0.50;
pub const PROGRESSIVE_WALL_STAGE2_HEALTH_RATIO: f32 = 0.25;
pub const PROGRESSIVE_WALL_MIN_FRAGMENT_LENGTH: f32 = 12.0;

// ── Mobile bandwidth profile ────────────────────────────────────────
pub const MOBILE_AOI_MAX_VISIBLE_PLAYERS: usize = 24;
pub const MOBILE_AOI_MAX_VISIBLE_PROJECTILES: usize = 80;
pub const MOBILE_AOI_MAX_VISIBLE_PICKUPS: usize = 16;
pub const MOBILE_AOI_MAX_VISIBLE_WALLS: usize = 40;
pub const MOBILE_DELTA_SKIP_MODULUS: usize = 3; // send every 3rd frame = ~20 Hz
pub const MOBILE_COMPRESSION_LEVEL: u32 = 5;

// ── Mobile position quantization ───────────────────────────────────
// Reduces delta-compression entropy for mobile clients by snapping
// position/velocity/rotation to a coarser grid.  Values are quantized
// to integer representations, then converted back to f32 so the
// FlatBuffers schema (which uses f32) remains unchanged.

/// Minimum world coordinate representable by quantization.
/// Signed offset so that negative world coordinates are preserved.
pub const QUANTIZE_POSITION_MIN: f32 = -4096.0;

/// Maximum world coordinate representable by u16 quantization.
/// Supports maps spanning [-4096, +4096] units.
pub const QUANTIZE_POSITION_MAX: f32 = 4096.0;

/// Total range covered by the quantization window.
pub const QUANTIZE_POSITION_RANGE: f32 = QUANTIZE_POSITION_MAX - QUANTIZE_POSITION_MIN; // 8192.0

/// Scale factor: maps [QUANTIZE_POSITION_MIN, QUANTIZE_POSITION_MAX] -> [0, u16::MAX].
pub const QUANTIZE_POSITION_SCALE: f32 = u16::MAX as f32 / QUANTIZE_POSITION_RANGE;

/// Maximum velocity magnitude representable by i8 quantization.
/// Player speeds are typically 150-300 units/s; 127 * VELOCITY_SCALE covers that.
pub const QUANTIZE_VELOCITY_MAX: f32 = 500.0;

/// Scale factor: maps [-QUANTIZE_VELOCITY_MAX, +QUANTIZE_VELOCITY_MAX] -> [-127, 127].
pub const QUANTIZE_VELOCITY_SCALE: f32 = 127.0 / QUANTIZE_VELOCITY_MAX;

/// Quantize a world-space position component (x or y) to u16-grid precision.
/// The value is clamped to [QUANTIZE_POSITION_MIN, QUANTIZE_POSITION_MAX],
/// offset-shifted into [0, RANGE], quantized to u16, then dequantized back
/// to f32 on the same grid.  This correctly handles negative coordinates.
#[inline]
pub fn quantize_position(v: f32) -> f32 {
    if !v.is_finite() {
        return 0.0;
    }
    let clamped = v.clamp(QUANTIZE_POSITION_MIN, QUANTIZE_POSITION_MAX);
    let shifted = clamped - QUANTIZE_POSITION_MIN; // now in [0, RANGE]
    let q = (shifted * QUANTIZE_POSITION_SCALE).round() as u16;
    q as f32 / QUANTIZE_POSITION_SCALE + QUANTIZE_POSITION_MIN
}

/// Dequantize a u16 fixed-point value back to world-space f32.
#[inline]
pub fn dequantize_position(q: u16) -> f32 {
    q as f32 / QUANTIZE_POSITION_SCALE + QUANTIZE_POSITION_MIN
}

/// Quantize a velocity component to i8-grid precision (-127..127 scaled).
/// The result is snapped to the i8 grid, then dequantized back to f32.
#[inline]
pub fn quantize_velocity(v: f32) -> f32 {
    if !v.is_finite() {
        return 0.0;
    }
    let clamped = v.clamp(-QUANTIZE_VELOCITY_MAX, QUANTIZE_VELOCITY_MAX);
    let q = (clamped * QUANTIZE_VELOCITY_SCALE).round() as i8;
    q as f32 / QUANTIZE_VELOCITY_SCALE
}

/// Quantize a rotation (radians) to u8-grid precision (256 steps over 2*PI).
/// The result is snapped to the u8 grid, then dequantized back to f32.
#[inline]
pub fn quantize_rotation(v: f32) -> f32 {
    if !v.is_finite() {
        return 0.0;
    }
    // Normalize to [0, 2*PI)
    let two_pi = std::f32::consts::TAU;
    let normalized = ((v % two_pi) + two_pi) % two_pi;
    let q = ((normalized / two_pi) * 256.0).floor() as u8;
    q as f32 / 256.0 * two_pi
}

/// Runtime validation for partition grid size (for values loaded from config).
/// Panics if `grid_size` is zero.
pub fn validate_partition_grid_size(grid_size: usize) {
    assert!(
        grid_size >= 1,
        "Partition grid size must be >= 1, got {}",
        grid_size
    );
}

// Other game constants
pub const DEFAULT_RESPAWN_DURATION_SECS: f32 = 2.5;
pub const MAX_INPUT_QUEUE_SIZE_PER_PLAYER: usize = 32;
pub const MAX_INPUTS_PROCESSED_PER_TICK_PER_PLAYER: usize = 8;

// Input sequence validation – prevents replay attacks and suspicious jumps.
// Inputs with sequence <= last accepted are rejected (replay).
// Inputs with sequence > last accepted + MAX_SEQUENCE_GAP are rejected (suspicious jump).
pub const MAX_INPUT_SEQUENCE_GAP: u32 = 60;
pub const GAME_PROTOCOL_VERSION: u32 = 1;

pub const DEFAULT_INPUT_RATE_LIMIT_PER_SEC: u32 = 240;
pub const DEFAULT_INPUT_RATE_LIMIT_BURST: u32 = 360;
pub const INPUT_RATE_LIMIT_THROTTLE_LOG_INTERVAL_SECS: u64 = 5;
pub const DEFAULT_LAG_COMPENSATION_MS: u64 = 60;
pub const MAX_LAG_COMPENSATION_MS: u64 = 200; // Maximum rewind window for lag compensation (prevents exploit)
pub const MAX_POSITION_HISTORY_SAMPLES: usize = 32;
pub const AIMBOT_SUSPICION_ROTATION_RAD_PER_SEC: f32 = 18.0;
pub const AIMBOT_SUSPICION_DECAY_PER_SEC: f32 = 0.6;
pub const AIMBOT_SUSPICION_SHOT_WEIGHT: f32 = 0.35;
pub const AIMBOT_SUSPICION_THRESHOLD: f32 = 3.5;

pub const SAFE_SPAWN_RADIUS_FROM_ENEMY: f32 = 300.0; // Example value, adjust as needed

// ── Variable-rate entity updates ───────────────────────────────────
// Entities moving faster than HIGH threshold get position updates every 2nd frame (30 Hz).
// Entities between LOW and HIGH get updates every 6th frame (10 Hz).
// Entities below LOW threshold only get updates when non-position fields change.
pub const VARIABLE_RATE_HIGH_VELOCITY_THRESHOLD: f32 = 50.0; // pixels/sec
pub const VARIABLE_RATE_LOW_VELOCITY_THRESHOLD: f32 = 5.0; // pixels/sec
pub const VARIABLE_RATE_HIGH_STRIDE: u64 = 2; // every 2nd frame for fast entities
pub const VARIABLE_RATE_LOW_STRIDE: u64 = 6; // every 6th frame for slow entities

// Performance
pub const TARGET_TICK_MS: u64 = 16; // 60 Hz
pub const SLOW_TICK_LOG_MS: u64 = 12; // warn if physics+logic exceed this
pub const NET_IO_TIMEOUT_MS: u64 = 10; // drop network read if it blocks
pub const AI_TIMEOUT_MS: u64 = 10; // fail-safe for runaway AI
pub const FAN_OUT_TIMEOUT_MS: u64 = 50; // serialization + broadcast (increased for initial state)
pub const AI_UPDATE_STRIDE: u64 = 2; // run AI every N frames (≈ 30 Hz) - more responsive bots

// Placeholder constants for projectile speeds (define these in your core::constants.rs)
pub const PISTOL_PROJECTILE_SPEED: f32 = 450.0;
pub const SHOTGUN_PROJECTILE_SPEED: f32 = 450.0;
pub const RIFLE_PROJECTILE_SPEED: f32 = 600.0;
pub const SNIPER_PROJECTILE_SPEED: f32 = 800.0;

// ── Quick Match mode ───────────────────────────────────────────────
pub const QUICK_MATCH_MAX_PLAYERS: usize = 32;
pub const QUICK_MATCH_DURATION_SECS: f32 = 300.0; // 5 minutes
pub const QUICK_MATCH_BOT_FILL_DELAY_SECS: f32 = 15.0;
pub const QUICK_MATCH_MIN_HUMANS: usize = 16;

// ── Mobile match sizing ───────────────────────────────────────────
pub const MOBILE_BLITZ_MAX_PLAYERS: usize = 16;
pub const MOBILE_BLITZ_DURATION_SECS: f32 = 180.0; // 3 minutes
pub const MOBILE_STANDARD_MAX_PLAYERS: usize = 32;
pub const MOBILE_STANDARD_DURATION_SECS: f32 = 300.0; // 5 minutes

// ── Default full match ────────────────────────────────────────────
pub const FULL_MATCH_DURATION_SECS: f32 = 300.0; // 5 minutes (existing default)

// AoI tuning for higher player counts: smaller radius + capped nearest entities per client.
pub const AOI_RADIUS: f32 = 520.0;
pub const AOI_EXIT_RADIUS: f32 = 560.0;
/// Default AoI update interval.  At 60 Hz tick rate this means AoI is
/// refreshed every 3 ticks (~20 Hz).  Override at runtime with the
/// `MGS_AOI_UPDATE_DIVISOR` env var (tick divisor, e.g. 3 = 20 Hz, 6 = 10 Hz).
pub const AOI_UPDATE_INTERVAL_SECS: f32 = 0.05;
/// Default tick divisor for AoI updates (60 / 3 = 20 Hz).
pub const AOI_UPDATE_DIVISOR_DEFAULT: u64 = 3;
pub const AOI_MAX_VISIBLE_PLAYERS: usize = 96;
pub const AOI_MAX_VISIBLE_PROJECTILES: usize = 420;
pub const AOI_MAX_VISIBLE_PICKUPS: usize = 64;
pub const AOI_MAX_VISIBLE_WALLS: usize = 120;

#[cfg(test)]
mod tests {
    use super::*;

    // ── quantize_position tests ──────────────────────────────────

    #[test]
    fn quantize_position_zero() {
        let q = quantize_position(0.0);
        assert!(q.abs() < 0.2, "quantize_position(0.0) = {q}, expected ~0.0");
    }

    #[test]
    fn quantize_position_positive() {
        let q = quantize_position(500.0);
        assert!((q - 500.0).abs() < 0.2, "quantize_position(500.0) = {q}");
    }

    #[test]
    fn quantize_position_negative() {
        let q = quantize_position(-500.0);
        assert!((q - -500.0).abs() < 0.2, "quantize_position(-500.0) = {q}");
    }

    #[test]
    fn quantize_position_negative_world_min() {
        // World min is -800; must survive quantization.
        let q = quantize_position(-800.0);
        assert!((q - -800.0).abs() < 0.3, "quantize_position(-800.0) = {q}");
    }

    #[test]
    fn quantize_position_positive_world_max() {
        // World max is 800; must survive quantization.
        let q = quantize_position(800.0);
        assert!((q - 800.0).abs() < 0.3, "quantize_position(800.0) = {q}");
    }

    #[test]
    fn quantize_position_clamps_below_min() {
        let q = quantize_position(-5000.0);
        assert!(
            (q - QUANTIZE_POSITION_MIN).abs() < 0.3,
            "quantize_position(-5000.0) = {q}, expected ~{}",
            QUANTIZE_POSITION_MIN
        );
    }

    #[test]
    fn quantize_position_clamps_above_max() {
        let q = quantize_position(5000.0);
        assert!(
            (q - QUANTIZE_POSITION_MAX).abs() < 0.3,
            "quantize_position(5000.0) = {q}, expected ~{}",
            QUANTIZE_POSITION_MAX
        );
    }

    #[test]
    fn quantize_position_preserves_sign() {
        // Positive values stay positive
        assert!(quantize_position(100.0) > 0.0);
        // Negative values stay negative
        assert!(quantize_position(-100.0) < 0.0);
    }

    #[test]
    fn quantize_position_roundtrip_accuracy() {
        // Check that typical game positions survive quantization with < 0.25 unit error.
        for &v in &[-750.0, -400.0, -100.0, 0.0, 100.0, 400.0, 750.0] {
            let q = quantize_position(v);
            assert!(
                (q - v).abs() < 0.25,
                "quantize_position({v}) = {q}, error = {}",
                (q - v).abs()
            );
        }
    }

    #[test]
    fn quantize_position_non_finite_defaults_to_zero() {
        assert_eq!(quantize_position(f32::NAN), 0.0);
        assert_eq!(quantize_position(f32::INFINITY), 0.0);
        assert_eq!(quantize_position(f32::NEG_INFINITY), 0.0);
    }

    #[test]
    fn dequantize_position_zero_is_min() {
        // u16 value 0 should map back to QUANTIZE_POSITION_MIN
        let v = dequantize_position(0);
        assert!(
            (v - QUANTIZE_POSITION_MIN).abs() < 0.2,
            "dequantize_position(0) = {v}, expected ~{}",
            QUANTIZE_POSITION_MIN
        );
    }

    #[test]
    fn dequantize_position_max_is_max() {
        // u16::MAX should map back to QUANTIZE_POSITION_MAX
        let v = dequantize_position(u16::MAX);
        assert!(
            (v - QUANTIZE_POSITION_MAX).abs() < 0.2,
            "dequantize_position(u16::MAX) = {v}, expected ~{}",
            QUANTIZE_POSITION_MAX
        );
    }

    #[test]
    fn dequantize_position_midpoint() {
        // Midpoint of u16 range should map to 0.0 (center of [-4096, 4096])
        let mid = u16::MAX / 2;
        let v = dequantize_position(mid);
        assert!(
            v.abs() < 0.2,
            "dequantize_position({mid}) = {v}, expected ~0.0"
        );
    }

    // ── quantize_velocity tests ──────────────────────────────────

    #[test]
    fn quantize_velocity_negative_preserved() {
        let q = quantize_velocity(-200.0);
        assert!(
            q < 0.0,
            "quantize_velocity(-200.0) = {q}, expected negative"
        );
        assert!((q - -200.0).abs() < 10.0);
    }

    #[test]
    fn quantize_velocity_non_finite_defaults_to_zero() {
        assert_eq!(quantize_velocity(f32::NAN), 0.0);
        assert_eq!(quantize_velocity(f32::INFINITY), 0.0);
        assert_eq!(quantize_velocity(f32::NEG_INFINITY), 0.0);
    }

    // ── quantize_rotation tests ──────────────────────────────────

    #[test]
    fn quantize_rotation_negative_wraps() {
        let q = quantize_rotation(-std::f32::consts::FRAC_PI_2);
        // Should wrap to ~3*PI/2
        assert!(q > 0.0, "quantize_rotation(-PI/2) should wrap to positive");
    }

    #[test]
    fn quantize_rotation_non_finite_defaults_to_zero() {
        assert_eq!(quantize_rotation(f32::NAN), 0.0);
        assert_eq!(quantize_rotation(f32::INFINITY), 0.0);
        assert_eq!(quantize_rotation(f32::NEG_INFINITY), 0.0);
    }

    #[test]
    fn quantize_rotation_wraps_tau_to_zero_bucket() {
        let at_zero = quantize_rotation(0.0);
        let at_tau = quantize_rotation(std::f32::consts::TAU);
        assert!(
            (at_zero - at_tau).abs() < f32::EPSILON,
            "expected TAU to quantize to same bucket as 0, got {at_zero} vs {at_tau}"
        );
    }

    // ── partition & tick tests ──────────────────────────────────

    #[test]
    fn partition_grid_size_is_nonzero() {
        const { assert!(PARTITION_GRID_SIZE >= 1, "PARTITION_GRID_SIZE must be >= 1") };
    }

    #[test]
    fn partition_sizes_are_positive() {
        const { assert!(PARTITION_SIZE_X > 0.0, "PARTITION_SIZE_X must be > 0") };
        const { assert!(PARTITION_SIZE_Y > 0.0, "PARTITION_SIZE_Y must be > 0") };
    }

    #[test]
    fn validate_partition_grid_size_accepts_valid() {
        validate_partition_grid_size(1);
        validate_partition_grid_size(8);
        validate_partition_grid_size(128);
    }

    #[test]
    #[should_panic(expected = "Partition grid size must be >= 1")]
    fn validate_partition_grid_size_rejects_zero() {
        validate_partition_grid_size(0);
    }

    #[test]
    fn tick_duration_is_consistent() {
        assert_eq!(TICK_DURATION_MS, 1000 / SERVER_TICK_RATE);
        assert_eq!(TICK_DURATION.as_millis() as u64, TICK_DURATION_MS);
    }
}
