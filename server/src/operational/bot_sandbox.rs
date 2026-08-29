use crate::operational::validation::sanitize_model_id;
use parking_lot::RwLock;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, warn};
use wasmtime::{
    Config, Engine, ExternType, Module, Store, StoreLimits, StoreLimitsBuilder, TypedFunc, ValType,
};

const DEFAULT_WASM_DIR: &str = "data/arena_bots";
const DEFAULT_FUEL_PER_TICK: u64 = 1_000_000;
const DEFAULT_MAX_TICKS: u32 = 600;
const DEFAULT_REPLAY_MAX_FRAMES: usize = 1024;
const MAX_ALLOWED_TICKS: u32 = 5_000;
pub(crate) const MAX_TEAM_BATTLE_SIZE: u32 = 20;
const MAX_TEAM_BATTLE_ROUNDS: u32 = 32;
pub const MIN_WORLD_BATTLE_ENTRANTS: usize = 2;
pub const MAX_WORLD_BATTLE_ENTRANTS: usize = 16;
pub const MAX_WORLD_SQUAD_SIZE: u32 = 5;
pub const MAX_WORLD_BATTLE_ROUNDS: u32 = 16;
pub const MAX_WORLD_BATTLE_TICKS: u32 = 2_000;
const MAX_COMPILED_MODULE_CACHE_ENTRIES: usize = 256;
const MAX_REPLAY_FRAMES: usize = 8_192;
const BOT_TICK_EXPORT: &str = "bot_tick";
const BOT_TICK_V2_EXPORT: &str = "bot_tick_v2";
const BOT_TICK_V2_PARAM_COUNT: usize = 11;
const DEFAULT_RESPAWNS_NON_ARENA: i32 = 3;
const TEAM_SUPPORT_DAMAGE_PERCENT: i32 = 50;
const TEAM_ASSIST_SCORE: i32 = 20;
const EXHIBITION_MAX_LINEAR_MEMORY_BYTES: usize = 2 * 1024 * 1024;
const EXHIBITION_MAX_TABLE_ELEMENTS: u32 = 128;
const EXHIBITION_MAX_INSTANCES: usize = 1;
const EXHIBITION_MAX_TABLES: usize = 1;
const EXHIBITION_MAX_MEMORIES: usize = 1;
const MAX_PUBLISHED_WASM_BYTES: usize = 2 * 1024 * 1024;

/// Bump this whenever deterministic combat, targeting, objective, scoring, or
/// action-resolution semantics change. Season checkpoints bind to this value
/// so results from different simulator rules can never be mixed.
pub const ARENA_SIMULATOR_RULES_VERSION: &str = "arena-world-sim-v3.0.0";

const WORLD_PLACEMENT_POINTS: [u32; MAX_WORLD_BATTLE_ENTRANTS] = [
    1_000, 600, 360, 220, 140, 90, 60, 40, 30, 22, 16, 12, 9, 7, 5, 3,
];

#[derive(Clone)]
struct CachedModule {
    content_sha256: [u8; 32],
    module: Module,
}

#[derive(Clone)]
pub struct BotSandbox {
    engine: Option<Engine>,
    wasm_dir: PathBuf,
    fuel_per_tick: u64,
    default_max_ticks: u32,
    replay_max_frames: usize,
    module_cache: Arc<RwLock<HashMap<PathBuf, CachedModule>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArenaMatchMode {
    Arena,
    Ctf,
    Koth,
    TeamDeathmatch,
}

impl ArenaMatchMode {
    pub fn parse(raw: &str) -> Option<Self> {
        let normalized = raw.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "" | "arena" | "duel" | "classic" => Some(Self::Arena),
            "ctf" | "capture_the_flag" | "capture-the-flag" => Some(Self::Ctf),
            "koth" | "king_of_the_hill" | "king-of-the-hill" => Some(Self::Koth),
            "tdm" | "team_deathmatch" | "team-deathmatch" | "teamdeathmatch" => {
                Some(Self::TeamDeathmatch)
            }
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Arena => "arena",
            Self::Ctf => "ctf",
            Self::Koth => "koth",
            Self::TeamDeathmatch => "tdm",
        }
    }

