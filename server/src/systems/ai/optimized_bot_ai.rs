// Optimized Bot AI with CTF Support

use crate::core::constants::*;
use crate::core::types::{
    CorePickupType, EntityId, PlayerID, PlayerInputData, PlayerState, ServerWeaponType, Vec2, Wall,
    FIELD_MISC,
};
use crate::flatbuffers_generated::game_protocol as fb;
use crate::server::instance::{BotBehaviorState, BotController, MassiveGameServer};
use crate::systems::ai::commander::{
    MotionSample, PredictiveMotionModel, ThreatPredictor, ThreatSample,
};
use crate::world::navigation::GridNav;

use crate::core::deterministic_rng::DeterministicRng;
use dashmap::DashMap;
use parking_lot::RwLock as ParkingLotRwLock;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracing::{debug, trace};

thread_local! {
    /// Per-thread deterministic RNG for bot AI.  Re-seeded each frame in
    /// `update_bots_batch` so that given the same frame counter the entire
    /// bot decision sequence is reproducible.
    static BOT_RNG: RefCell<DeterministicRng> = RefCell::new(DeterministicRng::new(0));
}

/// Convenience: borrow the thread-local deterministic RNG for the duration of
/// the closure.  All bot AI randomness should go through this so that the
/// simulation is fully reproducible from a given seed.
#[inline]
fn with_bot_rng<R>(f: impl FnOnce(&mut DeterministicRng) -> R) -> R {
    BOT_RNG.with(|cell| f(&mut cell.borrow_mut()))
}

// Optimized constants
const BOT_SIMPLE_MOVEMENT_ONLY: bool = false; // Enable full AI with combat
const BOT_TARGET_ACQUISITION_RANGE: f32 = 600.0; // Increased combat range
const BOT_FLAG_DETECTION_RANGE: f32 = 2000.0; // See flags from far away
const BOT_FLAG_CHASE_PRIORITY: f32 = 3.0; // High priority for flag objectives
const BOT_MOVEMENT_TOLERANCE: f32 = 50.0; // Distance to consider "at target"
const BOT_STUCK_THRESHOLD: f32 = 10.0; // Min distance to move to not be considered stuck
const BOT_STUCK_TIME_THRESHOLD: f32 = 2.0; // Seconds before considering bot stuck
const BOT_STUCK_CHECK_INTERVAL: f32 = 0.5; // Check every half second
const BOT_STUCK_TARGET_TOLERANCE: f32 = BOT_MOVEMENT_TOLERANCE + 20.0;
const BOT_PREDICTIVE_MODEL_MAX_ENTRIES: usize = 4096;
const BOT_PREDICTIVE_MODEL_CLEANUP_INTERVAL_TICKS: u64 = 300;
const BOT_PICKUP_INTEREST_RADIUS: f32 = 780.0;
const BOT_PICKUP_EMERGENCY_HEALTH: i32 = 40;
const BOT_PICKUP_LOW_HEALTH: i32 = 70;
const BOT_PICKUP_LOW_AMMO_RATIO: f32 = 0.35;

// ── A* pathfinding constants ─────────────────────────────────────────
/// Cell size for the A* navigation grid (world units per cell).
const BOT_NAV_GRID_CELL_SIZE: f32 = 20.0;
/// Maximum number of waypoints to keep in a bot's path (longer paths are
/// down-sampled by striding).
const BOT_PATH_MAX_WAYPOINTS: usize = 18;
/// Distance (world units) at which a bot considers it has reached a waypoint
/// and advances to the next one.
const BOT_WAYPOINT_ARRIVAL_DIST: f32 = 30.0;
/// Ticks between forced path recomputation (60 ticks = 1 second at 60 Hz).
const BOT_PATH_RECOMPUTE_INTERVAL_TICKS: u64 = 60;
/// If the target moves more than this distance squared since last path compute,
/// recompute the path immediately.
const BOT_PATH_TARGET_MOVED_THRESHOLD_SQ: f32 = 150.0 * 150.0;

// ── Cached GridNav for A* pathfinding ────────────────────────────────
#[derive(Clone)]
struct BotNavGridCache {
    grid: Option<Arc<GridNav>>,
    wall_index_frame: u64,
    active_walls_by_id: HashMap<EntityId, Wall>,
}

impl Default for BotNavGridCache {
    fn default() -> Self {
        Self {
            grid: None,
            wall_index_frame: u64::MAX,
            active_walls_by_id: HashMap::new(),
        }
    }
}

static BOT_NAV_GRID_CACHE: OnceLock<ParkingLotRwLock<BotNavGridCache>> = OnceLock::new();

fn bot_nav_grid_cache() -> &'static ParkingLotRwLock<BotNavGridCache> {
    BOT_NAV_GRID_CACHE.get_or_init(|| ParkingLotRwLock::new(BotNavGridCache::default()))
}

// ── Tick-based timing constants ──────────────────────────────────────
// At 60 Hz, 30 ticks = 0.5s decision interval, 6 ticks = 100ms reaction time,
// 60 ticks = 1s weapon switch cooldown.
const BOT_DECISION_INTERVAL_TICKS: u64 = 30; // 0.5s at 60 Hz
const BOT_REACTION_TIME_TICKS: u64 = 6; // ~100ms at 60 Hz
const BOT_WEAPON_SWITCH_COOLDOWN_TICKS: u64 = 60; // 1s at 60 Hz

// ── AI Level-of-Detail (LOD) constants ───────────────────────────────
// Bots far from all human players receive reduced AI processing to save CPU.
/// Near tier: within AoI of any human (full AI every tick).
const BOT_LOD_NEAR_DISTANCE: f32 = AOI_RADIUS; // 520 units
/// Medium tier: between Near and Far (AI every 4th tick, simplified decisions).
const BOT_LOD_MEDIUM_DISTANCE: f32 = 1500.0;
/// Far tier: beyond Medium from all humans (AI every 8th tick, basic wander only).
/// Stride for Medium LOD tier: run AI every N-th tick.
const BOT_LOD_MEDIUM_STRIDE: u64 = 4;
/// Stride for Far LOD tier: run AI every N-th tick.
const BOT_LOD_FAR_STRIDE: u64 = 8;

/// Level-of-Detail tier for bot AI processing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BotAiLodTier {
    /// Within AoI range of a human player: full AI every tick.
    Near,
    /// 520-1500 units from nearest human: simplified AI every 4th tick.
    Medium,
    /// >1500 units from all humans: basic wander every 8th tick.
    Far,
}

impl BotAiLodTier {
    /// Classify a bot into a LOD tier based on distance (squared) to the
    /// nearest human player.
    pub fn classify(min_dist_sq_to_human: f32) -> Self {
        let near_sq = BOT_LOD_NEAR_DISTANCE * BOT_LOD_NEAR_DISTANCE;
        let medium_sq = BOT_LOD_MEDIUM_DISTANCE * BOT_LOD_MEDIUM_DISTANCE;
        if min_dist_sq_to_human <= near_sq {
            BotAiLodTier::Near
        } else if min_dist_sq_to_human <= medium_sq {
            BotAiLodTier::Medium
        } else {
            BotAiLodTier::Far
        }
    }

    /// Whether this tick should be processed for the given LOD tier.
    #[inline]
    pub fn should_process(self, frame_count: u64) -> bool {
        match self {
            BotAiLodTier::Near => true,
            BotAiLodTier::Medium => frame_count.is_multiple_of(BOT_LOD_MEDIUM_STRIDE),
            BotAiLodTier::Far => frame_count.is_multiple_of(BOT_LOD_FAR_STRIDE),
        }
    }

    #[inline]
    pub fn classify_from_human_presence(
        has_any_humans: bool,
        has_near_human: bool,
        has_medium_human: bool,
    ) -> Self {
        if !has_any_humans || has_near_human {
            BotAiLodTier::Near
        } else if has_medium_human {
            BotAiLodTier::Medium
        } else {
            BotAiLodTier::Far
        }
    }
}

#[derive(Debug, Clone)]
enum BotObjective {
    AttackEnemyFlag,        // Go get the enemy flag
    DefendOwnFlag,          // Stay near own flag base
    ChaseEnemyCarrier,      // Chase enemy who has our flag
    ProtectFriendlyCarrier, // Protect teammate with enemy flag
    PatrolMidfield,         // General patrol
    EngageNearbyEnemy,      // Fight nearby enemy
}

/// Personality profiles that influence bot decision-making, weapon preferences,
/// engagement ranges, and retreat thresholds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BotPersonality {
    /// Prefer Shotgun/Melee, rush targets, shorter engagement range (150u), chase fleeing enemies
    Aggressive,
    /// Prefer Rifle/Sniper, camp/hold position, longer engagement range (400u), retreat when health < 50%
    Defensive,
    /// Use current defaults, adapt weapon choice to situation
    Balanced,
}

impl BotPersonality {
    /// Deterministically assign a personality at creation.
    pub fn random() -> Self {
        with_bot_rng(|rng| match rng.gen_range_u8(0, 3) {
            0 => BotPersonality::Aggressive,
            1 => BotPersonality::Defensive,
            _ => BotPersonality::Balanced,
        })
    }

    /// The engagement range threshold for this personality.
    pub fn engagement_range(&self) -> f32 {
        match self {
            BotPersonality::Aggressive => 150.0,
            BotPersonality::Defensive => 400.0,
            BotPersonality::Balanced => 300.0,
        }
    }

    /// Preferred weapon slot to switch to (1 = primary, 2 = secondary).
    /// Returns None if the bot should keep its current weapon.
    pub fn preferred_weapon_slot(
        &self,
        current_weapon: ServerWeaponType,
        enemy_distance: f32,
    ) -> Option<u8> {
        match self {
            BotPersonality::Aggressive => {
                // Prefer Shotgun at close range, Melee at very close
                if enemy_distance < 60.0 {
                    if current_weapon != ServerWeaponType::Melee {
                        return Some(2); // Switch to melee slot
                    }
                } else if enemy_distance < 200.0 && current_weapon != ServerWeaponType::Shotgun {
                    return Some(1); // Switch to shotgun slot
                }
                None
            }
            BotPersonality::Defensive => {
                // Prefer Sniper at long range, Rifle at medium
                if enemy_distance > 400.0 {
                    if current_weapon != ServerWeaponType::Sniper {
                        return Some(2); // Switch to sniper slot
                    }
                } else if enemy_distance > 150.0 && current_weapon != ServerWeaponType::Rifle {
                    return Some(1); // Switch to rifle slot
                }
                None
            }
            BotPersonality::Balanced => {
                // Adapt to situation
                if enemy_distance < 100.0 && current_weapon != ServerWeaponType::Shotgun {
                    Some(1)
                } else if enemy_distance > 350.0 && current_weapon != ServerWeaponType::Sniper {
                    Some(2)
                } else {
                    None
                }
            }
        }
    }

