use super::replays::MAX_ARENA_REPLAY_WARNINGS;
use super::scoring::{
    apply_match_result, atomic_update_max, atomic_update_min_nonzero, normalize_match_mode,
    to_model_view, to_queued_match_view, unix_now, update_elo_pair, MatchResult,
};
use super::types::{
    ArenaError, ArenaModelView, ArenaOverviewResponse, ArenaReplayView, ClaimMatchResponse,
    ExecuteNextBody, ExecuteNextMatchResponse, ModelHeartbeatBody, QueueMatchBody,
    QueueMatchResponse, QueueRoundRobinBody, QueuedMatch, QueuedMatchView, RegisterModelBody,
    ReportMatchBody, ReportMatchResponse, SimulateTeamBattleBody, SimulateTeamBattleResponse,
    SimulateWorldBattleBody, SimulateWorldBattleResponse, UploadModelWasmBody,
    UploadModelWasmResponse,
};
use super::{ArenaLeaderboardResponse, ArenaService};
use crate::operational::bot_sandbox::{
    ArenaMatchMode, BotMatchExecution, MAX_WORLD_BATTLE_ENTRANTS, MAX_WORLD_BATTLE_ROUNDS,
    MAX_WORLD_BATTLE_TICKS, MAX_WORLD_SQUAD_SIZE, MIN_WORLD_BATTLE_ENTRANTS,
};
use crate::operational::validation::sanitize_model_id;
use base64::Engine as _;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::Ordering;
use tracing::warn;
use uuid::Uuid;

pub(super) const DEFAULT_BASE_ELO: f64 = 1000.0;
pub(super) const MAX_ARENA_LEADERBOARD_LIMIT: usize = 100;
pub(super) const MAX_ARENA_PENDING_MATCHES: usize = 10_000;

impl ArenaService {
    pub(super) fn prune_stale_in_flight_matches(&self) {
        let ttl_secs = super::arena_in_flight_ttl_secs();
        let now = unix_now();
        let mut removed = 0usize;
        self.inner.in_flight_matches.retain(|_match_id, queued| {
            let keep = now.saturating_sub(queued.queued_at) <= ttl_secs;
            if !keep {
                removed += 1;
            }
            keep
        });
        if removed > 0 {
            warn!(
                "Pruned {} stale arena in-flight matches older than {} seconds",
                removed, ttl_secs
            );
        }
    }

    pub(super) fn register_model(
        &self,
        body: RegisterModelBody,
    ) -> Result<ArenaModelView, ArenaError> {
        let model_name = body.model_name.trim();
        if model_name.is_empty() {
            return Err(ArenaError::InvalidInput(
                "invalid_model_name",
                "model_name cannot be empty".to_owned(),
            ));
        }

        let now = unix_now();
        let model_id = body
            .model_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("model_{}", Uuid::new_v4().simple()));

        let provider = body
            .provider
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("custom")
            .to_owned();
        let version = body
            .version
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("latest")
            .to_owned();
        let active = body.active.unwrap_or(true);
        let initial_elo = body
            .initial_elo
            .unwrap_or(DEFAULT_BASE_ELO)
            .clamp(100.0, 4000.0);

        {
            let mut store = self.inner.persistent_store.write();
            let entry = store.models.entry(model_id.clone()).or_insert_with(|| {
                super::types::ArenaModelRecord {
                    model_id: model_id.clone(),
                    model_name: model_name.to_owned(),
                    provider: provider.clone(),
                    version: version.clone(),
                    active,
                    created_at: now,
                    updated_at: now,
                    last_seen_at: now,
                    elo_rating: initial_elo,
                    matches_played: 0,
                    wins: 0,
                    losses: 0,
                    draws: 0,
                    cumulative_score: 0,
                }
            });

            entry.model_name = model_name.to_owned();
            entry.provider = provider;
            entry.version = version;
            entry.active = active;
            entry.updated_at = now;
            entry.last_seen_at = now;
            if entry.matches_played == 0 {
                entry.elo_rating = initial_elo;
            }
        }

        self.spawn_persist_store();

