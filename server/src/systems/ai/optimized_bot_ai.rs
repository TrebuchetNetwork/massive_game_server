// Optimized Bot AI with CTF Support

use crate::core::constants::*;
use crate::core::types::{PlayerID, PlayerInputData, ServerWeaponType, Vec2};
use crate::flatbuffers_generated::game_protocol as fb;
use crate::server::instance::{BotBehaviorState, BotController, MassiveGameServer};
use crate::systems::ai::commander::{
    MotionSample, PredictiveMotionModel, ThreatPredictor, ThreatSample,
};

use crate::core::deterministic_rng::DeterministicRng;
use dashmap::DashMap;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
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
const BOT_SHOOT_ACCURACY: f32 = 0.80; // 80% accuracy
const BOT_FLAG_CHASE_PRIORITY: f32 = 3.0; // High priority for flag objectives
const BOT_MOVEMENT_TOLERANCE: f32 = 50.0; // Distance to consider "at target"
const BOT_STUCK_THRESHOLD: f32 = 10.0; // Min distance to move to not be considered stuck
const BOT_STUCK_TIME_THRESHOLD: f32 = 2.0; // Seconds before considering bot stuck
const BOT_STUCK_CHECK_INTERVAL: f32 = 0.5; // Check every half second
const BOT_STUCK_TARGET_TOLERANCE: f32 = BOT_MOVEMENT_TOLERANCE + 20.0;

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
            BotAiLodTier::Medium => frame_count % BOT_LOD_MEDIUM_STRIDE == 0,
            BotAiLodTier::Far => frame_count % BOT_LOD_FAR_STRIDE == 0,
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
    pub fn preferred_weapon_slot(&self, current_weapon: ServerWeaponType, enemy_distance: f32) -> Option<u8> {
        match self {
            BotPersonality::Aggressive => {
                // Prefer Shotgun at close range, Melee at very close
                if enemy_distance < 60.0 {
                    if current_weapon != ServerWeaponType::Melee {
                        return Some(2); // Switch to melee slot
                    }
                } else if enemy_distance < 200.0
                    && current_weapon != ServerWeaponType::Shotgun {
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
                } else if enemy_distance > 150.0
                    && current_weapon != ServerWeaponType::Rifle {
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

pub struct OptimizedBotAI;

#[derive(Clone)]
struct EnemySnapshot {
    id: PlayerID,
    x: f32,
    y: f32,
    velocity_x: f32,
    velocity_y: f32,
    team_id: u8,
    carries_flag_team_id: u8,
    weapon: ServerWeaponType,
}

#[derive(Clone)]
struct BotSnapshotOwned {
    id: PlayerID,
    username: String,
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
            static HUMAN_POSITIONS: RefCell<Vec<(f32, f32)>> = const { RefCell::new(Vec::new()) };
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
            BOT_IDS.with(|cell| *cell.borrow_mut() = bot_ids);
            return;
        }

        // Collect human player positions for LOD classification.
        // A human player is any live player whose ID is NOT in bot_players.
        let mut human_positions =
            HUMAN_POSITIONS.with(|cell| std::mem::take(&mut *cell.borrow_mut()));
        human_positions.clear();
        server_instance
            .player_manager
            .for_each_player(|id, player| {
                if player.alive && !server_instance.bot_players.contains_key(id) {
                    human_positions.push((player.x, player.y));
                }
            });

        trace!(
            "Frame {}: Processing {} bots, {} human positions for LOD",
            frame_count,
            bot_ids.len(),
            human_positions.len()
        );

        // Get current match info
        let match_info_guard = server_instance.match_info.read();
        let game_mode = match_info_guard.game_mode;
        let match_state = match_info_guard.match_state;
        let flag_states = &match_info_guard.flag_states;

        // Precompute enemies and team objective counts once per tick.
        let mut enemies_team1 = Vec::new();
        let mut enemies_team2 = Vec::new();
        let mut live_players_by_id: HashMap<PlayerID, EnemySnapshot> = HashMap::new();
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

                let snapshot = EnemySnapshot {
                    id: id.clone(),
                    x: player.x,
                    y: player.y,
                    velocity_x: player.velocity_x,
                    velocity_y: player.velocity_y,
                    team_id: player.team_id,
                    carries_flag_team_id: player.is_carrying_flag_team_id,
                    weapon: player.weapon,
                };

                live_players_by_id.insert(id.clone(), snapshot.clone());

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
                    enemies_team2.push(snapshot);
                } else if player.team_id == 2 {
                    enemies_team1.push(snapshot);
                }
            });

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
            // Compute squared distance to nearest human player.
            let min_dist_sq_to_human = if human_positions.is_empty() {
                // No humans online: treat all bots as Near so they still play.
                0.0
            } else {
                let mut best = f32::MAX;
                for &(hx, hy) in &human_positions {
                    let dx = bot_snapshot.x - hx;
                    let dy = bot_snapshot.y - hy;
                    let d = dx * dx + dy * dy;
                    if d < best {
                        best = d;
                    }
                }
                best
            };
            let lod_tier = BotAiLodTier::classify(min_dist_sq_to_human);

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
        if frame_count.is_multiple_of(120) {
            let mut live_ids: HashSet<PlayerID> = HashSet::with_capacity(live_players_by_id.len());
            for id in live_players_by_id.keys() {
                live_ids.insert(id.clone());
            }
            predictive_models
                .motion_models
                .retain(|player_id, _| live_ids.contains(player_id));
            predictive_models
                .threat_models
                .retain(|player_id, _| live_ids.contains(player_id));
        }

        drop(match_info_guard);
        HUMAN_POSITIONS.with(|cell| *cell.borrow_mut() = human_positions);
        BOT_IDS.with(|cell| *cell.borrow_mut() = bot_ids);
    }

    /// Far-tier wander: pick a random nearby target and patrol. No combat, no
    /// objective logic. Minimal CPU cost.
    fn make_far_wander_decision(
        bot_controller: &mut BotController,
        bot_state: &BotSnapshotOwned,
    ) {
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
        live_players_by_id: &HashMap<PlayerID, EnemySnapshot>,
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
                    (rng.gen_range_f32(-400.0, 400.0), rng.gen_range_f32(-400.0, 400.0))
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
        live_players_by_id: &HashMap<PlayerID, EnemySnapshot>,
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
            BotPersonality::Balanced => (40, 70),    // 40% engage, 30% flank, 30% patrol
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
                (rng.gen_range_f32(-range, range), rng.gen_range_f32(-range, range))
            });
            bot_controller.target_position = Some(Vec2::new(target_x, target_y));
            bot_controller.behavior_state = BotBehaviorState::Engaging;
        } else if behavior_choice < flank_pct {
            // Flanking: Move to sides
            let (side, target_x_abs, target_y) = with_bot_rng(|rng| {
                let side = if rng.gen_bool(0.5) { 1.0 } else { -1.0 };
                (side, rng.gen_range_f32(300.0, 600.0), rng.gen_range_f32(-400.0, 400.0))
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
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let distance = (dx * dx + dy * dy).sqrt();

        // Number of steps to check along the line
        let steps = (distance / 20.0).ceil() as usize;

        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let check_x = from.x + dx * t;
            let check_y = from.y + dy * t;

            // Query walls near this point
            let nearby_walls = server_instance
                .wall_spatial_index
                .query_radius(check_x, check_y, 5.0);

            // Check if any wall blocks this point
            for wall in nearby_walls {
                // Skip destructible walls that are destroyed
                if wall.is_destructible && wall.current_health <= 0 {
                    continue;
                }

                // Check if point is inside wall
                if check_x >= wall.x
                    && check_x <= wall.x + wall.width
                    && check_y >= wall.y
                    && check_y <= wall.y + wall.height
                {
                    return false; // Wall blocks line of sight
                }
            }
        }

        true // Clear line of sight
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

            let candidate = TargetSolution {
                enemy_id: enemy.id.clone(),
                direct_position,
                predicted_position,
                distance_sq: dist_sq,
                aim_angle,
            };

            let should_replace = selected
                .as_ref()
                .is_none_or(|(best_score, _)| threat_score > *best_score);
            if should_replace {
                selected = Some((threat_score, candidate));
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
        let can_switch_weapon = frame_count
            .saturating_sub(bot_controller.last_weapon_switch_tick)
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

        // Movement towards objective - THIS IS THE KEY PART
        let mut movement_handled = false;

        if let Some(target_pos) = bot_controller.target_position {
            let nav_target = server_instance
                .navigation_waypoint_towards(Vec2::new(bot_state.x, bot_state.y), target_pos);
            let dx = nav_target.x - bot_state.x;
            let dy = nav_target.y - bot_state.y;
            let dist_sq = dx * dx + dy * dy;

            // Always set rotation towards target
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
                    "Bot {} moving to target at ({:.0}, {:.0}), distance: {:.0}",
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
            let aim_offset = with_bot_rng(|rng| rng.gen_range_f32(-0.2, 0.2)) * (1.0 - BOT_SHOOT_ACCURACY);
            input.rotation = nearest_enemy_angle + aim_offset;
            if let Some(target) = selected_target.as_ref() {
                // Tighten movement vector around predicted enemy motion when engaging.
                let predicted = target.predicted_position;
                let predict_angle = (predicted.y - bot_state.y).atan2(predicted.x - bot_state.x);
                input.rotation = (input.rotation + predict_angle) * 0.5;
                trace!(
                    "Bot {} ({:?}) engaging predicted target {}",
                    bot_state.username,
                    personality,
                    target.enemy_id.as_str()
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
                if ticks_since_decision >= BOT_REACTION_TIME_TICKS {
                    input.shooting = with_bot_rng(|rng| rng.gen_bool(0.7)); // 70% chance to shoot when in range

                    // Aggressive bots prefer melee at very close range
                    let melee_chance = match personality {
                        BotPersonality::Aggressive => 0.5,
                        BotPersonality::Defensive => 0.1,
                        BotPersonality::Balanced => 0.3,
                    };
                    if nearest_enemy_dist < 60.0 * 60.0 && with_bot_rng(|rng| rng.gen_bool(melee_chance)) {
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
                        if nearest_enemy_dist < engagement_range_sq {
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
                        if nearest_enemy_dist < 200.0 * 200.0 {
                            // Strafe at close range
                            if with_bot_rng(|rng| rng.gen_bool(0.6)) {
                                input.move_left = with_bot_rng(|rng| rng.gen_bool(0.5));
                                input.move_right = !input.move_left;
                            }
                            // Sometimes retreat
                            if nearest_enemy_dist < 100.0 * 100.0 && with_bot_rng(|rng| rng.gen_bool(0.3)) {
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
                    // Bot is definitely stuck - take action
                    debug!(
                        "Bot {} is stuck at ({:.0}, {:.0}), generating new target",
                        bot_state.username, current_pos.x, current_pos.y
                    );

                    // Try to move in a random direction away from current position
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
        let slot = BotPersonality::Defensive.preferred_weapon_slot(ServerWeaponType::Shotgun, 250.0);
        assert_eq!(slot, Some(1)); // switch to rifle
    }

    #[test]
    fn defensive_keeps_weapon_at_close_range() {
        let slot = BotPersonality::Defensive.preferred_weapon_slot(ServerWeaponType::Shotgun, 100.0);
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
}