    /// Whether this personality should retreat given current health (0-100 scale).
    pub fn should_retreat(&self, health_pct: f32) -> bool {
        match self {
            BotPersonality::Aggressive => false, // Never retreat
            BotPersonality::Defensive => health_pct < 50.0,
            BotPersonality::Balanced => health_pct < 25.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BotDifficultyTier {
    Easy,
    Normal,
    Hard,
}

impl BotDifficultyTier {
    fn from_bot_id(bot_id: &PlayerID) -> Self {
        let mut hash = 1469598103934665603u64;
        for byte in bot_id.as_ref().as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(1099511628211u64);
        }
        match hash % 100 {
            0..=29 => BotDifficultyTier::Easy,
            30..=74 => BotDifficultyTier::Normal,
            _ => BotDifficultyTier::Hard,
        }
    }

    #[inline]
    fn aim_accuracy(self) -> f32 {
        match self {
            BotDifficultyTier::Easy => 0.50,
            BotDifficultyTier::Normal => 0.70,
            BotDifficultyTier::Hard => 0.90,
        }
    }

    #[inline]
    fn reaction_time_ticks(self) -> u64 {
        match self {
            BotDifficultyTier::Easy => BOT_REACTION_TIME_TICKS.saturating_mul(2),
            BotDifficultyTier::Normal => BOT_REACTION_TIME_TICKS,
            BotDifficultyTier::Hard => BOT_REACTION_TIME_TICKS.saturating_sub(2).max(1),
        }
    }

    #[inline]
    fn shoot_probability(self) -> f32 {
        match self {
            BotDifficultyTier::Easy => 0.50,
            BotDifficultyTier::Normal => 0.68,
            BotDifficultyTier::Hard => 0.84,
        }
    }
}

pub struct OptimizedBotAI;

#[derive(Clone)]
struct EnemySnapshot {
    id: PlayerID,
    x: f32,
    y: f32,
    velocity_x: f32,
    velocity_y: f32,
    carries_flag_team_id: u8,
    weapon: ServerWeaponType,
}

#[derive(Clone, Copy)]
struct LivePlayerSnapshot {
    x: f32,
    y: f32,
    team_id: u8,
}

#[derive(Clone)]
struct PickupSnapshot {
    x: f32,
    y: f32,
    pickup_type: CorePickupType,
}

#[derive(Clone)]
struct BotSnapshotOwned {
    id: PlayerID,
    username: String,
    health: i32,
    x: f32,
    y: f32,
    velocity_x: f32,
    velocity_y: f32,
    rotation: f32,
    ammo: i32,
    weapon: ServerWeaponType,
    team_id: u8,
    is_carrying_flag_team_id: u8,
    last_processed_input_sequence: u32,
}

#[derive(Default)]
struct RuntimePredictiveModels {
    motion_models: DashMap<PlayerID, PredictiveMotionModel>,
    threat_models: DashMap<PlayerID, ThreatPredictor>,
}

static RUNTIME_PREDICTIVE_MODELS: OnceLock<RuntimePredictiveModels> = OnceLock::new();

fn runtime_predictive_models() -> &'static RuntimePredictiveModels {
    RUNTIME_PREDICTIVE_MODELS.get_or_init(RuntimePredictiveModels::default)
}

fn should_cleanup_predictive_models(
    frame_count: u64,
    predictive_models: &RuntimePredictiveModels,
) -> bool {
    predictive_models.motion_models.len() > BOT_PREDICTIVE_MODEL_MAX_ENTRIES
        || predictive_models.threat_models.len() > BOT_PREDICTIVE_MODEL_MAX_ENTRIES
        || frame_count.is_multiple_of(BOT_PREDICTIVE_MODEL_CLEANUP_INTERVAL_TICKS)
}

#[derive(Clone, Copy, Default)]
struct TeamObjectiveMetrics {
    defenders_at_base: usize,
    attackers_going_for_flag: usize,
}

#[derive(Clone, Copy, Default)]
struct TeamObjectiveSummary {
    team1: TeamObjectiveMetrics,
    team2: TeamObjectiveMetrics,
}

#[derive(Clone)]
struct TargetSolution {
    enemy_id: PlayerID,
    direct_position: Vec2,
    predicted_position: Vec2,
    distance_sq: f32,
    aim_angle: f32,
}

impl TeamObjectiveSummary {
    #[inline]
    fn for_team(&self, team_id: u8) -> TeamObjectiveMetrics {
        match team_id {
            1 => self.team1,
            2 => self.team2,
            _ => TeamObjectiveMetrics::default(),
        }
    }

    #[inline]
    fn for_team_mut(&mut self, team_id: u8) -> Option<&mut TeamObjectiveMetrics> {
        match team_id {
            1 => Some(&mut self.team1),
            2 => Some(&mut self.team2),
            _ => None,
        }
    }
}

impl OptimizedBotAI {
    /// Process bots with distance-based LOD: Near bots get full AI every tick,
    /// Medium bots get simplified AI every 4th tick, Far bots get basic wander
    /// every 8th tick.  Timing uses tick counts for determinism.
    pub fn update_bots_batch(server_instance: &MassiveGameServer, delta_time: f32) {
        let frame_count = server_instance
            .frame_counter
            .load(std::sync::atomic::Ordering::Relaxed);
        let now_ms = server_instance.get_server_timestamp_ms();
        let predictive_models = runtime_predictive_models();

        // Seed the deterministic RNG from the frame counter so that all bot
        // decisions this tick are reproducible given the same frame number.
        // The constant is an arbitrary mixer to avoid degenerate patterns for
        // small frame numbers.
        BOT_RNG.with(|cell| {
            *cell.borrow_mut() = DeterministicRng::new(
                frame_count.wrapping_mul(2654435761), // Knuth multiplicative hash
            );
        });

        // Get list of bot IDs (reuse allocation)
        thread_local! {
            static BOT_IDS: RefCell<Vec<PlayerID>> = const { RefCell::new(Vec::new()) };
        }
        let mut bot_ids = BOT_IDS.with(|cell| std::mem::take(&mut *cell.borrow_mut()));
        bot_ids.clear();
        bot_ids.extend(
            server_instance
                .bot_players
                .iter()
                .map(|entry| entry.key().clone()),
        );

        if bot_ids.is_empty() {
            if !predictive_models.motion_models.is_empty()
                || !predictive_models.threat_models.is_empty()
            {
                predictive_models.motion_models.clear();
                predictive_models.threat_models.clear();
                debug!("Cleared predictive bot AI models because there are no active bots.");
            }
            BOT_IDS.with(|cell| *cell.borrow_mut() = bot_ids);
            return;
        }

        // Get current match info
        let match_info_guard = server_instance.match_info.read();
        let game_mode = match_info_guard.game_mode;
        let match_state = match_info_guard.match_state;
        let flag_states = &match_info_guard.flag_states;

        // Precompute enemies and team objective counts once per tick.
        let mut enemies_team1 = Vec::new();
        let mut enemies_team2 = Vec::new();
        let mut live_human_ids: HashSet<PlayerID> = HashSet::new();
        let mut live_players_by_id: HashMap<PlayerID, LivePlayerSnapshot> = HashMap::new();
        let mut team_objectives = TeamObjectiveSummary::default();
        let team1_base = MassiveGameServer::get_flag_base_position(1);
        let team2_base = MassiveGameServer::get_flag_base_position(2);
        let team1_enemy_flag_pos = flag_states.get(&2).map(|f| f.position);
        let team2_enemy_flag_pos = flag_states.get(&1).map(|f| f.position);
        let commander_waypoint_team1 = server_instance.commander_primary_waypoint_for_team(1);
        let commander_waypoint_team2 = server_instance.commander_primary_waypoint_for_team(2);
        let commander_attack_bias_team1 = server_instance.commander_attack_bias_for_team(1);
        let commander_attack_bias_team2 = server_instance.commander_attack_bias_for_team(2);

        server_instance
            .player_manager
            .for_each_player(|id, player| {
                if !player.alive || player.team_id == 0 {
                    return;
                }

                {
                    let mut motion_model = predictive_models
                        .motion_models
                        .entry(id.clone())
                        .or_default();
                    motion_model.push_sample(MotionSample {
                        timestamp_ms: now_ms,
                        position: Vec2::new(player.x, player.y),
                    });
                }

                let enemy_snapshot = EnemySnapshot {
                    id: id.clone(),
                    x: player.x,
                    y: player.y,
                    velocity_x: player.velocity_x,
                    velocity_y: player.velocity_y,
                    carries_flag_team_id: player.is_carrying_flag_team_id,
                    weapon: player.weapon,
                };

                live_players_by_id.insert(
                    id.clone(),
                    LivePlayerSnapshot {
                        x: player.x,
                        y: player.y,
                        team_id: player.team_id,
                    },
                );
                if !server_instance.bot_players.contains_key(id) {
                    live_human_ids.insert(id.clone());
                }

                if let Some(metrics) = team_objectives.for_team_mut(player.team_id) {
                    let own_base = if player.team_id == 1 {
                        team1_base
                    } else {
                        team2_base
                    };
                    let dist_to_base_sq =
                        (player.x - own_base.x).powi(2) + (player.y - own_base.y).powi(2);
                    if dist_to_base_sq < 200.0 * 200.0 {
                        metrics.defenders_at_base += 1;
                    }

                    let enemy_flag_pos = if player.team_id == 1 {
                        team1_enemy_flag_pos
                    } else {
                        team2_enemy_flag_pos
                    };
                    if let Some(flag_pos) = enemy_flag_pos {
                        let dist_to_enemy_flag_sq =
                            (player.x - flag_pos.x).powi(2) + (player.y - flag_pos.y).powi(2);
                        if dist_to_enemy_flag_sq < BOT_FLAG_DETECTION_RANGE.powi(2) {
                            metrics.attackers_going_for_flag += 1;
                        }
                    }
                }

                if player.team_id == 1 {
                    enemies_team2.push(enemy_snapshot);
                } else if player.team_id == 2 {
                    enemies_team1.push(enemy_snapshot);
                }
            });

        trace!(
            "Frame {}: Processing {} bots, {} live humans for LOD",
            frame_count,
            bot_ids.len(),
            live_human_ids.len()
        );

        let active_pickups: Vec<PickupSnapshot> = {
            let pickups_snapshot = server_instance.snapshots.pickup_soa_snapshot.load();
            pickups_snapshot
                .states()
                .iter()
                .filter(|pickup| pickup.is_active)
                .map(|pickup| PickupSnapshot {
                    x: pickup.x,
                    y: pickup.y,
                    pickup_type: pickup.pickup_type.clone(),
                })
                .collect()
        };

        // Process bots with LOD-based tick skipping
        for bot_id in bot_ids.iter() {
            // Build an owned snapshot first so any read guard is dropped before mutable access.
            let bot_snapshot = {
                let bot_state_guard_opt = server_instance.player_manager.get_player_state(bot_id);
                let Some(bot_state_guard) = bot_state_guard_opt else {
                    continue;
                };
                let bot_state = &*bot_state_guard;
                if !bot_state.alive {
                    continue;
                }
                BotSnapshotOwned {
                    id: bot_state.id.clone(),
                    username: bot_state.username.clone(),
                    health: bot_state.health,
                    x: bot_state.x,
                    y: bot_state.y,
                    velocity_x: bot_state.velocity_x,
                    velocity_y: bot_state.velocity_y,
                    rotation: bot_state.rotation,
                    ammo: bot_state.ammo,
                    weapon: bot_state.weapon,
                    team_id: bot_state.team_id,
                    is_carrying_flag_team_id: bot_state.is_carrying_flag_team_id,
                    last_processed_input_sequence: bot_state.last_processed_input_sequence,
                }
            };

            // ── LOD classification ───────────────────────────────────
            // Use the shared player spatial index to bucket the bot into
            // Near / Medium / Far without scanning every human player.
            let lod_tier = if live_human_ids.is_empty() {
                BotAiLodTier::Near
            } else {
                let has_near_human = server_instance
                    .spatial_index
                    .query_nearby_players(bot_snapshot.x, bot_snapshot.y, BOT_LOD_NEAR_DISTANCE)
                    .into_iter()
                    .any(|candidate_id| live_human_ids.contains(&candidate_id));
                let has_medium_human = has_near_human
                    || server_instance
                        .spatial_index
                        .query_nearby_players(
                            bot_snapshot.x,
                            bot_snapshot.y,
                            BOT_LOD_MEDIUM_DISTANCE,
                        )
                        .into_iter()
                        .any(|candidate_id| live_human_ids.contains(&candidate_id));
                BotAiLodTier::classify_from_human_presence(
                    !live_human_ids.is_empty(),
                    has_near_human,
                    has_medium_human,
                )
            };

            // Skip this bot entirely if the LOD tier says so.
            if !lod_tier.should_process(frame_count) {
                continue;
            }

            // Update bot controller
            if let Some(mut bot_controller_entry) = server_instance.bot_players.get_mut(bot_id) {
                let bot_controller = bot_controller_entry.value_mut();

                // Tick-based decision interval check
                let ticks_since_decision =
                    frame_count.saturating_sub(bot_controller.last_decision_tick);
                if ticks_since_decision >= BOT_DECISION_INTERVAL_TICKS {
                    bot_controller.last_decision_tick = frame_count;
                    // Keep the Instant for any legacy/external code that might reference it.
                    bot_controller.last_decision_time = Instant::now();

                    // Remember old target so we can detect if the decision changed it.
                    let old_target = bot_controller.target_position;
                    let pickup_override = lod_tier != BotAiLodTier::Far
                        && Self::maybe_retarget_for_pickup(
                            bot_controller,
                            &bot_snapshot,
                            &active_pickups,
                        );

                    if !pickup_override {
                        match lod_tier {
                            BotAiLodTier::Far => {
                                // Far tier: basic wander only
                                Self::make_far_wander_decision(bot_controller, &bot_snapshot);
                            }
                            BotAiLodTier::Medium => {
                                // Medium tier: simplified decisions (no CTF objective, no commander)
                                Self::make_simple_movement_decision(bot_controller, &bot_snapshot);
                            }
                            BotAiLodTier::Near => {
                                // Near tier: full AI
                                if BOT_SIMPLE_MOVEMENT_ONLY {
                                    Self::make_simple_movement_decision(
                                        bot_controller,
                                        &bot_snapshot,
                                    );
                                } else if game_mode == fb::GameModeType::CaptureTheFlag
                                    && match_state == fb::MatchStateType::Active
                                {
                                    let enemies = if bot_snapshot.team_id == 1 {
                                        &enemies_team1
                                    } else {
                                        &enemies_team2
                                    };
                                    Self::make_ctf_decision(
                                        bot_controller,
                                        &bot_snapshot,
                                        flag_states,
                                        &live_players_by_id,
                                        team_objectives,
                                        enemies,
                                        if bot_snapshot.team_id == 1 {
                                            commander_attack_bias_team1
                                        } else {
                                            commander_attack_bias_team2
                                        },
                                    );
                                } else {
                                    Self::make_simple_movement_decision(
                                        bot_controller,
                                        &bot_snapshot,
                                    );
                                }

                                let commander_waypoint = if bot_snapshot.team_id == 1 {
                                    commander_waypoint_team1
                                } else {
                                    commander_waypoint_team2
                                };
                                Self::apply_commander_waypoint(
                                    bot_controller,
                                    &bot_snapshot,
                                    commander_waypoint,
                                );
                            }
                        }
                    }

                    // If the decision changed the target, invalidate the A* path
                    // so it will be recomputed with the new destination.
                    if bot_controller.target_position != old_target {
                        Self::invalidate_path(bot_controller);
                    }

                    debug!(
                        "Bot {} made new decision: {:?} targeting {:?} (LOD={:?})",
                        bot_snapshot.username,
                        bot_controller.behavior_state,
                        bot_controller.target_position,
                        lod_tier
                    );
                }

                // Check if bot is stuck before generating input
                Self::check_stuck_status(bot_controller, &bot_snapshot, delta_time);

                // Generate input - Far bots get simplified movement only
                let enemies = if bot_snapshot.team_id == 1 {
                    &enemies_team1
                } else {
                    &enemies_team2
                };
                let input = if lod_tier == BotAiLodTier::Far {
                    Self::generate_simple_movement_input(&bot_snapshot, bot_controller)
                } else {
                    Self::generate_combat_input(
                        &bot_snapshot,
                        bot_controller,
                        server_instance,
                        game_mode,
                        enemies,
                        frame_count,
                    )
                };

                // Queue the input
                if let Some(mut player_state_entry) =
                    server_instance.player_manager.get_player_state_mut(bot_id)
                {
                    let next_behavior = bot_controller.behavior_state.as_u8();
                    let misc_changed = !player_state_entry.is_bot
                        || player_state_entry.bot_behavior != next_behavior;
                    player_state_entry.is_bot = true;
                    player_state_entry.bot_behavior = next_behavior;
                    if misc_changed {
                        player_state_entry.mark_field_changed(FIELD_MISC);
                    }
                    if input.move_forward
                        || input.move_backward
                        || input.move_left
                        || input.move_right
                        || input.shooting
                    {
                        trace!(
                            "Bot {} input - forward:{} back:{} left:{} right:{} rot:{:.2} shoot:{}",
                            bot_snapshot.username,
                            input.move_forward,
                            input.move_backward,
                            input.move_left,
                            input.move_right,
                            input.rotation,
                            input.shooting
                        );
                    }
                    player_state_entry.queue_input(input);
                }
            }
        }
        let live_player_count = live_players_by_id.len();
        let predictive_over_capacity = predictive_models.motion_models.len()
            > BOT_PREDICTIVE_MODEL_MAX_ENTRIES
            || predictive_models.threat_models.len() > BOT_PREDICTIVE_MODEL_MAX_ENTRIES;
        if should_cleanup_predictive_models(frame_count, predictive_models) {
            let motion_before = predictive_models.motion_models.len();
            let threat_before = predictive_models.threat_models.len();
            predictive_models
                .motion_models
                .retain(|player_id, _| live_players_by_id.contains_key(player_id));
            predictive_models
                .threat_models
                .retain(|player_id, _| live_players_by_id.contains_key(player_id));
            let motion_after = predictive_models.motion_models.len();
            let threat_after = predictive_models.threat_models.len();
            let removed_motion = motion_before.saturating_sub(motion_after);
            let removed_threat = threat_before.saturating_sub(threat_after);
            if predictive_over_capacity || removed_motion > 0 || removed_threat > 0 {
                debug!(
                    "Predictive model cleanup complete (live={} motion:{}->{} threat:{}->{} cap_guard={}).",
                    live_player_count,
                    motion_before,
                    motion_after,
                    threat_before,
                    threat_after,
                    predictive_over_capacity
                );
            }
        }

        drop(match_info_guard);
        BOT_IDS.with(|cell| *cell.borrow_mut() = bot_ids);
    }

