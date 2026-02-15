use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use tracing::warn;
use wasmtime::{Config, Engine, ExternType, Module, Store, TypedFunc, ValType};

const DEFAULT_WASM_DIR: &str = "data/arena_bots";
const DEFAULT_FUEL_PER_TICK: u64 = 1_000_000;
const DEFAULT_MAX_TICKS: u32 = 600;
const DEFAULT_REPLAY_MAX_FRAMES: usize = 1024;
const MAX_ALLOWED_TICKS: u32 = 5_000;
const MAX_TEAM_BATTLE_SIZE: u32 = 20;
const MAX_TEAM_BATTLE_ROUNDS: u32 = 32;
const MAX_REPLAY_FRAMES: usize = 8_192;
const BOT_TICK_EXPORT: &str = "bot_tick";
const DEFAULT_RESPAWNS_NON_ARENA: i32 = 3;

#[derive(Clone)]
pub struct BotSandbox {
    engine: Engine,
    wasm_dir: PathBuf,
    fuel_per_tick: u64,
    default_max_ticks: u32,
    replay_max_frames: usize,
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
    pub winner_model_id: Option<String>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamBattleOutcome {
    pub mode: String,
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
    pub winner_model_id: Option<String>,
    pub draw: bool,
    pub duration_ms: u64,
    pub rounds_detail: Vec<TeamBattleRoundOutcome>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BotAction {
    Idle,
    Attack,
    Defend,
    Charge,
}

impl BotAction {
    fn from_code(raw: i32) -> Self {
        match raw.rem_euclid(4) {
            1 => Self::Attack,
            2 => Self::Defend,
            3 => Self::Charge,
            _ => Self::Idle,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Attack => "attack",
            Self::Defend => "defend",
            Self::Charge => "charge",
        }
    }
}

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
        store: Store<()>,
        tick_fn: TypedFunc<(i32, i32, i32, i32), i32>,
    },
    Fallback {
        prng_state: u64,
    },
}

struct FighterState {
    health: i32,
    score: i32,
    respawns_remaining: i32,
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
        let engine = Engine::new(&config).expect("failed to create wasmtime engine");

