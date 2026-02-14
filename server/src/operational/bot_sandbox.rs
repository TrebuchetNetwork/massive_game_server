use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use tracing::warn;
use wasmtime::{Config, Engine, ExternType, Module, Store, TypedFunc, ValType};

const DEFAULT_WASM_DIR: &str = "data/arena_bots";
const DEFAULT_FUEL_PER_TICK: u64 = 1_000_000;
const DEFAULT_MAX_TICKS: u32 = 600;
const MAX_ALLOWED_TICKS: u32 = 5_000;
const BOT_TICK_EXPORT: &str = "bot_tick";

#[derive(Clone)]
pub struct BotSandbox {
    engine: Engine,
    wasm_dir: PathBuf,
    fuel_per_tick: u64,
    default_max_ticks: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct BotMatchOutcome {
    pub winner_model_id: Option<String>,
    pub draw: bool,
    pub model_a_score: i32,
    pub model_b_score: i32,
    pub model_a_runtime: String,
    pub model_b_runtime: String,
    pub ticks_executed: u32,
    pub duration_ms: u64,
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
}

enum BotProgram {
    Wasm { module: Module, source_path: PathBuf },
    Fallback { reason: String },
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

        let mut config = Config::new();
        config.consume_fuel(true);
        let engine = Engine::new(&config).expect("failed to create wasmtime engine");

        Self {
            engine,
            wasm_dir,
            fuel_per_tick,
            default_max_ticks,
        }
    }

    pub fn default_max_ticks(&self) -> u32 {
        self.default_max_ticks
    }

    pub fn execute_duel(
        &self,
        model_a_id: &str,
        model_b_id: &str,
        seed: u64,
        requested_ticks: Option<u32>,
    ) -> BotMatchOutcome {
        let started_at = Instant::now();
        let mut warnings = Vec::new();
        let max_ticks = requested_ticks
            .unwrap_or(self.default_max_ticks)
            .max(1)
            .min(MAX_ALLOWED_TICKS);

        let program_a = self.load_program(model_a_id);
        let program_b = self.load_program(model_b_id);

        let mut runtime_a = self.build_runtime(program_a, seed ^ 0xA5A5_A5A5_A5A5_A5A5, &mut warnings);
        let mut runtime_b = self.build_runtime(program_b, seed ^ 0x5A5A_5A5A_5A5A_5A5A, &mut warnings);

        let runtime_a_name = runtime_a.runtime_name().to_owned();
        let runtime_b_name = runtime_b.runtime_name().to_owned();

        let mut a = FighterState {
            health: 100,
            score: 0,
        };
        let mut b = FighterState {
            health: 100,
            score: 0,
        };

        let mut ticks_executed = 0u32;
        for tick in 0..max_ticks {
            ticks_executed = tick + 1;
            let action_a = runtime_a.next_action(
                self.fuel_per_tick,
                a.health,
                b.health,
                a.score,
                tick,
                &mut warnings,
                "model_a",
            );
            let action_b = runtime_b.next_action(
                self.fuel_per_tick,
                b.health,
                a.health,
                b.score,
                tick,
                &mut warnings,
                "model_b",
            );

            resolve_combat_tick(
                &mut a,
                &mut b,
                action_a,
                action_b,
                seed,
                tick,
            );

            if a.health <= 0 || b.health <= 0 {
                break;
            }
        }

        let (winner_model_id, draw) = if a.health <= 0 && b.health <= 0 {
            (None, true)
        } else if a.health <= 0 {
            a.score -= 10;
            b.score += 50;
            (Some(model_b_id.to_owned()), false)
        } else if b.health <= 0 {
            b.score -= 10;
            a.score += 50;
            (Some(model_a_id.to_owned()), false)
        } else if a.score > b.score {
            (Some(model_a_id.to_owned()), false)
        } else if b.score > a.score {
            (Some(model_b_id.to_owned()), false)
        } else if a.health > b.health {
            (Some(model_a_id.to_owned()), false)
        } else if b.health > a.health {
            (Some(model_b_id.to_owned()), false)
        } else {
            (None, true)
        };

        let duration_ms = started_at.elapsed().as_millis() as u64;
        BotMatchOutcome {
            winner_model_id,
            draw,
            model_a_score: a.score.max(0),
            model_b_score: b.score.max(0),
            model_a_runtime: runtime_a_name,
            model_b_runtime: runtime_b_name,
            ticks_executed,
            duration_ms,
            warnings,
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
                reason: format!("model '{}' has invalid id format; fallback runtime used", model_id),
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

    fn next_action(
        &mut self,
        fuel_per_tick: u64,
        self_health: i32,
        enemy_health: i32,
        self_score: i32,
        tick: u32,
        warnings: &mut Vec<String>,
        side: &str,
    ) -> BotAction {
        let raw = match self {
            BotRuntime::Wasm { store, tick_fn } => {
                if let Err(err) = store.set_fuel(fuel_per_tick) {
                    warnings.push(format!("{}: failed to set wasm fuel: {}", side, err));
                    0
                } else {
                    match tick_fn.call(
                        store,
                        (self_health, enemy_health, self_score, tick as i32),
                    ) {
                        Ok(value) => value,
                        Err(err) => {
                            warnings.push(format!("{}: wasm bot_tick trapped: {}", side, err));
                            0
                        }
                    }
                }
            }
            BotRuntime::Fallback { prng_state } => fallback_action(*prng_state, tick, self_health, enemy_health),
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

fn fallback_action(prng_state: u64, tick: u32, self_health: i32, enemy_health: i32) -> i32 {
    if self_health < 25 {
        return BotAction::Defend as i32;
    }
    if enemy_health < 18 {
        return BotAction::Attack as i32;
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
    if !func
        .params()
        .all(|param| matches!(param, ValType::I32))
    {
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
        assert!(
            outcome.model_a_runtime == "fallback" || outcome.model_b_runtime == "fallback"
        );
    }

    #[test]
    fn sanitize_model_id_rejects_path_traversal() {
        assert!(sanitize_model_id("../etc/passwd").is_none());
        assert!(sanitize_model_id("bot-alpha_1").is_some());
    }
}