    /// Far-tier wander: pick a random nearby target and patrol. No combat, no
    /// objective logic. Minimal CPU cost.
    fn make_far_wander_decision(bot_controller: &mut BotController, bot_state: &BotSnapshotOwned) {
        let (target_x, target_y) = with_bot_rng(|rng| {
            (
                (bot_state.x + rng.gen_range_f32(-200.0, 200.0))
                    .clamp(WORLD_MIN_X + 50.0, WORLD_MAX_X - 50.0),
                (bot_state.y + rng.gen_range_f32(-200.0, 200.0))
                    .clamp(WORLD_MIN_Y + 50.0, WORLD_MAX_Y - 50.0),
            )
        });
        bot_controller.target_position = Some(Vec2::new(target_x, target_y));
        bot_controller.behavior_state = BotBehaviorState::Patrolling;
    }

    /// Generate a simple movement-only input (no combat, no abilities).
    /// Used for Far LOD tier bots.
    fn generate_simple_movement_input(
        bot_state: &BotSnapshotOwned,
        bot_controller: &BotController,
    ) -> PlayerInputData {
        let mut input = PlayerInputData {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_millis() as u64,
            sequence: bot_state.last_processed_input_sequence.wrapping_add(1),
            move_forward: false,
            move_backward: false,
            move_left: false,
            move_right: false,
            shooting: false,
            reload: false,
            rotation: bot_state.rotation,
            melee_attack: false,
            change_weapon_slot: 0,
            use_ability_slot: 0,
            ping_x: 0.0,
            ping_y: 0.0,
        };

        if let Some(target_pos) = bot_controller.target_position {
            let dx = target_pos.x - bot_state.x;
            let dy = target_pos.y - bot_state.y;
            let dist_sq = dx * dx + dy * dy;
            input.rotation = dy.atan2(dx);
            if dist_sq > BOT_MOVEMENT_TOLERANCE * BOT_MOVEMENT_TOLERANCE {
                input.move_forward = true;
            }
        }

        input
    }

    /// Make CTF-specific decisions
    fn make_ctf_decision(
        bot_controller: &mut BotController,
        bot_state: &BotSnapshotOwned,
        flag_states: &HashMap<u8, crate::server::instance::ServerFlagState>,
        live_players_by_id: &HashMap<PlayerID, LivePlayerSnapshot>,
        team_objectives: TeamObjectiveSummary,
        enemies: &[EnemySnapshot],
        commander_attack_bias: Option<f32>,
    ) {
        let bot_team = bot_state.team_id;
        let enemy_team = if bot_team == 1 { 2 } else { 1 };

        // Determine objective based on game state
        let objective = Self::determine_ctf_objective(
            bot_state,
            flag_states,
            live_players_by_id,
            team_objectives,
            commander_attack_bias,
        );

        debug!(
            "Bot {} (Team {}) objective: {:?}",
            bot_state.username, bot_team, objective
        );

        match objective {
            BotObjective::AttackEnemyFlag => {
                // If carrying flag, go to own base. Otherwise attack enemy flag
                if bot_state.is_carrying_flag_team_id != 0 {
                    // Bot has enemy flag, return to own base
                    let own_base = MassiveGameServer::get_flag_base_position(bot_team);
                    bot_controller.target_position = Some(own_base);
                    bot_controller.behavior_state = BotBehaviorState::MovingToObjective;
                    debug!(
                        "Bot {} carrying flag, returning to base at {:?}",
                        bot_state.username, own_base
                    );
                } else if let Some(enemy_flag) = flag_states.get(&enemy_team) {
                    // Go get enemy flag
                    bot_controller.target_position = Some(enemy_flag.position);
                    bot_controller.behavior_state = BotBehaviorState::MovingToObjective;
                    debug!(
                        "Bot {} going for enemy flag at {:?}",
                        bot_state.username, enemy_flag.position
                    );
                }
            }
            BotObjective::DefendOwnFlag => {
                // Stay near own flag base with some variation
                let base_pos = MassiveGameServer::get_flag_base_position(bot_team);
                let defend_radius = 150.0;
                let (angle, distance) = with_bot_rng(|rng| {
                    (
                        rng.gen_range_f32(0.0, 2.0 * std::f32::consts::PI),
                        rng.gen_range_f32(50.0, defend_radius),
                    )
                });
                bot_controller.target_position = Some(Vec2::new(
                    base_pos.x + distance * angle.cos(),
                    base_pos.y + distance * angle.sin(),
                ));
                bot_controller.behavior_state = BotBehaviorState::Defending;
            }
            BotObjective::ChaseEnemyCarrier => {
                // Find and chase the enemy carrying our flag
                if let Some(own_flag) = flag_states.get(&bot_team) {
                    if let Some(carrier_id) = &own_flag.carrier_id {
                        if let Some(carrier_state) = live_players_by_id.get(carrier_id) {
                            bot_controller.target_position =
                                Some(Vec2::new(carrier_state.x, carrier_state.y));
                            bot_controller.target_enemy_id = Some(carrier_id.clone());
                            bot_controller.behavior_state = BotBehaviorState::Engaging;
                        }
                    }
                }
            }
            BotObjective::ProtectFriendlyCarrier => {
                // Find and protect teammate carrying enemy flag
                if let Some(enemy_flag) = flag_states.get(&enemy_team) {
                    if let Some(carrier_id) = &enemy_flag.carrier_id {
                        if let Some(carrier_state) = live_players_by_id.get(carrier_id) {
                            // Move near the carrier but not too close
                            let offset_angle = with_bot_rng(|rng| {
                                rng.gen_range_f32(0.0, 2.0 * std::f32::consts::PI)
                            });
                            let offset_dist = 100.0;
                            bot_controller.target_position = Some(Vec2::new(
                                carrier_state.x + offset_dist * offset_angle.cos(),
                                carrier_state.y + offset_dist * offset_angle.sin(),
                            ));
                            bot_controller.behavior_state = BotBehaviorState::Defending;
                        }
                    }
                }
            }
            BotObjective::PatrolMidfield => {
                // Patrol center area
                let (patrol_x, patrol_y) = with_bot_rng(|rng| {
                    (
                        rng.gen_range_f32(-400.0, 400.0),
                        rng.gen_range_f32(-400.0, 400.0),
                    )
                });
                bot_controller.target_position = Some(Vec2::new(patrol_x, patrol_y));
                bot_controller.behavior_state = BotBehaviorState::Patrolling;
            }
            BotObjective::EngageNearbyEnemy => {
                // Find nearest enemy
                if let Some((enemy_pos, enemy_id)) = Self::find_nearest_enemy(bot_state, enemies) {
                    bot_controller.target_position = Some(enemy_pos);
                    bot_controller.target_enemy_id = Some(enemy_id.clone());
                    bot_controller.behavior_state = BotBehaviorState::Engaging;
                }
            }
        }
    }