        Self {
            engine,
            wasm_dir,
            fuel_per_tick,
            default_max_ticks,
            replay_max_frames,
        }
    }

    pub fn default_max_ticks(&self) -> u32 {
        self.default_max_ticks
    }

    pub fn replay_max_frames(&self) -> usize {
        self.replay_max_frames
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
        let max_ticks = requested_ticks
            .unwrap_or(self.default_max_ticks)
            .max(1)
            .min(MAX_ALLOWED_TICKS);
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
            let action_a = runtime_a.next_action(
                self.fuel_per_tick,
                a.health,
                b.health,
                a.score,
                a.score - b.score,
                tick,
                mode,
                &mut warnings,
                "model_a",
            );
            let action_b = runtime_b.next_action(
                self.fuel_per_tick,
                b.health,
                a.health,
                b.score,
                b.score - a.score,
                tick,
                mode,
                &mut warnings,
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
            .max(1)
            .min(MAX_ALLOWED_TICKS);

        let mut team_a_round_wins = 0u32;
        let mut team_b_round_wins = 0u32;
        let mut round_draws = 0u32;
        let mut total_team_a_objective = 0i64;
        let mut total_team_b_objective = 0i64;
        let mut total_team_a_score = 0i64;
        let mut total_team_b_score = 0i64;
        let mut all_warnings = Vec::new();
        let mut rounds_detail = Vec::with_capacity(normalized_rounds as usize);

        for round in 0..normalized_rounds {
            let round_started_at = Instant::now();
            let mut round_draw_count = 0u32;
            let mut round_team_a_objective = 0i64;
            let mut round_team_b_objective = 0i64;
            let mut round_team_a_score = 0i64;
            let mut round_team_b_score = 0i64;

            for slot in 0..normalized_team_size {
                let battle_seed = seed
                    ^ ((round as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
                    ^ ((slot as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9));
                let engagement =
                    self.execute_match(model_a_id, model_b_id, mode, battle_seed, Some(max_ticks));

                round_team_a_objective += engagement.objective_a as i64;
                round_team_b_objective += engagement.objective_b as i64;
                round_team_a_score += engagement.model_a_score as i64;
                round_team_b_score += engagement.model_b_score as i64;
                if engagement.draw {
                    round_draw_count = round_draw_count.saturating_add(1);
                }

                if all_warnings.len() < 256 {
                    for warning in engagement.warnings {
                        all_warnings.push(format!(
                            "round={} slot={}: {}",
                            round + 1,
                            slot + 1,
                            warning
                        ));
                        if all_warnings.len() >= 256 {
                            break;
                        }
                    }
                }
            }

            total_team_a_objective += round_team_a_objective;
            total_team_b_objective += round_team_b_objective;
            total_team_a_score += round_team_a_score;
            total_team_b_score += round_team_b_score;

            let round_winner = compare_team_round(
                model_a_id,
                model_b_id,
                round_team_a_objective,
                round_team_b_objective,
                round_team_a_score,
                round_team_b_score,
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
                draws: round_draw_count,
                team_a_objective: saturating_i64_to_i32(round_team_a_objective),
                team_b_objective: saturating_i64_to_i32(round_team_b_objective),
                team_a_score: saturating_i64_to_i32(round_team_a_score),
                team_b_score: saturating_i64_to_i32(round_team_b_score),
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
                total_team_a_score,
                total_team_b_score,
            )
        };
        let draw = winner_model_id.is_none();

        TeamBattleOutcome {
            mode: mode.as_str().to_owned(),
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
            winner_model_id,
            draw,
            duration_ms: started_at.elapsed().as_millis() as u64,
            rounds_detail,
            warnings: all_warnings,
        }
    }

    fn build_runtime(
        &self,
        program: BotProgram,
        fallback_seed: u64,
        warnings: &mut Vec<String>,
    ) -> BotRuntime {
        match program {
            BotProgram::Wasm {
                module,
                source_path,
            } => {
                let mut store = Store::new(&self.engine, ());
                if let Err(err) = store.set_fuel(self.fuel_per_tick) {
                    warnings.push(format!(
                        "wasm store fuel init failed for '{}': {}. using fallback runtime",
                        source_path.display(),
                        err
                    ));
                    return BotRuntime::Fallback {
                        prng_state: fallback_seed,
                    };
                }

                match wasmtime::Instance::new(&mut store, &module, &[]) {
                    Ok(instance) => {
                        match instance.get_typed_func::<(i32, i32, i32, i32), i32>(
                            &mut store,
                            BOT_TICK_EXPORT,
                        ) {
                            Ok(tick_fn) => BotRuntime::Wasm { store, tick_fn },
                            Err(err) => {
                                warnings.push(format!(
                                    "missing/invalid '{}' export in '{}': {}. using fallback runtime",
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
                    Err(err) => {
                        warnings.push(format!(
                            "wasm instantiate failed for '{}': {}. using fallback runtime",
                            source_path.display(),
                            err
                        ));
                        BotRuntime::Fallback {
                            prng_state: fallback_seed,
                        }
                    }
                }
            }
            BotProgram::Fallback { reason } => {
                warnings.push(reason);
                BotRuntime::Fallback {
                    prng_state: fallback_seed,
                }
            }
        }
    }

    fn load_program(&self, model_id: &str) -> BotProgram {
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
            return BotProgram::Fallback {
                reason: format!(
                    "wasm not found for model '{}': expected '{}'; fallback runtime used",
                    model_id,
                    path.display()
                ),
            };
        }

        match fs::read(&path) {
            Ok(bytes) => match Module::from_binary(&self.engine, &bytes) {
                Ok(module) => match validate_bot_tick_export(&module) {
                    Ok(()) => BotProgram::Wasm {
                        module,
                        source_path: path,
                    },
                    Err(err) => {
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
                Err(err) => BotProgram::Fallback {
                    reason: format!(
                        "failed to compile wasm for model '{}': {}; fallback runtime used",
                        model_id, err
                    ),
                },
            },
            Err(err) => BotProgram::Fallback {
                reason: format!(
                    "failed to read wasm for model '{}': {}; fallback runtime used",
                    model_id, err
                ),
            },
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

    #[allow(clippy::too_many_arguments)]
    fn next_action(
        &mut self,
        fuel_per_tick: u64,
        self_health: i32,
        enemy_health: i32,
        self_score: i32,
        score_delta: i32,
        tick: u32,
        mode: ArenaMatchMode,
        warnings: &mut Vec<String>,
        side: &str,
    ) -> BotAction {
        let raw = match self {
            BotRuntime::Wasm { store, tick_fn } => {
                if let Err(err) = store.set_fuel(fuel_per_tick) {
                    warnings.push(format!("{}: failed to set wasm fuel: {}", side, err));
                    0
                } else {
                    match tick_fn.call(store, (self_health, enemy_health, self_score, tick as i32))
                    {
                        Ok(value) => value,
                        Err(err) => {
                            warnings.push(format!("{}: wasm bot_tick trapped: {}", side, err));
                            0
                        }
                    }
                }
            }
            BotRuntime::Fallback { prng_state } => fallback_action(
                *prng_state,
                tick,
                self_health,
                enemy_health,
                score_delta,
                mode,
            ),
        };

        if let BotRuntime::Fallback { prng_state } = self {
            *prng_state = next_prng(*prng_state ^ tick as u64);
        }

        BotAction::from_code(raw)
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
                BotAction::Idle => 0,
            };
            let push_b = match action_b {
                BotAction::Charge => 4,
                BotAction::Attack => 2,
                BotAction::Defend => 1,
                BotAction::Idle => 0,
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

            if tick % 24 == 0 {
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
            if tick % 10 == 0 {
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
            if tick % 4 == 0 {
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
            if tick % 3 == 0 {
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
    let Some(export) = module.get_export(BOT_TICK_EXPORT) else {
        return Err(format!("missing '{}' export", BOT_TICK_EXPORT));
    };

    let ExternType::Func(func) = export else {
        return Err(format!("'{}' export must be a function", BOT_TICK_EXPORT));
    };

    if func.params().len() != 4 || func.results().len() != 1 {
        return Err(format!(
            "'{}' signature must be (i32, i32, i32, i32) -> i32",
            BOT_TICK_EXPORT
        ));
    }
    if !func.params().all(|param| matches!(param, ValType::I32)) {
        return Err(format!("'{}' params must be i32", BOT_TICK_EXPORT));
    }
    if !matches!(func.results().next(), Some(ValType::I32)) {
        return Err(format!("'{}' return type must be i32", BOT_TICK_EXPORT));
    }
    Ok(())
}

fn sanitize_model_id(model_id: &str) -> Option<String> {
    let trimmed = model_id.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed
        .bytes()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == b'_' || ch == b'-' || ch == b'.')
    {
        Some(trimmed.to_owned())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_duel_runs_without_wasm_files() {
        let sandbox = BotSandbox::new_from_env();
        let outcome = sandbox.execute_duel("model_a", "model_b", 42, Some(120));
        assert!(outcome.ticks_executed > 0);
        assert!(outcome.model_a_score >= 0);
        assert!(outcome.model_b_score >= 0);
        assert!(outcome.duration_ms <= 5_000);
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
        assert!(outcome.duration_ms <= 30_000);
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