    fn objective_label(&self) -> &'static str {
        match self {
            Self::Arena => "score",
            Self::Ctf => "captures",
            Self::Koth => "hill_control",
            Self::TeamDeathmatch => "eliminations",
        }
    }

    /// Stable integer supplied to `bot_tick_v2`.
    fn code(self) -> i32 {
        match self {
            Self::Arena => 0,
            Self::TeamDeathmatch => 1,
            Self::Ctf => 2,
            Self::Koth => 3,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BotMatchOutcome {
    pub winner_model_id: Option<String>,
    pub draw: bool,
    pub mode: String,
    pub objective_label: String,
    pub objective_a: i32,
    pub objective_b: i32,
    pub model_a_score: i32,
    pub model_b_score: i32,
    pub model_a_runtime: String,
    pub model_b_runtime: String,
    pub ticks_executed: u32,
    pub duration_ms: u64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BotReplayFrame {
    pub tick: u32,
    pub action_model_a: String,
    pub action_model_b: String,
    pub health_model_a: i32,
    pub health_model_b: i32,
    pub score_model_a: i32,
    pub score_model_b: i32,
    pub objective_a: i32,
    pub objective_b: i32,
    pub respawns_model_a: i32,
    pub respawns_model_b: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct BotMatchReplay {
    pub mode: String,
    pub objective_label: String,
    pub seed: u64,
    pub max_ticks: u32,
    pub captured_frames: usize,
    pub total_ticks_executed: u32,
    pub truncated: bool,
    pub frames: Vec<BotReplayFrame>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BotMatchExecution {
    pub outcome: BotMatchOutcome,
    pub replay: BotMatchReplay,
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamBattleRoundOutcome {
    pub round: u32,
    pub engagements: u32,
    pub draws: u32,
    pub team_a_objective: i32,
    pub team_b_objective: i32,
    pub team_a_score: i32,
    pub team_b_score: i32,
    /// Sum of fighter-local combat points. Kept separate from objective/team points.
    pub team_a_personal_score: i32,
    pub team_b_personal_score: i32,
    /// Objective and elimination points earned by the team as a unit.
    pub team_a_team_score: i32,
    pub team_b_team_score: i32,
    /// Causal teammate benefit: ally damage prevented plus same-tick assists.
    pub team_a_collaboration_score: i32,
    pub team_b_collaboration_score: i32,
    pub winner_model_id: Option<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TeamActionCounts {
    pub idle: u64,
    pub attack: u64,
    pub defend: u64,
    pub charge: u64,
    pub support: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct BotFaultCounts {
    /// Number of fighter runtimes instantiated with the deterministic fallback.
    pub fallback_count: u64,
    /// Number of calls to a compiled strategy that trapped.
    pub trap_count: u64,
    /// Number of out-of-range values returned by `bot_tick_v2`.
    pub invalid_action_count: u64,
    /// Number of failures while replenishing Wasmtime fuel for a tick.
    pub fuel_error_count: u64,
}

/// High-level directive returned by a published arena fighter when it is used
/// in a live, human-facing exhibition. These values intentionally mirror the
/// competition ABI without exposing the private simulator implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExhibitionBotAction {
    Idle,
    Attack,
    Defend,
    Charge,
    Support,
}

impl ExhibitionBotAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Attack => "attack",
            Self::Defend => "defend",
            Self::Charge => "charge",
            Self::Support => "support",
        }
    }
}

/// Live-game observations passed to the same `bot_tick_v2` ABI used by the
/// deterministic arena evaluator. The live adapter owns the tick counter so a
/// caller cannot replay or skip strategy ticks accidentally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExhibitionBotObservation {
    pub self_health: i32,
    pub target_health: i32,
    pub personal_score: i32,
    pub team_score_delta: i32,
    pub objective_delta: i32,
    pub allies_alive: i32,
    pub enemies_alive: i32,
    pub lowest_ally_health: i32,
    pub slot: i32,
    pub mode: ArenaMatchMode,
}

/// A strict, fuel-metered instance of a published strategy for human-facing
/// exhibitions. Construction fails unless the model has a valid
/// `bot_tick_v2` WebAssembly artifact; unlike official simulator execution it
/// never substitutes the deterministic fallback fighter.
pub struct ExhibitionBotRuntime {
    model_id: String,
    runtime: BotRuntime,
    fuel_per_tick: u64,
    tick: u32,
    warnings: Vec<String>,
    faults: BotFaultCounts,
}

impl ExhibitionBotRuntime {
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub fn tick(&self) -> u32 {
        self.tick
    }

    pub fn fault_counts(&self) -> BotFaultCounts {
        self.faults
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub fn next_action(
        &mut self,
        observation: ExhibitionBotObservation,
    ) -> Result<ExhibitionBotAction, String> {
        let faults_before = self.faults;
        let action = self.runtime.next_action(
            self.fuel_per_tick,
            BotObservation {
                self_health: observation.self_health,
                target_health: observation.target_health,
                personal_score: observation.personal_score,
                team_score_delta: observation.team_score_delta,
                objective_delta: observation.objective_delta,
                allies_alive: observation.allies_alive,
                enemies_alive: observation.enemies_alive,
                lowest_ally_health: observation.lowest_ally_health,
                slot: observation.slot,
                mode: observation.mode,
                tick: self.tick,
            },
            &mut self.warnings,
            &mut self.faults,
            self.model_id.as_str(),
        );
        self.tick = self.tick.saturating_add(1);
        if self.faults.trap_count > faults_before.trap_count
            || self.faults.fuel_error_count > faults_before.fuel_error_count
            || self.faults.invalid_action_count > faults_before.invalid_action_count
        {
            return Err(self
                .warnings
                .last()
                .cloned()
                .unwrap_or_else(|| "published fighter failed during a live tick".to_owned()));
        }
        Ok(action.into())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamBattleOutcome {
    pub mode: String,
    pub rules_version: String,
    pub model_a_id: String,
    pub model_b_id: String,
    pub team_size: u32,
    pub rounds: u32,
    pub max_ticks: u32,
    pub team_a_round_wins: u32,
    pub team_b_round_wins: u32,
    pub round_draws: u32,
    pub total_engagements: u32,
    pub total_team_a_objective: i64,
    pub total_team_b_objective: i64,
    pub total_team_a_score: i64,
    pub total_team_b_score: i64,
    pub total_team_a_personal_score: i64,
    pub total_team_b_personal_score: i64,
    pub total_team_a_team_score: i64,
    pub total_team_b_team_score: i64,
    pub total_team_a_collaboration_score: i64,
    pub total_team_b_collaboration_score: i64,
    pub team_a_action_counts: TeamActionCounts,
    pub team_b_action_counts: TeamActionCounts,
    pub team_a_v2_fighters: u32,
    pub team_b_v2_fighters: u32,
    pub fallback_count: u64,
    pub trap_count: u64,
    pub invalid_action_count: u64,
    pub fuel_error_count: u64,
    pub winner_model_id: Option<String>,
    pub draw: bool,
    pub duration_ms: u64,
    pub rounds_detail: Vec<TeamBattleRoundOutcome>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorldBattlePlacement {
    pub rank: u32,
    pub model_id: String,
    pub points: u32,
    pub eliminations: u64,
    pub deaths: u64,
    pub personal_score: i64,
    pub team_score: i64,
    pub collaboration_score: i64,
    pub fighters_alive: u32,
    pub remaining_health: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorldBattleRoundOutcome {
    pub round: u32,
    pub seed: u64,
    pub ticks_executed: u32,
    pub winner_model_id: Option<String>,
    pub draw: bool,
    pub placements: Vec<WorldBattlePlacement>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorldModelOutcome {
    pub rank: u32,
    pub model_id: String,
    pub runtime: String,
    pub points: u64,
    pub round_wins: u32,
    pub eliminations: u64,
    pub deaths: u64,
    pub personal_score: i64,
    pub team_score: i64,
    pub collaboration_score: i64,
    pub fighters_alive_total: u64,
    pub remaining_health_total: i64,
    pub action_counts: TeamActionCounts,
    pub v2_fighter_rounds: u64,
    pub fallback_count: u64,
    pub trap_count: u64,
    pub invalid_action_count: u64,
    pub fuel_error_count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorldBattleOutcome {
    pub mode: String,
    pub rules_version: String,
    pub seed: u64,
    pub entrants: u32,
    pub squad_size: u32,
    pub rounds: u32,
    pub max_ticks: u32,
    pub total_ticks_executed: u64,
    pub winner_model_id: Option<String>,
    pub draw: bool,
    pub rankings: Vec<WorldModelOutcome>,
    pub rounds_detail: Vec<WorldBattleRoundOutcome>,
    pub warnings: Vec<String>,
    pub duration_ms: u64,
}

/// Per-fighter attribution for a mixed-team battle. One entry exists for
/// every (side, slot) in the fixture; `model_id` is the model whose program
/// drove that fighter, which is what makes pair-level chemistry computable.
#[derive(Debug, Clone, Serialize)]
pub struct MixedTeamFighterOutcome {
    pub side: String,
    pub slot: u32,
    pub model_id: String,
    pub runtime: String,
    pub eliminations: u64,
    pub deaths: u64,
    pub personal_score: i64,
    pub collaboration_score: i64,
    pub action_counts: TeamActionCounts,
}

#[derive(Debug, Clone, Serialize)]
pub struct MixedTeamBattleRoundOutcome {
    pub round: u32,
    pub seed: u64,
    pub team_a_objective: i32,
    pub team_b_objective: i32,
    pub team_a_score: i32,
    pub team_b_score: i32,
    pub winner_side: Option<String>,
    pub draw: bool,
    pub duration_ms: u64,
}

/// Outcome of a two-team battle where each squad is a mix of models: fighter
/// `slot` on side A is driven by `team_a_models[slot]`'s program. The winner
/// is reported per side (not per model) since each side fields several
/// models.
#[derive(Debug, Clone, Serialize)]
pub struct MixedTeamBattleOutcome {
    pub mode: String,
    pub match_mode: String,
    pub rules_version: String,
    pub seed: u64,
    pub team_a_models: Vec<String>,
    pub team_b_models: Vec<String>,
    pub team_size: u32,
    pub rounds: u32,
    pub max_ticks: u32,
    pub team_a_round_wins: u32,
    pub team_b_round_wins: u32,
    pub round_draws: u32,
    pub total_team_a_objective: i64,
    pub total_team_b_objective: i64,
    pub total_team_a_score: i64,
    pub total_team_b_score: i64,
    pub total_team_a_team_score: i64,
    pub total_team_b_team_score: i64,
    pub total_team_a_collaboration_score: i64,
    pub total_team_b_collaboration_score: i64,
    pub team_a_action_counts: TeamActionCounts,
    pub team_b_action_counts: TeamActionCounts,
    pub winner_side: Option<String>,
    pub draw: bool,
    pub fighters: Vec<MixedTeamFighterOutcome>,
    pub rounds_detail: Vec<MixedTeamBattleRoundOutcome>,
    pub fallback_count: u64,
    pub trap_count: u64,
    pub invalid_action_count: u64,
    pub fuel_error_count: u64,
    pub warnings: Vec<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BotAction {
    Idle,
    Attack,
    Defend,
    Charge,
    Support,
}

impl BotAction {
    /// Legacy bots historically had every integer normalized modulo four.
    /// Preserve that behavior for `bot_tick`, including -1 => charge.
    fn from_v1_code(raw: i32) -> Self {
        match raw.rem_euclid(4) {
            1 => Self::Attack,
            2 => Self::Defend,
            3 => Self::Charge,
            _ => Self::Idle,
        }
    }

    /// V2 is strict: undefined actions become idle instead of gaining a modulo alias.
    fn from_v2_code(raw: i32) -> Option<Self> {
        match raw {
            0 => Some(Self::Idle),
            1 => Some(Self::Attack),
            2 => Some(Self::Defend),
            3 => Some(Self::Charge),
            4 => Some(Self::Support),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Attack => "attack",
            Self::Defend => "defend",
            Self::Charge => "charge",
            Self::Support => "support",
        }
    }

    fn record(self, counts: &mut TeamActionCounts) {
        match self {
            Self::Idle => counts.idle = counts.idle.saturating_add(1),
            Self::Attack => counts.attack = counts.attack.saturating_add(1),
            Self::Defend => counts.defend = counts.defend.saturating_add(1),
            Self::Charge => counts.charge = counts.charge.saturating_add(1),
            Self::Support => counts.support = counts.support.saturating_add(1),
        }
    }
}

impl From<BotAction> for ExhibitionBotAction {
    fn from(action: BotAction) -> Self {
        match action {
            BotAction::Idle => Self::Idle,
            BotAction::Attack => Self::Attack,
            BotAction::Defend => Self::Defend,
            BotAction::Charge => Self::Charge,
            BotAction::Support => Self::Support,
        }
    }
}

#[derive(Clone)]
enum BotProgram {
    Wasm {
        module: Module,
        source_path: PathBuf,
    },
    Fallback {
        reason: String,
    },
}

enum BotRuntime {
    Wasm {
        store: Store<BotStoreState>,
        tick_fn: BotTickFunction,
    },
    Fallback {
        prng_state: u64,
    },
}

struct BotStoreState {
    limits: StoreLimits,
}

type BotTickV2Params = (i32, i32, i32, i32, i32, i32, i32, i32, i32, i32, i32);

enum BotTickFunction {
    V1(TypedFunc<(i32, i32, i32, i32), i32>),
    V2(TypedFunc<BotTickV2Params, i32>),
}

#[derive(Debug, Clone, Copy)]
struct BotObservation {
    self_health: i32,
    target_health: i32,
    personal_score: i32,
    team_score_delta: i32,
    objective_delta: i32,
    allies_alive: i32,
    enemies_alive: i32,
    lowest_ally_health: i32,
    slot: i32,
    mode: ArenaMatchMode,
    tick: u32,
}

struct FighterState {
    health: i32,
    score: i32,
    respawns_remaining: i32,
}

struct TeamFighter {
    state: FighterState,
    runtime: BotRuntime,
    collaboration_score: i32,
}

/// Per-slot round statistics. The frozen same-model team path never reads
/// these; the mixed-team executor folds them into per-model attribution.
#[derive(Clone, Default)]
struct FighterRoundStats {
    kills: u64,
    deaths: u64,
    actions: TeamActionCounts,
    /// Round-final values (not accumulated): the mixed executor sums them
    /// across rounds.
    personal_score: i64,
    collaboration_score: i64,
}

#[derive(Default)]
struct TeamRoundSimulation {
    draws: u32,
    objective_a: i64,
    objective_b: i64,
    personal_a: i64,
    personal_b: i64,
    team_score_a: i64,
    team_score_b: i64,
    collaboration_a: i64,
    collaboration_b: i64,
    actions_a: TeamActionCounts,
    actions_b: TeamActionCounts,
    v2_fighters_a: u32,
    v2_fighters_b: u32,
    faults_a: BotFaultCounts,
    faults_b: BotFaultCounts,
    fighter_stats_a: Vec<FighterRoundStats>,
    fighter_stats_b: Vec<FighterRoundStats>,
}

struct WorldFactionState {
    model_id: String,
    fighters: Vec<TeamFighter>,
    eliminations: u64,
    deaths: u64,
    team_score: i64,
    actions: TeamActionCounts,
    faults: BotFaultCounts,
    v2_fighters: u32,
}

#[derive(Default)]
struct WorldAggregate {
    model_id: String,
    points: u64,
    round_wins: u32,
    eliminations: u64,
    deaths: u64,
    personal_score: i64,
    team_score: i64,
    collaboration_score: i64,
    fighters_alive_total: u64,
    remaining_health_total: i64,
    actions: TeamActionCounts,
    v2_fighter_rounds: u64,
    faults: BotFaultCounts,
}

struct WorldRoundResult {
    outcome: WorldBattleRoundOutcome,
    aggregates: Vec<WorldAggregate>,
}

#[derive(Clone, Copy)]
struct WorldIncomingHit {
    attacker_faction: usize,
    attacker_slot: usize,
    damage: i32,
}

struct WorldDamageResolution {
    target_faction: usize,
    target_slot: usize,
    damage: i32,
    effective_hits: Vec<(usize, usize, i32)>,
    support_credits: Vec<(usize, i32)>,
    killer: Option<(usize, usize)>,
}

#[derive(Default)]
struct MatchObjectiveState {
    ctf_progress_a: i32,
    ctf_progress_b: i32,
    ctf_captures_a: i32,
    ctf_captures_b: i32,
    koth_control_a: i32,
    koth_control_b: i32,
    tdm_elims_a: i32,
    tdm_elims_b: i32,
}

impl BotSandbox {
    pub fn new_from_env() -> Self {
        let wasm_dir = std::env::var("MGS_ARENA_WASM_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_WASM_DIR));
        let fuel_per_tick = std::env::var("MGS_ARENA_BOT_FUEL_PER_TICK")
            .ok()
            .and_then(|raw| raw.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_FUEL_PER_TICK);
        let default_max_ticks = std::env::var("MGS_ARENA_BOT_MAX_TICKS")
            .ok()
            .and_then(|raw| raw.parse::<u32>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_MAX_TICKS)
            .min(MAX_ALLOWED_TICKS);
        let replay_max_frames = std::env::var("MGS_ARENA_REPLAY_MAX_FRAMES")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(DEFAULT_REPLAY_MAX_FRAMES)
            .clamp(64, MAX_REPLAY_FRAMES);

        let mut config = Config::new();
        config.consume_fuel(true);
        let engine = match Engine::new(&config) {
            Ok(e) => Some(e),
            Err(err) => {
                error!(
                    "Failed to initialize wasmtime engine with fuel metering: {}. \
                     WASM runtime disabled; only fallback runtime will be used.",
                    err
                );
                None
            }
        };

        Self {
            engine,
            wasm_dir,
            fuel_per_tick,
            default_max_ticks,
            replay_max_frames,
            module_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_wasm_dir_for_tests(wasm_dir: PathBuf) -> Self {
        let mut sandbox = Self::new_from_env();
        sandbox.wasm_dir = wasm_dir;
        sandbox
    }

    pub fn default_max_ticks(&self) -> u32 {
        self.default_max_ticks
    }

    pub fn replay_max_frames(&self) -> usize {
        self.replay_max_frames
    }

    /// Instantiate one published fighter for live exhibition play.
    ///
    /// This path is deliberately separate from deterministic rating matches:
    /// it neither reads nor writes rating state, and rejects missing/legacy
    /// artifacts rather than disguising them behind the simulator fallback.
    pub fn build_exhibition_runtime(
        &self,
        model_id: &str,
        expected_wasm_bytes: usize,
        expected_wasm_sha256: &str,
        seed: u64,
    ) -> Result<ExhibitionBotRuntime, String> {
        let mut warnings = Vec::new();
        let runtime = self.build_runtime_with_policy(
            self.load_program_with_artifact_binding(
                model_id,
                expected_wasm_bytes,
                expected_wasm_sha256,
            ),
            seed,
            &mut warnings,
            true,
            true,
        );
        if runtime.is_fallback() {
            let reason = warnings
                .last()
                .cloned()
                .unwrap_or_else(|| "published fighter could not be instantiated".to_owned());
            return Err(reason);
        }

        Ok(ExhibitionBotRuntime {
            model_id: model_id.to_owned(),
            runtime,
            fuel_per_tick: self.fuel_per_tick,
            tick: 0,
            warnings,
            faults: BotFaultCounts::default(),
        })
    }

    pub fn execute_duel_with_replay(
        &self,
        model_a_id: &str,
        model_b_id: &str,
        seed: u64,
        requested_ticks: Option<u32>,
    ) -> BotMatchExecution {
        self.execute_match_internal(
            model_a_id,
            model_b_id,
            ArenaMatchMode::Arena,
            seed,
            requested_ticks,
        )
    }

    pub fn execute_duel(
        &self,
        model_a_id: &str,
        model_b_id: &str,
        seed: u64,
        requested_ticks: Option<u32>,
    ) -> BotMatchOutcome {
        self.execute_match_internal(
            model_a_id,
            model_b_id,
            ArenaMatchMode::Arena,
            seed,
            requested_ticks,
        )
        .outcome
    }

    pub fn execute_match(
        &self,
        model_a_id: &str,
        model_b_id: &str,
        mode: ArenaMatchMode,
        seed: u64,
        requested_ticks: Option<u32>,
    ) -> BotMatchOutcome {
        self.execute_match_internal(model_a_id, model_b_id, mode, seed, requested_ticks)
            .outcome
    }

    pub fn execute_match_with_replay(
        &self,
        model_a_id: &str,
        model_b_id: &str,
        mode: ArenaMatchMode,
        seed: u64,
        requested_ticks: Option<u32>,
    ) -> BotMatchExecution {
        self.execute_match_internal(model_a_id, model_b_id, mode, seed, requested_ticks)
    }

    fn execute_match_internal(
        &self,
        model_a_id: &str,
        model_b_id: &str,
        mode: ArenaMatchMode,
        seed: u64,
        requested_ticks: Option<u32>,
    ) -> BotMatchExecution {
        let started_at = Instant::now();
        let mut warnings = Vec::new();
        let mut faults_a = BotFaultCounts::default();
        let mut faults_b = BotFaultCounts::default();
        let max_ticks = requested_ticks
            .unwrap_or(self.default_max_ticks)
            .clamp(1, MAX_ALLOWED_TICKS);
        let replay_capacity = self
            .replay_max_frames
            .min(max_ticks as usize)
            .min(MAX_REPLAY_FRAMES);

        let program_a = self.load_program(model_a_id);
        let program_b = self.load_program(model_b_id);

        let mut runtime_a =
            self.build_runtime(program_a, seed ^ 0xA5A5_A5A5_A5A5_A5A5, &mut warnings);
        let mut runtime_b =
            self.build_runtime(program_b, seed ^ 0x5A5A_5A5A_5A5A_5A5A, &mut warnings);

        let runtime_a_name = runtime_a.runtime_name().to_owned();
        let runtime_b_name = runtime_b.runtime_name().to_owned();

        let default_respawns = if matches!(mode, ArenaMatchMode::Arena) {
            0
        } else {
            DEFAULT_RESPAWNS_NON_ARENA
        };

        let mut a = FighterState {
            health: 100,
            score: 0,
            respawns_remaining: default_respawns,
        };
        let mut b = FighterState {
            health: 100,
            score: 0,
            respawns_remaining: default_respawns,
        };
        let mut objectives = MatchObjectiveState::default();
        let mut replay_frames = Vec::with_capacity(replay_capacity);
        let mut replay_truncated = false;

        let mut ticks_executed = 0u32;
        for tick in 0..max_ticks {
            ticks_executed = tick + 1;
            let (objective_before_a, objective_before_b) =
                objective_values(mode, &objectives, &a, &b);
            let action_a = runtime_a.next_action(
                self.fuel_per_tick,
                BotObservation {
                    self_health: a.health,
                    target_health: b.health,
                    personal_score: a.score,
                    team_score_delta: a.score.saturating_sub(b.score),
                    objective_delta: objective_before_a.saturating_sub(objective_before_b),
                    allies_alive: i32::from(a.health > 0),
                    enemies_alive: i32::from(b.health > 0),
                    lowest_ally_health: 0,
                    slot: 0,
                    mode,
                    tick,
                },
                &mut warnings,
                &mut faults_a,
                "model_a",
            );
            let action_b = runtime_b.next_action(
                self.fuel_per_tick,
                BotObservation {
                    self_health: b.health,
                    target_health: a.health,
                    personal_score: b.score,
                    team_score_delta: b.score.saturating_sub(a.score),
                    objective_delta: objective_before_b.saturating_sub(objective_before_a),
                    allies_alive: i32::from(b.health > 0),
                    enemies_alive: i32::from(a.health > 0),
                    lowest_ally_health: 0,
                    slot: 0,
                    mode,
                    tick,
                },
                &mut warnings,
                &mut faults_b,
                "model_b",
            );

            let prev_a_health = a.health;
            let prev_b_health = b.health;

            resolve_combat_tick(&mut a, &mut b, action_a, action_b, seed, tick);
            apply_mode_objectives(
                mode,
                &mut a,
                &mut b,
                action_a,
                action_b,
                &mut objectives,
                tick,
            );

            let (tick_objective_a, tick_objective_b) = objective_values(mode, &objectives, &a, &b);
            if replay_frames.len() < replay_capacity {
                replay_frames.push(BotReplayFrame {
                    tick: tick + 1,
                    action_model_a: action_a.as_str().to_owned(),
                    action_model_b: action_b.as_str().to_owned(),
                    health_model_a: a.health,
                    health_model_b: b.health,
                    score_model_a: a.score,
                    score_model_b: b.score,
                    objective_a: tick_objective_a,
                    objective_b: tick_objective_b,
                    respawns_model_a: a.respawns_remaining,
                    respawns_model_b: b.respawns_remaining,
                });
            } else {
                replay_truncated = true;
            }

            let a_eliminated = prev_a_health > 0 && a.health <= 0;
            let b_eliminated = prev_b_health > 0 && b.health <= 0;

            if matches!(mode, ArenaMatchMode::TeamDeathmatch) {
                if a_eliminated {
                    objectives.tdm_elims_b += 1;
                    b.score += 40;
                }
                if b_eliminated {
                    objectives.tdm_elims_a += 1;
                    a.score += 40;
                }
            }

            if matches!(mode, ArenaMatchMode::Arena) {
                if a.health <= 0 || b.health <= 0 {
                    break;
                }
                continue;
            }

            if a_eliminated {
                a.score = a.score.saturating_sub(4);
                if a.respawns_remaining > 0 {
                    a.respawns_remaining -= 1;
                    a.health = 100;
                }
            }
            if b_eliminated {
                b.score = b.score.saturating_sub(4);
                if b.respawns_remaining > 0 {
                    b.respawns_remaining -= 1;
                    b.health = 100;
                }
            }

            let a_permanently_out = a.health <= 0 && a.respawns_remaining <= 0;
            let b_permanently_out = b.health <= 0 && b.respawns_remaining <= 0;
            if a_permanently_out || b_permanently_out {
                break;
            }

            if matches!(mode, ArenaMatchMode::Ctf)
                && (objectives.ctf_captures_a >= 3 || objectives.ctf_captures_b >= 3)
            {
                break;
            }
            if matches!(mode, ArenaMatchMode::Koth)
                && (objectives.koth_control_a >= 160 || objectives.koth_control_b >= 160)
            {
                break;
            }
        }

        if matches!(mode, ArenaMatchMode::Arena) {
            if a.health <= 0 && b.health > 0 {
                a.score -= 10;
                b.score += 50;
            } else if b.health <= 0 && a.health > 0 {
                b.score -= 10;
                a.score += 50;
            }
        }

        let (objective_a, objective_b) = objective_values(mode, &objectives, &a, &b);
        let (winner_model_id, draw) = determine_winner(
            mode,
            model_a_id,
            model_b_id,
            &a,
            &b,
            objective_a,
            objective_b,
        );

        let duration_ms = started_at.elapsed().as_millis() as u64;
        let outcome = BotMatchOutcome {
            winner_model_id,
            draw,
            mode: mode.as_str().to_owned(),
            objective_label: mode.objective_label().to_owned(),
            objective_a,
            objective_b,
            model_a_score: a.score.max(0),
            model_b_score: b.score.max(0),
            model_a_runtime: runtime_a_name,
            model_b_runtime: runtime_b_name,
            ticks_executed,
            duration_ms,
            warnings,
        };
        let replay = BotMatchReplay {
            mode: mode.as_str().to_owned(),
            objective_label: mode.objective_label().to_owned(),
            seed,
            max_ticks,
            captured_frames: replay_frames.len(),
            total_ticks_executed: ticks_executed,
            truncated: replay_truncated,
            frames: replay_frames,
        };
        BotMatchExecution { outcome, replay }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn execute_team_battle(
        &self,
        model_a_id: &str,
        model_b_id: &str,
        mode: ArenaMatchMode,
        team_size: u32,
        rounds: u32,
        seed: u64,
        requested_ticks: Option<u32>,
    ) -> TeamBattleOutcome {
        let started_at = Instant::now();
        let normalized_team_size = team_size.clamp(1, MAX_TEAM_BATTLE_SIZE);
        let normalized_rounds = rounds.clamp(1, MAX_TEAM_BATTLE_ROUNDS);
        let max_ticks = requested_ticks
            .unwrap_or(self.default_max_ticks)
            .clamp(1, MAX_ALLOWED_TICKS);

        let mut team_a_round_wins = 0u32;
        let mut team_b_round_wins = 0u32;
        let mut round_draws = 0u32;
        let mut total_team_a_objective = 0i64;
        let mut total_team_b_objective = 0i64;
        let mut total_team_a_score = 0i64;
        let mut total_team_b_score = 0i64;
        let mut total_team_a_team_score = 0i64;
        let mut total_team_b_team_score = 0i64;
        let mut total_team_a_collaboration_score = 0i64;
        let mut total_team_b_collaboration_score = 0i64;
        let mut team_a_action_counts = TeamActionCounts::default();
        let mut team_b_action_counts = TeamActionCounts::default();
        let mut team_a_v2_fighters = 0u32;
        let mut team_b_v2_fighters = 0u32;
        let mut fault_counts = BotFaultCounts::default();
        let mut all_warnings = Vec::new();
        let mut rounds_detail = Vec::with_capacity(normalized_rounds as usize);

        for round in 0..normalized_rounds {
            let round_started_at = Instant::now();
            let round_seed = seed ^ ((round as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let simulation = self.simulate_interacting_team_round(
                model_a_id,
                model_b_id,
                mode,
                normalized_team_size,
                round_seed,
                max_ticks,
                &mut all_warnings,
            );

            total_team_a_objective += simulation.objective_a;
            total_team_b_objective += simulation.objective_b;
            total_team_a_score += simulation.personal_a;
            total_team_b_score += simulation.personal_b;
            total_team_a_team_score += simulation.team_score_a;
            total_team_b_team_score += simulation.team_score_b;
            total_team_a_collaboration_score += simulation.collaboration_a;
            total_team_b_collaboration_score += simulation.collaboration_b;
            add_action_counts(&mut team_a_action_counts, &simulation.actions_a);
            add_action_counts(&mut team_b_action_counts, &simulation.actions_b);
            team_a_v2_fighters = team_a_v2_fighters.max(simulation.v2_fighters_a);
            team_b_v2_fighters = team_b_v2_fighters.max(simulation.v2_fighters_b);
            add_fault_counts(&mut fault_counts, simulation.faults_a);
            add_fault_counts(&mut fault_counts, simulation.faults_b);

            let round_winner = compare_team_round(
                model_a_id,
                model_b_id,
                simulation.objective_a,
                simulation.objective_b,
                simulation.team_score_a,
                simulation.team_score_b,
            );
            match round_winner.as_deref() {
                Some(winner) if winner == model_a_id => {
                    team_a_round_wins = team_a_round_wins.saturating_add(1);
                }
                Some(winner) if winner == model_b_id => {
                    team_b_round_wins = team_b_round_wins.saturating_add(1);
                }
                _ => {
                    round_draws = round_draws.saturating_add(1);
                }
            }

            rounds_detail.push(TeamBattleRoundOutcome {
                round: round + 1,
                engagements: normalized_team_size,
                draws: simulation.draws,
                team_a_objective: saturating_i64_to_i32(simulation.objective_a),
                team_b_objective: saturating_i64_to_i32(simulation.objective_b),
                // Legacy aliases retain their old meaning: aggregate personal points.
                team_a_score: saturating_i64_to_i32(simulation.personal_a),
                team_b_score: saturating_i64_to_i32(simulation.personal_b),
                team_a_personal_score: saturating_i64_to_i32(simulation.personal_a),
                team_b_personal_score: saturating_i64_to_i32(simulation.personal_b),
                team_a_team_score: saturating_i64_to_i32(simulation.team_score_a),
                team_b_team_score: saturating_i64_to_i32(simulation.team_score_b),
                team_a_collaboration_score: saturating_i64_to_i32(simulation.collaboration_a),
                team_b_collaboration_score: saturating_i64_to_i32(simulation.collaboration_b),
                winner_model_id: round_winner,
                duration_ms: round_started_at.elapsed().as_millis() as u64,
            });
        }

        let winner_model_id = if team_a_round_wins > team_b_round_wins {
            Some(model_a_id.to_owned())
        } else if team_b_round_wins > team_a_round_wins {
            Some(model_b_id.to_owned())
        } else {
            compare_team_round(
                model_a_id,
                model_b_id,
                total_team_a_objective,
                total_team_b_objective,
                total_team_a_team_score,
                total_team_b_team_score,
            )
        };
        let draw = winner_model_id.is_none();

        TeamBattleOutcome {
            mode: mode.as_str().to_owned(),
            rules_version: ARENA_SIMULATOR_RULES_VERSION.to_owned(),
            model_a_id: model_a_id.to_owned(),
            model_b_id: model_b_id.to_owned(),
            team_size: normalized_team_size,
            rounds: normalized_rounds,
            max_ticks,
            team_a_round_wins,
            team_b_round_wins,
            round_draws,
            total_engagements: normalized_team_size.saturating_mul(normalized_rounds),
            total_team_a_objective,
            total_team_b_objective,
            total_team_a_score,
            total_team_b_score,
            total_team_a_personal_score: total_team_a_score,
            total_team_b_personal_score: total_team_b_score,
            total_team_a_team_score,
            total_team_b_team_score,
            total_team_a_collaboration_score,
            total_team_b_collaboration_score,
            team_a_action_counts,
            team_b_action_counts,
            team_a_v2_fighters,
            team_b_v2_fighters,
            fallback_count: fault_counts.fallback_count,
            trap_count: fault_counts.trap_count,
            invalid_action_count: fault_counts.invalid_action_count,
            fuel_error_count: fault_counts.fuel_error_count,
            winner_model_id,
            draw,
            duration_ms: started_at.elapsed().as_millis() as u64,
            rounds_detail,
            warnings: all_warnings,
        }
    }

    /// Executes a two-team battle where each squad is a mix of models:
    /// fighter `slot` on side A is driven by `team_a_models[slot]`'s program
    /// (likewise for side B). Both squads must be the same size — the round
    /// core shares the objective scaling of the frozen same-model path, which
    /// is defined per team size. Every per-fighter contribution (kills,
    /// deaths, personal/collaboration score, actions) is attributed to the
    /// fighter's model so pair-level chemistry can be computed downstream.
    /// Seeding is deterministic: same rosters + mode + rounds + seed produce
    /// an identical outcome.
    pub fn execute_mixed_team_battle(
        &self,
        team_a_models: &[String],
        team_b_models: &[String],
        mode: ArenaMatchMode,
        rounds: u32,
        seed: u64,
        requested_ticks: Option<u32>,
    ) -> MixedTeamBattleOutcome {
        let started_at = Instant::now();
        let canonicalize = |models: &[String]| -> Vec<String> {
            let mut seen = std::collections::HashSet::new();
            models
                .iter()
                .map(|model_id| model_id.trim())
                .filter(|model_id| sanitize_model_id(model_id).is_some())
                .filter(|model_id| seen.insert((*model_id).to_owned()))
                .map(ToOwned::to_owned)
                .take(MAX_TEAM_BATTLE_SIZE as usize)
                .collect()
        };
        let roster_a = canonicalize(team_a_models);
        let roster_b = canonicalize(team_b_models);
        let team_size = roster_a.len().min(roster_b.len()) as u32;
        let roster_a: Vec<String> = roster_a.into_iter().take(team_size as usize).collect();
        let roster_b: Vec<String> = roster_b.into_iter().take(team_size as usize).collect();
        let normalized_rounds = rounds.clamp(1, MAX_TEAM_BATTLE_ROUNDS);
        let max_ticks = requested_ticks
            .unwrap_or(self.default_max_ticks)
            .clamp(1, MAX_ALLOWED_TICKS);
        let respawns = if matches!(mode, ArenaMatchMode::Arena) {
            0
        } else {
            DEFAULT_RESPAWNS_NON_ARENA
        };

        #[derive(Clone, Default)]
        struct FighterAggregate {
            runtime: Option<String>,
            eliminations: u64,
            deaths: u64,
            personal_score: i64,
            collaboration_score: i64,
            action_counts: TeamActionCounts,
        }

        let mut aggregates_a = vec![FighterAggregate::default(); roster_a.len()];
        let mut aggregates_b = vec![FighterAggregate::default(); roster_b.len()];
        let mut team_a_round_wins = 0u32;
        let mut team_b_round_wins = 0u32;
        let mut round_draws = 0u32;
        let mut total_team_a_objective = 0i64;
        let mut total_team_b_objective = 0i64;
        let mut total_team_a_score = 0i64;
        let mut total_team_b_score = 0i64;
        let mut total_team_a_team_score = 0i64;
        let mut total_team_b_team_score = 0i64;
        let mut total_team_a_collaboration_score = 0i64;
        let mut total_team_b_collaboration_score = 0i64;
        let mut team_a_action_counts = TeamActionCounts::default();
        let mut team_b_action_counts = TeamActionCounts::default();
        let mut fault_counts = BotFaultCounts::default();
        let mut all_warnings = Vec::new();
        let mut rounds_detail = Vec::with_capacity(normalized_rounds as usize);

        for round in 0..normalized_rounds {
            let round_started_at = Instant::now();
            let round_seed = seed ^ ((round as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let team_a =
                self.build_mixed_team(&roster_a, respawns, round_seed, 0xA11A, &mut all_warnings);
            let team_b =
                self.build_mixed_team(&roster_b, respawns, round_seed, 0xB22B, &mut all_warnings);
            if round == 0 {
                for (slot, fighter) in team_a.iter().enumerate() {
                    aggregates_a[slot].runtime =
                        Some(mixed_runtime_name(&fighter.runtime).to_owned());
                }
                for (slot, fighter) in team_b.iter().enumerate() {
                    aggregates_b[slot].runtime =
                        Some(mixed_runtime_name(&fighter.runtime).to_owned());
                }
            }
            let simulation = self.run_team_round(
                team_a,
                team_b,
                mode,
                team_size,
                round_seed,
                max_ticks,
                &mut all_warnings,
            );

            total_team_a_objective += simulation.objective_a;
            total_team_b_objective += simulation.objective_b;
            total_team_a_score += simulation.personal_a;
            total_team_b_score += simulation.personal_b;
            total_team_a_team_score += simulation.team_score_a;
            total_team_b_team_score += simulation.team_score_b;
            total_team_a_collaboration_score += simulation.collaboration_a;
            total_team_b_collaboration_score += simulation.collaboration_b;
            add_action_counts(&mut team_a_action_counts, &simulation.actions_a);
            add_action_counts(&mut team_b_action_counts, &simulation.actions_b);
            add_fault_counts(&mut fault_counts, simulation.faults_a);
            add_fault_counts(&mut fault_counts, simulation.faults_b);
            for (slot, stats) in simulation.fighter_stats_a.iter().enumerate() {
                let aggregate = &mut aggregates_a[slot];
                aggregate.eliminations = aggregate.eliminations.saturating_add(stats.kills);
                aggregate.deaths = aggregate.deaths.saturating_add(stats.deaths);
                aggregate.personal_score += stats.personal_score;
                aggregate.collaboration_score += stats.collaboration_score;
                add_action_counts(&mut aggregate.action_counts, &stats.actions);
            }
            for (slot, stats) in simulation.fighter_stats_b.iter().enumerate() {
                let aggregate = &mut aggregates_b[slot];
                aggregate.eliminations = aggregate.eliminations.saturating_add(stats.kills);
                aggregate.deaths = aggregate.deaths.saturating_add(stats.deaths);
                aggregate.personal_score += stats.personal_score;
                aggregate.collaboration_score += stats.collaboration_score;
                add_action_counts(&mut aggregate.action_counts, &stats.actions);
            }

            let round_winner = compare_sides(
                simulation.objective_a,
                simulation.objective_b,
                simulation.team_score_a,
                simulation.team_score_b,
            );
            match round_winner {
                Some("team_a") => team_a_round_wins = team_a_round_wins.saturating_add(1),
                Some("team_b") => team_b_round_wins = team_b_round_wins.saturating_add(1),
                _ => round_draws = round_draws.saturating_add(1),
            }
            rounds_detail.push(MixedTeamBattleRoundOutcome {
                round: round + 1,
                seed: round_seed,
                team_a_objective: saturating_i64_to_i32(simulation.objective_a),
                team_b_objective: saturating_i64_to_i32(simulation.objective_b),
                team_a_score: saturating_i64_to_i32(simulation.team_score_a),
                team_b_score: saturating_i64_to_i32(simulation.team_score_b),
                winner_side: round_winner.map(str::to_owned),
                draw: round_winner.is_none(),
                duration_ms: round_started_at.elapsed().as_millis() as u64,
            });
        }

        let winner_side = if team_a_round_wins > team_b_round_wins {
            Some("team_a")
        } else if team_b_round_wins > team_a_round_wins {
            Some("team_b")
        } else {
            compare_sides(
                total_team_a_objective,
                total_team_b_objective,
                total_team_a_team_score,
                total_team_b_team_score,
            )
        };

        let mut fighters = Vec::with_capacity(roster_a.len() + roster_b.len());
        for (slot, (model_id, aggregate)) in
            roster_a.iter().zip(aggregates_a.iter()).enumerate()
        {
            fighters.push(MixedTeamFighterOutcome {
                side: "team_a".to_owned(),
                slot: slot as u32,
                model_id: model_id.clone(),
                runtime: aggregate.runtime.clone().unwrap_or_else(|| "fallback".to_owned()),
                eliminations: aggregate.eliminations,
                deaths: aggregate.deaths,
                personal_score: aggregate.personal_score,
                collaboration_score: aggregate.collaboration_score,
                action_counts: aggregate.action_counts.clone(),
            });
        }
        for (slot, (model_id, aggregate)) in
            roster_b.iter().zip(aggregates_b.iter()).enumerate()
        {
            fighters.push(MixedTeamFighterOutcome {
                side: "team_b".to_owned(),
                slot: slot as u32,
                model_id: model_id.clone(),
                runtime: aggregate.runtime.clone().unwrap_or_else(|| "fallback".to_owned()),
                eliminations: aggregate.eliminations,
                deaths: aggregate.deaths,
                personal_score: aggregate.personal_score,
                collaboration_score: aggregate.collaboration_score,
                action_counts: aggregate.action_counts.clone(),
            });
        }

        MixedTeamBattleOutcome {
            mode: "mixed_team".to_owned(),
            match_mode: mode.as_str().to_owned(),
            rules_version: ARENA_SIMULATOR_RULES_VERSION.to_owned(),
            seed,
            team_a_models: roster_a,
            team_b_models: roster_b,
            team_size,
            rounds: normalized_rounds,
            max_ticks,
            team_a_round_wins,
            team_b_round_wins,
            round_draws,
            total_team_a_objective,
            total_team_b_objective,
            total_team_a_score,
            total_team_b_score,
            total_team_a_team_score,
            total_team_b_team_score,
            total_team_a_collaboration_score,
            total_team_b_collaboration_score,
            team_a_action_counts,
            team_b_action_counts,
            winner_side: winner_side.map(str::to_owned),
            draw: winner_side.is_none(),
            fighters,
            rounds_detail,
            fallback_count: fault_counts.fallback_count,
            trap_count: fault_counts.trap_count,
            invalid_action_count: fault_counts.invalid_action_count,
            fuel_error_count: fault_counts.fuel_error_count,
            warnings: all_warnings,
            duration_ms: started_at.elapsed().as_millis() as u64,
        }
    }

    /// Executes a genuine shared-world free-for-all. Every model controls one
    /// faction and all faction fighters observe and act in the same tick. The
    /// caller-facing service validates the entrant count and uniqueness; this
    /// layer still canonicalizes order so request order cannot affect results.
    pub fn execute_world_battle(
        &self,
        model_ids: &[String],
        squad_size: u32,
        rounds: u32,
        seed: u64,
        requested_ticks: Option<u32>,
    ) -> WorldBattleOutcome {
        let started_at = Instant::now();
        let mut canonical_ids: Vec<String> = model_ids
            .iter()
            .map(|model_id| model_id.trim())
            .filter(|model_id| sanitize_model_id(model_id).is_some())
            .map(ToOwned::to_owned)
            .collect();
        canonical_ids.sort();
        canonical_ids.dedup();
        canonical_ids.truncate(MAX_WORLD_BATTLE_ENTRANTS);

        let normalized_squad_size = squad_size.clamp(1, MAX_WORLD_SQUAD_SIZE);
        let normalized_rounds = rounds.clamp(1, MAX_WORLD_BATTLE_ROUNDS);
        let max_ticks = requested_ticks
            .unwrap_or(self.default_max_ticks)
            .clamp(1, MAX_WORLD_BATTLE_TICKS);
        let mut warnings = Vec::new();
        let mut totals: HashMap<String, WorldAggregate> = canonical_ids
            .iter()
            .map(|model_id| {
                (
                    model_id.clone(),
                    WorldAggregate {
                        model_id: model_id.clone(),
                        ..WorldAggregate::default()
                    },
                )
            })
            .collect();
        let mut rounds_detail = Vec::with_capacity(normalized_rounds as usize);
        let mut total_ticks_executed = 0u64;

        for round in 0..normalized_rounds {
            let round_seed = seed ^ ((round as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let result = self.simulate_world_round(
                &canonical_ids,
                normalized_squad_size,
                round_seed,
                max_ticks,
                round + 1,
                &mut warnings,
            );
            total_ticks_executed =
                total_ticks_executed.saturating_add(result.outcome.ticks_executed as u64);
            for round_total in result.aggregates {
                let Some(total) = totals.get_mut(&round_total.model_id) else {
                    continue;
                };
                total.points = total.points.saturating_add(round_total.points);
                total.round_wins = total.round_wins.saturating_add(round_total.round_wins);
                total.eliminations = total.eliminations.saturating_add(round_total.eliminations);
                total.deaths = total.deaths.saturating_add(round_total.deaths);
                total.personal_score = total
                    .personal_score
                    .saturating_add(round_total.personal_score);
                total.team_score = total.team_score.saturating_add(round_total.team_score);
                total.collaboration_score = total
                    .collaboration_score
                    .saturating_add(round_total.collaboration_score);
                total.fighters_alive_total = total
                    .fighters_alive_total
                    .saturating_add(round_total.fighters_alive_total);
                total.remaining_health_total = total
                    .remaining_health_total
                    .saturating_add(round_total.remaining_health_total);
                add_action_counts(&mut total.actions, &round_total.actions);
                total.v2_fighter_rounds = total
                    .v2_fighter_rounds
                    .saturating_add(round_total.v2_fighter_rounds);
                add_fault_counts(&mut total.faults, round_total.faults);
            }
            rounds_detail.push(result.outcome);
        }

        let mut rankings: Vec<WorldModelOutcome> = totals
            .into_values()
            .map(|total| {
                let expected_fighter_rounds =
                    normalized_squad_size as u64 * normalized_rounds as u64;
                let runtime = if total.v2_fighter_rounds == expected_fighter_rounds
                    && total.faults.fallback_count == 0
                {
                    "wasm_v2"
                } else if total.v2_fighter_rounds == 0 {
                    "fallback"
                } else {
                    "mixed"
                };
                WorldModelOutcome {
                    rank: 0,
                    model_id: total.model_id,
                    runtime: runtime.to_owned(),
                    points: total.points,
                    round_wins: total.round_wins,
                    eliminations: total.eliminations,
                    deaths: total.deaths,
                    personal_score: total.personal_score,
                    team_score: total.team_score,
                    collaboration_score: total.collaboration_score,
                    fighters_alive_total: total.fighters_alive_total,
                    remaining_health_total: total.remaining_health_total,
                    action_counts: total.actions,
                    v2_fighter_rounds: total.v2_fighter_rounds,
                    fallback_count: total.faults.fallback_count,
                    trap_count: total.faults.trap_count,
                    invalid_action_count: total.faults.invalid_action_count,
                    fuel_error_count: total.faults.fuel_error_count,
                }
            })
            .collect();
        rank_world_totals(&mut rankings);
        let first_place_count = rankings.iter().filter(|entry| entry.rank == 1).count();
        let draw = first_place_count != 1;
        let winner_model_id = (!draw)
            .then(|| rankings.first().map(|entry| entry.model_id.clone()))
            .flatten();

        WorldBattleOutcome {
            mode: "world_ffa".to_owned(),
            rules_version: ARENA_SIMULATOR_RULES_VERSION.to_owned(),
            seed,
            entrants: canonical_ids.len() as u32,
            squad_size: normalized_squad_size,
            rounds: normalized_rounds,
            max_ticks,
            total_ticks_executed,
            winner_model_id,
            draw,
            rankings,
            rounds_detail,
            warnings,
            duration_ms: started_at.elapsed().as_millis() as u64,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn simulate_world_round(
        &self,
        model_ids: &[String],
        squad_size: u32,
        seed: u64,
        max_ticks: u32,
        round: u32,
        warnings: &mut Vec<String>,
    ) -> WorldRoundResult {
        let mut factions: Vec<WorldFactionState> = model_ids
            .iter()
            .enumerate()
            .map(|(faction_index, model_id)| {
                let program = self.load_program(model_id);
                let fighters: Vec<TeamFighter> = (0..squad_size)
                    .map(|slot| {
                        let runtime_seed = seed
                            ^ ((faction_index as u64 + 1).wrapping_mul(0xD6E8_FEB8_6659_FD93))
                            ^ ((slot as u64 + 1).wrapping_mul(0xA076_1D64_78BD_642F));
                        TeamFighter {
                            state: FighterState {
                                health: 100,
                                score: 0,
                                respawns_remaining: 0,
                            },
                            runtime: self.build_v2_runtime(program.clone(), runtime_seed, warnings),
                            collaboration_score: 0,
                        }
                    })
                    .collect();
                let v2_fighters = fighters
                    .iter()
                    .filter(|fighter| fighter.runtime.uses_v2())
                    .count() as u32;
                let fallback_count = fighters
                    .iter()
                    .filter(|fighter| fighter.runtime.is_fallback())
                    .count() as u64;
                WorldFactionState {
                    model_id: model_id.clone(),
                    fighters,
                    eliminations: 0,
                    deaths: 0,
                    team_score: 0,
                    actions: TeamActionCounts::default(),
                    faults: BotFaultCounts {
                        fallback_count,
                        ..BotFaultCounts::default()
                    },
                    v2_fighters,
                }
            })
            .collect();

        let mut ticks_executed = 0u32;
        for tick in 0..max_ticks {
            if living_world_faction_count(&factions) <= 1 {
                break;
            }
            ticks_executed = tick + 1;

            let targets: Vec<Vec<Option<(usize, usize)>>> = factions
                .iter()
                .enumerate()
                .map(|(faction_index, faction)| {
                    (0..faction.fighters.len())
                        .map(|slot| select_world_target(&factions, faction_index, slot, seed, tick))
                        .collect()
                })
                .collect();
            let observations: Vec<Vec<BotObservation>> = factions
                .iter()
                .enumerate()
                .map(|(faction_index, faction)| {
                    let allies_alive = living_count(&faction.fighters) as i32;
                    let enemies_alive = factions
                        .iter()
                        .enumerate()
                        .filter(|(other, _)| *other != faction_index)
                        .map(|(_, other)| living_count(&other.fighters) as i32)
                        .sum();
                    let strongest_opponent_score = factions
                        .iter()
                        .enumerate()
                        .filter(|(other, _)| *other != faction_index)
                        .map(|(_, other)| other.team_score)
                        .max()
                        .unwrap_or(0);
                    let leading_opponent_eliminations = factions
                        .iter()
                        .enumerate()
                        .filter(|(other, _)| *other != faction_index)
                        .map(|(_, other)| other.eliminations)
                        .max()
                        .unwrap_or(0);
                    faction
                        .fighters
                        .iter()
                        .enumerate()
                        .map(|(slot, fighter)| {
                            let target_health = targets[faction_index][slot]
                                .map(|(target_faction, target_slot)| {
                                    factions[target_faction].fighters[target_slot].state.health
                                })
                                .unwrap_or(0);
                            BotObservation {
                                self_health: fighter.state.health,
                                target_health,
                                personal_score: fighter.state.score,
                                team_score_delta: saturating_i64_to_i32(
                                    faction.team_score - strongest_opponent_score,
                                ),
                                objective_delta: saturating_i64_to_i32(
                                    faction.eliminations as i64
                                        - leading_opponent_eliminations as i64,
                                ),
                                allies_alive,
                                enemies_alive,
                                lowest_ally_health: lowest_living_ally_health(
                                    &faction.fighters,
                                    slot,
                                ),
                                slot: slot as i32,
                                mode: ArenaMatchMode::TeamDeathmatch,
                                tick,
                            }
                        })
                        .collect()
                })
                .collect();

            let mut actions: Vec<Vec<BotAction>> = Vec::with_capacity(factions.len());
            for (faction_index, faction) in factions.iter_mut().enumerate() {
                let mut faction_actions = Vec::with_capacity(faction.fighters.len());
                for (slot, fighter) in faction.fighters.iter_mut().enumerate() {
                    let action = if fighter.state.health > 0 {
                        fighter.runtime.next_action(
                            self.fuel_per_tick,
                            observations[faction_index][slot],
                            warnings,
                            &mut faction.faults,
                            &format!("world model='{}' slot={}", faction.model_id, slot),
                        )
                    } else {
                        BotAction::Idle
                    };
                    if fighter.state.health > 0 {
                        action.record(&mut faction.actions);
                    }
                    faction_actions.push(action);
                }
                actions.push(faction_actions);
            }

            let health_before: Vec<Vec<i32>> = factions
                .iter()
                .map(|faction| {
                    faction
                        .fighters
                        .iter()
                        .map(|fighter| fighter.state.health)
                        .collect()
                })
                .collect();
            let support_maps: Vec<Vec<Vec<usize>>> = factions
                .iter()
                .zip(actions.iter())
                .map(|(faction, faction_actions)| {
                    support_targets(&faction.fighters, faction_actions)
                })
                .collect();
            for (faction, faction_actions) in factions.iter_mut().zip(actions.iter()) {
                apply_charge_costs(&mut faction.fighters, faction_actions);
            }

            let mut incoming: Vec<Vec<Vec<WorldIncomingHit>>> = factions
                .iter()
                .map(|faction| vec![Vec::new(); faction.fighters.len()])
                .collect();
            for (attacker_faction, faction_actions) in actions.iter().enumerate() {
                for (attacker_slot, action) in faction_actions.iter().copied().enumerate() {
                    if health_before[attacker_faction][attacker_slot] <= 0 {
                        continue;
                    }
                    let Some((target_faction, target_slot)) =
                        targets[attacker_faction][attacker_slot]
                    else {
                        continue;
                    };
                    let damage = outgoing_damage(
                        action,
                        seed ^ ((attacker_faction as u64 + 1).wrapping_mul(0xD6E8_FEB8_6659_FD93))
                            ^ ((attacker_slot as u64 + 1).wrapping_mul(0xA076_1D64_78BD_642F)),
                        tick,
                    );
                    if damage > 0 {
                        incoming[target_faction][target_slot].push(WorldIncomingHit {
                            attacker_faction,
                            attacker_slot,
                            damage,
                        });
                    }
                }
            }

            let resolutions =
                build_world_damage_resolutions(&factions, &actions, &support_maps, &incoming);
            apply_world_damage_resolutions(&mut factions, resolutions);

            for faction_index in 0..factions.len() {
                for slot in 0..factions[faction_index].fighters.len() {
                    if health_before[faction_index][slot] > 0
                        && factions[faction_index].fighters[slot].state.health <= 0
                    {
                        factions[faction_index].deaths =
                            factions[faction_index].deaths.saturating_add(1);
                        factions[faction_index].fighters[slot].state.score =
                            factions[faction_index].fighters[slot]
                                .state
                                .score
                                .saturating_sub(4);
                    }
                }
            }
        }

        let mut placements: Vec<WorldBattlePlacement> = factions
            .iter()
            .map(|faction| WorldBattlePlacement {
                rank: 0,
                model_id: faction.model_id.clone(),
                points: 0,
                eliminations: faction.eliminations,
                deaths: faction.deaths,
                personal_score: team_personal_score(&faction.fighters),
                team_score: faction.team_score,
                collaboration_score: team_collaboration_score(&faction.fighters),
                fighters_alive: living_count(&faction.fighters) as u32,
                remaining_health: faction
                    .fighters
                    .iter()
                    .map(|fighter| fighter.state.health.max(0) as i64)
                    .sum(),
            })
            .collect();
        rank_world_placements(&mut placements);
        let first_place_count = placements.iter().filter(|entry| entry.rank == 1).count();
        let draw = first_place_count != 1;
        let winner_model_id = (!draw)
            .then(|| placements.first().map(|entry| entry.model_id.clone()))
            .flatten();
        let placement_by_model: HashMap<&str, &WorldBattlePlacement> = placements
            .iter()
            .map(|placement| (placement.model_id.as_str(), placement))
            .collect();
        let aggregates = factions
            .into_iter()
            .map(|faction| {
                let placement = placement_by_model
                    .get(faction.model_id.as_str())
                    .expect("every world faction receives a placement");
                WorldAggregate {
                    model_id: faction.model_id,
                    points: placement.points as u64,
                    round_wins: u32::from(placement.rank == 1 && !draw),
                    eliminations: faction.eliminations,
                    deaths: faction.deaths,
                    personal_score: placement.personal_score,
                    team_score: faction.team_score,
                    collaboration_score: placement.collaboration_score,
                    fighters_alive_total: placement.fighters_alive as u64,
                    remaining_health_total: placement.remaining_health,
                    actions: faction.actions,
                    v2_fighter_rounds: faction.v2_fighters as u64,
                    faults: faction.faults,
                }
            })
            .collect();

        WorldRoundResult {
            outcome: WorldBattleRoundOutcome {
                round,
                seed,
                ticks_executed,
                winner_model_id,
                draw,
                placements,
            },
            aggregates,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn simulate_interacting_team_round(
        &self,
        model_a_id: &str,
        model_b_id: &str,
        mode: ArenaMatchMode,
        team_size: u32,
        seed: u64,
        max_ticks: u32,
        warnings: &mut Vec<String>,
    ) -> TeamRoundSimulation {
        let respawns = if matches!(mode, ArenaMatchMode::Arena) {
            0
        } else {
            DEFAULT_RESPAWNS_NON_ARENA
        };
        let team_a = self.build_team(model_a_id, team_size, respawns, seed, 0xA11A, warnings);
        let team_b = self.build_team(model_b_id, team_size, respawns, seed, 0xB22B, warnings);
        self.run_team_round(team_a, team_b, mode, team_size, seed, max_ticks, warnings)
    }

    /// Shared two-team round core: runs the tick loop over pre-built teams.
    /// The frozen same-model path reaches this through
    /// `simulate_interacting_team_round`; the mixed-team path builds one
    /// fighter per roster model and calls it directly. Besides the team
    /// aggregates it also records per-slot kills/deaths/actions so callers
    /// can attribute contributions to individual fighters.
    #[allow(clippy::too_many_arguments)]
    fn run_team_round(
        &self,
        mut team_a: Vec<TeamFighter>,
        mut team_b: Vec<TeamFighter>,
        mode: ArenaMatchMode,
        team_size: u32,
        seed: u64,
        max_ticks: u32,
        warnings: &mut Vec<String>,
    ) -> TeamRoundSimulation {
        let mut objectives = MatchObjectiveState::default();
        let mut team_score_a = 0i64;
        let mut team_score_b = 0i64;
        let mut actions_count_a = TeamActionCounts::default();
        let mut actions_count_b = TeamActionCounts::default();
        let mut faults_a = BotFaultCounts {
            fallback_count: team_a
                .iter()
                .filter(|fighter| fighter.runtime.is_fallback())
                .count() as u64,
            ..BotFaultCounts::default()
        };
        let mut faults_b = BotFaultCounts {
            fallback_count: team_b
                .iter()
                .filter(|fighter| fighter.runtime.is_fallback())
                .count() as u64,
            ..BotFaultCounts::default()
        };

        let v2_fighters_a = team_a
            .iter()
            .filter(|fighter| fighter.runtime.uses_v2())
            .count() as u32;
        let v2_fighters_b = team_b
            .iter()
            .filter(|fighter| fighter.runtime.uses_v2())
            .count() as u32;
        let mut fighter_stats_a = vec![FighterRoundStats::default(); team_a.len()];
        let mut fighter_stats_b = vec![FighterRoundStats::default(); team_b.len()];

        for tick in 0..max_ticks {
            let alive_a = living_count(&team_a);
            let alive_b = living_count(&team_b);
            if alive_a == 0 || alive_b == 0 {
                break;
            }

            let (objective_a, objective_b) =
                team_objective_values(mode, &objectives, &team_a, &team_b);
            let targets_a: Vec<Option<usize>> = (0..team_a.len())
                .map(|slot| select_living_target(&team_b, slot))
                .collect();
            let targets_b: Vec<Option<usize>> = (0..team_b.len())
                .map(|slot| select_living_target(&team_a, slot))
                .collect();

            let observations_a: Vec<BotObservation> = team_a
                .iter()
                .enumerate()
                .map(|(slot, fighter)| BotObservation {
                    self_health: fighter.state.health,
                    target_health: targets_a[slot]
                        .map(|idx| team_b[idx].state.health)
                        .unwrap_or(0),
                    personal_score: fighter.state.score,
                    team_score_delta: saturating_i64_to_i32(team_score_a - team_score_b),
                    objective_delta: objective_a.saturating_sub(objective_b),
                    allies_alive: alive_a as i32,
                    enemies_alive: alive_b as i32,
                    lowest_ally_health: lowest_living_ally_health(&team_a, slot),
                    slot: slot as i32,
                    mode,
                    tick,
                })
                .collect();
            let observations_b: Vec<BotObservation> = team_b
                .iter()
                .enumerate()
                .map(|(slot, fighter)| BotObservation {
                    self_health: fighter.state.health,
                    target_health: targets_b[slot]
                        .map(|idx| team_a[idx].state.health)
                        .unwrap_or(0),
                    personal_score: fighter.state.score,
                    team_score_delta: saturating_i64_to_i32(team_score_b - team_score_a),
                    objective_delta: objective_b.saturating_sub(objective_a),
                    allies_alive: alive_b as i32,
                    enemies_alive: alive_a as i32,
                    lowest_ally_health: lowest_living_ally_health(&team_b, slot),
                    slot: slot as i32,
                    mode,
                    tick,
                })
                .collect();

            let mut actions_a = Vec::with_capacity(team_a.len());
            for (slot, fighter) in team_a.iter_mut().enumerate() {
                let action = if fighter.state.health > 0 {
                    fighter.runtime.next_action(
                        self.fuel_per_tick,
                        observations_a[slot],
                        warnings,
                        &mut faults_a,
                        &format!("team_a slot={}", slot),
                    )
                } else {
                    BotAction::Idle
                };
                if fighter.state.health > 0 {
                    action.record(&mut actions_count_a);
                    action.record(&mut fighter_stats_a[slot].actions);
                }
                actions_a.push(action);
            }
            let mut actions_b = Vec::with_capacity(team_b.len());
            for (slot, fighter) in team_b.iter_mut().enumerate() {
                let action = if fighter.state.health > 0 {
                    fighter.runtime.next_action(
                        self.fuel_per_tick,
                        observations_b[slot],
                        warnings,
                        &mut faults_b,
                        &format!("team_b slot={}", slot),
                    )
                } else {
                    BotAction::Idle
                };
                if fighter.state.health > 0 {
                    action.record(&mut actions_count_b);
                    action.record(&mut fighter_stats_b[slot].actions);
                }
                actions_b.push(action);
            }

            let health_before_a: Vec<i32> = team_a.iter().map(|f| f.state.health).collect();
            let health_before_b: Vec<i32> = team_b.iter().map(|f| f.state.health).collect();
            let support_targets_a = support_targets(&team_a, &actions_a);
            let support_targets_b = support_targets(&team_b, &actions_b);
            apply_charge_costs(&mut team_a, &actions_a);
            apply_charge_costs(&mut team_b, &actions_b);

            let kills_by_a = apply_team_attacks(
                &mut team_a,
                &mut team_b,
                &actions_a,
                &actions_b,
                &targets_a,
                &support_targets_b,
                seed,
                tick,
            );
            let kills_by_b = apply_team_attacks(
                &mut team_b,
                &mut team_a,
                &actions_b,
                &actions_a,
                &targets_b,
                &support_targets_a,
                seed,
                tick,
            );
            for (killer_slot, _) in kills_by_a {
                if let Some(stats) = fighter_stats_a.get_mut(killer_slot) {
                    stats.kills = stats.kills.saturating_add(1);
                }
            }
            for (killer_slot, _) in kills_by_b {
                if let Some(stats) = fighter_stats_b.get_mut(killer_slot) {
                    stats.kills = stats.kills.saturating_add(1);
                }
            }

            let eliminated_a = eliminated_indices(&health_before_a, &team_a);
            let eliminated_b = eliminated_indices(&health_before_b, &team_b);
            for slot in eliminated_a.iter().copied() {
                if let Some(stats) = fighter_stats_a.get_mut(slot) {
                    stats.deaths = stats.deaths.saturating_add(1);
                }
            }
            for slot in eliminated_b.iter().copied() {
                if let Some(stats) = fighter_stats_b.get_mut(slot) {
                    stats.deaths = stats.deaths.saturating_add(1);
                }
            }
            if !eliminated_b.is_empty() {
                objectives.tdm_elims_a = objectives
                    .tdm_elims_a
                    .saturating_add(eliminated_b.len() as i32);
                team_score_a = team_score_a.saturating_add(40 * eliminated_b.len() as i64);
            }
            if !eliminated_a.is_empty() {
                objectives.tdm_elims_b = objectives
                    .tdm_elims_b
                    .saturating_add(eliminated_a.len() as i32);
                team_score_b = team_score_b.saturating_add(40 * eliminated_a.len() as i64);
            }

            apply_team_mode_objectives(
                mode,
                &actions_a,
                &actions_b,
                &mut objectives,
                team_size,
                tick,
                &mut team_score_a,
                &mut team_score_b,
            );
            respawn_team_fighters(mode, &mut team_a, &eliminated_a);
            respawn_team_fighters(mode, &mut team_b, &eliminated_b);

            if team_round_should_end(mode, &objectives, &team_a, &team_b, team_size) {
                break;
            }
        }

        let (objective_a, objective_b) = team_objective_values(mode, &objectives, &team_a, &team_b);
        let draws = team_a
            .iter()
            .zip(team_b.iter())
            .filter(|(a, b)| (a.state.health > 0) == (b.state.health > 0))
            .count() as u32;
        for (slot, fighter) in team_a.iter().enumerate() {
            fighter_stats_a[slot].personal_score = i64::from(fighter.state.score);
            fighter_stats_a[slot].collaboration_score = i64::from(fighter.collaboration_score);
        }
        for (slot, fighter) in team_b.iter().enumerate() {
            fighter_stats_b[slot].personal_score = i64::from(fighter.state.score);
            fighter_stats_b[slot].collaboration_score = i64::from(fighter.collaboration_score);
        }

        TeamRoundSimulation {
            draws,
            objective_a: objective_a as i64,
            objective_b: objective_b as i64,
            personal_a: team_personal_score(&team_a),
            personal_b: team_personal_score(&team_b),
            team_score_a,
            team_score_b,
            collaboration_a: team_collaboration_score(&team_a),
            collaboration_b: team_collaboration_score(&team_b),
            actions_a: actions_count_a,
            actions_b: actions_count_b,
            v2_fighters_a,
            v2_fighters_b,
            faults_a,
            faults_b,
            fighter_stats_a,
            fighter_stats_b,
        }
    }

    fn build_team(
        &self,
        model_id: &str,
        team_size: u32,
        respawns: i32,
        seed: u64,
        team_salt: u64,
        warnings: &mut Vec<String>,
    ) -> Vec<TeamFighter> {
        let program = self.load_program(model_id);
        (0..team_size)
            .map(|slot| {
                let runtime_seed =
                    seed ^ team_salt ^ ((slot as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9));
                TeamFighter {
                    state: FighterState {
                        health: 100,
                        score: 0,
                        respawns_remaining: respawns,
                    },
                    runtime: self.build_runtime(program.clone(), runtime_seed, warnings),
                    collaboration_score: 0,
                }
            })
            .collect()
    }

    /// Builds a heterogeneous squad: fighter `slot` runs `model_ids[slot]`'s
    /// program. Uses the same runtime policy and per-slot seeding as
    /// `build_team`, so a mixed battle plays by the exact same rules as the
    /// frozen same-model team battles.
    fn build_mixed_team(
        &self,
        model_ids: &[String],
        respawns: i32,
        seed: u64,
        team_salt: u64,
        warnings: &mut Vec<String>,
    ) -> Vec<TeamFighter> {
        model_ids
            .iter()
            .enumerate()
            .map(|(slot, model_id)| {
                let program = self.load_program(model_id);
                let runtime_seed =
                    seed ^ team_salt ^ ((slot as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9));
                TeamFighter {
                    state: FighterState {
                        health: 100,
                        score: 0,
                        respawns_remaining: respawns,
                    },
                    runtime: self.build_runtime(program, runtime_seed, warnings),
                    collaboration_score: 0,
                }
            })
            .collect()
    }

    fn build_runtime(
        &self,
        program: BotProgram,
        fallback_seed: u64,
        warnings: &mut Vec<String>,
    ) -> BotRuntime {
        self.build_runtime_with_policy(program, fallback_seed, warnings, false, false)
    }

    fn build_v2_runtime(
        &self,
        program: BotProgram,
        fallback_seed: u64,
        warnings: &mut Vec<String>,
    ) -> BotRuntime {
        self.build_runtime_with_policy(program, fallback_seed, warnings, true, false)
    }

    fn build_runtime_with_policy(
        &self,
        program: BotProgram,
        fallback_seed: u64,
        warnings: &mut Vec<String>,
        require_v2: bool,
        strict_resource_limits: bool,
    ) -> BotRuntime {
        match program {
            BotProgram::Wasm {
                module,
                source_path,
            } => {
                let limits = if strict_resource_limits {
                    StoreLimitsBuilder::new()
                        .memory_size(EXHIBITION_MAX_LINEAR_MEMORY_BYTES)
                        .table_elements(EXHIBITION_MAX_TABLE_ELEMENTS)
                        .instances(EXHIBITION_MAX_INSTANCES)
                        .tables(EXHIBITION_MAX_TABLES)
                        .memories(EXHIBITION_MAX_MEMORIES)
                        .trap_on_grow_failure(true)
                        .build()
                } else {
                    StoreLimitsBuilder::new().build()
                };
                let mut store = Store::new(module.engine(), BotStoreState { limits });
                if strict_resource_limits {
                    store.limiter(|state| &mut state.limits);
                }
                if let Err(err) = store.set_fuel(self.fuel_per_tick) {
                    push_bot_warning(
                        warnings,
                        format!(
                            "wasm store fuel init failed for '{}': {}. using fallback runtime",
                            source_path.display(),
                            err
                        ),
                    );
                    return BotRuntime::Fallback {
                        prng_state: fallback_seed,
                    };
                }

                match wasmtime::Instance::new(&mut store, &module, &[]) {
                    Ok(instance) => {
                        let v2 = instance
                            .get_typed_func::<BotTickV2Params, i32>(&mut store, BOT_TICK_V2_EXPORT);
                        if let Ok(tick_fn) = v2 {
                            BotRuntime::Wasm {
                                store,
                                tick_fn: BotTickFunction::V2(tick_fn),
                            }
                        } else if require_v2 {
                            push_bot_warning(warnings, format!(
                                "missing/invalid required '{}' export in '{}'; using fallback runtime",
                                BOT_TICK_V2_EXPORT,
                                source_path.display()
                            ));
                            BotRuntime::Fallback {
                                prng_state: fallback_seed,
                            }
                        } else {
                            match instance.get_typed_func::<(i32, i32, i32, i32), i32>(
                                &mut store,
                                BOT_TICK_EXPORT,
                            ) {
                                Ok(tick_fn) => BotRuntime::Wasm {
                                    store,
                                    tick_fn: BotTickFunction::V1(tick_fn),
                                },
                                Err(err) => {
                                    push_bot_warning(warnings, format!(
                                        "missing/invalid '{}' or '{}' export in '{}': {}. using fallback runtime",
                                        BOT_TICK_V2_EXPORT,
                                        BOT_TICK_EXPORT,
                                        source_path.display(),
                                        err
                                    ));
                                    BotRuntime::Fallback {
                                        prng_state: fallback_seed,
                                    }
                                }
                            }
                        }
                    }
                    Err(err) => {
                        push_bot_warning(
                            warnings,
                            format!(
                                "wasm instantiate failed for '{}': {}. using fallback runtime",
                                source_path.display(),
                                err
                            ),
                        );
                        BotRuntime::Fallback {
                            prng_state: fallback_seed,
                        }
                    }
                }
            }
            BotProgram::Fallback { reason } => {
                push_bot_warning(warnings, reason);
                BotRuntime::Fallback {
                    prng_state: fallback_seed,
                }
            }
        }
    }

    fn load_program(&self, model_id: &str) -> BotProgram {
        self.load_program_with_expected_artifact(model_id, None)
    }

    fn load_program_with_artifact_binding(
        &self,
        model_id: &str,
        expected_wasm_bytes: usize,
        expected_wasm_sha256: &str,
    ) -> BotProgram {
        if expected_wasm_bytes == 0 || expected_wasm_bytes > MAX_PUBLISHED_WASM_BYTES {
            return BotProgram::Fallback {
                reason: format!(
                    "published wasm size is invalid for model '{}'; fallback runtime used",
                    model_id
                ),
            };
        }
        if !is_lowercase_sha256(expected_wasm_sha256) {
            return BotProgram::Fallback {
                reason: format!(
                    "published wasm digest is invalid for model '{}'; fallback runtime used",
                    model_id
                ),
            };
        }
        self.load_program_with_expected_artifact(
            model_id,
            Some((expected_wasm_bytes, expected_wasm_sha256)),
        )
    }

    fn load_program_with_expected_artifact(
        &self,
        model_id: &str,
        expected_artifact: Option<(usize, &str)>,
    ) -> BotProgram {
        if self.engine.is_none() {
            return BotProgram::Fallback {
                reason:
                    "wasm runtime unavailable because wasmtime fuel metering could not initialize"
                        .to_owned(),
            };
        }

        let Some(safe_model_id) = sanitize_model_id(model_id) else {
            return BotProgram::Fallback {
                reason: format!(
                    "model '{}' has invalid id format; fallback runtime used",
                    model_id
                ),
            };
        };

        let path = self.wasm_dir.join(format!("{}.wasm", safe_model_id));
        if !path.exists() {
            self.module_cache.write().remove(&path);
            return BotProgram::Fallback {
                reason: format!(
                    "wasm not found for model '{}': expected '{}'; fallback runtime used",
                    model_id,
                    path.display()
                ),
            };
        }

        if let Some((expected_bytes, _)) = expected_artifact {
            let actual_bytes = match fs::metadata(&path) {
                Ok(metadata) if metadata.is_file() => usize::try_from(metadata.len()).ok(),
                _ => None,
            };
            if actual_bytes != Some(expected_bytes) {
                self.module_cache.write().remove(&path);
                return BotProgram::Fallback {
                    reason: format!(
                        "published wasm size mismatch for model '{}'; fallback runtime used",
                        model_id
                    ),
                };
            }
        }

        let bytes_result = if let Some((expected_bytes, _)) = expected_artifact {
            fs::File::open(&path).and_then(|file| {
                let mut bytes = Vec::with_capacity(expected_bytes);
                file.take(expected_bytes as u64 + 1)
                    .read_to_end(&mut bytes)?;
                Ok(bytes)
            })
        } else {
            fs::read(&path)
        };
        match bytes_result {
            Ok(bytes) => {
                let content_sha256: [u8; 32] = Sha256::digest(&bytes).into();
                if let Some((expected_bytes, expected_sha256)) = expected_artifact {
                    if bytes.len() != expected_bytes {
                        self.module_cache.write().remove(&path);
                        return BotProgram::Fallback {
                            reason: format!(
                                "published wasm size mismatch for model '{}'; fallback runtime used",
                                model_id
                            ),
                        };
                    }
                    let actual_sha256 = sha256_hex(&bytes);
                    if actual_sha256 != expected_sha256 {
                        self.module_cache.write().remove(&path);
                        return BotProgram::Fallback {
                            reason: format!(
                                "published wasm digest mismatch for model '{}'; fallback runtime used",
                                model_id
                            ),
                        };
                    }
                }
                if let Some(cached) = self.module_cache.read().get(&path) {
                    if cached.content_sha256 == content_sha256 {
                        return BotProgram::Wasm {
                            module: cached.module.clone(),
                            source_path: path,
                        };
                    }
                }

                let engine = self
                    .engine
                    .as_ref()
                    .expect("engine checked above for load_program");
                match Module::from_binary(engine, &bytes) {
                    Ok(module) => match validate_bot_tick_export(&module) {
                        Ok(()) => {
                            let mut cache = self.module_cache.write();
                            if !cache.contains_key(&path)
                                && cache.len() >= MAX_COMPILED_MODULE_CACHE_ENTRIES
                            {
                                // The cache is a performance optimization, so a
                                // deterministic bounded eviction is sufficient
                                // and avoids attacker-controlled memory growth.
                                if let Some(eviction_key) = cache.keys().min().cloned() {
                                    cache.remove(&eviction_key);
                                }
                            }
                            cache.insert(
                                path.clone(),
                                CachedModule {
                                    content_sha256,
                                    module: module.clone(),
                                },
                            );
                            drop(cache);
                            BotProgram::Wasm {
                                module,
                                source_path: path,
                            }
                        }
                        Err(err) => {
                            self.module_cache.write().remove(&path);
                            warn!(
                                "Bot sandbox validation failed for '{}': {}",
                                path.display(),
                                err
                            );
                            BotProgram::Fallback {
                                reason: format!(
                                    "wasm validation failed for model '{}': {}; fallback runtime used",
                                    model_id, err
                                ),
                            }
                        }
                    },
                    Err(err) => {
                        self.module_cache.write().remove(&path);
                        BotProgram::Fallback {
                            reason: format!(
                                "failed to compile wasm for model '{}': {}; fallback runtime used",
                                model_id, err
                            ),
                        }
                    }
                }
            }
            Err(err) => {
                self.module_cache.write().remove(&path);
                BotProgram::Fallback {
                    reason: format!(
                        "failed to read wasm for model '{}': {}; fallback runtime used",
                        model_id, err
                    ),
                }
            }
        }
    }
}

fn is_lowercase_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn add_action_counts(total: &mut TeamActionCounts, round: &TeamActionCounts) {
    total.idle = total.idle.saturating_add(round.idle);
    total.attack = total.attack.saturating_add(round.attack);
    total.defend = total.defend.saturating_add(round.defend);
    total.charge = total.charge.saturating_add(round.charge);
    total.support = total.support.saturating_add(round.support);
}

fn add_fault_counts(total: &mut BotFaultCounts, round: BotFaultCounts) {
    total.fallback_count = total.fallback_count.saturating_add(round.fallback_count);
    total.trap_count = total.trap_count.saturating_add(round.trap_count);
    total.invalid_action_count = total
        .invalid_action_count
        .saturating_add(round.invalid_action_count);
    total.fuel_error_count = total
        .fuel_error_count
        .saturating_add(round.fuel_error_count);
}

fn living_world_faction_count(factions: &[WorldFactionState]) -> usize {
    factions
        .iter()
        .filter(|faction| living_count(&faction.fighters) > 0)
        .count()
}

fn select_world_target(
    factions: &[WorldFactionState],
    attacker_faction: usize,
    attacker_slot: usize,
    seed: u64,
    tick: u32,
) -> Option<(usize, usize)> {
    if factions
        .get(attacker_faction)?
        .fighters
        .get(attacker_slot)?
        .state
        .health
        <= 0
    {
        return None;
    }
    let candidates: Vec<(usize, usize)> = factions
        .iter()
        .enumerate()
        .filter(|(faction_index, _)| *faction_index != attacker_faction)
        .flat_map(|(faction_index, faction)| {
            faction
                .fighters
                .iter()
                .enumerate()
                .filter(|(_, fighter)| fighter.state.health > 0)
                .map(move |(slot, _)| (faction_index, slot))
        })
        .collect();
    if candidates.is_empty() {
        return None;
    }
    let mixed = next_prng(
        seed ^ ((tick as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15))
            ^ ((attacker_faction as u64 + 1).wrapping_mul(0xD6E8_FEB8_6659_FD93))
            ^ ((attacker_slot as u64 + 1).wrapping_mul(0xA076_1D64_78BD_642F)),
    );
    candidates.get(mixed as usize % candidates.len()).copied()
}

fn build_world_damage_resolutions(
    factions: &[WorldFactionState],
    actions: &[Vec<BotAction>],
    support_maps: &[Vec<Vec<usize>>],
    incoming: &[Vec<Vec<WorldIncomingHit>>],
) -> Vec<WorldDamageResolution> {
    let mut resolutions = Vec::new();
    for (target_faction, faction_incoming) in incoming.iter().enumerate() {
        for (target_slot, hits) in faction_incoming.iter().enumerate() {
            let health = factions[target_faction].fighters[target_slot]
                .state
                .health
                .max(0);
            if health == 0 || hits.is_empty() {
                continue;
            }
            let defended = actions[target_faction].get(target_slot) == Some(&BotAction::Defend);
            let supporters = support_maps[target_faction]
                .get(target_slot)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let mitigated: Vec<WorldIncomingHit> = hits
                .iter()
                .map(|hit| WorldIncomingHit {
                    damage: if defended {
                        scale_damage(hit.damage, 40)
                    } else {
                        hit.damage
                    },
                    ..*hit
                })
                .collect();
            let before_support: i32 = mitigated.iter().map(|hit| hit.damage.max(0)).sum();
            let after_support: Vec<WorldIncomingHit> = mitigated
                .iter()
                .map(|hit| WorldIncomingHit {
                    damage: if supporters.is_empty() {
                        hit.damage
                    } else {
                        scale_damage(hit.damage, TEAM_SUPPORT_DAMAGE_PERCENT)
                    },
                    ..*hit
                })
                .collect();
            let after_support_total: i32 = after_support.iter().map(|hit| hit.damage.max(0)).sum();
            let prevented = before_support
                .min(health)
                .saturating_sub(after_support_total.min(health));
            let support_credits = split_integer_credit(supporters, prevented);
            let effective_total = after_support_total.min(health);
            let effective_damage = proportional_effective_damage(&after_support, effective_total);
            let effective_hits: Vec<(usize, usize, i32)> = after_support
                .iter()
                .zip(effective_damage)
                .filter_map(|(hit, effective)| {
                    (effective > 0).then_some((hit.attacker_faction, hit.attacker_slot, effective))
                })
                .collect();
            let killer = if effective_total >= health {
                effective_hits
                    .iter()
                    .max_by(|left, right| {
                        left.2
                            .cmp(&right.2)
                            .then_with(|| right.0.cmp(&left.0))
                            .then_with(|| right.1.cmp(&left.1))
                    })
                    .map(|(faction, slot, _)| (*faction, *slot))
            } else {
                None
            };
            resolutions.push(WorldDamageResolution {
                target_faction,
                target_slot,
                damage: effective_total,
                effective_hits,
                support_credits,
                killer,
            });
        }
    }
    resolutions
}

fn proportional_effective_damage(hits: &[WorldIncomingHit], effective_total: i32) -> Vec<i32> {
    let total_damage: i32 = hits.iter().map(|hit| hit.damage.max(0)).sum();
    if total_damage <= 0 || effective_total <= 0 {
        return vec![0; hits.len()];
    }
    let mut allocated: Vec<i32> = hits
        .iter()
        .map(|hit| hit.damage.max(0).saturating_mul(effective_total) / total_damage)
        .collect();
    let allocated_total: i32 = allocated.iter().sum();
    let mut remainder = effective_total.saturating_sub(allocated_total);
    let mut order: Vec<usize> = (0..hits.len()).collect();
    order.sort_by(|left, right| {
        let left_remainder =
            hits[*left].damage.max(0).saturating_mul(effective_total) % total_damage;
        let right_remainder =
            hits[*right].damage.max(0).saturating_mul(effective_total) % total_damage;
        right_remainder
            .cmp(&left_remainder)
            .then_with(|| {
                hits[*left]
                    .attacker_faction
                    .cmp(&hits[*right].attacker_faction)
            })
            .then_with(|| hits[*left].attacker_slot.cmp(&hits[*right].attacker_slot))
    });
    for index in order {
        if remainder == 0 {
            break;
        }
        allocated[index] = allocated[index].saturating_add(1);
        remainder -= 1;
    }
    allocated
}

fn split_integer_credit(supporters: &[usize], credit: i32) -> Vec<(usize, i32)> {
    if credit <= 0 || supporters.is_empty() {
        return Vec::new();
    }
    let count = supporters.len() as i32;
    let base = credit / count;
    let remainder = credit % count;
    supporters
        .iter()
        .copied()
        .enumerate()
        .map(|(position, slot)| (slot, base + i32::from((position as i32) < remainder)))
        .collect()
}

fn apply_world_damage_resolutions(
    factions: &mut [WorldFactionState],
    resolutions: Vec<WorldDamageResolution>,
) {
    for resolution in resolutions {
        let target = &mut factions[resolution.target_faction].fighters[resolution.target_slot];
        target.state.health = target.state.health.saturating_sub(resolution.damage);

        for (supporter_slot, credit) in resolution.support_credits {
            let supporter = &mut factions[resolution.target_faction].fighters[supporter_slot];
            supporter.collaboration_score = supporter.collaboration_score.saturating_add(credit);
        }
        for (attacker_faction, attacker_slot, effective) in &resolution.effective_hits {
            let attacker = &mut factions[*attacker_faction].fighters[*attacker_slot];
            attacker.state.score = attacker
                .state
                .score
                .saturating_add(2 + effective.saturating_div(3));
        }
        let Some((killer_faction, killer_slot)) = resolution.killer else {
            continue;
        };
        factions[killer_faction].fighters[killer_slot].state.score = factions[killer_faction]
            .fighters[killer_slot]
            .state
            .score
            .saturating_add(40);
        factions[killer_faction].eliminations =
            factions[killer_faction].eliminations.saturating_add(1);
        factions[killer_faction].team_score =
            factions[killer_faction].team_score.saturating_add(40);
        for (attacker_faction, attacker_slot, _) in resolution.effective_hits {
            if (attacker_faction, attacker_slot) == (killer_faction, killer_slot) {
                continue;
            }
            // World factions are rivals: only a same-faction contributor can
            // earn collaboration credit for helping its teammate secure a KO.
            if attacker_faction != killer_faction {
                continue;
            }
            let assistant = &mut factions[attacker_faction].fighters[attacker_slot];
            assistant.collaboration_score = assistant
                .collaboration_score
                .saturating_add(TEAM_ASSIST_SCORE);
        }
    }
}

fn compare_world_placement_performance(
    left: &WorldBattlePlacement,
    right: &WorldBattlePlacement,
) -> Ordering {
    right
        .fighters_alive
        .cmp(&left.fighters_alive)
        .then_with(|| right.eliminations.cmp(&left.eliminations))
        .then_with(|| right.remaining_health.cmp(&left.remaining_health))
        .then_with(|| right.team_score.cmp(&left.team_score))
        .then_with(|| right.personal_score.cmp(&left.personal_score))
        .then_with(|| right.collaboration_score.cmp(&left.collaboration_score))
        .then_with(|| left.deaths.cmp(&right.deaths))
}

fn rank_world_placements(placements: &mut [WorldBattlePlacement]) {
    placements.sort_by(|left, right| {
        compare_world_placement_performance(left, right)
            .then_with(|| left.model_id.cmp(&right.model_id))
    });
    let mut rank = 1u32;
    for index in 0..placements.len() {
        if index > 0
            && compare_world_placement_performance(&placements[index - 1], &placements[index])
                != Ordering::Equal
        {
            rank = index as u32 + 1;
        }
        placements[index].rank = rank;
        placements[index].points = WORLD_PLACEMENT_POINTS
            .get(rank.saturating_sub(1) as usize)
            .copied()
            .unwrap_or(0);
    }
}

fn compare_world_total_performance(
    left: &WorldModelOutcome,
    right: &WorldModelOutcome,
) -> Ordering {
    right
        .points
        .cmp(&left.points)
        .then_with(|| right.round_wins.cmp(&left.round_wins))
        .then_with(|| right.eliminations.cmp(&left.eliminations))
        .then_with(|| right.fighters_alive_total.cmp(&left.fighters_alive_total))
        .then_with(|| {
            right
                .remaining_health_total
                .cmp(&left.remaining_health_total)
        })
        .then_with(|| right.team_score.cmp(&left.team_score))
        .then_with(|| right.personal_score.cmp(&left.personal_score))
        .then_with(|| right.collaboration_score.cmp(&left.collaboration_score))
        .then_with(|| left.deaths.cmp(&right.deaths))
}

fn rank_world_totals(rankings: &mut [WorldModelOutcome]) {
    rankings.sort_by(|left, right| {
        compare_world_total_performance(left, right)
            .then_with(|| left.model_id.cmp(&right.model_id))
    });
    let mut rank = 1u32;
    for index in 0..rankings.len() {
        if index > 0
            && compare_world_total_performance(&rankings[index - 1], &rankings[index])
                != Ordering::Equal
        {
            rank = index as u32 + 1;
        }
        rankings[index].rank = rank;
    }
}

fn living_count(team: &[TeamFighter]) -> usize {
    team.iter()
        .filter(|fighter| fighter.state.health > 0)
        .count()
}

fn team_personal_score(team: &[TeamFighter]) -> i64 {
    team.iter()
        .map(|fighter| fighter.state.score.max(0) as i64)
        .sum()
}

fn team_collaboration_score(team: &[TeamFighter]) -> i64 {
    team.iter()
        .map(|fighter| fighter.collaboration_score.max(0) as i64)
        .sum()
}

fn select_living_target(team: &[TeamFighter], slot: usize) -> Option<usize> {
    if team.is_empty() {
        return None;
    }
    (0..team.len())
        .map(|offset| (slot + offset) % team.len())
        .find(|idx| team[*idx].state.health > 0)
}

fn lowest_living_ally_index(team: &[TeamFighter], self_slot: usize) -> Option<usize> {
    team.iter()
        .enumerate()
        .filter(|(slot, fighter)| *slot != self_slot && fighter.state.health > 0)
        .min_by_key(|(slot, fighter)| (fighter.state.health, *slot))
        .map(|(slot, _)| slot)
}

fn lowest_living_ally_health(team: &[TeamFighter], self_slot: usize) -> i32 {
    lowest_living_ally_index(team, self_slot)
        .map(|slot| team[slot].state.health)
        .unwrap_or(0)
}

fn support_targets(team: &[TeamFighter], actions: &[BotAction]) -> Vec<Vec<usize>> {
    let mut targets = vec![Vec::new(); team.len()];
    for (supporter, action) in actions.iter().copied().enumerate() {
        if action != BotAction::Support || team[supporter].state.health <= 0 {
            continue;
        }
        if let Some(target) = lowest_living_ally_index(team, supporter) {
            targets[target].push(supporter);
        }
    }
    targets
}

fn apply_charge_costs(team: &mut [TeamFighter], actions: &[BotAction]) {
    for (fighter, action) in team.iter_mut().zip(actions.iter().copied()) {
        if fighter.state.health > 0 && action == BotAction::Charge {
            fighter.state.health = fighter.state.health.saturating_sub(4);
        }
    }
}

#[allow(clippy::too_many_arguments)]
/// Resolves one team's attacks against the other. Returns the kill events as
/// `(killer_slot, victim_slot)` pairs (slots index into the attacker and
/// defender slices respectively) so mixed-team callers can attribute
/// eliminations to the fighter — and thereby the model — that scored them.
/// Damage, scoring, and collaboration semantics are unchanged.
#[allow(clippy::too_many_arguments)]
fn apply_team_attacks(
    attackers: &mut [TeamFighter],
    defenders: &mut [TeamFighter],
    attacker_actions: &[BotAction],
    defender_actions: &[BotAction],
    targets: &[Option<usize>],
    defender_support_targets: &[Vec<usize>],
    seed: u64,
    tick: u32,
) -> Vec<(usize, usize)> {
    let mut kills = Vec::new();
    let mut incoming: Vec<Vec<(usize, i32)>> = vec![Vec::new(); defenders.len()];
    for (attacker_slot, action) in attacker_actions.iter().copied().enumerate() {
        let Some(target_slot) = targets.get(attacker_slot).copied().flatten() else {
            continue;
        };
        let damage = outgoing_damage(
            action,
            seed ^ (attacker_slot as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93),
            tick,
        );
        if damage > 0 && target_slot < incoming.len() {
            incoming[target_slot].push((attacker_slot, damage));
        }
    }

    for target_slot in 0..defenders.len() {
        if incoming[target_slot].is_empty() || defenders[target_slot].state.health <= 0 {
            continue;
        }

        let defended = defender_actions.get(target_slot) == Some(&BotAction::Defend);
        let supporters = defender_support_targets
            .get(target_slot)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let health_before = defenders[target_slot].state.health.max(0);

        let after_defend: Vec<(usize, i32)> = incoming[target_slot]
            .iter()
            .map(|(attacker, damage)| {
                let mitigated = if defended {
                    scale_damage(*damage, 40)
                } else {
                    *damage
                };
                (*attacker, mitigated)
            })
            .collect();
        let before_support_total: i32 = after_defend
            .iter()
            .map(|(_, damage)| *damage)
            .sum::<i32>()
            .max(0);
        let after_support: Vec<(usize, i32)> = if supporters.is_empty() {
            after_defend
        } else {
            after_defend
                .iter()
                .map(|(attacker, damage)| {
                    (
                        *attacker,
                        scale_damage(*damage, TEAM_SUPPORT_DAMAGE_PERCENT),
                    )
                })
                .collect()
        };
        let after_support_total: i32 = after_support
            .iter()
            .map(|(_, damage)| *damage)
            .sum::<i32>()
            .max(0);

        if !supporters.is_empty() {
            let effective_before = before_support_total.min(health_before);
            let effective_after = after_support_total.min(health_before);
            let prevented = effective_before.saturating_sub(effective_after);
            distribute_collaboration_credit(defenders, supporters, prevented);
        }

        let total_effective = after_support_total.min(health_before);
        let mut remaining = total_effective;
        let mut effective_by_attacker = Vec::with_capacity(after_support.len());
        for (attacker_slot, damage) in after_support {
            let effective = damage.max(0).min(remaining);
            remaining = remaining.saturating_sub(effective);
            if effective > 0 {
                if let Some(attacker) = attackers.get_mut(attacker_slot) {
                    attacker.state.score = attacker.state.score.saturating_add(2 + effective / 3);
                }
                effective_by_attacker.push((attacker_slot, effective));
            }
        }

        defenders[target_slot].state.health = defenders[target_slot]
            .state
            .health
            .saturating_sub(total_effective);
        if health_before > 0
            && defenders[target_slot].state.health <= 0
            && !effective_by_attacker.is_empty()
        {
            let killer_slot = effective_by_attacker
                .iter()
                .max_by_key(|(slot, damage)| (*damage, std::cmp::Reverse(*slot)))
                .map(|(slot, _)| *slot)
                .unwrap_or(0);
            if let Some(killer) = attackers.get_mut(killer_slot) {
                killer.state.score = killer.state.score.saturating_add(40);
            }
            kills.push((killer_slot, target_slot));
            for (attacker_slot, _) in effective_by_attacker {
                if attacker_slot != killer_slot {
                    if let Some(assistant) = attackers.get_mut(attacker_slot) {
                        assistant.collaboration_score = assistant
                            .collaboration_score
                            .saturating_add(TEAM_ASSIST_SCORE);
                    }
                }
            }
        }
    }
    kills
}

fn scale_damage(damage: i32, percent: i32) -> i32 {
    damage.max(0).saturating_mul(percent).saturating_add(50) / 100
}

fn distribute_collaboration_credit(
    defenders: &mut [TeamFighter],
    supporters: &[usize],
    prevented: i32,
) {
    if prevented <= 0 || supporters.is_empty() {
        return;
    }
    let count = supporters.len() as i32;
    let base = prevented / count;
    let remainder = prevented % count;
    for (position, supporter) in supporters.iter().copied().enumerate() {
        let credit = base + i32::from((position as i32) < remainder);
        if let Some(fighter) = defenders.get_mut(supporter) {
            fighter.collaboration_score = fighter.collaboration_score.saturating_add(credit);
        }
    }
}

fn eliminated_indices(health_before: &[i32], team: &[TeamFighter]) -> Vec<usize> {
    health_before
        .iter()
        .zip(team.iter())
        .enumerate()
        .filter_map(|(slot, (before, fighter))| {
            (*before > 0 && fighter.state.health <= 0).then_some(slot)
        })
        .collect()
}

fn respawn_team_fighters(mode: ArenaMatchMode, team: &mut [TeamFighter], eliminated: &[usize]) {
    for slot in eliminated.iter().copied() {
        let Some(fighter) = team.get_mut(slot) else {
            continue;
        };
        fighter.state.score = fighter.state.score.saturating_sub(4);
        if !matches!(mode, ArenaMatchMode::Arena) && fighter.state.respawns_remaining > 0 {
            fighter.state.respawns_remaining -= 1;
            fighter.state.health = 100;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_team_mode_objectives(
    mode: ArenaMatchMode,
    actions_a: &[BotAction],
    actions_b: &[BotAction],
    objectives: &mut MatchObjectiveState,
    team_size: u32,
    tick: u32,
    team_score_a: &mut i64,
    team_score_b: &mut i64,
) {
    match mode {
        ArenaMatchMode::Arena | ArenaMatchMode::TeamDeathmatch => {}
        ArenaMatchMode::Ctf => {
            let push_a: i32 = actions_a.iter().copied().map(team_ctf_push).sum();
            let push_b: i32 = actions_b.iter().copied().map(team_ctf_push).sum();
            let block_a: i32 = actions_a.iter().copied().map(team_ctf_block).sum();
            let block_b: i32 = actions_b.iter().copied().map(team_ctf_block).sum();
            objectives.ctf_progress_a = objectives
                .ctf_progress_a
                .saturating_add((push_a - block_b).max(0));
            objectives.ctf_progress_b = objectives
                .ctf_progress_b
                .saturating_add((push_b - block_a).max(0));
            let capture_threshold = 14i32.saturating_mul(team_size as i32);
            if objectives.ctf_progress_a >= capture_threshold {
                objectives.ctf_progress_a -= capture_threshold;
                objectives.ctf_captures_a = objectives.ctf_captures_a.saturating_add(1);
                *team_score_a = team_score_a.saturating_add(70);
            }
            if objectives.ctf_progress_b >= capture_threshold {
                objectives.ctf_progress_b -= capture_threshold;
                objectives.ctf_captures_b = objectives.ctf_captures_b.saturating_add(1);
                *team_score_b = team_score_b.saturating_add(70);
            }
            if tick.is_multiple_of(24) {
                objectives.ctf_progress_a = objectives.ctf_progress_a.saturating_sub(1);
                objectives.ctf_progress_b = objectives.ctf_progress_b.saturating_sub(1);
            }
        }
        ArenaMatchMode::Koth => {
            let presence_a: i32 = actions_a.iter().copied().map(team_zone_presence).sum();
            let presence_b: i32 = actions_b.iter().copied().map(team_zone_presence).sum();
            if presence_a > presence_b {
                let gain = presence_a - presence_b;
                objectives.koth_control_a = objectives.koth_control_a.saturating_add(gain);
                *team_score_a = team_score_a.saturating_add((1 + gain / 2) as i64);
            } else if presence_b > presence_a {
                let gain = presence_b - presence_a;
                objectives.koth_control_b = objectives.koth_control_b.saturating_add(gain);
                *team_score_b = team_score_b.saturating_add((1 + gain / 2) as i64);
            }
        }
    }
}

fn team_ctf_push(action: BotAction) -> i32 {
    match action {
        BotAction::Idle | BotAction::Support => 0,
        BotAction::Defend => 1,
        BotAction::Attack => 2,
        BotAction::Charge => 4,
    }
}

fn team_ctf_block(action: BotAction) -> i32 {
    match action {
        BotAction::Defend => 3,
        BotAction::Support => 1,
        _ => 0,
    }
}

fn team_zone_presence(action: BotAction) -> i32 {
    match action {
        BotAction::Idle => 0,
        BotAction::Support => 1,
        BotAction::Attack | BotAction::Charge => 2,
        BotAction::Defend => 3,
    }
}

fn team_objective_values(
    mode: ArenaMatchMode,
    objectives: &MatchObjectiveState,
    _team_a: &[TeamFighter],
    _team_b: &[TeamFighter],
) -> (i32, i32) {
    match mode {
        ArenaMatchMode::Arena | ArenaMatchMode::TeamDeathmatch => {
            (objectives.tdm_elims_a, objectives.tdm_elims_b)
        }
        ArenaMatchMode::Ctf => (objectives.ctf_captures_a, objectives.ctf_captures_b),
        ArenaMatchMode::Koth => (objectives.koth_control_a, objectives.koth_control_b),
    }
}

fn team_round_should_end(
    mode: ArenaMatchMode,
    objectives: &MatchObjectiveState,
    team_a: &[TeamFighter],
    team_b: &[TeamFighter],
    team_size: u32,
) -> bool {
    if living_count(team_a) == 0 || living_count(team_b) == 0 {
        return true;
    }
    match mode {
        ArenaMatchMode::Arena | ArenaMatchMode::TeamDeathmatch => false,
        ArenaMatchMode::Ctf => objectives.ctf_captures_a >= 3 || objectives.ctf_captures_b >= 3,
        ArenaMatchMode::Koth => {
            let target = 160i32.saturating_mul(team_size as i32);
            objectives.koth_control_a >= target || objectives.koth_control_b >= target
        }
    }
}

impl BotRuntime {
    fn runtime_name(&self) -> &'static str {
        match self {
            BotRuntime::Wasm { .. } => "wasm",
            BotRuntime::Fallback { .. } => "fallback",
        }
    }

    fn uses_v2(&self) -> bool {
        matches!(
            self,
            BotRuntime::Wasm {
                tick_fn: BotTickFunction::V2(_),
                ..
            }
        )
    }

    fn is_fallback(&self) -> bool {
        matches!(self, BotRuntime::Fallback { .. })
    }

    #[allow(clippy::too_many_arguments)]
    fn next_action(
        &mut self,
        fuel_per_tick: u64,
        observation: BotObservation,
        warnings: &mut Vec<String>,
        faults: &mut BotFaultCounts,
        side: &str,
    ) -> BotAction {
        let (raw, is_v2) = match self {
            BotRuntime::Wasm { store, tick_fn } => {
                let is_v2 = matches!(tick_fn, BotTickFunction::V2(_));
                if let Err(err) = store.set_fuel(fuel_per_tick) {
                    faults.fuel_error_count = faults.fuel_error_count.saturating_add(1);
                    push_bot_warning(
                        warnings,
                        format!("{}: failed to set wasm fuel: {}", side, err),
                    );
                    (0, is_v2)
                } else {
                    let result = match tick_fn {
                        BotTickFunction::V1(tick_fn) => tick_fn.call(
                            store,
                            (
                                observation.self_health,
                                observation.target_health,
                                observation.personal_score,
                                observation.tick as i32,
                            ),
                        ),
                        BotTickFunction::V2(tick_fn) => tick_fn.call(
                            store,
                            (
                                observation.self_health,
                                observation.target_health,
                                observation.personal_score,
                                observation.team_score_delta,
                                observation.objective_delta,
                                observation.allies_alive,
                                observation.enemies_alive,
                                observation.lowest_ally_health,
                                observation.slot,
                                observation.mode.code(),
                                observation.tick as i32,
                            ),
                        ),
                    };
                    let value = match result {
                        Ok(value) => value,
                        Err(err) => {
                            faults.trap_count = faults.trap_count.saturating_add(1);
                            push_bot_warning(
                                warnings,
                                format!("{}: wasm bot_tick trapped: {}", side, err),
                            );
                            0
                        }
                    };
                    (value, is_v2)
                }
            }
            BotRuntime::Fallback { prng_state } => (
                fallback_action(
                    *prng_state,
                    observation.tick,
                    observation.self_health,
                    observation.target_health,
                    observation.team_score_delta,
                    observation.mode,
                ),
                false,
            ),
        };

        if let BotRuntime::Fallback { prng_state } = self {
            *prng_state = next_prng(*prng_state ^ observation.tick as u64);
        }

        if is_v2 {
            BotAction::from_v2_code(raw).unwrap_or_else(|| {
                faults.invalid_action_count = faults.invalid_action_count.saturating_add(1);
                push_bot_warning(
                    warnings,
                    format!(
                        "{}: {} returned invalid action {}; idle applied",
                        side, BOT_TICK_V2_EXPORT, raw
                    ),
                );
                BotAction::Idle
            })
        } else {
            BotAction::from_v1_code(raw)
        }
    }
}

fn push_bot_warning(warnings: &mut Vec<String>, warning: String) {
    if warnings.len() < 256 {
        warnings.push(warning);
    }
}

fn resolve_combat_tick(
    a: &mut FighterState,
    b: &mut FighterState,
    action_a: BotAction,
    action_b: BotAction,
    seed: u64,
    tick: u32,
) {
    let mut damage_to_b = outgoing_damage(action_a, seed ^ 0x1111, tick);
    let mut damage_to_a = outgoing_damage(action_b, seed ^ 0x2222, tick);

    if action_b == BotAction::Defend {
        damage_to_b = ((damage_to_b as f32) * 0.4).round() as i32;
    }
    if action_a == BotAction::Defend {
        damage_to_a = ((damage_to_a as f32) * 0.4).round() as i32;
    }

    if damage_to_b > 0 {
        a.score += 2 + damage_to_b / 3;
    }
    if damage_to_a > 0 {
        b.score += 2 + damage_to_a / 3;
    }

    if action_a == BotAction::Charge {
        a.health -= 4;
    }
    if action_b == BotAction::Charge {
        b.health -= 4;
    }

    a.health -= damage_to_a.max(0);
    b.health -= damage_to_b.max(0);
}

fn apply_mode_objectives(
    mode: ArenaMatchMode,
    a: &mut FighterState,
    b: &mut FighterState,
    action_a: BotAction,
    action_b: BotAction,
    objectives: &mut MatchObjectiveState,
    tick: u32,
) {
    match mode {
        ArenaMatchMode::Arena => {}
        ArenaMatchMode::Ctf => {
            let push_a = match action_a {
                BotAction::Charge => 4,
                BotAction::Attack => 2,
                BotAction::Defend => 1,
                BotAction::Idle | BotAction::Support => 0,
            };
            let push_b = match action_b {
                BotAction::Charge => 4,
                BotAction::Attack => 2,
                BotAction::Defend => 1,
                BotAction::Idle | BotAction::Support => 0,
            };
            let block_a = if action_a == BotAction::Defend { 3 } else { 0 };
            let block_b = if action_b == BotAction::Defend { 3 } else { 0 };

            objectives.ctf_progress_a = (objectives.ctf_progress_a + push_a - block_b).max(0);
            objectives.ctf_progress_b = (objectives.ctf_progress_b + push_b - block_a).max(0);

            if objectives.ctf_progress_a >= 14 {
                objectives.ctf_captures_a += 1;
                objectives.ctf_progress_a = 0;
                a.score += 70;
            }
            if objectives.ctf_progress_b >= 14 {
                objectives.ctf_captures_b += 1;
                objectives.ctf_progress_b = 0;
                b.score += 70;
            }

            if tick.is_multiple_of(24) {
                objectives.ctf_progress_a = objectives.ctf_progress_a.saturating_sub(1);
                objectives.ctf_progress_b = objectives.ctf_progress_b.saturating_sub(1);
            }
        }
        ArenaMatchMode::Koth => {
            let presence_a = action_zone_presence(action_a);
            let presence_b = action_zone_presence(action_b);
            if presence_a > presence_b {
                let gain = presence_a - presence_b;
                objectives.koth_control_a += gain;
                a.score += 1 + gain / 2;
            } else if presence_b > presence_a {
                let gain = presence_b - presence_a;
                objectives.koth_control_b += gain;
                b.score += 1 + gain / 2;
            }
            if tick.is_multiple_of(10) {
                if objectives.koth_control_a > objectives.koth_control_b {
                    a.score += 2;
                } else if objectives.koth_control_b > objectives.koth_control_a {
                    b.score += 2;
                }
            }
        }
        ArenaMatchMode::TeamDeathmatch => {}
    }
}

fn action_zone_presence(action: BotAction) -> i32 {
    match action {
        BotAction::Idle => 0,
        BotAction::Attack => 2,
        BotAction::Defend => 3,
        BotAction::Charge => 2,
        BotAction::Support => 0,
    }
}

fn objective_values(
    mode: ArenaMatchMode,
    objectives: &MatchObjectiveState,
    a: &FighterState,
    b: &FighterState,
) -> (i32, i32) {
    match mode {
        ArenaMatchMode::Arena => (a.score.max(0), b.score.max(0)),
        ArenaMatchMode::Ctf => (objectives.ctf_captures_a, objectives.ctf_captures_b),
        ArenaMatchMode::Koth => (objectives.koth_control_a, objectives.koth_control_b),
        ArenaMatchMode::TeamDeathmatch => (objectives.tdm_elims_a, objectives.tdm_elims_b),
    }
}

fn determine_winner(
    mode: ArenaMatchMode,
    model_a_id: &str,
    model_b_id: &str,
    a: &FighterState,
    b: &FighterState,
    objective_a: i32,
    objective_b: i32,
) -> (Option<String>, bool) {
    let mut ordering = objective_a.cmp(&objective_b);
    if ordering == std::cmp::Ordering::Equal {
        ordering = a.score.cmp(&b.score);
    }
    if ordering == std::cmp::Ordering::Equal {
        ordering = a.health.cmp(&b.health);
    }
    if ordering == std::cmp::Ordering::Equal && !matches!(mode, ArenaMatchMode::Arena) {
        ordering = a.respawns_remaining.cmp(&b.respawns_remaining);
    }

    match ordering {
        std::cmp::Ordering::Greater => (Some(model_a_id.to_owned()), false),
        std::cmp::Ordering::Less => (Some(model_b_id.to_owned()), false),
        std::cmp::Ordering::Equal => (None, true),
    }
}

fn compare_team_round(
    model_a_id: &str,
    model_b_id: &str,
    objective_a: i64,
    objective_b: i64,
    score_a: i64,
    score_b: i64,
) -> Option<String> {
    use std::cmp::Ordering;
    match objective_a
        .cmp(&objective_b)
        .then_with(|| score_a.cmp(&score_b))
    {
        Ordering::Greater => Some(model_a_id.to_owned()),
        Ordering::Less => Some(model_b_id.to_owned()),
        Ordering::Equal => None,
    }
}

/// Side-based equivalent of `compare_team_round` for mixed-team battles,
/// where a winner cannot be named by a single model id.
fn compare_sides(objective_a: i64, objective_b: i64, score_a: i64, score_b: i64) -> Option<&'static str> {
    use std::cmp::Ordering;
    match objective_a
        .cmp(&objective_b)
        .then_with(|| score_a.cmp(&score_b))
    {
        Ordering::Greater => Some("team_a"),
        Ordering::Less => Some("team_b"),
        Ordering::Equal => None,
    }
}

fn mixed_runtime_name(runtime: &BotRuntime) -> &'static str {
    match runtime {
        BotRuntime::Fallback { .. } => "fallback",
        BotRuntime::Wasm {
            tick_fn: BotTickFunction::V2(_),
            ..
        } => "wasm_v2",
        BotRuntime::Wasm { .. } => "wasm_v1",
    }
}

fn saturating_i64_to_i32(value: i64) -> i32 {
    if value > i32::MAX as i64 {
        i32::MAX
    } else if value < i32::MIN as i64 {
        i32::MIN
    } else {
        value as i32
    }
}

fn outgoing_damage(action: BotAction, seed: u64, tick: u32) -> i32 {
    let base = match action {
        BotAction::Idle => 0,
        BotAction::Attack => 10,
        BotAction::Defend => 0,
        BotAction::Charge => 16,
        BotAction::Support => 0,
    };
    if base == 0 {
        return 0;
    }
    let jitter = ((next_prng(seed ^ tick as u64) >> 62) as i32) - 1;
    (base + jitter).max(1)
}

fn fallback_action(
    prng_state: u64,
    tick: u32,
    self_health: i32,
    enemy_health: i32,
    score_delta: i32,
    mode: ArenaMatchMode,
) -> i32 {
    if self_health < 20 {
        return BotAction::Defend as i32;
    }

    match mode {
        ArenaMatchMode::Arena | ArenaMatchMode::TeamDeathmatch => {
            if enemy_health < 16 {
                return BotAction::Attack as i32;
            }
            if score_delta <= -28 && self_health > 48 {
                return BotAction::Charge as i32;
            }
            if score_delta >= 24 && self_health < 56 {
                return BotAction::Defend as i32;
            }
        }
        ArenaMatchMode::Ctf => {
            if score_delta < 0 && self_health > 40 {
                return BotAction::Charge as i32;
            }
            if tick.is_multiple_of(4) {
                return BotAction::Defend as i32;
            }
            if enemy_health < 30 {
                return BotAction::Attack as i32;
            }
        }
        ArenaMatchMode::Koth => {
            if score_delta > 12 {
                return BotAction::Defend as i32;
            }
            if tick.is_multiple_of(3) {
                return BotAction::Defend as i32;
            }
            if score_delta < -8 {
                return BotAction::Charge as i32;
            }
        }
    }

    (next_prng(prng_state ^ tick as u64) & 0b11) as i32
}

fn next_prng(state: u64) -> u64 {
    state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407)
}

fn validate_bot_tick_export(module: &Module) -> Result<(), String> {
    if let Some(export) = module.get_export(BOT_TICK_V2_EXPORT) {
        return validate_tick_function_type(export, BOT_TICK_V2_EXPORT, BOT_TICK_V2_PARAM_COUNT);
    }

    let Some(export) = module.get_export(BOT_TICK_EXPORT) else {
        return Err(format!(
            "missing '{}' or '{}' export",
            BOT_TICK_V2_EXPORT, BOT_TICK_EXPORT
        ));
    };
    validate_tick_function_type(export, BOT_TICK_EXPORT, 4)
}

fn validate_tick_function_type(
    export: ExternType,
    export_name: &str,
    expected_params: usize,
) -> Result<(), String> {
    let ExternType::Func(func) = export else {
        return Err(format!("'{}' export must be a function", export_name));
    };

    if func.params().len() != expected_params || func.results().len() != 1 {
        return Err(format!(
            "'{}' signature must have {} i32 parameters and one i32 result",
            export_name, expected_params
        ));
    }
    if !func.params().all(|param| matches!(param, ValType::I32)) {
        return Err(format!("'{}' params must be i32", export_name));
    }
    if !matches!(func.results().next(), Some(ValType::I32)) {
        return Err(format!("'{}' return type must be i32", export_name));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Equivalent to a tiny `bot_tick_v2` module that returns `(slot + tick) % 5`.
    // Keeping the fixture as raw WebAssembly avoids adding a WAT compiler to test builds.
    const CYCLING_V2_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // magic + version
        0x01, 0x10, 0x01, 0x60, 0x0b, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f,
        0x7f, 0x01, 0x7f, // type: (11 * i32) -> i32
        0x03, 0x02, 0x01, 0x00, // one function with type 0
        0x07, 0x0f, 0x01, 0x0b, b'b', b'o', b't', b'_', b't', b'i', b'c', b'k', b'_', b'v', b'2',
        0x00, 0x00, // export function 0 as bot_tick_v2
        0x0a, 0x0c, 0x01, 0x0a, 0x00, 0x20, 0x08, 0x20, 0x0a, 0x6a, 0x41, 0x05, 0x6f,
        0x0b, // body: (slot + tick) % 5
    ];

    fn constant_v2_wasm(action: i8) -> Vec<u8> {
        let mut wasm = vec![
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // magic + version
            0x01, 0x10, 0x01, 0x60, 0x0b, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f,
            0x7f, 0x7f, 0x01, 0x7f, // type: (11 * i32) -> i32
            0x03, 0x02, 0x01, 0x00, // one function with type 0
            0x07, 0x0f, 0x01, 0x0b, b'b', b'o', b't', b'_', b't', b'i', b'c', b'k', b'_', b'v',
            b'2', 0x00, 0x00, // export function 0 as bot_tick_v2
            0x0a, 0x06, 0x01, 0x04, 0x00, 0x41,
        ];
        wasm.push(action as u8);
        wasm.push(0x0b);
        wasm
    }

    fn trapping_v2_wasm() -> Vec<u8> {
        vec![
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // magic + version
            0x01, 0x10, 0x01, 0x60, 0x0b, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f,
            0x7f, 0x7f, 0x01, 0x7f, // type: (11 * i32) -> i32
            0x03, 0x02, 0x01, 0x00, // one function with type 0
            0x07, 0x0f, 0x01, 0x0b, b'b', b'o', b't', b'_', b't', b'i', b'c', b'k', b'_', b'v',
            b'2', 0x00, 0x00, // export function 0 as bot_tick_v2
            0x0a, 0x05, 0x01, 0x03, 0x00, 0x00, 0x0b, // unreachable
        ]
    }

    struct V2WasmFixture {
        dir: PathBuf,
    }

    impl V2WasmFixture {
        fn new(model_ids: &[&str]) -> Self {
            let dir =
                std::env::temp_dir().join(format!("mgs-bot-sandbox-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&dir).expect("temporary bot directory should be created");
            for model_id in model_ids {
                assert!(sanitize_model_id(model_id).is_some());
                fs::write(dir.join(format!("{model_id}.wasm")), CYCLING_V2_WASM)
                    .expect("test bot should be written");
            }
            Self { dir }
        }

        fn sandbox(&self) -> BotSandbox {
            let mut sandbox = BotSandbox::new_from_env();
            assert!(
                sandbox.engine.is_some(),
                "test requires the Wasmtime engine"
            );
            sandbox.wasm_dir = self.dir.clone();
            sandbox
        }

        fn write_wasm(&self, model_id: &str, wasm: &[u8]) {
            assert!(sanitize_model_id(model_id).is_some());
            fs::write(self.dir.join(format!("{model_id}.wasm")), wasm)
                .expect("test bot should be written");
        }
    }

    impl Drop for V2WasmFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    fn remove_duration_telemetry(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(fields) => {
                fields.remove("duration_ms");
                for field in fields.values_mut() {
                    remove_duration_telemetry(field);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    remove_duration_telemetry(item);
                }
            }
            _ => {}
        }
    }

    fn deterministic_telemetry(outcome: &TeamBattleOutcome) -> serde_json::Value {
        let mut value = serde_json::to_value(outcome).expect("team outcome should serialize");
        remove_duration_telemetry(&mut value);
        value
    }

    fn deterministic_world_telemetry(outcome: &WorldBattleOutcome) -> serde_json::Value {
        let mut value = serde_json::to_value(outcome).expect("world outcome should serialize");
        remove_duration_telemetry(&mut value);
        value
    }

    fn total_actions(counts: &TeamActionCounts) -> u64 {
        counts
            .idle
            .saturating_add(counts.attack)
            .saturating_add(counts.defend)
            .saturating_add(counts.charge)
            .saturating_add(counts.support)
    }

    fn test_fighter(health: i32) -> TeamFighter {
        TeamFighter {
            state: FighterState {
                health,
                score: 0,
                respawns_remaining: 0,
            },
            runtime: BotRuntime::Fallback { prng_state: 1 },
            collaboration_score: 0,
        }
    }

    #[test]
    fn fallback_duel_runs_without_wasm_files() {
        let sandbox = BotSandbox::new_from_env();
        let outcome = sandbox.execute_duel("model_a", "model_b", 42, Some(120));
        assert!(outcome.ticks_executed > 0);
        assert!(outcome.ticks_executed <= 120);
        assert!(outcome.model_a_score >= 0);
        assert!(outcome.model_b_score >= 0);
        assert!(outcome.model_a_runtime == "fallback" || outcome.model_b_runtime == "fallback");
    }

    #[test]
    fn fallback_modes_execute_and_report_objective() {
        let sandbox = BotSandbox::new_from_env();
        for mode in [
            ArenaMatchMode::Arena,
            ArenaMatchMode::Ctf,
            ArenaMatchMode::Koth,
            ArenaMatchMode::TeamDeathmatch,
        ] {
            let outcome = sandbox.execute_match("model_a", "model_b", mode, 7, Some(160));
            assert_eq!(outcome.mode, mode.as_str());
            assert_eq!(outcome.objective_label, mode.objective_label());
            assert!(outcome.ticks_executed > 0);
        }
    }

    #[test]
    fn parse_mode_accepts_aliases() {
        assert_eq!(ArenaMatchMode::parse("arena"), Some(ArenaMatchMode::Arena));
        assert_eq!(ArenaMatchMode::parse("ctf"), Some(ArenaMatchMode::Ctf));
        assert_eq!(
            ArenaMatchMode::parse("king-of-the-hill"),
            Some(ArenaMatchMode::Koth)
        );
        assert_eq!(
            ArenaMatchMode::parse("team_deathmatch"),
            Some(ArenaMatchMode::TeamDeathmatch)
        );
        assert_eq!(ArenaMatchMode::parse("unknown"), None);
    }

    #[test]
    fn sanitize_model_id_rejects_path_traversal() {
        assert!(sanitize_model_id("../etc/passwd").is_none());
        assert!(sanitize_model_id(".hidden_model").is_none());
        assert!(sanitize_model_id("model..double-dot").is_none());
        assert!(sanitize_model_id("bot-alpha_1").is_some());
    }

    #[test]
    fn team_battle_simulates_10v10_with_rounds() {
        let sandbox = BotSandbox::new_from_env();
        let outcome = sandbox.execute_team_battle(
            "model_a",
            "model_b",
            ArenaMatchMode::TeamDeathmatch,
            10,
            3,
            91,
            Some(120),
        );
        assert_eq!(outcome.team_size, 10);
        assert_eq!(outcome.rounds, 3);
        assert_eq!(outcome.total_engagements, 30);
        assert_eq!(outcome.mode, "tdm");
        assert_eq!(outcome.rounds_detail.len(), 3);
        assert_eq!(
            outcome.total_team_a_score,
            outcome.total_team_a_personal_score
        );
        assert_eq!(
            outcome.total_team_b_score,
            outcome.total_team_b_personal_score
        );
        let observed_actions = total_actions(&outcome.team_a_action_counts)
            .saturating_add(total_actions(&outcome.team_b_action_counts));
        let maximum_actions = 2u64 * 10 * 3 * 120;
        assert!(observed_actions > 0);
        assert!(observed_actions <= maximum_actions);
    }

    fn deterministic_mixed_telemetry(outcome: &MixedTeamBattleOutcome) -> serde_json::Value {
        let mut value = serde_json::to_value(outcome).expect("mixed outcome should serialize");
        remove_duration_telemetry(&mut value);
        value
    }

    #[test]
    fn mixed_team_battle_attributes_every_fighter_to_its_model() {
        let fixture = V2WasmFixture::new(&["ally_one", "ally_two", "enemy_one", "enemy_two"]);
        let sandbox = fixture.sandbox();
        let team_a = vec!["ally_one".to_owned(), "ally_two".to_owned()];
        let team_b = vec!["enemy_one".to_owned(), "enemy_two".to_owned()];
        let outcome = sandbox.execute_mixed_team_battle(
            &team_a,
            &team_b,
            ArenaMatchMode::TeamDeathmatch,
            2,
            77,
            Some(120),
        );

        assert_eq!(outcome.mode, "mixed_team");
        assert_eq!(outcome.match_mode, "tdm");
        assert_eq!(outcome.team_size, 2);
        assert_eq!(outcome.rounds, 2);
        assert_eq!(outcome.team_a_models, team_a);
        assert_eq!(outcome.team_b_models, team_b);
        assert_eq!(outcome.rounds_detail.len(), 2);
        assert_eq!(outcome.draw, outcome.winner_side.is_none());
        if let Some(winner) = outcome.winner_side.as_deref() {
            assert!(winner == "team_a" || winner == "team_b");
        }
        assert_eq!(
            outcome.team_a_round_wins + outcome.team_b_round_wins + outcome.round_draws,
            2
        );

        // One fighter entry per (side, slot), each carrying its roster model.
        assert_eq!(outcome.fighters.len(), 4);
        for (slot, model_id) in team_a.iter().enumerate() {
            let fighter = outcome
                .fighters
                .iter()
                .find(|entry| entry.side == "team_a" && entry.slot == slot as u32)
                .expect("every team_a slot should be attributed");
            assert_eq!(&fighter.model_id, model_id);
            assert_eq!(fighter.runtime, "wasm_v2");
        }
        for (slot, model_id) in team_b.iter().enumerate() {
            let fighter = outcome
                .fighters
                .iter()
                .find(|entry| entry.side == "team_b" && entry.slot == slot as u32)
                .expect("every team_b slot should be attributed");
            assert_eq!(&fighter.model_id, model_id);
            assert_eq!(fighter.runtime, "wasm_v2");
        }

        // Per-fighter sums must reproduce the team totals exactly.
        let sum = |side: &str,
                   pick: &dyn Fn(&MixedTeamFighterOutcome) -> i64|
         -> i64 {
            outcome
                .fighters
                .iter()
                .filter(|entry| entry.side == side)
                .map(pick)
                .sum()
        };
        assert_eq!(sum("team_a", &|f| f.personal_score), outcome.total_team_a_score);
        assert_eq!(sum("team_b", &|f| f.personal_score), outcome.total_team_b_score);
        assert_eq!(
            sum("team_a", &|f| f.collaboration_score),
            outcome.total_team_a_collaboration_score
        );
        assert_eq!(
            sum("team_b", &|f| f.collaboration_score),
            outcome.total_team_b_collaboration_score
        );
        let actions_of = |side: &str| -> u64 {
            outcome
                .fighters
                .iter()
                .filter(|entry| entry.side == side)
                .map(|entry| total_actions(&entry.action_counts))
                .sum()
        };
        assert_eq!(actions_of("team_a"), total_actions(&outcome.team_a_action_counts));
        assert_eq!(actions_of("team_b"), total_actions(&outcome.team_b_action_counts));

        // TDM objectives count eliminations suffered by the opposing side,
        // so deaths on one side must equal the other side's objective.
        let deaths_a = sum("team_a", &|f| f.deaths as i64);
        let deaths_b = sum("team_b", &|f| f.deaths as i64);
        assert_eq!(deaths_b, outcome.total_team_a_objective);
        assert_eq!(deaths_a, outcome.total_team_b_objective);
        // Kills are credited only when an attacker lands the fatal damage;
        // self-inflicted charge deaths leave a non-negative gap.
        let kills_a = sum("team_a", &|f| f.eliminations as i64);
        let kills_b = sum("team_b", &|f| f.eliminations as i64);
        assert!(kills_a <= deaths_b);
        assert!(kills_b <= deaths_a);
    }

    #[test]
    fn mixed_team_battle_is_deterministic_for_fixed_seed() {
        let fixture = V2WasmFixture::new(&["mix_a", "mix_b", "mix_c", "mix_d"]);
        let sandbox = fixture.sandbox();
        let team_a = vec!["mix_a".to_owned(), "mix_b".to_owned()];
        let team_b = vec!["mix_c".to_owned(), "mix_d".to_owned()];
        let first = sandbox.execute_mixed_team_battle(
            &team_a,
            &team_b,
            ArenaMatchMode::Koth,
            2,
            1234,
            Some(150),
        );
        let second = sandbox.execute_mixed_team_battle(
            &team_a,
            &team_b,
            ArenaMatchMode::Koth,
            2,
            1234,
            Some(150),
        );
        assert_eq!(
            deterministic_mixed_telemetry(&first),
            deterministic_mixed_telemetry(&second)
        );
        let other_seed = sandbox.execute_mixed_team_battle(
            &team_a,
            &team_b,
            ArenaMatchMode::Koth,
            2,
            4321,
            Some(150),
        );
        assert_ne!(
            deterministic_mixed_telemetry(&first),
            deterministic_mixed_telemetry(&other_seed)
        );
    }

    #[test]
    fn mixed_team_battle_runs_on_fallback_runtimes_without_wasm() {
        let sandbox = BotSandbox::new_from_env();
        let outcome = sandbox.execute_mixed_team_battle(
            &["no_wasm_a".to_owned(), "no_wasm_b".to_owned()],
            &["no_wasm_c".to_owned(), "no_wasm_d".to_owned()],
            ArenaMatchMode::Arena,
            1,
            5,
            Some(80),
        );
        assert_eq!(outcome.fighters.len(), 4);
        assert!(
            outcome
                .fighters
                .iter()
                .all(|fighter| fighter.runtime == "fallback")
        );
        assert!(outcome.fallback_count >= 4);
        assert_eq!(outcome.draw, outcome.winner_side.is_none());
    }

    #[test]
    fn interacting_team_battle_is_repeatable_for_every_mode() {
        let fixture = V2WasmFixture::new(&["repeatable_a", "repeatable_b"]);
        let sandbox = fixture.sandbox();

        for mode in [
            ArenaMatchMode::Arena,
            ArenaMatchMode::Ctf,
            ArenaMatchMode::Koth,
            ArenaMatchMode::TeamDeathmatch,
        ] {
            let first = sandbox.execute_team_battle(
                "repeatable_a",
                "repeatable_b",
                mode,
                10,
                3,
                0xCAFE_BABE,
                Some(240),
            );
            let second = sandbox.execute_team_battle(
                "repeatable_a",
                "repeatable_b",
                mode,
                10,
                3,
                0xCAFE_BABE,
                Some(240),
            );

            assert_eq!(first.team_a_v2_fighters, 10);
            assert_eq!(first.team_b_v2_fighters, 10);
            assert!(first.team_a_action_counts.support > 0);
            assert!(first.team_b_action_counts.support > 0);
            assert_eq!(
                deterministic_telemetry(&first),
                deterministic_telemetry(&second),
                "{} telemetry changed for identical seed and inputs",
                mode.as_str()
            );
        }
    }

    #[test]
    fn shared_world_contains_every_entrant_and_is_order_independent() {
        let fixture = V2WasmFixture::new(&["world_a", "world_b", "world_c", "world_d"]);
        let sandbox = fixture.sandbox();
        let first_ids = vec![
            "world_d".to_owned(),
            "world_b".to_owned(),
            "world_a".to_owned(),
            "world_c".to_owned(),
        ];
        let second_ids = vec![
            "world_c".to_owned(),
            "world_a".to_owned(),
            "world_d".to_owned(),
            "world_b".to_owned(),
        ];
        let first = sandbox.execute_world_battle(&first_ids, 3, 2, 0xCAFE_BABE, Some(120));
        let second = sandbox.execute_world_battle(&second_ids, 3, 2, 0xCAFE_BABE, Some(120));

        assert_eq!(first.entrants, 4);
        assert_eq!(first.squad_size, 3);
        assert_eq!(first.rounds_detail.len(), 2);
        assert_eq!(first.rankings.len(), 4);
        assert!(first
            .rounds_detail
            .iter()
            .all(|round| round.placements.len() == 4));
        let mut observed_ids: Vec<&str> = first
            .rankings
            .iter()
            .map(|entry| entry.model_id.as_str())
            .collect();
        observed_ids.sort_unstable();
        assert_eq!(
            observed_ids,
            vec!["world_a", "world_b", "world_c", "world_d"]
        );
        assert!(first
            .rankings
            .iter()
            .all(|entry| entry.v2_fighter_rounds == 6 && entry.fallback_count == 0));
        assert_eq!(
            deterministic_world_telemetry(&first),
            deterministic_world_telemetry(&second),
            "request order must not influence a shared-world result"
        );
    }

    #[test]
    fn shared_world_runs_the_full_sixteen_model_roster() {
        let ids: Vec<String> = (0..MAX_WORLD_BATTLE_ENTRANTS)
            .map(|index| format!("world_max_{index:02}"))
            .collect();
        let id_refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let fixture = V2WasmFixture::new(&id_refs);
        let sandbox = fixture.sandbox();
        let world = sandbox.execute_world_battle(&ids, 1, 1, 104_729, Some(8));

        assert_eq!(world.entrants as usize, MAX_WORLD_BATTLE_ENTRANTS);
        assert_eq!(world.rankings.len(), MAX_WORLD_BATTLE_ENTRANTS);
        assert_eq!(
            world.rounds_detail[0].placements.len(),
            MAX_WORLD_BATTLE_ENTRANTS
        );
        assert!(world
            .rankings
            .iter()
            .all(|entry| entry.runtime == "wasm_v2" && entry.v2_fighter_rounds == 1));
    }

    #[test]
    fn world_and_team_expose_fallback_trap_and_invalid_action_counts() {
        let fixture = V2WasmFixture::new(&["fault_healthy"]);
        fixture.write_wasm("fault_invalid", &constant_v2_wasm(9));
        fixture.write_wasm("fault_trap", &trapping_v2_wasm());
        let sandbox = fixture.sandbox();
        let ids = vec![
            "fault_healthy".to_owned(),
            "fault_invalid".to_owned(),
            "fault_missing".to_owned(),
            "fault_trap".to_owned(),
        ];
        let world = sandbox.execute_world_battle(&ids, 1, 1, 77, Some(20));

        let missing = world
            .rankings
            .iter()
            .find(|entry| entry.model_id == "fault_missing")
            .expect("missing bot must remain an explicit entrant");
        let invalid = world
            .rankings
            .iter()
            .find(|entry| entry.model_id == "fault_invalid")
            .expect("invalid-action bot must remain an explicit entrant");
        let trapped = world
            .rankings
            .iter()
            .find(|entry| entry.model_id == "fault_trap")
            .expect("trapping bot must remain an explicit entrant");
        assert_eq!(missing.fallback_count, 1);
        assert_eq!(missing.v2_fighter_rounds, 0);
        assert!(invalid.invalid_action_count > 0);
        assert!(trapped.trap_count > 0);
        assert!(
            world
                .warnings
                .iter()
                .any(|warning| warning.contains("fault_missing")
                    && warning.contains("wasm not found"))
        );

        let team = sandbox.execute_team_battle(
            "fault_invalid",
            "fault_trap",
            ArenaMatchMode::TeamDeathmatch,
            1,
            1,
            77,
            Some(20),
        );
        assert_eq!(team.fallback_count, 0);
        assert!(team.invalid_action_count > 0);
        assert!(team.trap_count > 0);
    }

    #[test]
    fn compiled_module_cache_reuses_content_and_invalidates_on_overwrite() {
        let fixture = V2WasmFixture::new(&["cache_bot"]);
        let sandbox = fixture.sandbox();
        let path = fixture.dir.join("cache_bot.wasm");

        assert!(matches!(
            sandbox.load_program("cache_bot"),
            BotProgram::Wasm { .. }
        ));
        let first_hash = sandbox
            .module_cache
            .read()
            .get(&path)
            .expect("compiled module should be cached")
            .content_sha256;
        assert!(matches!(
            sandbox.load_program("cache_bot"),
            BotProgram::Wasm { .. }
        ));
        assert_eq!(sandbox.module_cache.read().len(), 1);
        assert_eq!(
            sandbox
                .module_cache
                .read()
                .get(&path)
                .unwrap()
                .content_sha256,
            first_hash
        );

        fixture.write_wasm("cache_bot", &constant_v2_wasm(1));
        assert!(matches!(
            sandbox.load_program("cache_bot"),
            BotProgram::Wasm { .. }
        ));
        let replacement_hash = sandbox
            .module_cache
            .read()
            .get(&path)
            .expect("replacement module should be cached")
            .content_sha256;
        assert_ne!(replacement_hash, first_hash);
        assert_eq!(sandbox.module_cache.read().len(), 1);

        fixture.write_wasm("cache_bot", b"not-webassembly");
        assert!(matches!(
            sandbox.load_program("cache_bot"),
            BotProgram::Fallback { .. }
        ));
        assert!(!sandbox.module_cache.read().contains_key(&path));
    }

    #[test]
    fn exhibition_runtime_requires_verified_v2_wasm_and_executes_live_observations() {
        let fixture = V2WasmFixture::new(&["live_fighter"]);
        let sandbox = fixture.sandbox();
        let digest = sha256_hex(CYCLING_V2_WASM);
        let mut runtime = sandbox
            .build_exhibition_runtime("live_fighter", CYCLING_V2_WASM.len(), &digest, 77)
            .expect("verified v2 fighter should instantiate");
        let observation = ExhibitionBotObservation {
            self_health: 80,
            target_health: 60,
            personal_score: 12,
            team_score_delta: -3,
            objective_delta: -1,
            allies_alive: 4,
            enemies_alive: 5,
            lowest_ally_health: 25,
            slot: 2,
            mode: ArenaMatchMode::Ctf,
        };

        assert_eq!(
            runtime.next_action(observation),
            Ok(ExhibitionBotAction::Defend)
        );
        assert_eq!(
            runtime.next_action(observation),
            Ok(ExhibitionBotAction::Charge)
        );
        assert_eq!(runtime.tick(), 2);
        assert_eq!(runtime.fault_counts(), BotFaultCounts::default());
        assert_eq!(runtime.model_id(), "live_fighter");

        assert!(sandbox
            .build_exhibition_runtime("missing_fighter", 1, &"0".repeat(64), 77)
            .is_err());
    }

    #[test]
    fn exhibition_runtime_rejects_size_or_digest_mismatch_before_cache_reuse() {
        let fixture = V2WasmFixture::new(&["bound_fighter"]);
        let sandbox = fixture.sandbox();
        let path = fixture.dir.join("bound_fighter.wasm");
        let digest = sha256_hex(CYCLING_V2_WASM);

        sandbox
            .build_exhibition_runtime("bound_fighter", CYCLING_V2_WASM.len(), &digest, 7)
            .expect("matching artifact should load");
        assert!(sandbox.module_cache.read().contains_key(&path));

        let size_error = sandbox
            .build_exhibition_runtime("bound_fighter", CYCLING_V2_WASM.len() + 1, &digest, 7)
            .err()
            .expect("size mismatch must fail closed");
        assert!(size_error.contains("size mismatch"));
        assert!(!sandbox.module_cache.read().contains_key(&path));

        let digest_error = sandbox
            .build_exhibition_runtime("bound_fighter", CYCLING_V2_WASM.len(), &"0".repeat(64), 7)
            .err()
            .expect("digest mismatch must fail closed");
        assert!(digest_error.contains("digest mismatch"));
        assert!(!sandbox.module_cache.read().contains_key(&path));

        let format_error = sandbox
            .build_exhibition_runtime(
                "bound_fighter",
                CYCLING_V2_WASM.len(),
                &digest.to_ascii_uppercase(),
                7,
            )
            .err()
            .expect("non-canonical digest must fail closed");
        assert!(format_error.contains("digest is invalid"));
        assert!(!sandbox.module_cache.read().contains_key(&path));
    }

    #[test]
    fn strict_exhibition_limits_reject_oversized_initial_resources() {
        let sandbox = BotSandbox::new_from_env();
        let engine = sandbox.engine.as_ref().expect("test requires Wasmtime");
        let oversized_modules = [
            (
                "memory",
                r#"(module
                    (memory 33)
                    (func (export "bot_tick_v2")
                        (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)
                        (result i32)
                        i32.const 0))"#,
            ),
            (
                "table",
                r#"(module
                    (table 129 funcref)
                    (func (export "bot_tick_v2")
                        (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)
                        (result i32)
                        i32.const 0))"#,
            ),
            (
                "memory-count",
                r#"(module
                    (memory 1)
                    (memory 1)
                    (func (export "bot_tick_v2")
                        (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)
                        (result i32)
                        i32.const 0))"#,
            ),
            (
                "table-count",
                r#"(module
                    (table 1 funcref)
                    (table 1 funcref)
                    (func (export "bot_tick_v2")
                        (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)
                        (result i32)
                        i32.const 0))"#,
            ),
        ];

        for (resource, wat) in oversized_modules {
            let module = Module::new(engine, wat).expect("resource-limit fixture should compile");
            validate_bot_tick_export(&module).expect("fixture should expose the v2 ABI");
            let mut warnings = Vec::new();
            let runtime = sandbox.build_runtime_with_policy(
                BotProgram::Wasm {
                    module,
                    source_path: PathBuf::from(format!("oversized-{resource}.wasm")),
                },
                7,
                &mut warnings,
                true,
                true,
            );

            assert!(
                runtime.is_fallback(),
                "oversized {resource} must not instantiate"
            );
            assert!(
                warnings
                    .iter()
                    .any(|warning| warning.contains("wasm instantiate failed")),
                "oversized {resource} should produce a safe instantiation warning: {warnings:?}"
            );
        }
    }

    #[test]
    fn strict_exhibition_memory_growth_traps_at_the_configured_ceiling() {
        let sandbox = BotSandbox::new_from_env();
        let engine = sandbox.engine.as_ref().expect("test requires Wasmtime");
        let module = Module::new(
            engine,
            r#"(module
                (memory 1)
                (func (export "bot_tick_v2")
                    (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)
                    (result i32)
                    i32.const 32
                    memory.grow
                    drop
                    i32.const 0))"#,
        )
        .expect("memory-growth fixture should compile");
        let mut warnings = Vec::new();
        let runtime = sandbox.build_runtime_with_policy(
            BotProgram::Wasm {
                module,
                source_path: PathBuf::from("growing-memory.wasm"),
            },
            11,
            &mut warnings,
            true,
            true,
        );
        assert!(!runtime.is_fallback(), "small initial memory should load");

        let mut exhibition = ExhibitionBotRuntime {
            model_id: "growing-memory".to_owned(),
            runtime,
            fuel_per_tick: sandbox.fuel_per_tick,
            tick: 0,
            warnings,
            faults: BotFaultCounts::default(),
        };
        let result = exhibition.next_action(ExhibitionBotObservation {
            self_health: 100,
            target_health: 100,
            personal_score: 0,
            team_score_delta: 0,
            objective_delta: 0,
            allies_alive: 1,
            enemies_alive: 1,
            lowest_ally_health: 100,
            slot: 0,
            mode: ArenaMatchMode::Arena,
        });

        assert!(result.is_err(), "growth beyond 2 MiB must trap");
        assert_eq!(exhibition.fault_counts().trap_count, 1);
    }

    #[test]
    fn exhibition_runtime_reports_a_trap_instead_of_hiding_it_as_idle() {
        let fixture = V2WasmFixture::new(&["trapping_live_fighter"]);
        let wasm = trapping_v2_wasm();
        fixture.write_wasm("trapping_live_fighter", &wasm);
        let sandbox = fixture.sandbox();
        let digest = sha256_hex(&wasm);
        let mut runtime = sandbox
            .build_exhibition_runtime("trapping_live_fighter", wasm.len(), &digest, 91)
            .expect("valid v2 export should instantiate before it traps");
        let result = runtime.next_action(ExhibitionBotObservation {
            self_health: 100,
            target_health: 100,
            personal_score: 0,
            team_score_delta: 0,
            objective_delta: 0,
            allies_alive: 1,
            enemies_alive: 1,
            lowest_ally_health: 0,
            slot: 0,
            mode: ArenaMatchMode::Arena,
        });

        assert!(result.is_err());
        assert_eq!(runtime.fault_counts().trap_count, 1);
    }

    #[test]
    fn full_size_evaluation_smoke_test_has_a_bounded_work_budget() {
        let fixture = V2WasmFixture::new(&["evaluation_a", "evaluation_b"]);
        let sandbox = fixture.sandbox();
        let team_size = 10u32;
        let rounds = 4u32;
        let max_ticks = 600u32;
        let outcome = sandbox.execute_team_battle(
            "evaluation_a",
            "evaluation_b",
            ArenaMatchMode::TeamDeathmatch,
            team_size,
            rounds,
            104_729,
            Some(max_ticks),
        );

        let observed_actions = total_actions(&outcome.team_a_action_counts)
            .saturating_add(total_actions(&outcome.team_b_action_counts));
        let maximum_actions = 2u64
            .saturating_mul(team_size as u64)
            .saturating_mul(rounds as u64)
            .saturating_mul(max_ticks as u64);

        assert_eq!(outcome.total_engagements, team_size * rounds);
        assert_eq!(outcome.team_a_v2_fighters, team_size);
        assert_eq!(outcome.team_b_v2_fighters, team_size);
        assert_eq!(outcome.rounds_detail.len(), rounds as usize);
        assert!(outcome
            .rounds_detail
            .iter()
            .all(|round| round.engagements == team_size));
        assert!(observed_actions > 0);
        assert!(observed_actions <= maximum_actions);
    }

    #[test]
    fn legacy_action_mapping_is_preserved_while_v2_is_strict() {
        assert_eq!(BotAction::from_v1_code(-1), BotAction::Charge);
        assert_eq!(BotAction::from_v1_code(5), BotAction::Attack);
        assert_eq!(BotAction::from_v2_code(4), Some(BotAction::Support));
        assert_eq!(BotAction::from_v2_code(-1), None);
        assert_eq!(BotAction::from_v2_code(5), None);
    }

    #[test]
    fn support_prevents_real_ally_damage_and_earns_only_causal_credit() {
        let seed = 77;
        let tick = 0;
        let mut attackers = vec![test_fighter(100), test_fighter(100)];
        let mut defenders = vec![test_fighter(100), test_fighter(50)];
        let attacker_actions = vec![BotAction::Idle, BotAction::Attack];
        let defender_actions = vec![BotAction::Support, BotAction::Idle];
        let targets = vec![None, Some(1)];
        let supports = support_targets(&defenders, &defender_actions);
        assert_eq!(supports[1], vec![0]);

        let raw_damage = outgoing_damage(
            BotAction::Attack,
            seed ^ 1u64.wrapping_mul(0xD6E8_FEB8_6659_FD93),
            tick,
        );
        let expected_damage = scale_damage(raw_damage, TEAM_SUPPORT_DAMAGE_PERCENT);
        apply_team_attacks(
            &mut attackers,
            &mut defenders,
            &attacker_actions,
            &defender_actions,
            &targets,
            &supports,
            seed,
            tick,
        );

        assert_eq!(defenders[1].state.health, 50 - expected_damage);
        assert_eq!(
            defenders[0].collaboration_score,
            raw_damage - expected_damage
        );

        let mut idle_attackers = vec![test_fighter(100), test_fighter(100)];
        let mut idle_defenders = vec![test_fighter(100), test_fighter(50)];
        let idle_actions = vec![BotAction::Idle, BotAction::Idle];
        let supports = support_targets(&idle_defenders, &defender_actions);
        apply_team_attacks(
            &mut idle_attackers,
            &mut idle_defenders,
            &idle_actions,
            &defender_actions,
            &[None, None],
            &supports,
            seed,
            tick,
        );
        assert_eq!(idle_defenders[0].collaboration_score, 0);
    }

    #[test]
    fn same_tick_multi_fighter_elimination_awards_a_separate_assist() {
        let mut attackers = vec![test_fighter(100), test_fighter(100)];
        let mut defenders = vec![test_fighter(15)];
        apply_team_attacks(
            &mut attackers,
            &mut defenders,
            &[BotAction::Attack, BotAction::Attack],
            &[BotAction::Idle],
            &[Some(0), Some(0)],
            &[Vec::new()],
            9,
            0,
        );

        assert!(defenders[0].state.health <= 0);
        assert_eq!(
            attackers
                .iter()
                .map(|fighter| fighter.collaboration_score)
                .sum::<i32>(),
            TEAM_ASSIST_SCORE
        );
        assert!(attackers.iter().any(|fighter| fighter.state.score >= 40));
    }

    #[test]
    fn v2_runtime_receives_mode_context_and_v1_export_remains_valid() {
        let mut config = Config::new();
        config.consume_fuel(true);
        let engine = Engine::new(&config).expect("test engine");
        let v2_module = Module::new(
            &engine,
            r#"(module
                (func (export "bot_tick_v2")
                    (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)
                    (result i32)
                    local.get 9))"#,
        )
        .expect("valid v2 module");
        validate_bot_tick_export(&v2_module).expect("v2 export should validate");

        let sandbox = BotSandbox::new_from_env();
        let mut warnings = Vec::new();
        let mut faults = BotFaultCounts::default();
        let mut runtime = sandbox.build_runtime(
            BotProgram::Wasm {
                module: v2_module,
                source_path: PathBuf::from("v2-test.wasm"),
            },
            1,
            &mut warnings,
        );
        let action = runtime.next_action(
            sandbox.fuel_per_tick,
            BotObservation {
                self_health: 100,
                target_health: 100,
                personal_score: 0,
                team_score_delta: -4,
                objective_delta: -2,
                allies_alive: 10,
                enemies_alive: 9,
                lowest_ally_health: 30,
                slot: 3,
                mode: ArenaMatchMode::Koth,
                tick: 12,
            },
            &mut warnings,
            &mut faults,
            "test",
        );
        assert_eq!(action, BotAction::Charge, "mode=3 must reach v2 param 9");
        assert!(runtime.uses_v2());

        let v1_module = Module::new(
            &engine,
            r#"(module
                (func (export "bot_tick")
                    (param i32 i32 i32 i32)
                    (result i32)
                    i32.const 1))"#,
        )
        .expect("valid legacy module");
        validate_bot_tick_export(&v1_module).expect("legacy export should remain valid");
    }

    #[test]
    fn execute_match_with_replay_captures_tick_frames() {
        let sandbox = BotSandbox::new_from_env();
        let execution = sandbox.execute_match_with_replay(
            "model_a",
            "model_b",
            ArenaMatchMode::TeamDeathmatch,
            121,
            Some(180),
        );
        assert_eq!(execution.outcome.mode, "tdm");
        assert_eq!(execution.replay.mode, "tdm");
        assert_eq!(
            execution.replay.total_ticks_executed,
            execution.outcome.ticks_executed
        );
        assert_eq!(
            execution.replay.captured_frames,
            execution.replay.frames.len()
        );
        assert!(!execution.replay.frames.is_empty());
        assert!(execution.replay.captured_frames <= sandbox.replay_max_frames());
        assert_eq!(execution.replay.frames[0].tick, 1);
    }
}