    /// Determine the best objective for a bot in CTF mode
    fn determine_ctf_objective(
        bot_state: &BotSnapshotOwned,
        flag_states: &HashMap<u8, crate::server::instance::ServerFlagState>,
        live_players_by_id: &HashMap<PlayerID, LivePlayerSnapshot>,
        team_objectives: TeamObjectiveSummary,
        commander_attack_bias: Option<f32>,
    ) -> BotObjective {
        let bot_team = bot_state.team_id;
        let enemy_team = if bot_team == 1 { 2 } else { 1 };

        // Check if bot is carrying flag - HIGHEST PRIORITY
        if bot_state.is_carrying_flag_team_id != 0 {
            // Bot has flag, should return to base
            return BotObjective::AttackEnemyFlag; // Will navigate to own base
        }

        // Check flag states
        let own_flag = flag_states.get(&bot_team);
        let enemy_flag = flag_states.get(&enemy_team);

        // Priority 1: Chase enemy who has our flag - VERY IMPORTANT
        if let Some(own_flag_state) = own_flag {
            if own_flag_state.status == fb::FlagStatus::Carried {
                if let Some(carrier_id) = &own_flag_state.carrier_id {
                    if live_players_by_id.get(carrier_id).is_some() {
                        // Always chase if our flag is taken
                        return BotObjective::ChaseEnemyCarrier;
                    }
                }
            }
        }

        // Priority 2: Protect friendly flag carrier
        if let Some(enemy_flag_state) = enemy_flag {
            if enemy_flag_state.status == fb::FlagStatus::Carried {
                if let Some(carrier_id) = &enemy_flag_state.carrier_id {
                    if let Some(carrier_state) = live_players_by_id.get(carrier_id) {
                        if carrier_state.team_id == bot_team {
                            // Always protect our flag carrier
                            return BotObjective::ProtectFriendlyCarrier;
                        }
                    }
                }
            }
        }

        let metrics = team_objectives.for_team(bot_team);
        let defenders_at_base = metrics.defenders_at_base;
        let attackers_going_for_flag = metrics.attackers_going_for_flag;
        let has_nearby_enemy = live_players_by_id.values().any(|player| {
            player.team_id != bot_team
                && (player.x - bot_state.x).powi(2) + (player.y - bot_state.y).powi(2)
                    <= BOT_TARGET_ACQUISITION_RANGE.powi(2)
        });

        // More aggressive role distribution
        let role_choice = with_bot_rng(|rng| rng.gen_range_i32(0, 100));

        let attack_bias = commander_attack_bias.unwrap_or(0.60).clamp(0.25, 0.85);
        let defend_roll_threshold = ((1.0 - attack_bias) * 40.0) as i32;
        let attack_roll_threshold = (defend_roll_threshold as f32 + attack_bias * 65.0) as i32;

        // Commander bias shifts attack-vs-defend composition for large teams.
        if defenders_at_base < 1 && role_choice < defend_roll_threshold {
            // Only 1-2 defenders needed
            BotObjective::DefendOwnFlag
        } else if attackers_going_for_flag < 5 && role_choice < attack_roll_threshold {
            // Most bots should attack
            BotObjective::AttackEnemyFlag
        } else if defenders_at_base < 2
            && own_flag.is_some_and(|f| f.status == fb::FlagStatus::Dropped)
        {
            // If our flag is dropped, help return it
            BotObjective::DefendOwnFlag
        } else if has_nearby_enemy {
            BotObjective::EngageNearbyEnemy
        } else if role_choice < 95 {
            BotObjective::AttackEnemyFlag
        } else {
            BotObjective::PatrolMidfield
        }
    }

    fn apply_commander_waypoint(
        bot_controller: &mut BotController,
        bot_state: &BotSnapshotOwned,
        commander_waypoint: Option<Vec2>,
    ) {
        if bot_state.team_id == 0 || bot_state.is_carrying_flag_team_id != 0 {
            return;
        }
        let Some(waypoint) = commander_waypoint else {
            return;
        };

        if with_bot_rng(|rng| rng.gen_bool(0.72)) {
            bot_controller.target_position = Some(waypoint);
            bot_controller.behavior_state = BotBehaviorState::MovingToObjective;
        }
    }

    /// Find nearest enemy to the bot from a precomputed list
    fn find_nearest_enemy(
        bot_state: &BotSnapshotOwned,
        enemies: &[EnemySnapshot],
    ) -> Option<(Vec2, PlayerID)> {
        let mut nearest_enemy = None;
        let mut nearest_dist_sq = f32::MAX;

        for enemy in enemies {
            let dist_sq = (enemy.x - bot_state.x).powi(2) + (enemy.y - bot_state.y).powi(2);
            if dist_sq < nearest_dist_sq && dist_sq < BOT_TARGET_ACQUISITION_RANGE.powi(2) {
                nearest_dist_sq = dist_sq;
                nearest_enemy = Some((Vec2::new(enemy.x, enemy.y), enemy.id.clone()));
            }
        }

        nearest_enemy
    }

    fn pickup_priority(bot_state: &BotSnapshotOwned, pickup_type: &CorePickupType) -> Option<f32> {
        let carrying_flag = bot_state.is_carrying_flag_team_id != 0;
        let max_ammo = PlayerState::get_max_ammo_for_weapon(bot_state.weapon).max(1) as f32;
        let ammo_ratio = (bot_state.ammo.max(0) as f32 / max_ammo).clamp(0.0, 1.0);

        match pickup_type {
            CorePickupType::Health => {
                if bot_state.health <= BOT_PICKUP_EMERGENCY_HEALTH {
                    Some(160.0)
                } else if bot_state.health <= BOT_PICKUP_LOW_HEALTH {
                    Some(115.0)
                } else if carrying_flag && bot_state.health < 95 {
                    Some(90.0)
                } else {
                    None
                }
            }
            CorePickupType::Ammo => {
                if carrying_flag {
                    None
                } else if ammo_ratio <= 0.12 {
                    Some(120.0)
                } else if ammo_ratio <= BOT_PICKUP_LOW_AMMO_RATIO {
                    Some(82.0)
                } else {
                    None
                }
            }
            CorePickupType::Shield => {
                if carrying_flag {
                    Some(92.0)
                } else if bot_state.health < 90 {
                    Some(62.0)
                } else {
                    None
                }
            }
            CorePickupType::SpeedBoost => {
                if carrying_flag {
                    Some(118.0)
                } else {
                    Some(42.0)
                }
            }
            CorePickupType::DamageBoost => {
                if carrying_flag {
                    None
                } else if bot_state.health >= 60 && ammo_ratio >= 0.25 {
                    Some(48.0)
                } else {
                    None
                }
            }
            CorePickupType::WeaponCrate(weapon) => {
                if carrying_flag {
                    None
                } else if *weapon != bot_state.weapon {
                    Some(34.0)
                } else {
                    None
                }
            }
        }
    }

    fn maybe_retarget_for_pickup(
        bot_controller: &mut BotController,
        bot_state: &BotSnapshotOwned,
        active_pickups: &[PickupSnapshot],
    ) -> bool {
        if active_pickups.is_empty() {
            return false;
        }

        let critical_need = bot_state.health <= BOT_PICKUP_EMERGENCY_HEALTH;
        let carrying_flag = bot_state.is_carrying_flag_team_id != 0;
        let interest_radius = if critical_need || carrying_flag {
            BOT_PICKUP_INTEREST_RADIUS * 1.55
        } else {
            BOT_PICKUP_INTEREST_RADIUS
        };
        let interest_radius_sq = interest_radius * interest_radius;

        let mut best_target = None;
        let mut best_score = f32::MIN;

        for pickup in active_pickups {
            let Some(priority) = Self::pickup_priority(bot_state, &pickup.pickup_type) else {
                continue;
            };

            let dx = pickup.x - bot_state.x;
            let dy = pickup.y - bot_state.y;
            let dist_sq = dx * dx + dy * dy;
            if dist_sq > interest_radius_sq {
                continue;
            }

            let score = priority - dist_sq.sqrt() * 0.11;
            if score > best_score {
                best_score = score;
                best_target = Some(Vec2::new(pickup.x, pickup.y));
            }
        }

        if let Some(target_position) = best_target {
            bot_controller.target_position = Some(target_position);
            bot_controller.target_enemy_id = None;
            bot_controller.behavior_state = BotBehaviorState::MovingToObjective;
            return true;
        }

        false
    }

    /// Enhanced movement decision with combat awareness, influenced by personality.
    fn make_simple_movement_decision(
        bot_controller: &mut BotController,
        bot_state: &BotSnapshotOwned,
    ) {
        let personality = bot_controller.personality;

        // Personality-weighted behavior distribution
        let (engage_pct, flank_pct) = match personality {
            BotPersonality::Aggressive => (60, 85), // 60% engage, 25% flank, 15% patrol
            BotPersonality::Defensive => (15, 35),  // 15% engage, 20% flank, 65% patrol/hold
            BotPersonality::Balanced => (40, 70),   // 40% engage, 30% flank, 30% patrol
        };

        let behavior_choice = with_bot_rng(|rng| rng.gen_range_i32(0, 100));

        if behavior_choice < engage_pct {
            // Aggressive: Move towards center for action
            let range = match personality {
                BotPersonality::Aggressive => 100.0, // Rush closer to center
                BotPersonality::Defensive => 300.0,  // Stay at range
                BotPersonality::Balanced => 200.0,
            };
            let (target_x, target_y) = with_bot_rng(|rng| {
                (
                    rng.gen_range_f32(-range, range),
                    rng.gen_range_f32(-range, range),
                )
            });
            bot_controller.target_position = Some(Vec2::new(target_x, target_y));
            bot_controller.behavior_state = BotBehaviorState::Engaging;
        } else if behavior_choice < flank_pct {
            // Flanking: Move to sides
            let (side, target_x_abs, target_y) = with_bot_rng(|rng| {
                let side = if rng.gen_bool(0.5) { 1.0 } else { -1.0 };
                (
                    side,
                    rng.gen_range_f32(300.0, 600.0),
                    rng.gen_range_f32(-400.0, 400.0),
                )
            });
            let target_x = side * target_x_abs;
            bot_controller.target_position = Some(Vec2::new(target_x, target_y));
            bot_controller.behavior_state = BotBehaviorState::Flanking;
        } else {
            // Patrol / hold position
            match personality {
                BotPersonality::Defensive => {
                    // Defensive bots hold near their current position
                    let (hold_x, hold_y) = with_bot_rng(|rng| {
                        (
                            bot_state.x + rng.gen_range_f32(-80.0, 80.0),
                            bot_state.y + rng.gen_range_f32(-80.0, 80.0),
                        )
                    });
                    let target_x = hold_x.clamp(WORLD_MIN_X + 100.0, WORLD_MAX_X - 100.0);
                    let target_y = hold_y.clamp(WORLD_MIN_Y + 100.0, WORLD_MAX_Y - 100.0);
                    bot_controller.target_position = Some(Vec2::new(target_x, target_y));
                    bot_controller.behavior_state = BotBehaviorState::Defending;
                }
                _ => {
                    let (target_x, target_y) = with_bot_rng(|rng| {
                        (
                            rng.gen_range_f32(WORLD_MIN_X + 100.0, WORLD_MAX_X - 100.0),
                            rng.gen_range_f32(WORLD_MIN_Y + 100.0, WORLD_MAX_Y - 100.0),
                        )
                    });
                    bot_controller.target_position = Some(Vec2::new(target_x, target_y));
                    bot_controller.behavior_state = BotBehaviorState::Patrolling;
                }
            }
        }

        trace!(
            "Bot {} behavior: {:?}, target: {:?}",
            bot_state.username,
            bot_controller.behavior_state,
            bot_controller.target_position
        );
    }

    /// Check if there's a clear line of sight between two positions
    fn has_line_of_sight(from: Vec2, to: Vec2, server_instance: &MassiveGameServer) -> bool {
        let candidate_walls = server_instance
            .wall_spatial_index
            .query_line_segment(from.x, from.y, to.x, to.y);
        for wall in candidate_walls {
            if wall.is_destructible && wall.current_health <= 0 {
                continue;
            }
            if Self::segment_hits_aabb(from, to, &wall) {
                return false;
            }
        }
        true
    }

    fn segment_hits_aabb(start: Vec2, end: Vec2, wall: &Wall) -> bool {
        let start_x = start.x;
        let start_y = start.y;
        let end_x = end.x;
        let end_y = end.y;
        let min_x = wall.x;
        let max_x = wall.x + wall.width;
        let min_y = wall.y;
        let max_y = wall.y + wall.height;

        let dx = end_x - start_x;
        let dy = end_y - start_y;
        let mut t_min = 0.0f32;
        let mut t_max = 1.0f32;

        if dx.abs() < f32::EPSILON {
            if start_x < min_x || start_x > max_x {
                return false;
            }
        } else {
            let inv_dx = 1.0 / dx;
            let mut t1 = (min_x - start_x) * inv_dx;
            let mut t2 = (max_x - start_x) * inv_dx;
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            t_min = t_min.max(t1);
            t_max = t_max.min(t2);
            if t_min > t_max {
                return false;
            }
        }

        if dy.abs() < f32::EPSILON {
            if start_y < min_y || start_y > max_y {
                return false;
            }
        } else {
            let inv_dy = 1.0 / dy;
            let mut t1 = (min_y - start_y) * inv_dy;
            let mut t2 = (max_y - start_y) * inv_dy;
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            t_min = t_min.max(t1);
            t_max = t_max.min(t2);
            if t_min > t_max {
                return false;
            }
        }

        !(t_max < 0.0 || t_min > 1.0)
    }