        self.get_model_view(&model_id).ok_or_else(|| {
            ArenaError::Internal("model registration verification failed".to_owned())
        })
    }

    pub(super) fn heartbeat_model(
        &self,
        body: ModelHeartbeatBody,
    ) -> Result<ArenaModelView, ArenaError> {
        let now = unix_now();
        {
            let mut store = self.inner.persistent_store.write();
            let Some(model) = store.models.get_mut(body.model_id.trim()) else {
                return Err(ArenaError::NotFound(
                    "model_not_found",
                    format!("model '{}' does not exist", body.model_id.trim()),
                ));
            };
            model.last_seen_at = now;
            model.updated_at = now;
            model.active = true;
        }

        self.spawn_persist_store();

        self.get_model_view(body.model_id.trim())
            .ok_or_else(|| ArenaError::Internal("heartbeat verification failed".to_owned()))
    }

    pub(super) fn upload_model_wasm(
        &self,
        body: UploadModelWasmBody,
    ) -> Result<UploadModelWasmResponse, ArenaError> {
        let model_id = body.model_id.trim();
        if model_id.is_empty() {
            return Err(ArenaError::InvalidInput(
                "invalid_model_id",
                "model_id is required".to_owned(),
            ));
        }
        let Some(safe_model_id) = sanitize_model_id(model_id) else {
            return Err(ArenaError::InvalidInput(
                "invalid_model_id",
                "model_id contains unsupported characters".to_owned(),
            ));
        };

        {
            let store = self.inner.persistent_store.read();
            if !store.models.contains_key(model_id) {
                return Err(ArenaError::NotFound(
                    "model_not_found",
                    format!("model '{}' does not exist", model_id),
                ));
            }
        }

        let wasm_bytes = base64::engine::general_purpose::STANDARD
            .decode(body.wasm_base64.trim())
            .map_err(|err| {
                ArenaError::InvalidInput("invalid_wasm_base64", format!("invalid base64: {}", err))
            })?;
        if wasm_bytes.is_empty() {
            return Err(ArenaError::InvalidInput(
                "empty_wasm",
                "decoded wasm payload is empty".to_owned(),
            ));
        }
        if wasm_bytes.len() > self.inner.wasm_max_bytes {
            return Err(ArenaError::InvalidInput(
                "wasm_too_large",
                format!(
                    "wasm payload exceeds max size ({} > {})",
                    wasm_bytes.len(),
                    self.inner.wasm_max_bytes
                ),
            ));
        }
        if wasm_bytes.len() < 4 || wasm_bytes[..4] != [0, 0x61, 0x73, 0x6d] {
            return Err(ArenaError::InvalidInput(
                "invalid_wasm_header",
                "payload is not a valid wasm module".to_owned(),
            ));
        }

        fs::create_dir_all(&self.inner.wasm_dir).map_err(|err| {
            ArenaError::Internal(format!(
                "failed to create wasm dir '{}': {}",
                self.inner.wasm_dir.display(),
                err
            ))
        })?;
        let wasm_path = self.inner.wasm_dir.join(format!("{}.wasm", safe_model_id));
        let overwrite = body.overwrite.unwrap_or(false);
        let existed = wasm_path.exists();
        if existed && !overwrite {
            return Err(ArenaError::Conflict(
                "wasm_exists",
                format!(
                    "wasm already exists for model '{}'; set overwrite=true to replace",
                    model_id
                ),
            ));
        }

        atomic_replace_bytes(&wasm_path, &wasm_bytes).map_err(|err| {
            ArenaError::Internal(format!(
                "failed to write wasm file '{}': {}",
                wasm_path.display(),
                err
            ))
        })?;

        {
            let now = unix_now();
            let mut store = self.inner.persistent_store.write();
            if let Some(model) = store.models.get_mut(model_id) {
                model.updated_at = now;
                model.last_seen_at = now;
            }
        }
        self.spawn_persist_store();

        Ok(UploadModelWasmResponse {
            model_id: model_id.to_owned(),
            wasm_path: wasm_path.display().to_string(),
            bytes_written: wasm_bytes.len(),
            wasm_sha256: sha256_hex(&wasm_bytes),
            overwritten: existed,
        })
    }

    pub(super) fn queue_match(
        &self,
        body: QueueMatchBody,
    ) -> Result<QueueMatchResponse, ArenaError> {
        let model_a_id = body.model_a_id.trim();
        let model_b_id = body.model_b_id.trim();
        if model_a_id.is_empty() || model_b_id.is_empty() {
            return Err(ArenaError::InvalidInput(
                "invalid_model_id",
                "model_a_id and model_b_id are required".to_owned(),
            ));
        }
        if model_a_id == model_b_id {
            return Err(ArenaError::InvalidInput(
                "invalid_matchup",
                "model_a_id and model_b_id must be different".to_owned(),
            ));
        }

        let store = self.inner.persistent_store.read();
        if !store.models.contains_key(model_a_id) {
            return Err(ArenaError::NotFound(
                "model_not_found",
                format!("model '{}' does not exist", model_a_id),
            ));
        }
        if !store.models.contains_key(model_b_id) {
            return Err(ArenaError::NotFound(
                "model_not_found",
                format!("model '{}' does not exist", model_b_id),
            ));
        }
        drop(store);

        let mode = normalize_match_mode(body.mode.as_deref())?;
        let metadata = body.metadata.unwrap_or_default();
        let queued = QueuedMatch {
            match_id: format!("match_{}", Uuid::new_v4().simple()),
            model_a_id: model_a_id.to_owned(),
            model_b_id: model_b_id.to_owned(),
            mode,
            queued_at: unix_now(),
            metadata,
        };

        let mut pending = self.inner.pending_matches.lock();
        if pending.len() >= MAX_ARENA_PENDING_MATCHES {
            return Err(ArenaError::Conflict(
                "queue_full",
                format!(
                    "pending queue is full (max {} matches)",
                    MAX_ARENA_PENDING_MATCHES
                ),
            ));
        }
        pending.push_back(queued.clone());
        let pending_total = pending.len();
        drop(pending);

        Ok(QueueMatchResponse {
            queued_count: 1,
            pending_total,
            queued_matches: vec![to_queued_match_view(&queued)],
        })
    }

    pub(super) fn queue_round_robin(
        &self,
        body: QueueRoundRobinBody,
    ) -> Result<QueueMatchResponse, ArenaError> {
        let include_inactive = body.include_inactive.unwrap_or(false);
        let mode = normalize_match_mode(body.mode.as_deref())?;
        let max_pairs = body.max_pairs.unwrap_or(512).max(1);
        let pending_before = self.inner.pending_matches.lock().len();
        if pending_before >= MAX_ARENA_PENDING_MATCHES {
            return Err(ArenaError::Conflict(
                "queue_full",
                format!(
                    "pending queue is full (max {} matches)",
                    MAX_ARENA_PENDING_MATCHES
                ),
            ));
        }
        let available_slots = MAX_ARENA_PENDING_MATCHES - pending_before;
        let capped_max_pairs = max_pairs.min(available_slots);

        let mut model_ids: Vec<String> = {
            let store = self.inner.persistent_store.read();
            store
                .models
                .values()
                .filter(|model| include_inactive || model.active)
                .map(|model| model.model_id.clone())
                .collect()
        };
        model_ids.sort_unstable();
        if model_ids.len() < 2 {
            return Err(ArenaError::InvalidInput(
                "not_enough_models",
                "need at least 2 models to queue round-robin matches".to_owned(),
            ));
        }

        let mut queued_matches = Vec::new();
        let now = unix_now();
        'outer: for left in 0..model_ids.len() {
            for right in (left + 1)..model_ids.len() {
                let queued = QueuedMatch {
                    match_id: format!("match_{}", Uuid::new_v4().simple()),
                    model_a_id: model_ids[left].clone(),
                    model_b_id: model_ids[right].clone(),
                    mode: mode.clone(),
                    queued_at: now,
                    metadata: HashMap::new(),
                };
                queued_matches.push(queued);
                if queued_matches.len() >= capped_max_pairs {
                    break 'outer;
                }
            }
        }

        let mut pending = self.inner.pending_matches.lock();
        let remaining_slots = MAX_ARENA_PENDING_MATCHES.saturating_sub(pending.len());
        let enqueue_count = queued_matches.len().min(remaining_slots);
        for queued in queued_matches.iter().take(enqueue_count) {
            pending.push_back(queued.clone());
        }
        let pending_total = pending.len();
        drop(pending);

        Ok(QueueMatchResponse {
            queued_count: enqueue_count,
            pending_total,
            queued_matches: queued_matches
                .iter()
                .take(enqueue_count)
                .map(to_queued_match_view)
                .collect(),
        })
    }

    pub(super) fn claim_next_match(&self) -> ClaimMatchResponse {
        self.prune_stale_in_flight_matches();
        let maybe_claimed = {
            let mut pending = self.inner.pending_matches.lock();
            pending.pop_front()
        };

        if let Some(claimed) = maybe_claimed {
            self.inner
                .in_flight_matches
                .insert(claimed.match_id.clone(), claimed.clone());
            ClaimMatchResponse {
                claimed: Some(to_queued_match_view(&claimed)),
                pending_total: self.inner.pending_matches.lock().len(),
                in_flight_total: self.inner.in_flight_matches.len(),
            }
        } else {
            ClaimMatchResponse {
                claimed: None,
                pending_total: 0,
                in_flight_total: self.inner.in_flight_matches.len(),
            }
        }
    }

    pub(super) fn list_pending_matches(&self, limit: usize) -> Vec<QueuedMatchView> {
        let bounded = limit.max(1);
        let pending = self.inner.pending_matches.lock();
        pending
            .iter()
            .take(bounded)
            .map(to_queued_match_view)
            .collect()
    }

    pub(super) fn leaderboard(&self, limit: usize) -> ArenaLeaderboardResponse {
        let bounded_limit = limit.clamp(1, MAX_ARENA_LEADERBOARD_LIMIT);
        let store = self.inner.persistent_store.read();
        let mut models: Vec<ArenaModelView> = store.models.values().map(to_model_view).collect();
        models.sort_by(|left, right| {
            right
                .elo_rating
                .partial_cmp(&left.elo_rating)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| right.wins.cmp(&left.wins))
                .then_with(|| left.losses.cmp(&right.losses))
        });
        models.truncate(bounded_limit);

        ArenaLeaderboardResponse {
            generated_at: unix_now(),
            total_models: store.models.len(),
            total_completed_matches: store.completed_matches.len(),
            models,
        }
    }

    pub(super) fn overview(&self) -> ArenaOverviewResponse {
        self.prune_stale_in_flight_matches();
        let store = self.inner.persistent_store.read();
        ArenaOverviewResponse {
            generated_at: unix_now(),
            active_models: store.models.values().filter(|model| model.active).count(),
            pending_matches: self.inner.pending_matches.lock().len(),
            in_flight_matches: self.inner.in_flight_matches.len(),
            total_completed_matches: store.completed_matches.len(),
            total_matches_reported_runtime: self
                .inner
                .total_matches_reported
                .load(Ordering::Relaxed),
        }
    }

    pub(super) fn report_match(
        &self,
        body: ReportMatchBody,
    ) -> Result<ReportMatchResponse, ArenaError> {
        let match_id = body.match_id.trim();
        let in_flight = self.inner.in_flight_matches.remove(match_id);
        let pending_match = in_flight
            .map(|(_, value)| value)
            .or_else(|| {
                let mut pending = self.inner.pending_matches.lock();
                let idx = pending
                    .iter()
                    .position(|entry| entry.match_id == match_id)?;
                pending.remove(idx)
            })
            .ok_or_else(|| {
                ArenaError::NotFound(
                    "match_not_found",
                    format!(
                        "match '{}' was not found in pending/in-flight queue",
                        match_id
                    ),
                )
            })?;

        let model_a_id = body
            .model_a_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(pending_match.model_a_id.as_str())
            .to_owned();
        let model_b_id = body
            .model_b_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(pending_match.model_b_id.as_str())
            .to_owned();
        let draw = body.draw.unwrap_or(false);
        let winner_model_id = body
            .winner_model_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);

        if !draw {
            if let Some(winner) = winner_model_id.as_deref() {
                if winner != model_a_id && winner != model_b_id {
                    return Err(ArenaError::InvalidInput(
                        "invalid_winner",
                        "winner_model_id must equal model_a_id or model_b_id".to_owned(),
                    ));
                }
            } else {
                return Err(ArenaError::InvalidInput(
                    "missing_winner",
                    "winner_model_id is required unless draw=true".to_owned(),
                ));
            }
        }

        let model_a_score = body.model_a_score.unwrap_or(0);
        let model_b_score = body.model_b_score.unwrap_or(0);
        let completed_at = unix_now();

        let model_a_result = if draw {
            MatchResult::Draw
        } else if winner_model_id.as_deref() == Some(model_a_id.as_str()) {
            MatchResult::Win
        } else {
            MatchResult::Loss
        };
        let model_b_result = model_a_result.inverse();

        let (model_a_view, model_b_view) = {
            let mut store = self.inner.persistent_store.write();
            let (model_a_old_elo, model_b_old_elo) = {
                let Some(model_a) = store.models.get(&model_a_id) else {
                    return Err(ArenaError::NotFound(
                        "model_not_found",
                        format!("model '{}' does not exist", model_a_id),
                    ));
                };
                let Some(model_b) = store.models.get(&model_b_id) else {
                    return Err(ArenaError::NotFound(
                        "model_not_found",
                        format!("model '{}' does not exist", model_b_id),
                    ));
                };
                (model_a.elo_rating, model_b.elo_rating)
            };
            let (new_a, new_b) = update_elo_pair(
                model_a_old_elo,
                model_b_old_elo,
                model_a_result,
                model_b_result,
            );

            {
                let Some(model_a) = store.models.get_mut(&model_a_id) else {
                    return Err(ArenaError::NotFound(
                        "model_not_found",
                        format!("model '{}' does not exist", model_a_id),
                    ));
                };
                apply_match_result(model_a, model_a_result, model_a_score, completed_at);
                model_a.elo_rating = new_a;
            }

            {
                let Some(model_b) = store.models.get_mut(&model_b_id) else {
                    return Err(ArenaError::NotFound(
                        "model_not_found",
                        format!("model '{}' does not exist", model_b_id),
                    ));
                };
                apply_match_result(model_b, model_b_result, model_b_score, completed_at);
                model_b.elo_rating = new_b;
            }

            store
                .completed_matches
                .push(super::types::CompletedMatchRecord {
                    match_id: body.match_id.trim().to_owned(),
                    model_a_id: model_a_id.clone(),
                    model_b_id: model_b_id.clone(),
                    winner_model_id: winner_model_id.clone(),
                    draw,
                    model_a_score,
                    model_b_score,
                    duration_ms: body.duration_ms,
                    completed_at,
                });

            let model_a_view = store
                .models
                .get(&model_a_id)
                .map(to_model_view)
                .ok_or_else(|| {
                    ArenaError::Internal("post-report lookup failed for model_a".to_owned())
                })?;
            let model_b_view = store
                .models
                .get(&model_b_id)
                .map(to_model_view)
                .ok_or_else(|| {
                    ArenaError::Internal("post-report lookup failed for model_b".to_owned())
                })?;
            (model_a_view, model_b_view)
        };

        self.inner
            .total_matches_reported
            .fetch_add(1, Ordering::Relaxed);

        self.spawn_persist_store();

        Ok(ReportMatchResponse {
            match_id: body.match_id.trim().to_owned(),
            completed_at,
            model_a: model_a_view,
            model_b: model_b_view,
        })
    }

    pub(super) fn execute_next_match(
        &self,
        body: ExecuteNextBody,
    ) -> Result<ExecuteNextMatchResponse, ArenaError> {
        let pending_before = self.inner.pending_matches.lock().len();
        let claimed = self.claim_next_match();
        let Some(claimed_match) = claimed.claimed else {
            return Err(ArenaError::NotFound(
                "no_pending_match",
                "no pending arena matches available for execution".to_owned(),
            ));
        };

        let seed = body.seed.unwrap_or_else(unix_now);
        let mode = ArenaMatchMode::parse(&claimed_match.mode).ok_or_else(|| {
            ArenaError::InvalidInput(
                "invalid_mode",
                format!("unsupported match mode '{}'", claimed_match.mode),
            )
        })?;
        let BotMatchExecution {
            outcome: sandbox_outcome,
            replay: sandbox_replay,
        } = self.inner.bot_sandbox.execute_match_with_replay(
            &claimed_match.model_a_id,
            &claimed_match.model_b_id,
            mode,
            seed,
            body.max_ticks,
        );

        let report = self.report_match(ReportMatchBody {
            match_id: claimed_match.match_id.clone(),
            model_a_id: Some(claimed_match.model_a_id.clone()),
            model_b_id: Some(claimed_match.model_b_id.clone()),
            winner_model_id: sandbox_outcome.winner_model_id.clone(),
            draw: Some(sandbox_outcome.draw),
            model_a_score: Some(sandbox_outcome.model_a_score),
            model_b_score: Some(sandbox_outcome.model_b_score),
            duration_ms: Some(sandbox_outcome.duration_ms),
        })?;

        self.push_recent_replay(ArenaReplayView {
            replay_id: format!(
                "replay_{:016x}",
                self.inner.replay_sequence.fetch_add(1, Ordering::Relaxed) + 1
            ),
            match_id: claimed_match.match_id.clone(),
            executed_at: report.completed_at,
            mode: sandbox_outcome.mode.clone(),
            model_a_id: claimed_match.model_a_id.clone(),
            model_b_id: claimed_match.model_b_id.clone(),
            winner_model_id: sandbox_outcome.winner_model_id.clone(),
            draw: sandbox_outcome.draw,
            model_a_score: sandbox_outcome.model_a_score,
            model_b_score: sandbox_outcome.model_b_score,
            objective_label: sandbox_outcome.objective_label.clone(),
            objective_a: sandbox_outcome.objective_a,
            objective_b: sandbox_outcome.objective_b,
            model_a_runtime: sandbox_outcome.model_a_runtime.clone(),
            model_b_runtime: sandbox_outcome.model_b_runtime.clone(),
            ticks_executed: sandbox_outcome.ticks_executed,
            duration_ms: sandbox_outcome.duration_ms,
            warnings: sandbox_outcome
                .warnings
                .iter()
                .take(MAX_ARENA_REPLAY_WARNINGS)
                .cloned()
                .collect(),
        });
        self.record_match_replay_events(
            &claimed_match,
            seed,
            report.completed_at,
            &sandbox_outcome,
            &sandbox_replay,
        );

        let pending_after = self.inner.pending_matches.lock().len();
        Ok(ExecuteNextMatchResponse {
            pending_before,
            pending_after,
            report,
            sandbox: sandbox_outcome,
        })
    }

    pub(super) fn simulate_team_battle(
        &self,
        body: SimulateTeamBattleBody,
    ) -> Result<SimulateTeamBattleResponse, ArenaError> {
        let model_a_id = body.model_a_id.trim();
        let model_b_id = body.model_b_id.trim();
        if model_a_id.is_empty() || model_b_id.is_empty() {
            return Err(ArenaError::InvalidInput(
                "invalid_model_id",
                "model_a_id and model_b_id are required".to_owned(),
            ));
        }
        if model_a_id == model_b_id {
            return Err(ArenaError::InvalidInput(
                "invalid_matchup",
                "model_a_id and model_b_id must be different".to_owned(),
            ));
        }

        let store = self.inner.persistent_store.read();
        if !store.models.contains_key(model_a_id) {
            return Err(ArenaError::NotFound(
                "model_not_found",
                format!("model '{}' does not exist", model_a_id),
            ));
        }
        if !store.models.contains_key(model_b_id) {
            return Err(ArenaError::NotFound(
                "model_not_found",
                format!("model '{}' does not exist", model_b_id),
            ));
        }
        drop(store);

        let mode = normalize_match_mode(body.mode.as_deref())?;
        let mode = ArenaMatchMode::parse(&mode).ok_or_else(|| {
            ArenaError::InvalidInput("invalid_mode", format!("unsupported match mode '{}'", mode))
        })?;
        let team_size = body.team_size.unwrap_or(10);
        let rounds = body.rounds.unwrap_or(1);
        let seed = body.seed.unwrap_or_else(unix_now);

        let simulation = self.inner.bot_sandbox.execute_team_battle(
            model_a_id,
            model_b_id,
            mode,
            team_size,
            rounds,
            seed,
            body.max_ticks,
        );

        Ok(SimulateTeamBattleResponse {
            generated_at: unix_now(),
            simulation,
        })
    }

    pub(super) fn simulate_world_battle(
        &self,
        body: SimulateWorldBattleBody,
    ) -> Result<SimulateWorldBattleResponse, ArenaError> {
        if !(MIN_WORLD_BATTLE_ENTRANTS..=MAX_WORLD_BATTLE_ENTRANTS).contains(&body.model_ids.len())
        {
            return Err(ArenaError::InvalidInput(
                "invalid_world_entrants",
                format!(
                    "model_ids must contain between {} and {} models",
                    MIN_WORLD_BATTLE_ENTRANTS, MAX_WORLD_BATTLE_ENTRANTS
                ),
            ));
        }

        let mut model_ids = Vec::with_capacity(body.model_ids.len());
        let mut seen = std::collections::HashSet::with_capacity(body.model_ids.len());
        for raw_model_id in body.model_ids {
            let model_id = raw_model_id.trim();
            if sanitize_model_id(model_id).is_none() {
                return Err(ArenaError::InvalidInput(
                    "invalid_model_id",
                    format!("model_id '{}' has an invalid format", model_id),
                ));
            }
            if !seen.insert(model_id.to_owned()) {
                return Err(ArenaError::InvalidInput(
                    "duplicate_world_model",
                    format!("model_id '{}' appears more than once", model_id),
                ));
            }
            model_ids.push(model_id.to_owned());
        }

        let squad_size = body.squad_size.unwrap_or(3);
        if !(1..=MAX_WORLD_SQUAD_SIZE).contains(&squad_size) {
            return Err(ArenaError::InvalidInput(
                "invalid_world_squad_size",
                format!("squad_size must be between 1 and {}", MAX_WORLD_SQUAD_SIZE),
            ));
        }
        let rounds = body.rounds.unwrap_or(1);
        if !(1..=MAX_WORLD_BATTLE_ROUNDS).contains(&rounds) {
            return Err(ArenaError::InvalidInput(
                "invalid_world_rounds",
                format!("rounds must be between 1 and {}", MAX_WORLD_BATTLE_ROUNDS),
            ));
        }
        if let Some(max_ticks) = body.max_ticks {
            if !(1..=MAX_WORLD_BATTLE_TICKS).contains(&max_ticks) {
                return Err(ArenaError::InvalidInput(
                    "invalid_world_max_ticks",
                    format!("max_ticks must be between 1 and {}", MAX_WORLD_BATTLE_TICKS),
                ));
            }
        }

        let store = self.inner.persistent_store.read();
        for model_id in &model_ids {
            if !store.models.contains_key(model_id) {
                return Err(ArenaError::NotFound(
                    "model_not_found",
                    format!("model '{}' does not exist", model_id),
                ));
            }
        }
        drop(store);

        let seed = body.seed.unwrap_or_else(unix_now);
        let simulation = self.inner.bot_sandbox.execute_world_battle(
            &model_ids,
            squad_size,
            rounds,
            seed,
            body.max_ticks,
        );
        Ok(SimulateWorldBattleResponse {
            generated_at: unix_now(),
            simulation,
        })
    }

    pub(super) fn record_worker_success_metrics(&self, response: &ExecuteNextMatchResponse) {
        let sandbox = &response.sandbox;
        let duration_ms = sandbox.duration_ms;
        let ticks = sandbox.ticks_executed as u64;

        self.inner
            .worker_total_duration_ms
            .fetch_add(duration_ms, Ordering::Relaxed);
        self.inner
            .worker_total_ticks
            .fetch_add(ticks, Ordering::Relaxed);
        atomic_update_max(&self.inner.worker_max_duration_ms, duration_ms);
        atomic_update_min_nonzero(&self.inner.worker_min_duration_ms, duration_ms);

        if !sandbox.warnings.is_empty() {
            self.inner
                .worker_warning_matches
                .fetch_add(1, Ordering::Relaxed);
        }
        if sandbox.draw {
            self.inner
                .worker_draw_matches
                .fetch_add(1, Ordering::Relaxed);
        }
        if sandbox.model_a_runtime == "fallback" || sandbox.model_b_runtime == "fallback" {
            self.inner
                .worker_runtime_fallback_matches
                .fetch_add(1, Ordering::Relaxed);
        }
        if let Some(winner) = sandbox.winner_model_id.as_ref() {
            let mut distribution = self.inner.worker_model_win_distribution.write();
            *distribution.entry(winner.clone()).or_insert(0) += 1;
        }

        let mut trap_warnings = 0u64;
        let mut timeout_warnings = 0u64;
        for warning in &sandbox.warnings {
            let normalized = warning.to_ascii_lowercase();
            if normalized.contains("trap") {
                trap_warnings = trap_warnings.saturating_add(1);
            }
            if normalized.contains("timeout") || normalized.contains("fuel") {
                timeout_warnings = timeout_warnings.saturating_add(1);
            }
        }
        if trap_warnings > 0 {
            self.inner
                .worker_trap_warnings
                .fetch_add(trap_warnings, Ordering::Relaxed);
        }
        if timeout_warnings > 0 {
            self.inner
                .worker_timeout_warnings
                .fetch_add(timeout_warnings, Ordering::Relaxed);
        }
    }

    pub(super) fn get_model_view(&self, model_id: &str) -> Option<ArenaModelView> {
        let store = self.inner.persistent_store.read();
        store.models.get(model_id).map(to_model_view)
    }

    /// Offloads arena store persistence to a blocking thread so that
    /// tokio worker threads are not stalled by file I/O.
    /// Falls back to synchronous persistence when no tokio runtime is
    /// available (e.g. in unit tests).
    pub(super) fn spawn_persist_store(&self) {
        let snapshot = {
            let store = self.inner.persistent_store.read();
            store.clone()
        };
        let path = self.inner.store_path.clone();
        let redis_store = self.inner.redis_store.clone();
        let do_persist = move || {
            if let Err(err) =
                super::persistence::persist_store(&path, &snapshot, redis_store.as_ref())
            {
                warn!("Background arena persist failed: {}", err);
            }
        };
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::task::spawn_blocking(do_persist);
        } else {
            do_persist();
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Publish a complete sibling file with one rename so concurrent sandbox
/// readers observe either the previous artifact or the complete replacement.
fn atomic_replace_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("fighter.wasm");
    let tmp_path = path.with_file_name(format!(".{file_name}.{}.tmp", Uuid::new_v4().simple()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp_path, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    result
}

#[cfg(test)]
mod artifact_tests {
    use super::*;

    #[test]
    fn uploaded_artifact_helper_replaces_complete_bytes_and_hashes_them() {
        let directory = std::env::temp_dir().join(format!(
            "mgs-arena-upload-atomic-{}",
            Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&directory).expect("temporary upload directory");
        let path = directory.join("fighter.wasm");
        fs::write(&path, b"old").expect("prior artifact");

        let replacement = b"\0asm\x01\0\0\0complete";
        atomic_replace_bytes(&path, replacement).expect("atomic upload publication");

        assert_eq!(fs::read(&path).expect("published upload"), replacement);
        assert_eq!(
            sha256_hex(replacement),
            "62f9ec8a2e7ed0b23dfa29381e0d972c22329f182f440a75339c7cc2537c4204"
        );
        assert_eq!(
            fs::read_dir(&directory)
                .expect("upload directory")
                .filter_map(Result::ok)
                .count(),
            1
        );
        let _ = fs::remove_dir_all(directory);
    }
}
