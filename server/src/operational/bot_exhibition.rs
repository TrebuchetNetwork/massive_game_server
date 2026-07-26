use crate::core::types::PlayerID;
use crate::operational::arena::ratings::{load_ratings_response, ratings_path_from_env};
use crate::operational::bot_sandbox::{
    BotSandbox, ExhibitionBotAction, ExhibitionBotObservation, ExhibitionBotRuntime,
};
use crate::operational::monitoring::metrics;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime};
use tracing::{info, warn};

const DEFAULT_EXHIBITION_ROSTER_SIZE: usize = 10;
const MAX_EXHIBITION_ROSTER_SIZE: usize = 64;
const DEFAULT_PREPARED_RUNTIMES_PER_FIGHTER: usize = 2;
const MAX_PREPARED_RUNTIMES_PER_FIGHTER: usize = 8;
const MAX_EXHIBITION_WASM_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct ExhibitionFighter {
    pub rank: usize,
    pub model_id: String,
    pub model_name: String,
    pub strategy_rating: f64,
    pub wasm_bytes: usize,
    pub wasm_sha256: String,
}

impl ExhibitionFighter {
    pub fn display_name(&self) -> String {
        format!("#{} {}", self.rank, self.model_name)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExhibitionAssignment {
    pub fighter: ExhibitionFighter,
    pub team_id: u8,
    /// Stable zero-based slot within the live team, matching the v2 ABI.
    pub slot: i32,
    pub rotation: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExhibitionDecision {
    pub assignment: ExhibitionAssignment,
    pub action: ExhibitionBotAction,
    pub tick: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExhibitionRefreshOutcome {
    Disabled,
    Busy,
    Unchanged,
    Refreshed,
    RetryRequired,
    InvalidSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExhibitionBinding {
    team_id: u8,
    team_slot: i32,
    seed: u64,
}

struct ExhibitionRuntimeEntry {
    assignment: ExhibitionAssignment,
    binding: ExhibitionBinding,
    runtime: ExhibitionBotRuntime,
}

struct ExhibitionRosterState {
    modified_at: Option<SystemTime>,
    season_id: Option<String>,
    /// Fighters requested by the last accepted ratings snapshot. Keep this
    /// separate from the loadable live roster so a transient artifact failure
    /// can recover without requiring the ratings file to be touched again.
    published_fighters: Arc<Vec<ExhibitionFighter>>,
    fighters: Arc<Vec<ExhibitionFighter>>,
    generation: u64,
    rotation: u64,
    /// Protected by the roster lock so a generation commit cannot race the
    /// authoritative transition into an Active round.
    round_active: bool,
}

struct PreparedRuntimePool {
    generation: u64,
    by_model: HashMap<String, Vec<ExhibitionBotRuntime>>,
}

impl PreparedRuntimePool {
    fn empty(generation: u64) -> Self {
        Self {
            generation,
            by_model: HashMap::new(),
        }
    }

    fn pop(&mut self, generation: u64, model_id: &str) -> Option<ExhibitionBotRuntime> {
        if self.generation != generation {
            return None;
        }
        self.by_model.get_mut(model_id)?.pop()
    }

    fn is_below_target(
        &self,
        generation: u64,
        fighters: &[ExhibitionFighter],
        target: usize,
    ) -> bool {
        self.generation != generation
            || fighters
                .iter()
                .any(|fighter| self.by_model.get(&fighter.model_id).map_or(0, Vec::len) < target)
    }
}

struct RefreshGuard<'a>(&'a AtomicBool);

impl Drop for RefreshGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Owns the live instances of published weekly fighters.
///
/// This is an exhibition-only adapter. It never calls the arena service's
/// match reporting or persistence paths, so human matches cannot affect the
/// official deterministic ratings. Expensive artifact reads, compilation,
/// and runtime rebuilding are performed only by `refresh_if_needed`, which is
/// called from a blocking background worker rather than the game loop.
pub struct ArenaExhibition {
    enabled: bool,
    ratings_path: PathBuf,
    roster_limit: usize,
    prepared_per_fighter: usize,
    sandbox: Option<BotSandbox>,
    roster: RwLock<ExhibitionRosterState>,
    prepared: Mutex<PreparedRuntimePool>,
    runtimes: Mutex<HashMap<PlayerID, ExhibitionRuntimeEntry>>,
    pending: Mutex<HashMap<PlayerID, ExhibitionBinding>>,
    requested_rotation: AtomicU64,
    round_rotation_requested: AtomicBool,
    refresh_running: AtomicBool,
}

impl ArenaExhibition {
    pub fn new_from_env() -> Self {
        let enabled = env_bool("MGS_ARENA_EXHIBITION_ENABLED").unwrap_or(false);
        let roster_limit = std::env::var("MGS_ARENA_EXHIBITION_ROSTER_SIZE")
            .ok()
            .and_then(|raw| raw.trim().parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_EXHIBITION_ROSTER_SIZE)
            .min(MAX_EXHIBITION_ROSTER_SIZE);
        let prepared_per_fighter = std::env::var("MGS_ARENA_EXHIBITION_PREPARED_PER_FIGHTER")
            .ok()
            .and_then(|raw| raw.trim().parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_PREPARED_RUNTIMES_PER_FIGHTER)
            .min(MAX_PREPARED_RUNTIMES_PER_FIGHTER);
        let ratings_path = ratings_path_from_env();
        // Capture the version before reading/building so a concurrent publish
        // is guaranteed to look newer to the background refresher.
        let modified_at = file_modified_at(&ratings_path);
        let sandbox = enabled.then(BotSandbox::new_from_env);
        let (season_id, published_fighters) = if enabled {
            load_published_roster(&ratings_path, roster_limit)
        } else {
            (None, Vec::new())
        };
        let (prepared_fighters, prepared_pool) = match sandbox.as_ref() {
            Some(sandbox) => prepare_runtime_pool(
                sandbox,
                published_fighters.clone(),
                prepared_per_fighter,
                0,
                0,
            ),
            None => (published_fighters.clone(), PreparedRuntimePool::empty(0)),
        };
        // The live roster is a single fairness unit. Never start a partial
        // generation that silently omits or substitutes a published fighter.
        let (initial_fighters, prepared) = if prepared_fighters == published_fighters {
            (prepared_fighters, prepared_pool)
        } else {
            warn!(
                requested_fighters = published_fighters.len(),
                prepared_fighters = prepared_fighters.len(),
                "Arena exhibition startup failed closed on a partial roster"
            );
            (Vec::new(), PreparedRuntimePool::empty(0))
        };
        if enabled {
            if initial_fighters.is_empty() {
                warn!("Arena exhibition enabled, but no verified published fighters are available");
            } else {
                info!(
                    fighters = initial_fighters.len(),
                    prepared_per_fighter,
                    "Arena exhibition loaded and prepared verified weekly fighters"
                );
            }
        }

        Self {
            enabled,
            ratings_path,
            roster_limit,
            prepared_per_fighter,
            sandbox,
            roster: RwLock::new(ExhibitionRosterState {
                modified_at,
                season_id,
                published_fighters: Arc::new(published_fighters),
                fighters: Arc::new(initial_fighters),
                generation: 0,
                rotation: 0,
                round_active: false,
            }),
            prepared: Mutex::new(prepared),
            runtimes: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            requested_rotation: AtomicU64::new(0),
            round_rotation_requested: AtomicBool::new(false),
            refresh_running: AtomicBool::new(false),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn active_runtime_count(&self) -> usize {
        self.runtimes.lock().len()
    }

    pub fn roster(&self) -> Arc<Vec<ExhibitionFighter>> {
        self.roster.read().fighters.clone()
    }

    /// True only while an Active round is using the fully committed rotation.
    /// A missed intermission therefore benches the stale side assignment until
    /// the pending generation can be installed at a later boundary.
    pub fn active_round_ready(&self) -> bool {
        let roster = self.roster.read();
        roster.round_active && self.requested_rotation.load(Ordering::Acquire) == roster.rotation
    }

    /// Records one rotation request for the current completed round. The
    /// background refresher performs the rebuild; this method does no I/O or
    /// compilation and is safe to call from the authoritative loop.
    pub fn request_round_rotation(&self) {
        if !self.enabled {
            return;
        }

        // This method is invoked on every Ended tick. Only the first tick
        // transitions the phase; a still-pending request deliberately remains
        // pending so a slow rebuild cannot skip side-swap parity.
        let round_just_ended = {
            let mut roster = self.roster.write();
            std::mem::replace(&mut roster.round_active, false)
        };
        if round_just_ended
            && self
                .round_rotation_requested
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            self.requested_rotation.fetch_add(1, Ordering::AcqRel);
            metrics::record_arena_exhibition_runtime_event("rotation_requested");
        }
    }

    pub fn mark_round_started(&self) {
        let committed_rotation = {
            let mut roster = self.roster.write();
            roster.round_active = true;
            roster.rotation
        };
        if self.requested_rotation.load(Ordering::Acquire) == committed_rotation {
            self.round_rotation_requested
                .store(false, Ordering::Release);
        }
    }

    /// Attach a prebuilt runtime. This function never reads or compiles a WASM
    /// artifact, so bot population changes cannot introduce frame-time spikes.
    pub fn attach_bot(
        &self,
        player_id: PlayerID,
        team_id: u8,
        team_slot: i32,
        seed: u64,
    ) -> Option<ExhibitionAssignment> {
        if !self.enabled || !matches!(team_id, 1 | 2) || team_slot < 0 {
            return None;
        }
        let binding = ExhibitionBinding {
            team_id,
            team_slot,
            seed,
        };

        for _attempt in 0..2 {
            let (generation, rotation, roster, round_active) = {
                let state = self.roster.read();
                (
                    state.generation,
                    state.rotation,
                    state.fighters.clone(),
                    state.round_active,
                )
            };
            if round_active || roster.is_empty() {
                self.queue_pending_binding(player_id, binding);
                return None;
            }

            let fighter =
                roster[fair_fighter_index(roster.len(), team_id, team_slot, rotation)].clone();
            let runtime = self.prepared.lock().pop(generation, &fighter.model_id);
            let Some(runtime) = runtime else {
                self.queue_pending_binding(player_id, binding);
                metrics::record_arena_exhibition_runtime_event("prepared_pool_exhausted");
                return None;
            };
            let assignment = ExhibitionAssignment {
                fighter,
                team_id,
                slot: team_slot,
                rotation,
            };

            let mut runtimes = self.runtimes.lock();
            let mut pending = self.pending.lock();
            let current = self.roster.read();
            if current.generation != generation
                || current.rotation != rotation
                || current.round_active
            {
                drop(current);
                drop(pending);
                drop(runtimes);
                continue;
            }
            pending.remove(&player_id);
            runtimes.insert(
                player_id.clone(),
                ExhibitionRuntimeEntry {
                    assignment: assignment.clone(),
                    binding: binding.clone(),
                    runtime,
                },
            );
            metrics::record_arena_exhibition_runtime_event("attach_success");
            metrics::set_arena_exhibition_runtimes_active(runtimes.len());
            return Some(assignment);
        }

        self.queue_pending_binding(player_id.clone(), binding);
        metrics::record_arena_exhibition_runtime_event("generic_fallback");
        warn!(
            player_id = player_id.as_ref(),
            team_id,
            team_slot,
            "No prebuilt verified fighter was available; retaining generic live bot identity"
        );
        None
    }

    fn queue_pending_binding(&self, player_id: PlayerID, binding: ExhibitionBinding) {
        let runtimes = self.runtimes.lock();
        let mut pending = self.pending.lock();
        if !runtimes.contains_key(&player_id) {
            pending.insert(player_id, binding);
            metrics::record_arena_exhibition_runtime_event("attach_pending");
        }
    }

    pub fn current_assignment(&self, player_id: &PlayerID) -> Option<ExhibitionAssignment> {
        self.runtimes
            .lock()
            .get(player_id)
            .map(|entry| entry.assignment.clone())
    }

    pub fn slot_is_reserved(&self, team_id: u8, team_slot: i32) -> bool {
        let runtimes = self.runtimes.lock();
        let pending = self.pending.lock();
        runtimes
            .values()
            .any(|entry| entry.binding.team_id == team_id && entry.binding.team_slot == team_slot)
            || pending
                .values()
                .any(|binding| binding.team_id == team_id && binding.team_slot == team_slot)
    }

    pub fn detach_bot(&self, player_id: &PlayerID) {
        let mut runtimes = self.runtimes.lock();
        let mut pending = self.pending.lock();
        runtimes.remove(player_id);
        pending.remove(player_id);
        metrics::set_arena_exhibition_runtimes_active(runtimes.len());
    }

    pub fn next_action(
        &self,
        player_id: &PlayerID,
        observation: ExhibitionBotObservation,
    ) -> Option<ExhibitionDecision> {
        // Do not advance strategy-local time during Waiting/Ended or while an
        // intermission rebuild is still pending. The AI caller treats this as
        // a bench state, not a runtime failure.
        if !self.active_round_ready() {
            metrics::record_arena_exhibition_runtime_event("decision_benched");
            return None;
        }
        let mut runtimes = self.runtimes.lock();
        let started_at = Instant::now();
        let Some(entry) = runtimes.get_mut(player_id) else {
            metrics::record_arena_exhibition_runtime_event("runtime_missing");
            metrics::record_arena_exhibition_runtime_event("generic_fallback");
            warn!(
                player_id = player_id.as_ref(),
                "Model identity had no live runtime; generic fallback required"
            );
            return None;
        };
        let tick = entry.runtime.tick();
        let result = entry.runtime.next_action(observation);
        let assignment = entry.assignment.clone();
        metrics::record_arena_exhibition_decision_time(started_at.elapsed().as_secs_f64());
        match result {
            Ok(action) => {
                metrics::record_arena_exhibition_runtime_event("decision_success");
                Some(ExhibitionDecision {
                    assignment,
                    action,
                    tick,
                })
            }
            Err(err) => {
                let runtime_failure = runtimes
                    .get(player_id)
                    .map(|runtime| runtime.runtime.fault_counts())
                    .map_or("runtime_failure", |faults| {
                        if faults.trap_count > 0 {
                            "trap"
                        } else if faults.fuel_error_count > 0 {
                            "fuel_failure"
                        } else if faults.invalid_action_count > 0 {
                            "invalid_action"
                        } else {
                            "runtime_failure"
                        }
                    });
                metrics::record_arena_exhibition_runtime_event(runtime_failure);
                metrics::record_arena_exhibition_runtime_event("generic_fallback");
                warn!(
                    player_id = player_id.as_ref(),
                    reason = err,
                    "Published fighter benched after a live runtime failure"
                );
                if let Some(failed) = runtimes.remove(player_id) {
                    self.pending
                        .lock()
                        .insert(player_id.clone(), failed.binding);
                    metrics::record_arena_exhibition_runtime_event("runtime_recovery_pending");
                }
                metrics::set_arena_exhibition_runtimes_active(runtimes.len());
                None
            }
        }
    }

    /// Poll and prepare a new generation. This method may read/compile WASM
    /// and therefore must only run on a blocking background thread.
    pub fn refresh_if_needed(&self) -> ExhibitionRefreshOutcome {
        if !self.enabled {
            return ExhibitionRefreshOutcome::Disabled;
        }
        if self
            .refresh_running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return ExhibitionRefreshOutcome::Busy;
        }
        let _guard = RefreshGuard(&self.refresh_running);
        let Some(sandbox) = self.sandbox.as_ref() else {
            return ExhibitionRefreshOutcome::Disabled;
        };

        let modified_at = file_modified_at(&self.ratings_path);
        let target_rotation = self.requested_rotation.load(Ordering::Acquire);
        let (
            current_modified,
            current_generation,
            current_rotation,
            current_published_fighters,
            current_fighters,
            round_active,
        ) = {
            let state = self.roster.read();
            (
                state.modified_at,
                state.generation,
                state.rotation,
                state.published_fighters.clone(),
                state.fighters.clone(),
                state.round_active,
            )
        };
        let ratings_changed = modified_at != current_modified;
        let rotation_changed = target_rotation != current_rotation;
        let roster_recovery_needed = current_published_fighters != current_fighters;
        let pending_bindings_present = !self.pending.lock().is_empty();
        let pool_needs_refill = self.prepared.lock().is_below_target(
            current_generation,
            &current_fighters,
            self.prepared_per_fighter,
        );
        let generation_change_required = ratings_changed
            || rotation_changed
            || roster_recovery_needed
            || pending_bindings_present;
        if !generation_change_required && !pool_needs_refill {
            return ExhibitionRefreshOutcome::Unchanged;
        }
        if generation_change_required && round_active {
            metrics::record_arena_exhibition_runtime_event("refresh_deferred_active_round");
            if pool_needs_refill {
                return self.refill_prepared_pool(
                    sandbox,
                    current_generation,
                    current_rotation,
                    &current_fighters,
                );
            }
            return ExhibitionRefreshOutcome::Unchanged;
        }
        if !generation_change_required {
            return self.refill_prepared_pool(
                sandbox,
                current_generation,
                current_rotation,
                &current_fighters,
            );
        }

        let (season_id, requested_fighters) = if ratings_changed {
            let response = load_ratings_response(&self.ratings_path);
            if !response.active || response.roster.is_empty() {
                self.mark_refresh_attempt(current_generation, modified_at);
                warn!("Arena exhibition kept its previous generation after an invalid ratings refresh");
                return ExhibitionRefreshOutcome::InvalidSnapshot;
            }
            let season_id = response.season_id.clone();
            let fighters = select_published_fighters(response, self.roster_limit);
            if fighters.is_empty() {
                self.mark_refresh_attempt(current_generation, modified_at);
                return ExhibitionRefreshOutcome::InvalidSnapshot;
            }
            (season_id, fighters)
        } else {
            let state = self.roster.read();
            (
                state.season_id.clone(),
                state.published_fighters.as_ref().clone(),
            )
        };

        let next_generation = current_generation.saturating_add(1);
        let (candidate_fighters, candidate_pool) = prepare_runtime_pool(
            sandbox,
            requested_fighters.clone(),
            self.prepared_per_fighter,
            next_generation,
            target_rotation,
        );
        if candidate_fighters != requested_fighters {
            warn!(
                requested_fighters = requested_fighters.len(),
                prepared_fighters = candidate_fighters.len(),
                "Arena exhibition kept its previous generation because the published roster was not fully loadable"
            );
            metrics::record_arena_exhibition_runtime_event("generation_prepare_incomplete");
            return ExhibitionRefreshOutcome::RetryRequired;
        }
        if candidate_fighters.is_empty() {
            // Pending generic bots remain queued until a later ratings publish
            // makes a complete verified roster available.
            return ExhibitionRefreshOutcome::Unchanged;
        }

        let bindings = snapshot_bindings(&self.runtimes, &self.pending);
        let replacement_runtimes = match build_replacement_runtimes(
            sandbox,
            &candidate_fighters,
            &bindings.all(),
            target_rotation,
        ) {
            Some(replacements) => replacements,
            None => return ExhibitionRefreshOutcome::RetryRequired,
        };

        let mut runtimes = self.runtimes.lock();
        let mut pending = self.pending.lock();
        if bindings_from_state(&runtimes, &pending) != bindings {
            metrics::record_arena_exhibition_runtime_event("refresh_race_retry");
            return ExhibitionRefreshOutcome::RetryRequired;
        }
        let mut roster = self.roster.write();
        if roster.generation != current_generation || roster.round_active {
            return ExhibitionRefreshOutcome::RetryRequired;
        }
        let mut prepared = self.prepared.lock();

        *runtimes = replacement_runtimes;
        pending.clear();
        roster.published_fighters = Arc::new(requested_fighters);
        roster.fighters = Arc::new(candidate_fighters);
        roster.modified_at = modified_at;
        roster.season_id = season_id.clone();
        roster.generation = next_generation;
        roster.rotation = target_rotation;
        *prepared = candidate_pool;
        metrics::set_arena_exhibition_runtimes_active(runtimes.len());
        metrics::record_arena_exhibition_runtime_event(if ratings_changed {
            "ratings_rotated"
        } else if rotation_changed {
            "round_rotated"
        } else if pending_bindings_present {
            "pending_attached"
        } else {
            "roster_recovered"
        });
        info!(
            season_id = season_id.as_deref().unwrap_or("unknown"),
            generation = roster.generation,
            rotation = roster.rotation,
            fighters = roster.fighters.len(),
            active_runtimes = runtimes.len(),
            "Arena exhibition committed a prepared generation"
        );
        ExhibitionRefreshOutcome::Refreshed
    }

    fn refill_prepared_pool(
        &self,
        sandbox: &BotSandbox,
        expected_generation: u64,
        expected_rotation: u64,
        fighters: &[ExhibitionFighter],
    ) -> ExhibitionRefreshOutcome {
        if fighters.is_empty() {
            return ExhibitionRefreshOutcome::Unchanged;
        }
        let (prepared_fighters, candidate_pool) = prepare_runtime_pool(
            sandbox,
            fighters.to_vec(),
            self.prepared_per_fighter,
            expected_generation,
            expected_rotation,
        );
        if prepared_fighters != fighters {
            metrics::record_arena_exhibition_runtime_event("pool_refill_incomplete");
            return ExhibitionRefreshOutcome::RetryRequired;
        }

        // A pool-only refill does not depend on the active bindings. Commit it
        // against the roster generation directly so concurrent joins/leaves
        // cannot repeatedly discard a fully prepared pool.
        let roster = self.roster.write();
        if roster.generation != expected_generation
            || roster.rotation != expected_rotation
            || roster.fighters.as_slice() != fighters
        {
            metrics::record_arena_exhibition_runtime_event("refresh_race_retry");
            return ExhibitionRefreshOutcome::RetryRequired;
        }
        let mut prepared = self.prepared.lock();
        *prepared = candidate_pool;
        metrics::record_arena_exhibition_runtime_event("pool_refilled");
        info!(
            generation = roster.generation,
            rotation = roster.rotation,
            fighters = roster.fighters.len(),
            "Arena exhibition committed a prepared runtime-pool refill"
        );
        ExhibitionRefreshOutcome::Refreshed
    }

    fn mark_refresh_attempt(&self, expected_generation: u64, modified_at: Option<SystemTime>) {
        let mut roster = self.roster.write();
        if roster.generation == expected_generation {
            roster.modified_at = modified_at;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExhibitionBindingSnapshot {
    active: HashMap<PlayerID, ExhibitionBinding>,
    pending: HashMap<PlayerID, ExhibitionBinding>,
}

impl ExhibitionBindingSnapshot {
    fn all(&self) -> HashMap<PlayerID, ExhibitionBinding> {
        let mut all = self.active.clone();
        for (player_id, binding) in &self.pending {
            all.entry(player_id.clone())
                .or_insert_with(|| binding.clone());
        }
        all
    }
}

fn snapshot_bindings(
    runtimes: &Mutex<HashMap<PlayerID, ExhibitionRuntimeEntry>>,
    pending: &Mutex<HashMap<PlayerID, ExhibitionBinding>>,
) -> ExhibitionBindingSnapshot {
    let runtimes = runtimes.lock();
    let pending = pending.lock();
    bindings_from_state(&runtimes, &pending)
}

fn bindings_from_state(
    entries: &HashMap<PlayerID, ExhibitionRuntimeEntry>,
    pending: &HashMap<PlayerID, ExhibitionBinding>,
) -> ExhibitionBindingSnapshot {
    ExhibitionBindingSnapshot {
        active: bindings_from_entries(entries),
        pending: pending.clone(),
    }
}

fn bindings_from_entries(
    entries: &HashMap<PlayerID, ExhibitionRuntimeEntry>,
) -> HashMap<PlayerID, ExhibitionBinding> {
    entries
        .iter()
        .map(|(player_id, entry)| (player_id.clone(), entry.binding.clone()))
        .collect()
}

fn build_replacement_runtimes(
    sandbox: &BotSandbox,
    fighters: &[ExhibitionFighter],
    bindings: &HashMap<PlayerID, ExhibitionBinding>,
    rotation: u64,
) -> Option<HashMap<PlayerID, ExhibitionRuntimeEntry>> {
    let mut replacements = HashMap::with_capacity(bindings.len());
    for (player_id, binding) in bindings {
        let fighter = fighters
            [fair_fighter_index(fighters.len(), binding.team_id, binding.team_slot, rotation)]
        .clone();
        let seed = runtime_seed(binding, &fighter, rotation);
        let runtime = match sandbox.build_exhibition_runtime(
            &fighter.model_id,
            fighter.wasm_bytes,
            &fighter.wasm_sha256,
            seed,
        ) {
            Ok(runtime) => runtime,
            Err(err) => {
                warn!(
                    player_id = player_id.as_ref(),
                    model_id = fighter.model_id,
                    reason = err,
                    "Could not prepare exact exhibition runtime; aborting generation"
                );
                return None;
            }
        };
        replacements.insert(
            player_id.clone(),
            ExhibitionRuntimeEntry {
                assignment: ExhibitionAssignment {
                    fighter,
                    team_id: binding.team_id,
                    slot: binding.team_slot,
                    rotation,
                },
                binding: binding.clone(),
                runtime,
            },
        );
    }
    Some(replacements)
}

fn prepare_runtime_pool(
    sandbox: &BotSandbox,
    fighters: Vec<ExhibitionFighter>,
    per_fighter: usize,
    generation: u64,
    rotation: u64,
) -> (Vec<ExhibitionFighter>, PreparedRuntimePool) {
    let mut verified = Vec::with_capacity(fighters.len());
    let mut pool = PreparedRuntimePool::empty(generation);
    for fighter in fighters {
        let mut prepared = Vec::with_capacity(per_fighter);
        for copy in 0..per_fighter {
            let seed = (fighter.rank as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
                ^ rotation.rotate_left(17)
                ^ copy as u64;
            match sandbox.build_exhibition_runtime(
                &fighter.model_id,
                fighter.wasm_bytes,
                &fighter.wasm_sha256,
                seed,
            ) {
                Ok(runtime) => prepared.push(runtime),
                Err(err) => {
                    warn!(
                        model_id = fighter.model_id,
                        reason = err,
                        "Published fighter rejected while preparing live runtimes"
                    );
                    break;
                }
            }
        }
        if prepared.len() == per_fighter {
            metrics::record_arena_exhibition_runtime_event("prewarm_success");
            pool.by_model.insert(fighter.model_id.clone(), prepared);
            verified.push(fighter);
        } else {
            metrics::record_arena_exhibition_runtime_event("prewarm_failure");
        }
    }
    (verified, pool)
}

fn fair_fighter_index(roster_len: usize, team_id: u8, team_slot: i32, rotation: u64) -> usize {
    if roster_len == 0 {
        return 0;
    }
    let cycle_shift = (rotation / 2) as usize % roster_len;
    let swapped = rotation % 2 == 1;
    let first_side = (team_id == 1) ^ swapped;
    let side_offset = if first_side { 0 } else { roster_len / 2 };
    (team_slot.max(0) as usize + cycle_shift + side_offset) % roster_len
}

fn runtime_seed(binding: &ExhibitionBinding, fighter: &ExhibitionFighter, rotation: u64) -> u64 {
    binding.seed
        ^ (fighter.rank as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (binding.team_slot as u64).rotate_left(11)
        ^ rotation.rotate_left(23)
}

fn load_published_roster(path: &Path, limit: usize) -> (Option<String>, Vec<ExhibitionFighter>) {
    let response = load_ratings_response(path);
    let season_id = response.season_id.clone();
    (season_id, select_published_fighters(response, limit))
}

fn select_published_fighters(
    response: crate::operational::arena::ArenaRatingsResponse,
    limit: usize,
) -> Vec<ExhibitionFighter> {
    if !response.active {
        return Vec::new();
    }
    let selected: Vec<_> = response.roster.into_iter().take(limit).collect();
    if selected.len() != limit {
        return Vec::new();
    }
    let mut fighters = Vec::with_capacity(selected.len());
    for fighter in selected {
        if !fighter.compiled
            || fighter.simulated
            || fighter.integrity_status.as_deref() != Some("verified_wasm")
        {
            return Vec::new();
        }
        let Some(wasm_bytes) = fighter
            .wasm_bytes
            .filter(|bytes| (1..=MAX_EXHIBITION_WASM_BYTES).contains(bytes))
        else {
            return Vec::new();
        };
        let Some(wasm_sha256) = fighter.wasm_sha256 else {
            return Vec::new();
        };
        if wasm_sha256.len() != 64
            || !wasm_sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Vec::new();
        }
        fighters.push(ExhibitionFighter {
            rank: fighter.rank,
            model_id: fighter.model_id,
            model_name: fighter.model_name,
            strategy_rating: fighter.strategy_rating,
            wasm_bytes,
            wasm_sha256,
        });
    }
    fighters
}

fn file_modified_at(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
}

fn env_bool(name: &str) -> Option<bool> {
    let raw = std::env::var(name).ok()?;
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operational::arena::{
        ArenaRatingMethodology, ArenaRatingRanking, ArenaRatingsResponse, ArenaSeasonRatingView,
    };
    use crate::operational::bot_sandbox::ArenaMatchMode;

    // Equivalent to a tiny `bot_tick_v2` module returning `(slot + tick) % 5`.
    const CYCLING_V2_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x10, 0x01, 0x60, 0x0b, 0x7f, 0x7f,
        0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x7f, 0x01, 0x7f, 0x03, 0x02, 0x01, 0x00,
        0x07, 0x0f, 0x01, 0x0b, b'b', b'o', b't', b'_', b't', b'i', b'c', b'k', b'_', b'v', b'2',
        0x00, 0x00, 0x0a, 0x0c, 0x01, 0x0a, 0x00, 0x20, 0x08, 0x20, 0x0a, 0x6a, 0x41, 0x05, 0x6f,
        0x0b,
    ];

    struct ExhibitionWasmFixture {
        dir: PathBuf,
    }

    impl ExhibitionWasmFixture {
        fn new(model_ids: &[&str]) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "mgs-exhibition-refresh-test-{}",
                uuid::Uuid::new_v4()
            ));
            std::fs::create_dir_all(&dir).expect("temporary WASM directory should be created");
            for model_id in model_ids {
                std::fs::write(dir.join(format!("{model_id}.wasm")), CYCLING_V2_WASM)
                    .expect("test WASM should be written");
            }
            Self { dir }
        }

        fn write_model(&self, model_id: &str) {
            std::fs::write(self.dir.join(format!("{model_id}.wasm")), CYCLING_V2_WASM)
                .expect("test WASM should be written");
        }
    }

    impl Drop for ExhibitionWasmFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn rating(rank: usize, verified: bool) -> ArenaSeasonRatingView {
        ArenaSeasonRatingView {
            rank,
            provider_rank: rank,
            model_id: format!("model_{rank}"),
            model_name: format!("Model {rank}"),
            provider_model: format!("provider/model-{rank}"),
            canonical_slug: None,
            personal_rating: 50.0,
            team_rating: 50.0,
            collaboration_rating: 50.0,
            overall_rating: 50.0,
            world_rating: 50.0,
            strategy_rating: 50.0,
            source_bytes: 100,
            source_limit_bytes: 50 * 1024,
            source_sha256: Some("a".repeat(64)),
            compiled: true,
            wasm_bytes: Some(100),
            wasm_sha256: Some("b".repeat(64)),
            compile_attempts: Some(1),
            simulated: false,
            wins: 1,
            losses: 0,
            draws: 0,
            matches_played: 1,
            evaluation_engagements: 1,
            personal_score_for: 0,
            personal_score_against: 0,
            team_objective_for: 0,
            team_objective_against: 0,
            collaboration_score_for: 0,
            collaboration_score_against: 0,
            world_points: 0,
            world_round_wins: 0,
            world_eliminations: 0,
            world_deaths: 0,
            world_collaboration_score: 0,
            season_points: 0,
            epochs_played: 0,
            epoch_wins: 0,
            best_epoch_rank: None,
            last_epoch_rank: None,
            integrity_status: verified.then(|| "verified_wasm".to_owned()),
        }
    }

    fn ratings(roster: Vec<ArenaSeasonRatingView>) -> ArenaRatingsResponse {
        ArenaRatingsResponse {
            schema_version: 1,
            active: true,
            status: "active".to_owned(),
            season_id: Some("weekly-test".to_owned()),
            generated_at: Some("2026-07-25T00:00:00Z".to_owned()),
            ranking: Some(ArenaRatingRanking {
                source: "test".to_owned(),
                window: "weekly".to_owned(),
                retrieved_at: "2026-07-25T00:00:00Z".to_owned(),
            }),
            methodology: Some(ArenaRatingMethodology {
                prompt_sha256: "a".repeat(64),
                source_limit_bytes: 50 * 1024,
                modes: vec!["arena".to_owned()],
                seed_sets: vec![1],
                team_size: 1,
                rounds: 1,
                personal_weight: 0.4,
                team_weight: 0.35,
                collaboration_weight: 0.25,
                duel_strategy_weight: 1.0,
                world_strategy_weight: 0.0,
                world_squad_size: 0,
                world_max_ticks: 0,
                collaboration_kind: "test".to_owned(),
                notes: Vec::new(),
            }),
            league: None,
            roster,
        }
    }

    fn phase_test_exhibition() -> ArenaExhibition {
        ArenaExhibition {
            enabled: true,
            ratings_path: PathBuf::new(),
            roster_limit: 10,
            prepared_per_fighter: 1,
            sandbox: None,
            roster: RwLock::new(ExhibitionRosterState {
                modified_at: None,
                season_id: None,
                published_fighters: Arc::new(Vec::new()),
                fighters: Arc::new(Vec::new()),
                generation: 0,
                rotation: 0,
                round_active: false,
            }),
            prepared: Mutex::new(PreparedRuntimePool::empty(0)),
            runtimes: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            requested_rotation: AtomicU64::new(0),
            round_rotation_requested: AtomicBool::new(false),
            refresh_running: AtomicBool::new(false),
        }
    }

    fn fighter(rank: usize, model_id: &str) -> ExhibitionFighter {
        use sha2::{Digest, Sha256};

        ExhibitionFighter {
            rank,
            model_id: model_id.to_owned(),
            model_name: model_id.to_owned(),
            strategy_rating: 50.0,
            wasm_bytes: CYCLING_V2_WASM.len(),
            wasm_sha256: format!("{:x}", Sha256::digest(CYCLING_V2_WASM)),
        }
    }

    fn observation(slot: i32) -> ExhibitionBotObservation {
        ExhibitionBotObservation {
            self_health: 100,
            target_health: 100,
            personal_score: 0,
            team_score_delta: 0,
            objective_delta: 0,
            allies_alive: 1,
            enemies_alive: 1,
            lowest_ally_health: 0,
            slot,
            mode: ArenaMatchMode::Arena,
        }
    }

    #[test]
    fn published_roster_requires_the_entire_selected_top_n_to_be_verified() {
        let verified = select_published_fighters(
            ratings(vec![rating(1, true), rating(2, true), rating(3, true)]),
            2,
        );
        assert_eq!(verified.len(), 2);
        assert_eq!(verified[0].model_id, "model_1");
        assert_eq!(verified[1].model_id, "model_2");
        assert_eq!(verified[0].display_name(), "#1 Model 1");

        let selected = select_published_fighters(
            ratings(vec![rating(1, true), rating(2, false), rating(3, true)]),
            2,
        );
        assert!(selected.is_empty());
    }

    #[test]
    fn published_roster_requires_the_full_configured_size() {
        let short = select_published_fighters(ratings(vec![rating(1, true), rating(2, true)]), 3);
        assert!(short.is_empty());
    }

    #[test]
    fn inactive_ratings_never_supply_exhibition_fighters() {
        let mut response = ratings(vec![rating(1, true)]);
        response.active = false;
        assert!(select_published_fighters(response, 10).is_empty());
    }

    #[test]
    fn live_roster_rejects_legacy_entries_without_a_wasm_digest() {
        let mut legacy = rating(1, true);
        legacy.wasm_sha256 = None;
        assert!(select_published_fighters(ratings(vec![legacy]), 10).is_empty());

        let mut noncanonical = rating(1, true);
        noncanonical.wasm_sha256 = Some("B".repeat(64));
        assert!(select_published_fighters(ratings(vec![noncanonical]), 10).is_empty());

        let mut oversized = rating(1, true);
        oversized.wasm_bytes = Some(MAX_EXHIBITION_WASM_BYTES + 1);
        assert!(select_published_fighters(ratings(vec![oversized]), 10).is_empty());
    }

    #[test]
    fn rank_assignment_is_side_balanced_and_swaps_each_rotation() {
        let roster_len = 10;
        for slot in 0..7 {
            let team1_first = fair_fighter_index(roster_len, 1, slot, 0);
            let team2_first = fair_fighter_index(roster_len, 2, slot, 0);
            let team1_second = fair_fighter_index(roster_len, 1, slot, 1);
            let team2_second = fair_fighter_index(roster_len, 2, slot, 1);
            assert_eq!(team1_first, team2_second);
            assert_eq!(team2_first, team1_second);
            assert_ne!(
                team1_first % 2,
                team2_first % 2,
                "rank parity must not bind to team"
            );
        }
    }

    #[test]
    fn abi_slot_is_the_stable_zero_based_team_slot() {
        let assignment = ExhibitionAssignment {
            fighter: ExhibitionFighter {
                rank: 1,
                model_id: "model_1".to_owned(),
                model_name: "Model 1".to_owned(),
                strategy_rating: 50.0,
                wasm_bytes: 100,
                wasm_sha256: "b".repeat(64),
            },
            team_id: 2,
            slot: 3,
            rotation: 4,
        };
        assert_eq!(assignment.slot, 3);
        assert_eq!(fair_fighter_index(10, 2, assignment.slot, 4), 0);
    }

    #[test]
    fn pending_rotation_survives_a_missed_intermission_without_skipping_parity() {
        let exhibition = phase_test_exhibition();
        assert!(!exhibition.active_round_ready());

        exhibition.mark_round_started();
        assert!(exhibition.active_round_ready());
        exhibition.request_round_rotation();
        assert_eq!(exhibition.requested_rotation.load(Ordering::Acquire), 1);
        assert!(!exhibition.roster.read().round_active);

        // Preparation missed the intermission. The next Active transition
        // closes the commit gate but deliberately leaves rotation 1 pending.
        exhibition.mark_round_started();
        assert!(exhibition.roster.read().round_active);
        assert!(exhibition.round_rotation_requested.load(Ordering::Acquire));
        assert!(!exhibition.active_round_ready());

        // The next Ended transition reopens the gate without incrementing to
        // rotation 2 and thereby skipping the pending side swap.
        exhibition.request_round_rotation();
        exhibition.request_round_rotation();
        assert!(!exhibition.roster.read().round_active);
        assert_eq!(exhibition.requested_rotation.load(Ordering::Acquire), 1);

        exhibition.roster.write().rotation = 1;
        exhibition.mark_round_started();
        assert!(exhibition.active_round_ready());
        assert!(!exhibition.round_rotation_requested.load(Ordering::Acquire));
        exhibition.request_round_rotation();
        assert_eq!(exhibition.requested_rotation.load(Ordering::Acquire), 2);
    }

    #[test]
    fn attached_runtime_is_atomically_replaced_on_round_rotation() {
        let fixture = ExhibitionWasmFixture::new(&["model_a", "model_b"]);
        let sandbox = BotSandbox::with_wasm_dir_for_tests(fixture.dir.clone());
        let published_fighters = vec![fighter(1, "model_a"), fighter(2, "model_b")];
        let (live_fighters, prepared) =
            prepare_runtime_pool(&sandbox, published_fighters.clone(), 1, 0, 0);
        assert_eq!(live_fighters, published_fighters);

        let exhibition = ArenaExhibition {
            enabled: true,
            ratings_path: fixture.dir.join("ratings-not-present.json"),
            roster_limit: 2,
            prepared_per_fighter: 1,
            sandbox: Some(sandbox),
            roster: RwLock::new(ExhibitionRosterState {
                modified_at: None,
                season_id: Some("test".to_owned()),
                published_fighters: Arc::new(published_fighters),
                fighters: Arc::new(live_fighters),
                generation: 0,
                rotation: 0,
                round_active: false,
            }),
            prepared: Mutex::new(prepared),
            runtimes: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            requested_rotation: AtomicU64::new(0),
            round_rotation_requested: AtomicBool::new(false),
            refresh_running: AtomicBool::new(false),
        };
        let player_id: PlayerID = Arc::from("rotation-test-bot");
        let attached = exhibition
            .attach_bot(player_id.clone(), 1, 0, 7)
            .expect("prepared model_a runtime should attach");
        assert_eq!(attached.fighter.model_id, "model_a");
        assert_eq!(exhibition.next_action(&player_id, observation(0)), None);
        assert_eq!(
            exhibition
                .runtimes
                .lock()
                .get(&player_id)
                .expect("waiting runtime remains attached")
                .runtime
                .tick(),
            0
        );
        exhibition.mark_round_started();
        assert_eq!(
            exhibition
                .next_action(&player_id, observation(0))
                .expect("initial runtime should decide")
                .tick,
            0
        );
        let pending_player_id: PlayerID = Arc::from("mid-round-pending-bot");
        assert_eq!(
            exhibition.attach_bot(pending_player_id.clone(), 2, 0, 11),
            None
        );
        assert!(exhibition.slot_is_reserved(2, 0));
        assert_eq!(exhibition.current_assignment(&pending_player_id), None);

        // Simulate a rebuild that misses its first intermission. While Active,
        // only the depleted spare pool may refill; the attached runtime stays
        // on rotation 0.
        exhibition.request_round_rotation();
        exhibition.mark_round_started();
        assert_eq!(
            exhibition.refresh_if_needed(),
            ExhibitionRefreshOutcome::Refreshed
        );
        assert_eq!(exhibition.next_action(&player_id, observation(0)), None);
        {
            let runtimes = exhibition.runtimes.lock();
            let still_initial = runtimes
                .get(&player_id)
                .expect("benched runtime must remain attached");
            assert_eq!(still_initial.assignment.rotation, 0);
            assert_eq!(still_initial.assignment.fighter.model_id, "model_a");
            assert_eq!(still_initial.runtime.tick(), 1);
        }

        // The following Ended transition reopens the commit gate without
        // advancing past the pending odd rotation. The already-attached entry
        // swaps to model_b and its fresh runtime restarts at strategy tick 0.
        exhibition.request_round_rotation();
        assert_eq!(
            exhibition.refresh_if_needed(),
            ExhibitionRefreshOutcome::Refreshed
        );
        exhibition.mark_round_started();
        let rotated = exhibition
            .next_action(&player_id, observation(0))
            .expect("replacement runtime should decide");
        assert_eq!(rotated.assignment.rotation, 1);
        assert_eq!(rotated.assignment.fighter.model_id, "model_b");
        assert_eq!(rotated.tick, 0);
        let promoted = exhibition
            .current_assignment(&pending_player_id)
            .expect("pending binding should be promoted by the same atomic swap");
        assert_eq!(promoted.rotation, 1);
        assert_eq!(promoted.fighter.model_id, "model_a");
        assert!(exhibition.pending.lock().is_empty());
        assert_eq!(
            exhibition
                .next_action(&pending_player_id, observation(0))
                .expect("promoted runtime should start at tick zero")
                .tick,
            0
        );
    }

    #[test]
    fn incomplete_startup_roster_recovers_without_a_ratings_mtime_change() {
        let fixture = ExhibitionWasmFixture::new(&["model_a"]);
        let sandbox = BotSandbox::with_wasm_dir_for_tests(fixture.dir.clone());
        let published_fighters = vec![fighter(1, "model_a"), fighter(2, "model_b")];
        let (partially_prepared_fighters, _partial_pool) =
            prepare_runtime_pool(&sandbox, published_fighters.clone(), 1, 0, 0);
        assert_eq!(partially_prepared_fighters, vec![fighter(1, "model_a")]);

        let exhibition = ArenaExhibition {
            enabled: true,
            ratings_path: fixture.dir.join("ratings-not-present.json"),
            roster_limit: 2,
            prepared_per_fighter: 1,
            sandbox: Some(sandbox),
            roster: RwLock::new(ExhibitionRosterState {
                modified_at: None,
                season_id: Some("test".to_owned()),
                published_fighters: Arc::new(published_fighters.clone()),
                fighters: Arc::new(Vec::new()),
                generation: 0,
                rotation: 0,
                round_active: false,
            }),
            prepared: Mutex::new(PreparedRuntimePool::empty(0)),
            runtimes: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            requested_rotation: AtomicU64::new(0),
            round_rotation_requested: AtomicBool::new(false),
            refresh_running: AtomicBool::new(false),
        };
        assert!(exhibition.roster().is_empty());

        fixture.write_model("model_b");
        assert_eq!(
            exhibition.refresh_if_needed(),
            ExhibitionRefreshOutcome::Refreshed
        );
        assert_eq!(exhibition.roster().as_ref(), &published_fighters);
        assert_eq!(exhibition.roster.read().generation, 1);
    }

    #[test]
    fn replacement_never_substitutes_a_different_loadable_fighter() {
        let fixture = ExhibitionWasmFixture::new(&["model_b"]);
        let sandbox = BotSandbox::with_wasm_dir_for_tests(fixture.dir.clone());
        let fighters = vec![fighter(1, "model_a"), fighter(2, "model_b")];
        let player_id: PlayerID = Arc::from("exact-model-test");
        let bindings = HashMap::from([(
            player_id,
            ExhibitionBinding {
                team_id: 1,
                team_slot: 0,
                seed: 3,
            },
        )]);

        assert!(build_replacement_runtimes(&sandbox, &fighters, &bindings, 0).is_none());
    }
}