    fn select_enemy_target(
        bot_state: &BotSnapshotOwned,
        enemies: &[EnemySnapshot],
        game_mode: fb::GameModeType,
        now_ms: u64,
    ) -> Option<TargetSolution> {
        let predictive_models = runtime_predictive_models();
        let mut threat_model = predictive_models
            .threat_models
            .entry(bot_state.id.clone())
            .or_default();

        let mut selected: Option<(f32, TargetSolution)> = None;

        for enemy in enemies {
            let dx = enemy.x - bot_state.x;
            let dy = enemy.y - bot_state.y;
            let dist_sq = dx * dx + dy * dy;
            if dist_sq > (BOT_TARGET_ACQUISITION_RANGE * 2.0).powi(2) {
                continue;
            }

            let relative_speed = ((enemy.velocity_x - bot_state.velocity_x).powi(2)
                + (enemy.velocity_y - bot_state.velocity_y).powi(2))
            .sqrt();
            let sample = ThreatSample {
                distance: dist_sq.sqrt(),
                relative_speed,
                recent_damage_taken: if game_mode == fb::GameModeType::CaptureTheFlag
                    && enemy.carries_flag_team_id == bot_state.team_id
                {
                    75.0
                } else {
                    0.0
                },
                target_visibility: 1.0,
            };

            let mut threat_score = threat_model.predict_threat_score(sample);

            // Weapon-aware threat weighting based on distance
            let distance = dist_sq.sqrt();
            let weapon_threat_weight = match enemy.weapon {
                ServerWeaponType::Sniper if distance > 400.0 => 2.0,
                ServerWeaponType::Shotgun if distance < 100.0 => 1.5,
                ServerWeaponType::Rifle if (200.0..=400.0).contains(&distance) => 1.3,
                _ => 1.0,
            };
            threat_score *= weapon_threat_weight;

            if game_mode == fb::GameModeType::CaptureTheFlag
                && enemy.carries_flag_team_id == bot_state.team_id
            {
                threat_score += BOT_FLAG_CHASE_PRIORITY * 0.1;
            }

            let pseudo_label = enemy.carries_flag_team_id == bot_state.team_id
                || dist_sq < (220.0f32).powi(2)
                || threat_score > 0.55;
            threat_model.train_online(sample, pseudo_label);

            let direct_position = Vec2::new(enemy.x, enemy.y);
            let predicted_position = predictive_models
                .motion_models
                .get(&enemy.id)
                .and_then(|motion_model| motion_model.predict_position(now_ms.saturating_add(120)))
                .unwrap_or(direct_position);
            let aim_angle =
                (predicted_position.y - bot_state.y).atan2(predicted_position.x - bot_state.x);

            let should_replace = selected
                .as_ref()
                .is_none_or(|(best_score, _)| threat_score > *best_score);
            if should_replace {
                selected = Some((
                    threat_score,
                    TargetSolution {
                        enemy_id: enemy.id.clone(),
                        direct_position,
                        predicted_position,
                        distance_sq: dist_sq,
                        aim_angle,
                    },
                ));
            }
        }

        selected.map(|(_, candidate)| candidate)
    }

    /// Generate enhanced combat input with shooting and movement.
    /// Uses tick-based timing for weapon switch cooldowns and reaction time.
    fn generate_combat_input(
        bot_state: &BotSnapshotOwned,
        bot_controller: &mut BotController,
        server_instance: &MassiveGameServer,
        game_mode: fb::GameModeType,
        enemies: &[EnemySnapshot],
        frame_count: u64,
    ) -> PlayerInputData {
        let can_switch_weapon = frame_count.saturating_sub(bot_controller.last_weapon_switch_tick)
            >= BOT_WEAPON_SWITCH_COOLDOWN_TICKS;

        let mut input = PlayerInputData {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_millis() as u64,
            sequence: bot_state.last_processed_input_sequence.wrapping_add(1),
            move_forward: false,
            move_backward: false,
            move_left: false,
            move_right: false,
            shooting: false,
            reload: false,
            rotation: bot_state.rotation,
            melee_attack: false,
            change_weapon_slot: 0,
            use_ability_slot: 0,
            ping_x: 0.0,
            ping_y: 0.0,
        };
        let personality = bot_controller.personality;
        let difficulty = BotDifficultyTier::from_bot_id(&bot_state.id);

        // Reload if low on ammo
        if bot_state.ammo == 0 {
            input.reload = true;
        }

        // Predictive target selection based on learned threat scores.
        let selected_target = Self::select_enemy_target(
            bot_state,
            enemies,
            game_mode,
            server_instance.get_server_timestamp_ms(),
        );
        let mut nearest_enemy_dist = f32::MAX;
        let mut nearest_enemy_angle = bot_state.rotation;
        let mut has_enemy_target = false;

        if let Some(target) = selected_target.as_ref() {
            nearest_enemy_dist = target.distance_sq;
            nearest_enemy_angle = target.aim_angle;
            let enemy_position = target.direct_position;
            let bot_pos = Vec2::new(bot_state.x, bot_state.y);
            has_enemy_target = Self::has_line_of_sight(bot_pos, enemy_position, server_instance);
        }

        // Movement towards objective using A* waypoint following
        let mut movement_handled = false;

        if let Some(target_pos) = bot_controller.target_position {
            let bot_pos = Vec2::new(bot_state.x, bot_state.y);

            // Check line-of-sight to target: if clear, move directly
            let los_to_target = Self::has_line_of_sight(bot_pos, target_pos, server_instance);

            let nav_target = if los_to_target {
                // Direct path is clear -- skip A* overhead
                target_pos
            } else {
                // Use A* to compute / follow waypoints around obstacles
                Self::ensure_path(
                    bot_controller,
                    bot_pos,
                    target_pos,
                    frame_count,
                    server_instance,
                );
                Self::next_waypoint(bot_controller, bot_pos)
            };

            let dx = nav_target.x - bot_state.x;
            let dy = nav_target.y - bot_state.y;
            let dist_sq = dx * dx + dy * dy;

            // Always set rotation towards next waypoint
            let target_angle = dy.atan2(dx);
            input.rotation = target_angle;

            // Move if not at target
            if dist_sq > BOT_MOVEMENT_TOLERANCE * BOT_MOVEMENT_TOLERANCE {
                // Always move forward when we have a target
                input.move_forward = true;
                movement_handled = true;

                // Add some zigzag movement occasionally
                if with_bot_rng(|rng| rng.gen_bool(0.1)) {
                    if with_bot_rng(|rng| rng.gen_bool(0.5)) {
                        input.move_left = true;
                    } else {
                        input.move_right = true;
                    }
                }

                // If carrying flag, sprint more
                if bot_state.is_carrying_flag_team_id != 0 {
                    input.move_forward = true;
                    // Less zigzag when carrying flag
                    if with_bot_rng(|rng| rng.gen_bool(0.05)) {
                        input.move_left = with_bot_rng(|rng| rng.gen_bool(0.5));
                        input.move_right = !input.move_left;
                    }
                }

                trace!(
                    "Bot {} following waypoint at ({:.0}, {:.0}), distance: {:.0}",
                    bot_state.username,
                    nav_target.x,
                    nav_target.y,
                    dist_sq.sqrt()
                );
            } else {
                // At objective - defensive behavior
                match bot_controller.behavior_state {
                    BotBehaviorState::Defending => {
                        // Look around while defending
                        if with_bot_rng(|rng| rng.gen_bool(0.02)) {
                            input.rotation += with_bot_rng(|rng| rng.gen_range_f32(-1.5, 1.5));
                        }
                        // Small movements to avoid being static
                        if with_bot_rng(|rng| rng.gen_bool(0.1)) {
                            with_bot_rng(|rng| {
                                input.move_forward = rng.gen_bool(0.3);
                                input.move_backward = rng.gen_bool(0.3);
                                input.move_left = rng.gen_bool(0.3);
                                input.move_right = rng.gen_bool(0.3);
                            });
                        }
                    }
                    _ => {
                        // Patrol movement
                        if with_bot_rng(|rng| rng.gen_bool(0.05)) {
                            with_bot_rng(|rng| {
                                input.move_forward = rng.gen_bool(0.5);
                                input.move_left = rng.gen_bool(0.5);
                                input.move_right = !input.move_left && rng.gen_bool(0.5);
                            });
                        }
                    }
                }
            }
        } else {
            // No target - wander randomly
            if with_bot_rng(|rng| rng.gen_bool(0.1)) {
                with_bot_rng(|rng| {
                    input.move_forward = rng.gen_bool(0.7);
                    input.move_backward = !input.move_forward && rng.gen_bool(0.3);
                    input.move_left = rng.gen_bool(0.3);
                    input.move_right = !input.move_left && rng.gen_bool(0.3);
                    input.rotation += rng.gen_range_f32(-0.5, 0.5);
                });
            }
        }

        // Combat behavior - only override rotation if we have a nearby enemy
        if has_enemy_target && nearest_enemy_dist < BOT_TARGET_ACQUISITION_RANGE.powi(2) {
            let enemy_dist_linear = nearest_enemy_dist.sqrt();

            // Personality-aware weapon switching when engaging
            if can_switch_weapon {
                if let Some(preferred_slot) =
                    personality.preferred_weapon_slot(bot_state.weapon, enemy_dist_linear)
                {
                    input.change_weapon_slot = preferred_slot;
                }
            }

            // Aim at enemy with some inaccuracy
            let accuracy_bias = match personality {
                BotPersonality::Aggressive => -0.05,
                BotPersonality::Defensive => 0.04,
                BotPersonality::Balanced => 0.0,
            };
            let effective_accuracy = (difficulty.aim_accuracy() + accuracy_bias).clamp(0.35, 0.95);
            let aim_offset =
                with_bot_rng(|rng| rng.gen_range_f32(-0.2, 0.2)) * (1.0 - effective_accuracy);
            input.rotation = nearest_enemy_angle + aim_offset;
            if let Some(target) = selected_target.as_ref() {
                // Tighten movement vector around predicted enemy motion when engaging.
                let predicted = target.predicted_position;
                let predict_angle = (predicted.y - bot_state.y).atan2(predicted.x - bot_state.x);
                let blend_x = input.rotation.cos() + predict_angle.cos();
                let blend_y = input.rotation.sin() + predict_angle.sin();
                if blend_x != 0.0 || blend_y != 0.0 {
                    input.rotation = blend_y.atan2(blend_x);
                }
                trace!(
                    "Bot {} ({:?}) engaging predicted target {}",
                    bot_state.username,
                    personality,
                    target.enemy_id.as_ref()
                );
            }

            // Shoot if close enough and have line of sight
            let shoot_range: f32 = match bot_state.weapon {
                ServerWeaponType::Shotgun => 150.0,
                ServerWeaponType::Sniper => 800.0,
                _ => 400.0,
            };

            if nearest_enemy_dist < shoot_range.powi(2) {
                // Apply reaction time (tick-based)
                let ticks_since_decision =
                    frame_count.saturating_sub(bot_controller.last_decision_tick);
                let reaction_ticks = difficulty.reaction_time_ticks();
                if ticks_since_decision >= reaction_ticks {
                    let shoot_probability = match personality {
                        BotPersonality::Aggressive => difficulty.shoot_probability() + 0.08,
                        BotPersonality::Defensive => difficulty.shoot_probability() - 0.08,
                        BotPersonality::Balanced => difficulty.shoot_probability(),
                    }
                    .clamp(0.30, 0.94);
                    input.shooting = with_bot_rng(|rng| rng.gen_bool(f64::from(shoot_probability)));

                    // Aggressive bots prefer melee at very close range
                    let melee_chance = match personality {
                        BotPersonality::Aggressive => 0.5,
                        BotPersonality::Defensive => 0.1,
                        BotPersonality::Balanced => 0.3,
                    };
                    if nearest_enemy_dist < 60.0 * 60.0
                        && with_bot_rng(|rng| rng.gen_bool(melee_chance))
                    {
                        input.melee_attack = true;
                        input.shooting = false;
                    }
                }
            }

            // Use movement abilities for outplay opportunities.
            if nearest_enemy_dist > 180.0 * 180.0
                && nearest_enemy_dist < 420.0 * 420.0
                && with_bot_rng(|rng| rng.gen_bool(0.06))
            {
                input.use_ability_slot = 1; // Dash engage
            } else if nearest_enemy_dist < 120.0 * 120.0 && with_bot_rng(|rng| rng.gen_bool(0.08)) {
                input.use_ability_slot = 2; // Dodge roll disengage
            }

            // Tactical movement during combat - personality-aware
            if has_enemy_target && !movement_handled {
                let engagement_range = personality.engagement_range();
                let engagement_range_sq = engagement_range * engagement_range;
                let health_pct = bot_state.health.clamp(0, 100) as f32;
                let retreat_now = personality.should_retreat(health_pct);

                match personality {
                    BotPersonality::Aggressive => {
                        // Aggressive: rush towards enemy, minimal retreat
                        if nearest_enemy_dist > engagement_range_sq {
                            input.move_forward = true; // Close the gap
                        } else {
                            // Strafe aggressively at close range
                            if with_bot_rng(|rng| rng.gen_bool(0.7)) {
                                input.move_left = with_bot_rng(|rng| rng.gen_bool(0.5));
                                input.move_right = !input.move_left;
                            }
                            // Aggressive bots chase fleeing enemies
                            input.move_forward = true;
                        }
                    }
                    BotPersonality::Defensive => {
                        // Defensive: maintain distance, retreat when too close
                        if retreat_now || nearest_enemy_dist < engagement_range_sq {
                            // Too close - retreat
                            input.move_backward = true;
                            input.move_forward = false;
                            // Strafe while retreating
                            if with_bot_rng(|rng| rng.gen_bool(0.4)) {
                                input.move_left = with_bot_rng(|rng| rng.gen_bool(0.5));
                                input.move_right = !input.move_left;
                            }
                        } else {
                            // Hold position, strafe to avoid being hit
                            if with_bot_rng(|rng| rng.gen_bool(0.5)) {
                                input.move_left = with_bot_rng(|rng| rng.gen_bool(0.5));
                                input.move_right = !input.move_left;
                            }
                        }
                    }
                    BotPersonality::Balanced => {
                        // Default balanced behavior
                        if retreat_now {
                            input.move_backward = true;
                            input.move_forward = false;
                            if with_bot_rng(|rng| rng.gen_bool(0.6)) {
                                input.move_left = with_bot_rng(|rng| rng.gen_bool(0.5));
                                input.move_right = !input.move_left;
                            }
                        } else if nearest_enemy_dist < 200.0 * 200.0 {
                            // Strafe at close range
                            if with_bot_rng(|rng| rng.gen_bool(0.6)) {
                                input.move_left = with_bot_rng(|rng| rng.gen_bool(0.5));
                                input.move_right = !input.move_left;
                            }
                            // Sometimes retreat
                            if nearest_enemy_dist < 100.0 * 100.0
                                && with_bot_rng(|rng| rng.gen_bool(0.3))
                            {
                                input.move_backward = true;
                                input.move_forward = false;
                            }
                        } else {
                            // Move towards enemy if not too close
                            if bot_controller.target_position.is_none() {
                                input.move_forward = true;
                            }
                        }
                    }
                }
            }
        }

        if input.change_weapon_slot != 0 {
            bot_controller.last_weapon_switch_tick = frame_count;
        }

        input
    }

    /// Check if bot is stuck and needs to change direction
    fn check_stuck_status(
        bot_controller: &mut BotController,
        bot_state: &BotSnapshotOwned,
        delta_time: f32,
    ) {
        let current_pos = Vec2::new(bot_state.x, bot_state.y);
        if bot_controller.behavior_state == BotBehaviorState::Defending && bot_state.team_id != 0 {
            let own_flag_base = MassiveGameServer::get_flag_base_position(bot_state.team_id);
            let dist_to_base_sq = (current_pos.x - own_flag_base.x).powi(2)
                + (current_pos.y - own_flag_base.y).powi(2);
            if dist_to_base_sq <= 220.0 * 220.0 {
                bot_controller.stuck_timer = 0.0;
                bot_controller.stuck_check_position = current_pos;
                bot_controller.last_position = current_pos;
                return;
            }
        }

        // Defenders and objective bots can be intentionally stationary once they reach their post.
        if let Some(target) = bot_controller.target_position {
            let dist_to_target_sq =
                (current_pos.x - target.x).powi(2) + (current_pos.y - target.y).powi(2);
            if dist_to_target_sq <= BOT_STUCK_TARGET_TOLERANCE.powi(2) {
                bot_controller.stuck_timer = 0.0;
                bot_controller.stuck_check_position = current_pos;
                bot_controller.last_position = current_pos;
                return;
            }
        } else if matches!(
            bot_controller.behavior_state,
            BotBehaviorState::Defending | BotBehaviorState::Patrolling
        ) {
            bot_controller.stuck_timer = 0.0;
            bot_controller.stuck_check_position = current_pos;
            bot_controller.last_position = current_pos;
            return;
        }

        // Update stuck timer
        bot_controller.stuck_timer += delta_time;

        // Check position every BOT_STUCK_CHECK_INTERVAL seconds
        if bot_controller.stuck_timer >= BOT_STUCK_CHECK_INTERVAL {
            let dx = current_pos.x - bot_controller.stuck_check_position.x;
            let dy = current_pos.y - bot_controller.stuck_check_position.y;
            let distance_moved = (dx * dx + dy * dy).sqrt();

            // Check if bot has moved enough
            if distance_moved < BOT_STUCK_THRESHOLD {
                // Bot is potentially stuck
                if bot_controller.stuck_timer >= BOT_STUCK_TIME_THRESHOLD {
                    // Bot is definitely stuck - invalidate current path and
                    // pick a new escape target so A* can try a different route.
                    debug!(
                        "Bot {} is stuck at ({:.0}, {:.0}), invalidating path and picking escape target",
                        bot_state.username, current_pos.x, current_pos.y
                    );

                    // Invalidate the current A* path so it will be recomputed
                    Self::invalidate_path(bot_controller);

                    // Pick a random nearby escape target
                    let (escape_angle, escape_distance) = with_bot_rng(|rng| {
                        (
                            rng.gen_range_f32(0.0, 2.0 * std::f32::consts::PI),
                            rng.gen_range_f32(100.0, 300.0),
                        )
                    });

                    let new_x = (current_pos.x + escape_distance * escape_angle.cos())
                        .clamp(WORLD_MIN_X + 100.0, WORLD_MAX_X - 100.0);
                    let new_y = (current_pos.y + escape_distance * escape_angle.sin())
                        .clamp(WORLD_MIN_Y + 100.0, WORLD_MAX_Y - 100.0);

                    bot_controller.target_position = Some(Vec2::new(new_x, new_y));
                    bot_controller.behavior_state = BotBehaviorState::MovingToPosition;

                    // Reset stuck detection
                    bot_controller.stuck_timer = 0.0;
                    bot_controller.stuck_check_position = current_pos;
                    bot_controller.last_position = current_pos;

                    // Force a new decision soon (set tick so only ~0.5s worth
                    // of interval remains before next decision).
                    bot_controller.last_decision_tick = bot_controller
                        .last_decision_tick
                        .saturating_sub(BOT_DECISION_INTERVAL_TICKS / 2);

                    debug!(
                        "Bot {} unstuck - new target: ({:.0}, {:.0})",
                        bot_state.username, new_x, new_y
                    );
                }
            } else {
                // Bot has moved, reset stuck detection
                bot_controller.stuck_timer = 0.0;
                bot_controller.stuck_check_position = current_pos;
            }
        }

        // Always update last position
        bot_controller.last_position = current_pos;
    }

    // ── A* pathfinding helpers ───────────────────────────────────────

    /// Get or lazily build the shared navigation grid.  The grid is cached and
    /// only rebuilt when the wall spatial index version changes (i.e. walls are
    /// destroyed / created).
    fn get_or_build_nav_grid(server_instance: &MassiveGameServer) -> Option<Arc<GridNav>> {
        let wall_index_frame = server_instance.wall_spatial_index.last_update_frame();
        {
            let cache = bot_nav_grid_cache().read();
            if cache.wall_index_frame == wall_index_frame {
                if let Some(grid) = cache.grid.as_ref() {
                    return Some(Arc::clone(grid));
                }
            }
        }

        let active_walls_by_id = Self::collect_active_walls_by_id(server_instance);
        let mut cache = bot_nav_grid_cache().write();
        if cache.wall_index_frame == wall_index_frame {
            if let Some(grid) = cache.grid.as_ref() {
                return Some(Arc::clone(grid));
            }
        }
        let rebuilt = match cache.grid.as_ref() {
            Some(existing_grid) => Self::update_nav_grid_incremental(
                existing_grid,
                &cache.active_walls_by_id,
                &active_walls_by_id,
            ),
            None => Self::build_nav_grid_from_walls(active_walls_by_id.values()),
        };
        cache.wall_index_frame = wall_index_frame;
        cache.active_walls_by_id = active_walls_by_id;
        cache.grid = rebuilt.clone();
        rebuilt
    }

    fn build_nav_grid_from_walls<'a>(
        walls: impl IntoIterator<Item = &'a Wall>,
    ) -> Option<Arc<GridNav>> {
        if BOT_NAV_GRID_CELL_SIZE <= 0.0 {
            return None;
        }

        let world_width = (WORLD_MAX_X - WORLD_MIN_X).max(BOT_NAV_GRID_CELL_SIZE);
        let world_height = (WORLD_MAX_Y - WORLD_MIN_Y).max(BOT_NAV_GRID_CELL_SIZE);
        let grid_width = (world_width / BOT_NAV_GRID_CELL_SIZE).ceil() as i32;
        let grid_height = (world_height / BOT_NAV_GRID_CELL_SIZE).ceil() as i32;
        let mut nav_grid = GridNav::with_origin(
            grid_width.max(1),
            grid_height.max(1),
            BOT_NAV_GRID_CELL_SIZE,
            WORLD_MIN_X,
            WORLD_MIN_Y,
        );

        for wall in walls {
            Self::mark_wall_cells_blocked(&mut nav_grid, wall);
        }
        Some(Arc::new(nav_grid))
    }

    fn collect_active_walls_by_id(server_instance: &MassiveGameServer) -> HashMap<EntityId, Wall> {
        let partitions = server_instance
            .world_partition_manager
            .get_partitions_for_processing();
        let mut active_walls_by_id = HashMap::new();
        for partition in partitions {
            for wall_entry in partition.all_walls_in_partition.iter() {
                let wall = wall_entry.value();
                if wall.is_destructible && wall.current_health <= 0 {
                    continue;
                }
                active_walls_by_id
                    .entry(wall.id)
                    .or_insert_with(|| wall.clone());
            }
        }
        active_walls_by_id
    }

    fn update_nav_grid_incremental(
        existing_grid: &Arc<GridNav>,
        previous_walls: &HashMap<EntityId, Wall>,
        current_walls: &HashMap<EntityId, Wall>,
    ) -> Option<Arc<GridNav>> {
        let mut nav_grid = (**existing_grid).clone();
        let mut changed = false;

        for (wall_id, previous_wall) in previous_walls {
            match current_walls.get(wall_id) {
                Some(current_wall)
                    if Self::wall_geometry_is_unchanged(previous_wall, current_wall) => {}
                _ => {
                    Self::mark_wall_cells_unblocked(&mut nav_grid, previous_wall);
                    changed = true;
                }
            }
        }

        for (wall_id, current_wall) in current_walls {
            match previous_walls.get(wall_id) {
                Some(previous_wall)
                    if Self::wall_geometry_is_unchanged(previous_wall, current_wall) => {}
                _ => {
                    Self::mark_wall_cells_blocked(&mut nav_grid, current_wall);
                    changed = true;
                }
            }
        }

        if changed {
            Some(Arc::new(nav_grid))
        } else {
            Some(Arc::clone(existing_grid))
        }
    }

    fn wall_geometry_is_unchanged(previous: &Wall, current: &Wall) -> bool {
        previous.x.to_bits() == current.x.to_bits()
            && previous.y.to_bits() == current.y.to_bits()
            && previous.width.to_bits() == current.width.to_bits()
            && previous.height.to_bits() == current.height.to_bits()
    }

    /// Mark grid cells covered by a wall (inflated by PLAYER_RADIUS) as blocked.
    fn mark_wall_cells_blocked(nav_grid: &mut GridNav, wall: &Wall) {
        let Some((min_cell_x, min_cell_y, max_cell_x, max_cell_y)) =
            Self::wall_cell_bounds(nav_grid, wall)
        else {
            return;
        };

        for gy in min_cell_y..=max_cell_y {
            for gx in min_cell_x..=max_cell_x {
                nav_grid.add_blocker(gx, gy);
            }
        }
    }

    fn mark_wall_cells_unblocked(nav_grid: &mut GridNav, wall: &Wall) {
        let Some((min_cell_x, min_cell_y, max_cell_x, max_cell_y)) =
            Self::wall_cell_bounds(nav_grid, wall)
        else {
            return;
        };

        for gy in min_cell_y..=max_cell_y {
            for gx in min_cell_x..=max_cell_x {
                nav_grid.remove_blocker(gx, gy);
            }
        }
    }

    fn wall_cell_bounds(nav_grid: &GridNav, wall: &Wall) -> Option<(i32, i32, i32, i32)> {
        let max_world_x = WORLD_MAX_X - f32::EPSILON;
        let max_world_y = WORLD_MAX_Y - f32::EPSILON;
        let inflated_min_x = wall.x - PLAYER_RADIUS;
        let inflated_min_y = wall.y - PLAYER_RADIUS;
        let inflated_max_x = wall.x + wall.width + PLAYER_RADIUS;
        let inflated_max_y = wall.y + wall.height + PLAYER_RADIUS;

        let Some((min_cell_x, min_cell_y)) = nav_grid.world_to_grid(
            inflated_min_x.clamp(WORLD_MIN_X, WORLD_MAX_X),
            inflated_min_y.clamp(WORLD_MIN_Y, WORLD_MAX_Y),
        ) else {
            return None;
        };
        let Some((max_cell_x, max_cell_y)) = nav_grid.world_to_grid(
            inflated_max_x.clamp(WORLD_MIN_X, max_world_x),
            inflated_max_y.clamp(WORLD_MIN_Y, max_world_y),
        ) else {
            return None;
        };
        Some((
            min_cell_x.min(max_cell_x),
            min_cell_y.min(max_cell_y),
            min_cell_x.max(max_cell_x),
            min_cell_y.max(max_cell_y),
        ))
    }

    /// Compute (or reuse) an A* path from the bot's position to its target.
    /// Populates `bot_controller.current_path` with a down-sampled waypoint list.
    /// Returns `true` if a valid path was found (or already exists).
    fn ensure_path(
        bot_controller: &mut BotController,
        bot_pos: Vec2,
        target: Vec2,
        frame_count: u64,
        server_instance: &MassiveGameServer,
    ) -> bool {
        // Check if we need to recompute
        let ticks_since_compute = frame_count.saturating_sub(bot_controller.path_compute_tick);
        let target_moved = bot_controller
            .last_path_target
            .map(|old| {
                let dx = target.x - old.x;
                let dy = target.y - old.y;
                dx * dx + dy * dy > BOT_PATH_TARGET_MOVED_THRESHOLD_SQ
            })
            .unwrap_or(true);

        let needs_recompute = bot_controller.current_path.is_empty()
            || ticks_since_compute >= BOT_PATH_RECOMPUTE_INTERVAL_TICKS
            || target_moved;

        if !needs_recompute {
            return true;
        }

        // Try A* pathfinding via the cached GridNav
        if let Some(nav_grid) = Self::get_or_build_nav_grid(server_instance) {
            if let Some(grid_path) = nav_grid.find_path_world(bot_pos, target) {
                let path_len = grid_path.len();
                bot_controller.current_path.clear();

                if path_len <= 1 {
                    bot_controller.current_path.push_back(target);
                } else {
                    // Down-sample long paths to BOT_PATH_MAX_WAYPOINTS
                    let stride = if path_len > BOT_PATH_MAX_WAYPOINTS {
                        (path_len / BOT_PATH_MAX_WAYPOINTS).max(1)
                    } else {
                        1
                    };

                    for (relative_idx, waypoint) in grid_path.iter().skip(1).enumerate() {
                        let absolute_idx = relative_idx + 1;
                        let is_last = absolute_idx + 1 == path_len;
                        if is_last || absolute_idx % stride == 0 {
                            bot_controller.current_path.push_back(*waypoint);
                        }
                    }

                    // Ensure the final waypoint is exactly the goal
                    if let Some(last) = bot_controller.current_path.back_mut() {
                        *last = target;
                    } else {
                        bot_controller.current_path.push_back(target);
                    }
                }

                bot_controller.path_compute_tick = frame_count;
                bot_controller.last_path_target = Some(target);
                trace!(
                    "A* path computed: {} waypoints from ({:.0},{:.0}) to ({:.0},{:.0})",
                    bot_controller.current_path.len(),
                    bot_pos.x,
                    bot_pos.y,
                    target.x,
                    target.y,
                );
                return true;
            }
        }

        // A* failed (no grid or no path) -- fall back to direct movement
        bot_controller.current_path.clear();
        bot_controller.current_path.push_back(target);
        bot_controller.path_compute_tick = frame_count;
        bot_controller.last_path_target = Some(target);
        false
    }

    /// Advance through the waypoint list and return the next position the bot
    /// should steer towards.  Pops completed waypoints along the way.
    fn next_waypoint(bot_controller: &mut BotController, bot_pos: Vec2) -> Vec2 {
        // Pop any waypoints we've already reached
        while bot_controller.current_path.len() > 1 {
            if let Some(wp) = bot_controller.current_path.front() {
                let dx = wp.x - bot_pos.x;
                let dy = wp.y - bot_pos.y;
                if dx * dx + dy * dy <= BOT_WAYPOINT_ARRIVAL_DIST * BOT_WAYPOINT_ARRIVAL_DIST {
                    bot_controller.current_path.pop_front();
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        // Return the current waypoint, or fall back to bot's own position (no movement)
        bot_controller
            .current_path
            .front()
            .copied()
            .unwrap_or(bot_pos)
    }

    /// Invalidate the current path so it will be recomputed on the next tick.
    fn invalidate_path(bot_controller: &mut BotController) {
        bot_controller.current_path.clear();
        bot_controller.last_path_target = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── BotPersonality engagement_range tests ────────────────────

    #[test]
    fn aggressive_engagement_range() {
        assert!((BotPersonality::Aggressive.engagement_range() - 150.0).abs() < f32::EPSILON);
    }

    #[test]
    fn defensive_engagement_range() {
        assert!((BotPersonality::Defensive.engagement_range() - 400.0).abs() < f32::EPSILON);
    }

    #[test]
    fn balanced_engagement_range() {
        assert!((BotPersonality::Balanced.engagement_range() - 300.0).abs() < f32::EPSILON);
    }

    // ── BotPersonality weapon preference tests ──────────────────

    #[test]
    fn aggressive_prefers_melee_at_very_close() {
        let slot = BotPersonality::Aggressive.preferred_weapon_slot(ServerWeaponType::Rifle, 30.0);
        assert_eq!(slot, Some(2)); // switch to melee
    }

    #[test]
    fn aggressive_prefers_shotgun_at_close() {
        let slot = BotPersonality::Aggressive.preferred_weapon_slot(ServerWeaponType::Rifle, 120.0);
        assert_eq!(slot, Some(1)); // switch to shotgun
    }

    #[test]
    fn aggressive_keeps_weapon_at_long_range() {
        let slot = BotPersonality::Aggressive.preferred_weapon_slot(ServerWeaponType::Rifle, 300.0);
        assert_eq!(slot, None);
    }

    #[test]
    fn defensive_prefers_sniper_at_long_range() {
        let slot = BotPersonality::Defensive.preferred_weapon_slot(ServerWeaponType::Rifle, 500.0);
        assert_eq!(slot, Some(2)); // switch to sniper
    }

    #[test]
    fn defensive_prefers_rifle_at_medium_range() {
        let slot =
            BotPersonality::Defensive.preferred_weapon_slot(ServerWeaponType::Shotgun, 250.0);
        assert_eq!(slot, Some(1)); // switch to rifle
    }

    #[test]
    fn defensive_keeps_weapon_at_close_range() {
        let slot =
            BotPersonality::Defensive.preferred_weapon_slot(ServerWeaponType::Shotgun, 100.0);
        assert_eq!(slot, None);
    }

    #[test]
    fn balanced_prefers_shotgun_at_close() {
        let slot = BotPersonality::Balanced.preferred_weapon_slot(ServerWeaponType::Rifle, 80.0);
        assert_eq!(slot, Some(1)); // switch to shotgun
    }

    #[test]
    fn balanced_prefers_sniper_at_long_range() {
        let slot = BotPersonality::Balanced.preferred_weapon_slot(ServerWeaponType::Rifle, 400.0);
        assert_eq!(slot, Some(2)); // switch to sniper
    }

    #[test]
    fn balanced_keeps_weapon_at_medium() {
        let slot = BotPersonality::Balanced.preferred_weapon_slot(ServerWeaponType::Rifle, 200.0);
        assert_eq!(slot, None);
    }

    // ── BotPersonality should_retreat tests ──────────────────────

    #[test]
    fn aggressive_never_retreats() {
        assert!(!BotPersonality::Aggressive.should_retreat(10.0));
        assert!(!BotPersonality::Aggressive.should_retreat(1.0));
    }

    #[test]
    fn defensive_retreats_below_50_pct() {
        assert!(BotPersonality::Defensive.should_retreat(49.0));
        assert!(!BotPersonality::Defensive.should_retreat(50.0));
        assert!(!BotPersonality::Defensive.should_retreat(80.0));
    }

    #[test]
    fn balanced_retreats_below_25_pct() {
        assert!(BotPersonality::Balanced.should_retreat(24.0));
        assert!(!BotPersonality::Balanced.should_retreat(25.0));
        assert!(!BotPersonality::Balanced.should_retreat(60.0));
    }

    // ── BotPersonality random covers all variants ───────────────

    #[test]
    fn random_personality_produces_valid_variant() {
        // Run a handful of times to ensure no panic / invalid state
        for _ in 0..20 {
            let p = BotPersonality::random();
            // Just ensure the engagement range is one of the valid values
            let r = p.engagement_range();
            assert!(r == 150.0 || r == 300.0 || r == 400.0);
        }
    }

    // ── Difficulty tier tests ────────────────────────────────────

    #[test]
    fn difficulty_from_bot_id_is_deterministic() {
        let bot_id: PlayerID = std::sync::Arc::from("bot-difficulty-test".to_string());
        let a = BotDifficultyTier::from_bot_id(&bot_id);
        let b = BotDifficultyTier::from_bot_id(&bot_id);
        assert_eq!(a, b);
    }

    #[test]
    fn difficulty_accuracy_is_ordered() {
        assert!(BotDifficultyTier::Easy.aim_accuracy() < BotDifficultyTier::Normal.aim_accuracy());
        assert!(BotDifficultyTier::Normal.aim_accuracy() < BotDifficultyTier::Hard.aim_accuracy());
    }

    fn make_bot_snapshot_for_pickups(
        health: i32,
        ammo: i32,
        weapon: ServerWeaponType,
    ) -> BotSnapshotOwned {
        BotSnapshotOwned {
            id: std::sync::Arc::from("bot-pickup".to_string()),
            username: "Bot".to_string(),
            health,
            x: 0.0,
            y: 0.0,
            velocity_x: 0.0,
            velocity_y: 0.0,
            rotation: 0.0,
            ammo,
            weapon,
            team_id: 1,
            is_carrying_flag_team_id: 0,
            last_processed_input_sequence: 1,
        }
    }

    #[test]
    fn pickup_priority_prefers_health_when_critical() {
        let bot = make_bot_snapshot_for_pickups(20, 10, ServerWeaponType::Rifle);
        let health = OptimizedBotAI::pickup_priority(&bot, &CorePickupType::Health).unwrap_or(0.0);
        let ammo = OptimizedBotAI::pickup_priority(&bot, &CorePickupType::Ammo).unwrap_or(0.0);
        assert!(health > ammo);
    }

    #[test]
    fn pickup_priority_ignores_ammo_when_full() {
        let max_ammo = PlayerState::get_max_ammo_for_weapon(ServerWeaponType::Rifle);
        let bot = make_bot_snapshot_for_pickups(90, max_ammo, ServerWeaponType::Rifle);
        assert!(OptimizedBotAI::pickup_priority(&bot, &CorePickupType::Ammo).is_none());
    }

    // ── Stuck detection constants are consistent ────────────────

    #[test]
    fn stuck_target_tolerance_greater_than_movement_tolerance() {
        const {
            assert!(BOT_STUCK_TARGET_TOLERANCE > BOT_MOVEMENT_TOLERANCE);
        }
        assert!((BOT_STUCK_TARGET_TOLERANCE - BOT_MOVEMENT_TOLERANCE - 20.0).abs() < f32::EPSILON);
    }

    #[test]
    fn stuck_time_threshold_is_multiple_of_check_interval() {
        // 2.0 / 0.5 = 4 checks before stuck is triggered
        assert!((BOT_STUCK_TIME_THRESHOLD / BOT_STUCK_CHECK_INTERVAL - 4.0).abs() < f32::EPSILON);
    }

    // ── AI LOD tier classification tests ─────────────────────────

    #[test]
    fn lod_near_within_aoi() {
        // 100 units away: well within AoI (520u)
        let dist_sq = 100.0f32 * 100.0;
        assert_eq!(BotAiLodTier::classify(dist_sq), BotAiLodTier::Near);
    }

    #[test]
    fn lod_near_at_aoi_boundary() {
        // Exactly at AoI boundary (520u)
        let dist_sq = BOT_LOD_NEAR_DISTANCE * BOT_LOD_NEAR_DISTANCE;
        assert_eq!(BotAiLodTier::classify(dist_sq), BotAiLodTier::Near);
    }

    #[test]
    fn lod_medium_just_beyond_aoi() {
        // Just beyond AoI (521u) -> Medium tier
        let dist_sq = 521.0f32 * 521.0;
        assert_eq!(BotAiLodTier::classify(dist_sq), BotAiLodTier::Medium);
    }

    #[test]
    fn lod_medium_at_medium_boundary() {
        // Exactly at medium boundary (1500u) -> still Medium
        let dist_sq = BOT_LOD_MEDIUM_DISTANCE * BOT_LOD_MEDIUM_DISTANCE;
        assert_eq!(BotAiLodTier::classify(dist_sq), BotAiLodTier::Medium);
    }

    #[test]
    fn lod_far_beyond_medium() {
        // 1501 units -> Far tier
        let dist_sq = 1501.0f32 * 1501.0;
        assert_eq!(BotAiLodTier::classify(dist_sq), BotAiLodTier::Far);
    }

    #[test]
    fn lod_far_very_distant() {
        let dist_sq = 5000.0f32 * 5000.0;
        assert_eq!(BotAiLodTier::classify(dist_sq), BotAiLodTier::Far);
    }

    #[test]
    fn lod_near_zero_distance() {
        // Bot co-located with human
        assert_eq!(BotAiLodTier::classify(0.0), BotAiLodTier::Near);
    }

    #[test]
    fn lod_presence_near_when_no_humans_exist() {
        assert_eq!(
            BotAiLodTier::classify_from_human_presence(false, false, false),
            BotAiLodTier::Near
        );
    }

    #[test]
    fn lod_presence_near_when_human_is_within_aoi() {
        assert_eq!(
            BotAiLodTier::classify_from_human_presence(true, true, true),
            BotAiLodTier::Near
        );
    }

    #[test]
    fn lod_presence_medium_when_only_medium_human_exists() {
        assert_eq!(
            BotAiLodTier::classify_from_human_presence(true, false, true),
            BotAiLodTier::Medium
        );
    }

    #[test]
    fn lod_presence_far_when_all_humans_are_distant() {
        assert_eq!(
            BotAiLodTier::classify_from_human_presence(true, false, false),
            BotAiLodTier::Far
        );
    }

    // ── LOD should_process tick skipping tests ──────────────────

    #[test]
    fn lod_near_processes_every_tick() {
        for frame in 0..16 {
            assert!(
                BotAiLodTier::Near.should_process(frame),
                "Near tier should process frame {}",
                frame
            );
        }
    }

    #[test]
    fn lod_medium_processes_every_4th_tick() {
        let processed: Vec<u64> = (0..16)
            .filter(|f| BotAiLodTier::Medium.should_process(*f))
            .collect();
        assert_eq!(processed, vec![0, 4, 8, 12]);
    }

    #[test]
    fn lod_far_processes_every_8th_tick() {
        let processed: Vec<u64> = (0..24)
            .filter(|f| BotAiLodTier::Far.should_process(*f))
            .collect();
        assert_eq!(processed, vec![0, 8, 16]);
    }

    // ── Tick-based timing constants sanity checks ────────────────

    #[test]
    fn decision_interval_ticks_matches_half_second() {
        // 30 ticks at 60Hz = 0.5s
        assert_eq!(BOT_DECISION_INTERVAL_TICKS, 30);
    }

    #[test]
    fn reaction_time_ticks_matches_100ms() {
        // 6 ticks at 60Hz = 100ms
        assert_eq!(BOT_REACTION_TIME_TICKS, 6);
    }

    #[test]
    fn weapon_switch_cooldown_ticks_matches_1s() {
        // 60 ticks at 60Hz = 1.0s
        assert_eq!(BOT_WEAPON_SWITCH_COOLDOWN_TICKS, 60);
    }

    #[test]
    fn predictive_cleanup_runs_on_interval() {
        let predictive_models = RuntimePredictiveModels::default();
        assert!(should_cleanup_predictive_models(
            BOT_PREDICTIVE_MODEL_CLEANUP_INTERVAL_TICKS,
            &predictive_models
        ));
        assert!(!should_cleanup_predictive_models(1, &predictive_models));
    }

    #[test]
    fn predictive_cleanup_runs_when_capacity_is_exceeded() {
        let predictive_models = RuntimePredictiveModels::default();
        for idx in 0..=BOT_PREDICTIVE_MODEL_MAX_ENTRIES {
            let player_id: PlayerID = Arc::from(format!("player-{idx}"));
            predictive_models
                .motion_models
                .insert(player_id.clone(), PredictiveMotionModel::default());
        }

        assert!(should_cleanup_predictive_models(1, &predictive_models));
    }

    fn make_test_wall(id: EntityId, x: f32, y: f32, width: f32, height: f32) -> Wall {
        Wall {
            id,
            x,
            y,
            width,
            height,
            is_destructible: false,
            current_health: 100,
            max_health: 100,
        }
    }

    #[test]
    fn incremental_nav_updates_preserve_overlapping_walls() {
        let wall_a = make_test_wall(1, 0.0, 0.0, 20.0, 20.0);
        let wall_b = make_test_wall(2, 0.0, 0.0, 20.0, 20.0);
        let existing_grid = OptimizedBotAI::build_nav_grid_from_walls([&wall_a, &wall_b])
            .expect("grid should build");

        let previous_walls =
            HashMap::from([(wall_a.id, wall_a.clone()), (wall_b.id, wall_b.clone())]);
        let current_walls = HashMap::from([(wall_b.id, wall_b.clone())]);

        let updated_grid = OptimizedBotAI::update_nav_grid_incremental(
            &existing_grid,
            &previous_walls,
            &current_walls,
        )
        .expect("grid should update");

        assert!(updated_grid
            .find_path_world(Vec2::new(10.0, 10.0), Vec2::new(80.0, 80.0))
            .is_none());
    }

    #[test]
    fn incremental_nav_updates_skip_rebuild_when_only_health_changes() {
        let wall_a = make_test_wall(1, 0.0, 0.0, 20.0, 20.0);
        let existing_grid =
            OptimizedBotAI::build_nav_grid_from_walls([&wall_a]).expect("grid should build");

        let mut damaged_wall = wall_a.clone();
        damaged_wall.current_health = 80;

        let previous_walls = HashMap::from([(wall_a.id, wall_a.clone())]);
        let current_walls = HashMap::from([(damaged_wall.id, damaged_wall)]);

        let updated_grid = OptimizedBotAI::update_nav_grid_incremental(
            &existing_grid,
            &previous_walls,
            &current_walls,
        )
        .expect("grid should update");

        assert!(Arc::ptr_eq(&existing_grid, &updated_grid));
    }

    // ── A* pathfinding / waypoint-following tests ─────────────────

    use std::collections::VecDeque;

    /// Helper: create a minimal BotController at a given position with an
    /// optional pre-loaded path.
    fn make_test_bot_controller(pos: Vec2, path: Vec<Vec2>) -> BotController {
        let mut bc = BotController {
            player_id: std::sync::Arc::from("test-bot".to_string()),
            target_position: None,
            target_enemy_id: None,
            last_decision_time: Instant::now(),
            last_decision_tick: 0,
            ai_update_accumulator_secs: 0.0,
            behavior_state: BotBehaviorState::Idle,
            current_path: VecDeque::from(path),
            path_recalculation_timer: Instant::now(),
            last_weapon_switch_time: Instant::now(),
            last_weapon_switch_tick: 0,
            last_position: pos,
            stuck_timer: 0.0,
            stuck_check_position: pos,
            personality: BotPersonality::Balanced,
            path_compute_tick: 0,
            last_path_target: None,
        };
        bc.target_position = bc.current_path.back().copied();
        bc
    }

    #[test]
    fn next_waypoint_returns_first_when_far_away() {
        let bot_pos = Vec2::new(0.0, 0.0);
        let wp1 = Vec2::new(100.0, 0.0);
        let wp2 = Vec2::new(200.0, 0.0);
        let mut bc = make_test_bot_controller(bot_pos, vec![wp1, wp2]);

        let next = OptimizedBotAI::next_waypoint(&mut bc, bot_pos);
        assert!(
            (next.x - wp1.x).abs() < 1.0 && (next.y - wp1.y).abs() < 1.0,
            "Should return first waypoint when bot is far from it"
        );
        assert_eq!(bc.current_path.len(), 2, "Path should not be consumed yet");
    }

    #[test]
    fn next_waypoint_advances_past_reached_waypoints() {
        let wp1 = Vec2::new(10.0, 0.0);
        let wp2 = Vec2::new(100.0, 0.0);
        let wp3 = Vec2::new(200.0, 0.0);
        let bot_pos = Vec2::new(12.0, 0.0); // very close to wp1
        let mut bc = make_test_bot_controller(bot_pos, vec![wp1, wp2, wp3]);

        let next = OptimizedBotAI::next_waypoint(&mut bc, bot_pos);
        // wp1 should be popped (within BOT_WAYPOINT_ARRIVAL_DIST=30)
        assert!(
            (next.x - wp2.x).abs() < 1.0,
            "Should advance past wp1 to wp2, got ({:.1}, {:.1})",
            next.x,
            next.y
        );
        assert_eq!(bc.current_path.len(), 2, "wp1 should have been popped");
    }

    #[test]
    fn next_waypoint_does_not_pop_last_waypoint() {
        let wp1 = Vec2::new(5.0, 5.0);
        let bot_pos = Vec2::new(5.0, 5.0); // exactly at wp1
        let mut bc = make_test_bot_controller(bot_pos, vec![wp1]);

        let next = OptimizedBotAI::next_waypoint(&mut bc, bot_pos);
        // Should NOT pop the last waypoint, even if we're on top of it
        assert_eq!(bc.current_path.len(), 1, "Last waypoint must not be popped");
        assert!((next.x - wp1.x).abs() < 1.0);
    }

    #[test]
    fn next_waypoint_returns_bot_pos_when_path_empty() {
        let bot_pos = Vec2::new(42.0, 99.0);
        let mut bc = make_test_bot_controller(bot_pos, vec![]);

        let next = OptimizedBotAI::next_waypoint(&mut bc, bot_pos);
        assert!((next.x - bot_pos.x).abs() < f32::EPSILON);
        assert!((next.y - bot_pos.y).abs() < f32::EPSILON);
    }

    #[test]
    fn next_waypoint_pops_multiple_reached_waypoints() {
        let wp1 = Vec2::new(0.0, 0.0);
        let wp2 = Vec2::new(5.0, 0.0);
        let wp3 = Vec2::new(10.0, 0.0);
        let wp4 = Vec2::new(500.0, 0.0);
        let bot_pos = Vec2::new(8.0, 0.0); // close to wp1, wp2, wp3
        let mut bc = make_test_bot_controller(bot_pos, vec![wp1, wp2, wp3, wp4]);

        let next = OptimizedBotAI::next_waypoint(&mut bc, bot_pos);
        // wp1, wp2, wp3 are all within BOT_WAYPOINT_ARRIVAL_DIST of bot_pos.
        // wp3 should NOT be popped because it's the second-to-last and close,
        // but wp4 is the one we want to reach.
        // Actually wp1 (dist=8), wp2 (dist=3), wp3 (dist=2) are all within 30u.
        // But we don't pop the last one. So wp1, wp2, wp3 get popped leaving wp4.
        assert_eq!(
            bc.current_path.len(),
            1,
            "Should have popped 3 waypoints, 1 remaining"
        );
        assert!((next.x - wp4.x).abs() < 1.0);
    }

    #[test]
    fn invalidate_path_clears_state() {
        let mut bc = make_test_bot_controller(Vec2::new(0.0, 0.0), vec![Vec2::new(100.0, 100.0)]);
        bc.path_compute_tick = 42;
        bc.last_path_target = Some(Vec2::new(100.0, 100.0));

        OptimizedBotAI::invalidate_path(&mut bc);
        assert!(bc.current_path.is_empty());
        assert!(bc.last_path_target.is_none());
    }

    #[test]
    fn pathfinding_constants_are_sane() {
        // Grid cell size should be positive
        const { assert!(BOT_NAV_GRID_CELL_SIZE > 0.0) };
        // Waypoint arrival distance should be smaller than movement tolerance
        // to avoid "jitter" between waypoints and "at target" detection
        const { assert!(BOT_WAYPOINT_ARRIVAL_DIST < BOT_MOVEMENT_TOLERANCE) };
        // Recompute interval should be positive
        const { assert!(BOT_PATH_RECOMPUTE_INTERVAL_TICKS > 0) };
        // Max waypoints should be reasonable
        const { assert!(BOT_PATH_MAX_WAYPOINTS >= 4) };
    }

    #[test]
    fn path_target_moved_threshold_is_significant() {
        // The threshold should correspond to a meaningful distance (>50u)
        let threshold_dist = BOT_PATH_TARGET_MOVED_THRESHOLD_SQ.sqrt();
        assert!(
            threshold_dist >= 100.0,
            "Path recompute distance threshold should be >= 100 units, got {}",
            threshold_dist
        );
    }
}
