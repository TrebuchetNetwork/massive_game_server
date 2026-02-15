use crate::operational::bot_sandbox::{ArenaMatchMode, BotMatchOutcome, BotSandbox};
use base64::Engine as _;
use dashmap::DashMap;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{info, warn};
use uuid::Uuid;
use warp::{Filter, Reply};

const DEFAULT_ARENA_LEADERBOARD_LIMIT: usize = 25;
const DEFAULT_ARENA_PENDING_LIMIT: usize = 20;
const DEFAULT_BASE_ELO: f64 = 1000.0;
const DEFAULT_ARENA_WASM_DIR: &str = "data/arena_bots";
const DEFAULT_ARENA_WASM_MAX_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
pub struct ArenaService {
    inner: Arc<ArenaInner>,
}

struct ArenaInner {
    store_path: PathBuf,
    bot_sandbox: BotSandbox,
    wasm_dir: PathBuf,
    wasm_max_bytes: usize,
    persistent_store: RwLock<PersistentArenaStore>,
    pending_matches: Mutex<VecDeque<QueuedMatch>>,
    in_flight_matches: DashMap<String, QueuedMatch>,
    total_matches_reported: AtomicU64,
    worker_runs: AtomicU64,
    worker_executed: AtomicU64,
    worker_idle: AtomicU64,
    worker_failures: AtomicU64,
    worker_last_success_at: AtomicU64,
    worker_last_failure_at: AtomicU64,
    worker_last_error: RwLock<Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistentArenaStore {
    models: HashMap<String, ArenaModelRecord>,
    completed_matches: Vec<CompletedMatchRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArenaModelRecord {
    model_id: String,
    model_name: String,
    provider: String,
    version: String,
    active: bool,
    created_at: u64,
    updated_at: u64,
    last_seen_at: u64,
    elo_rating: f64,
    matches_played: u64,
    wins: u64,
    losses: u64,
    draws: u64,
    cumulative_score: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CompletedMatchRecord {
    match_id: String,
    model_a_id: String,
    model_b_id: String,
    winner_model_id: Option<String>,
    draw: bool,
    model_a_score: i32,
    model_b_score: i32,
    duration_ms: Option<u64>,
    completed_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QueuedMatch {
    match_id: String,
    model_a_id: String,
    model_b_id: String,
    mode: String,
    queued_at: u64,
    metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArenaModelView {
    pub model_id: String,
    pub model_name: String,
    pub provider: String,
    pub version: String,
    pub active: bool,
    pub elo_rating: f64,
    pub matches_played: u64,
    pub wins: u64,
    pub losses: u64,
    pub draws: u64,
    pub cumulative_score: i64,
    pub win_rate: f64,
    pub created_at: u64,
    pub updated_at: u64,
    pub last_seen_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArenaLeaderboardResponse {
    pub generated_at: u64,
    pub total_models: usize,
    pub total_completed_matches: usize,
    pub models: Vec<ArenaModelView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArenaOverviewResponse {
    pub generated_at: u64,
    pub active_models: usize,
    pub pending_matches: usize,
    pub in_flight_matches: usize,
    pub total_completed_matches: usize,
    pub total_matches_reported_runtime: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueueMatchResponse {
    pub queued_count: usize,
    pub pending_total: usize,
    pub queued_matches: Vec<QueuedMatchView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueuedMatchView {
    pub match_id: String,
    pub model_a_id: String,
    pub model_b_id: String,
    pub mode: String,
    pub queued_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClaimMatchResponse {
    pub claimed: Option<QueuedMatchView>,
    pub pending_total: usize,
    pub in_flight_total: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportMatchResponse {
    pub match_id: String,
    pub completed_at: u64,
    pub model_a: ArenaModelView,
    pub model_b: ArenaModelView,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecuteNextMatchResponse {
    pub pending_before: usize,
    pub pending_after: usize,
    pub report: ReportMatchResponse,
    pub sandbox: BotMatchOutcome,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArenaWorkerStatsResponse {
    pub generated_at: u64,
    pub pending_matches: usize,
    pub in_flight_matches: usize,
    pub runs: u64,
    pub executed: u64,
    pub idle: u64,
    pub failures: u64,
    pub last_success_at: Option<u64>,
    pub last_failure_at: Option<u64>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UploadModelWasmResponse {
    pub model_id: String,
    pub wasm_path: String,
    pub bytes_written: usize,
    pub overwritten: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct RegisterModelBody {
    model_id: Option<String>,
    model_name: String,
    provider: Option<String>,
    version: Option<String>,
    active: Option<bool>,
    initial_elo: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
struct ModelHeartbeatBody {
    model_id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct UploadModelWasmBody {
    model_id: String,
    wasm_base64: String,
    overwrite: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
struct QueueMatchBody {
    model_a_id: String,
    model_b_id: String,
    mode: Option<String>,
    metadata: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize)]
struct QueueRoundRobinBody {
    mode: Option<String>,
    include_inactive: Option<bool>,
    max_pairs: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
struct ReportMatchBody {
    match_id: String,
    model_a_id: Option<String>,
    model_b_id: Option<String>,
    winner_model_id: Option<String>,
    draw: Option<bool>,
    model_a_score: Option<i32>,
    model_b_score: Option<i32>,
    duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct LeaderboardQuery {
    limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PendingQuery {
    limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ExecuteNextBody {
    max_ticks: Option<u32>,
    seed: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct ApiErrorBody {
    code: &'static str,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
struct ApiResponse<T>
where
    T: Serialize,
{
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ApiErrorBody>,
}

#[derive(Debug)]
enum ArenaError {
    InvalidInput(&'static str, String),
    NotFound(&'static str, String),
    Conflict(&'static str, String),
    Internal(String),
}

impl ArenaError {
    fn code(&self) -> &'static str {
        match self {
            ArenaError::InvalidInput(code, _) => code,
            ArenaError::NotFound(code, _) => code,
            ArenaError::Conflict(code, _) => code,
            ArenaError::Internal(_) => "internal_error",
        }
    }

    fn message(&self) -> String {
        match self {
            ArenaError::InvalidInput(_, msg)
            | ArenaError::NotFound(_, msg)
            | ArenaError::Conflict(_, msg)
            | ArenaError::Internal(msg) => msg.clone(),
        }
    }
}

impl ArenaService {
    pub fn new_from_env() -> Self {
        let store_path = std::env::var("MGS_ARENA_STORE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("data/arena_store.json"));
        let wasm_dir = std::env::var("MGS_ARENA_WASM_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_ARENA_WASM_DIR));
        let wasm_max_bytes = std::env::var("MGS_ARENA_WASM_MAX_BYTES")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .filter(|value| *value > 1024)
            .unwrap_or(DEFAULT_ARENA_WASM_MAX_BYTES);
        let persistent_store = load_persistent_store(&store_path);
        let completed_count = persistent_store.completed_matches.len() as u64;

        info!(
            "Arena service initialized. store_path='{}', wasm_dir='{}', models={}, completed_matches={}",
            store_path.display(),
            wasm_dir.display(),
            persistent_store.models.len(),
            completed_count
        );

        Self {
            inner: Arc::new(ArenaInner {
                store_path,
                bot_sandbox: BotSandbox::new_from_env(),
                wasm_dir,
                wasm_max_bytes,
                persistent_store: RwLock::new(persistent_store),
                pending_matches: Mutex::new(VecDeque::new()),
                in_flight_matches: DashMap::new(),
                total_matches_reported: AtomicU64::new(completed_count),
                worker_runs: AtomicU64::new(0),
                worker_executed: AtomicU64::new(0),
                worker_idle: AtomicU64::new(0),
                worker_failures: AtomicU64::new(0),
                worker_last_success_at: AtomicU64::new(0),
                worker_last_failure_at: AtomicU64::new(0),
                worker_last_error: RwLock::new(None),
            }),
        }
    }

    fn register_model(&self, body: RegisterModelBody) -> Result<ArenaModelView, ArenaError> {
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
            let entry = store
                .models
                .entry(model_id.clone())
                .or_insert_with(|| ArenaModelRecord {
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

        self.persist_store()
            .map_err(|err| ArenaError::Internal(format!("failed to persist model: {}", err)))?;

        self.get_model_view(&model_id).ok_or_else(|| {
            ArenaError::Internal("model registration verification failed".to_owned())
        })
    }

    fn heartbeat_model(&self, body: ModelHeartbeatBody) -> Result<ArenaModelView, ArenaError> {
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

        self.persist_store()
            .map_err(|err| ArenaError::Internal(format!("failed to persist heartbeat: {}", err)))?;

        self.get_model_view(body.model_id.trim())
            .ok_or_else(|| ArenaError::Internal("heartbeat verification failed".to_owned()))
    }

    fn upload_model_wasm(
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

        fs::write(&wasm_path, &wasm_bytes).map_err(|err| {
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
        self.persist_store().map_err(ArenaError::Internal)?;

        Ok(UploadModelWasmResponse {
            model_id: model_id.to_owned(),
            wasm_path: wasm_path.display().to_string(),
            bytes_written: wasm_bytes.len(),
            overwritten: existed,
        })
    }

    fn queue_match(&self, body: QueueMatchBody) -> Result<QueueMatchResponse, ArenaError> {
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
        pending.push_back(queued.clone());
        let pending_total = pending.len();
        drop(pending);

        Ok(QueueMatchResponse {
            queued_count: 1,
            pending_total,
            queued_matches: vec![to_queued_match_view(&queued)],
        })
    }

    fn queue_round_robin(
        &self,
        body: QueueRoundRobinBody,
    ) -> Result<QueueMatchResponse, ArenaError> {
        let include_inactive = body.include_inactive.unwrap_or(false);
        let mode = normalize_match_mode(body.mode.as_deref())?;
        let max_pairs = body.max_pairs.unwrap_or(512).max(1);

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
                if queued_matches.len() >= max_pairs {
                    break 'outer;
                }
            }
        }

        let mut pending = self.inner.pending_matches.lock();
        for queued in &queued_matches {
            pending.push_back(queued.clone());
        }
        let pending_total = pending.len();
        drop(pending);

        Ok(QueueMatchResponse {
            queued_count: queued_matches.len(),
            pending_total,
            queued_matches: queued_matches.iter().map(to_queued_match_view).collect(),
        })
    }

    fn claim_next_match(&self) -> ClaimMatchResponse {
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

    fn list_pending_matches(&self, limit: usize) -> Vec<QueuedMatchView> {
        let bounded = limit.max(1);
        let pending = self.inner.pending_matches.lock();
        pending
            .iter()
            .take(bounded)
            .map(to_queued_match_view)
            .collect()
    }

    fn leaderboard(&self, limit: usize) -> ArenaLeaderboardResponse {
        let bounded_limit = limit.max(1);
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

    fn overview(&self) -> ArenaOverviewResponse {
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

    fn report_match(&self, body: ReportMatchBody) -> Result<ReportMatchResponse, ArenaError> {
        let in_flight = self.inner.in_flight_matches.remove(body.match_id.trim());
        let pending_match = in_flight
            .map(|(_, value)| value)
            .or_else(|| {
                let pending = self.inner.pending_matches.lock();
                pending
                    .iter()
                    .find(|entry| entry.match_id == body.match_id.trim())
                    .cloned()
            })
            .ok_or_else(|| {
                ArenaError::NotFound(
                    "match_not_found",
                    format!(
                        "match '{}' was not found in pending/in-flight queue",
                        body.match_id.trim()
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

        let (model_a_old_elo, model_b_old_elo) = {
            let store = self.inner.persistent_store.read();
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

        let model_a_result = if draw {
            MatchResult::Draw
        } else if winner_model_id.as_deref() == Some(model_a_id.as_str()) {
            MatchResult::Win
        } else {
            MatchResult::Loss
        };
        let model_b_result = model_a_result.inverse();
        let (new_a, new_b) = update_elo_pair(
            model_a_old_elo,
            model_b_old_elo,
            model_a_result,
            model_b_result,
        );

        let (model_a_view, model_b_view) = {
            let mut store = self.inner.persistent_store.write();
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

            store.completed_matches.push(CompletedMatchRecord {
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

        self.persist_store().map_err(|err| {
            ArenaError::Internal(format!(
                "failed to persist match report '{}': {}",
                body.match_id, err
            ))
        })?;

        Ok(ReportMatchResponse {
            match_id: body.match_id.trim().to_owned(),
            completed_at,
            model_a: model_a_view,
            model_b: model_b_view,
        })
    }

    fn execute_next_match(
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
        let sandbox_outcome = self.inner.bot_sandbox.execute_match(
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

        let pending_after = self.inner.pending_matches.lock().len();
        Ok(ExecuteNextMatchResponse {
            pending_before,
            pending_after,
            report,
            sandbox: sandbox_outcome,
        })
    }

    pub fn worker_execute_next(
        &self,
        max_ticks: Option<u32>,
        seed: Option<u64>,
    ) -> Result<Option<ExecuteNextMatchResponse>, String> {
        self.inner.worker_runs.fetch_add(1, Ordering::Relaxed);
        match self.execute_next_match(ExecuteNextBody { max_ticks, seed }) {
            Ok(response) => {
                self.inner.worker_executed.fetch_add(1, Ordering::Relaxed);
                self.inner
                    .worker_last_success_at
                    .store(unix_now(), Ordering::Relaxed);
                *self.inner.worker_last_error.write() = None;
                Ok(Some(response))
            }
            Err(ArenaError::NotFound(code, _)) if code == "no_pending_match" => {
                self.inner.worker_idle.fetch_add(1, Ordering::Relaxed);
                Ok(None)
            }
            Err(err) => {
                self.inner.worker_failures.fetch_add(1, Ordering::Relaxed);
                self.inner
                    .worker_last_failure_at
                    .store(unix_now(), Ordering::Relaxed);
                let message = err.message();
                *self.inner.worker_last_error.write() = Some(message.clone());
                Err(message)
            }
        }
    }

    pub fn worker_stats(&self) -> ArenaWorkerStatsResponse {
        let last_success_raw = self.inner.worker_last_success_at.load(Ordering::Relaxed);
        let last_failure_raw = self.inner.worker_last_failure_at.load(Ordering::Relaxed);
        ArenaWorkerStatsResponse {
            generated_at: unix_now(),
            pending_matches: self.inner.pending_matches.lock().len(),
            in_flight_matches: self.inner.in_flight_matches.len(),
            runs: self.inner.worker_runs.load(Ordering::Relaxed),
            executed: self.inner.worker_executed.load(Ordering::Relaxed),
            idle: self.inner.worker_idle.load(Ordering::Relaxed),
            failures: self.inner.worker_failures.load(Ordering::Relaxed),
            last_success_at: (last_success_raw > 0).then_some(last_success_raw),
            last_failure_at: (last_failure_raw > 0).then_some(last_failure_raw),
            last_error: self.inner.worker_last_error.read().clone(),
        }
    }

    fn get_model_view(&self, model_id: &str) -> Option<ArenaModelView> {
        let store = self.inner.persistent_store.read();
        store.models.get(model_id).map(to_model_view)
    }

    fn persist_store(&self) -> Result<(), String> {
        let snapshot = {
            let store = self.inner.persistent_store.read();
            store.clone()
        };
        persist_store(&self.inner.store_path, &snapshot)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchResult {
    Win,
    Loss,
    Draw,
}

impl MatchResult {
    fn score(self) -> f64 {
        match self {
            MatchResult::Win => 1.0,
            MatchResult::Loss => 0.0,
            MatchResult::Draw => 0.5,
        }
    }

    fn inverse(self) -> MatchResult {
        match self {
            MatchResult::Win => MatchResult::Loss,
            MatchResult::Loss => MatchResult::Win,
            MatchResult::Draw => MatchResult::Draw,
        }
    }
}

fn apply_match_result(
    model: &mut ArenaModelRecord,
    result: MatchResult,
    score: i32,
    completed_at: u64,
) {
    model.matches_played = model.matches_played.saturating_add(1);
    match result {
        MatchResult::Win => model.wins = model.wins.saturating_add(1),
        MatchResult::Loss => model.losses = model.losses.saturating_add(1),
        MatchResult::Draw => model.draws = model.draws.saturating_add(1),
    }
    model.cumulative_score = model.cumulative_score.saturating_add(score as i64);
    model.updated_at = completed_at;
    model.last_seen_at = completed_at;
}

fn update_elo_pair(
    elo_a: f64,
    elo_b: f64,
    result_a: MatchResult,
    result_b: MatchResult,
) -> (f64, f64) {
    let expected_a = 1.0 / (1.0 + 10.0f64.powf((elo_b - elo_a) / 400.0));
    let expected_b = 1.0 / (1.0 + 10.0f64.powf((elo_a - elo_b) / 400.0));
    let k = 32.0;
    let updated_a = (elo_a + k * (result_a.score() - expected_a)).clamp(100.0, 4000.0);
    let updated_b = (elo_b + k * (result_b.score() - expected_b)).clamp(100.0, 4000.0);
    (updated_a, updated_b)
}

fn to_model_view(model: &ArenaModelRecord) -> ArenaModelView {
    let win_rate = if model.matches_played == 0 {
        0.0
    } else {
        model.wins as f64 / model.matches_played as f64
    };

    ArenaModelView {
        model_id: model.model_id.clone(),
        model_name: model.model_name.clone(),
        provider: model.provider.clone(),
        version: model.version.clone(),
        active: model.active,
        elo_rating: model.elo_rating,
        matches_played: model.matches_played,
        wins: model.wins,
        losses: model.losses,
        draws: model.draws,
        cumulative_score: model.cumulative_score,
        win_rate,
        created_at: model.created_at,
        updated_at: model.updated_at,
        last_seen_at: model.last_seen_at,
    }
}

fn to_queued_match_view(entry: &QueuedMatch) -> QueuedMatchView {
    QueuedMatchView {
        match_id: entry.match_id.clone(),
        model_a_id: entry.model_a_id.clone(),
        model_b_id: entry.model_b_id.clone(),
        mode: entry.mode.clone(),
        queued_at: entry.queued_at,
    }
}

fn normalize_match_mode(raw_mode: Option<&str>) -> Result<String, ArenaError> {
    let value = raw_mode
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("arena");
    let Some(mode) = ArenaMatchMode::parse(value) else {
        return Err(ArenaError::InvalidInput(
            "invalid_mode",
            format!(
                "unsupported mode '{}'; expected one of: arena, ctf, koth, tdm",
                value
            ),
        ));
    };
    Ok(mode.as_str().to_owned())
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

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn load_persistent_store(path: &Path) -> PersistentArenaStore {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) => {
            if err.kind() != std::io::ErrorKind::NotFound {
                warn!("Failed to read arena store '{}': {}", path.display(), err);
            }
            return PersistentArenaStore::default();
        }
    };

    serde_json::from_str(&raw).unwrap_or_else(|err| {
        warn!(
            "Failed to parse arena store '{}': {}. Starting with empty arena store.",
            path.display(),
            err
        );
        PersistentArenaStore::default()
    })
}

fn persist_store(path: &Path, store: &PersistentArenaStore) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create '{}': {}", parent.display(), err))?;
    }
    let serialized = serde_json::to_string_pretty(store)
        .map_err(|err| format!("failed to serialize arena store: {}", err))?;
    fs::write(path, serialized)
        .map_err(|err| format!("failed to write '{}': {}", path.display(), err))?;
    Ok(())
}

fn ok_response<T>(data: T) -> warp::reply::Json
where
    T: Serialize,
{
    warp::reply::json(&ApiResponse {
        ok: true,
        data: Some(data),
        error: None::<ApiErrorBody>,
    })
}

fn error_response(code: &'static str, message: String) -> warp::reply::Json {
    warp::reply::json(&ApiResponse::<serde_json::Value> {
        ok: false,
        data: None,
        error: Some(ApiErrorBody { code, message }),
    })
}

fn with_service(
    service: ArenaService,
) -> impl Filter<Extract = (ArenaService,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || service.clone())
}

pub fn build_arena_routes(
    service: ArenaService,
) -> impl Filter<Extract = (impl Reply,), Error = warp::Rejection> + Clone {
    let register_model = warp::path!("api" / "arena" / "models" / "register")
        .and(warp::post())
        .and(warp::body::json())
        .and(with_service(service.clone()))
        .map(
            |body: RegisterModelBody, arena: ArenaService| match arena.register_model(body) {
                Ok(model) => ok_response(model),
                Err(err) => error_response(err.code(), err.message()),
            },
        );

    let model_heartbeat = warp::path!("api" / "arena" / "models" / "heartbeat")
        .and(warp::post())
        .and(warp::body::json())
        .and(with_service(service.clone()))
        .map(
            |body: ModelHeartbeatBody, arena: ArenaService| match arena.heartbeat_model(body) {
                Ok(model) => ok_response(model),
                Err(err) => error_response(err.code(), err.message()),
            },
        );

    let upload_model_wasm = warp::path!("api" / "arena" / "models" / "upload_wasm")
        .and(warp::post())
        .and(warp::body::json())
        .and(with_service(service.clone()))
        .map(|body: UploadModelWasmBody, arena: ArenaService| {
            match arena.upload_model_wasm(body) {
                Ok(result) => ok_response(result),
                Err(err) => error_response(err.code(), err.message()),
            }
        });

    let list_models = warp::path!("api" / "arena" / "models")
        .and(warp::get())
        .and(
            warp::query::<LeaderboardQuery>()
                .or(warp::any().map(LeaderboardQuery::default))
                .unify(),
        )
        .and(with_service(service.clone()))
        .map(|query: LeaderboardQuery, arena: ArenaService| {
            let limit = query.limit.unwrap_or(DEFAULT_ARENA_LEADERBOARD_LIMIT);
            let leaderboard = arena.leaderboard(limit);
            ok_response(leaderboard.models)
        });

    let queue_match = warp::path!("api" / "arena" / "matches" / "queue")
        .and(warp::post())
        .and(warp::body::json())
        .and(with_service(service.clone()))
        .map(
            |body: QueueMatchBody, arena: ArenaService| match arena.queue_match(body) {
                Ok(result) => ok_response(result),
                Err(err) => error_response(err.code(), err.message()),
            },
        );

    let queue_round_robin = warp::path!("api" / "arena" / "matches" / "queue_round_robin")
        .and(warp::post())
        .and(warp::body::json())
        .and(with_service(service.clone()))
        .map(|body: QueueRoundRobinBody, arena: ArenaService| {
            match arena.queue_round_robin(body) {
                Ok(result) => ok_response(result),
                Err(err) => error_response(err.code(), err.message()),
            }
        });

    let claim_next = warp::path!("api" / "arena" / "matches" / "claim_next")
        .and(warp::post())
        .and(with_service(service.clone()))
        .map(|arena: ArenaService| ok_response(arena.claim_next_match()));

    let execute_next = warp::path!("api" / "arena" / "matches" / "execute_next")
        .and(warp::post())
        .and(
            warp::body::json::<ExecuteNextBody>()
                .or(warp::any().map(ExecuteNextBody::default))
                .unify(),
        )
        .and(with_service(service.clone()))
        .map(
            |body: ExecuteNextBody, arena: ArenaService| match arena.execute_next_match(body) {
                Ok(result) => ok_response(result),
                Err(err) => error_response(err.code(), err.message()),
            },
        );

    let list_pending = warp::path!("api" / "arena" / "matches" / "pending")
        .and(warp::get())
        .and(
            warp::query::<PendingQuery>()
                .or(warp::any().map(PendingQuery::default))
                .unify(),
        )
        .and(with_service(service.clone()))
        .map(|query: PendingQuery, arena: ArenaService| {
            let limit = query.limit.unwrap_or(DEFAULT_ARENA_PENDING_LIMIT);
            ok_response(arena.list_pending_matches(limit))
        });

    let report_match = warp::path!("api" / "arena" / "matches" / "report")
        .and(warp::post())
        .and(warp::body::json())
        .and(with_service(service.clone()))
        .map(
            |body: ReportMatchBody, arena: ArenaService| match arena.report_match(body) {
                Ok(result) => ok_response(result),
                Err(err) => error_response(err.code(), err.message()),
            },
        );

    let leaderboard = warp::path!("api" / "arena" / "leaderboard")
        .and(warp::get())
        .and(
            warp::query::<LeaderboardQuery>()
                .or(warp::any().map(LeaderboardQuery::default))
                .unify(),
        )
        .and(with_service(service.clone()))
        .map(|query: LeaderboardQuery, arena: ArenaService| {
            let limit = query.limit.unwrap_or(DEFAULT_ARENA_LEADERBOARD_LIMIT);
            ok_response(arena.leaderboard(limit))
        });

    let overview = warp::path!("api" / "arena" / "overview")
        .and(warp::get())
        .and(with_service(service.clone()))
        .map(|arena: ArenaService| ok_response(arena.overview()));

    let worker_stats = warp::path!("api" / "arena" / "worker" / "stats")
        .and(warp::get())
        .and(with_service(service))
        .map(|arena: ArenaService| ok_response(arena.worker_stats()));

    register_model
        .or(model_heartbeat)
        .or(upload_model_wasm)
        .or(list_models)
        .or(queue_match)
        .or(queue_round_robin)
        .or(claim_next)
        .or(execute_next)
        .or(list_pending)
        .or(report_match)
        .or(leaderboard)
        .or(overview)
        .or(worker_stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elo_updates_are_conservative() {
        let (new_a, new_b) = update_elo_pair(1000.0, 1000.0, MatchResult::Win, MatchResult::Loss);
        assert!(new_a > 1000.0);
        assert!(new_b < 1000.0);
        let delta_a = new_a - 1000.0;
        let delta_b = 1000.0 - new_b;
        assert!((delta_a - delta_b).abs() < 0.001);
    }

    #[test]
    fn round_robin_limits_pairs() {
        let service = ArenaService {
            inner: Arc::new(ArenaInner {
                store_path: PathBuf::from("/tmp/test_arena_store_unused.json"),
                bot_sandbox: BotSandbox::new_from_env(),
                wasm_dir: PathBuf::from("/tmp/test_arena_wasm"),
                wasm_max_bytes: DEFAULT_ARENA_WASM_MAX_BYTES,
                persistent_store: RwLock::new(PersistentArenaStore {
                    models: HashMap::from([
                        (
                            "a".to_owned(),
                            ArenaModelRecord {
                                model_id: "a".to_owned(),
                                model_name: "a".to_owned(),
                                provider: "x".to_owned(),
                                version: "1".to_owned(),
                                active: true,
                                created_at: 1,
                                updated_at: 1,
                                last_seen_at: 1,
                                elo_rating: 1000.0,
                                matches_played: 0,
                                wins: 0,
                                losses: 0,
                                draws: 0,
                                cumulative_score: 0,
                            },
                        ),
                        (
                            "b".to_owned(),
                            ArenaModelRecord {
                                model_id: "b".to_owned(),
                                model_name: "b".to_owned(),
                                provider: "x".to_owned(),
                                version: "1".to_owned(),
                                active: true,
                                created_at: 1,
                                updated_at: 1,
                                last_seen_at: 1,
                                elo_rating: 1000.0,
                                matches_played: 0,
                                wins: 0,
                                losses: 0,
                                draws: 0,
                                cumulative_score: 0,
                            },
                        ),
                        (
                            "c".to_owned(),
                            ArenaModelRecord {
                                model_id: "c".to_owned(),
                                model_name: "c".to_owned(),
                                provider: "x".to_owned(),
                                version: "1".to_owned(),
                                active: true,
                                created_at: 1,
                                updated_at: 1,
                                last_seen_at: 1,
                                elo_rating: 1000.0,
                                matches_played: 0,
                                wins: 0,
                                losses: 0,
                                draws: 0,
                                cumulative_score: 0,
                            },
                        ),
                    ]),
                    completed_matches: Vec::new(),
                }),
                pending_matches: Mutex::new(VecDeque::new()),
                in_flight_matches: DashMap::new(),
                total_matches_reported: AtomicU64::new(0),
                worker_runs: AtomicU64::new(0),
                worker_executed: AtomicU64::new(0),
                worker_idle: AtomicU64::new(0),
                worker_failures: AtomicU64::new(0),
                worker_last_success_at: AtomicU64::new(0),
                worker_last_failure_at: AtomicU64::new(0),
                worker_last_error: RwLock::new(None),
            }),
        };

        let result = service
            .queue_round_robin(QueueRoundRobinBody {
                mode: Some("TEAM_DEATHMATCH".to_owned()),
                include_inactive: Some(false),
                max_pairs: Some(2),
            })
            .expect("round robin should queue");
        assert_eq!(result.queued_count, 2);
        assert!(result
            .queued_matches
            .iter()
            .all(|entry| entry.mode == "tdm"));
    }

    #[test]
    fn execute_next_match_reports_result() {
        let service = ArenaService {
            inner: Arc::new(ArenaInner {
                store_path: PathBuf::from("/tmp/test_arena_store_unused.json"),
                bot_sandbox: BotSandbox::new_from_env(),
                wasm_dir: PathBuf::from("/tmp/test_arena_wasm"),
                wasm_max_bytes: DEFAULT_ARENA_WASM_MAX_BYTES,
                persistent_store: RwLock::new(PersistentArenaStore {
                    models: HashMap::from([
                        (
                            "a".to_owned(),
                            ArenaModelRecord {
                                model_id: "a".to_owned(),
                                model_name: "a".to_owned(),
                                provider: "x".to_owned(),
                                version: "1".to_owned(),
                                active: true,
                                created_at: 1,
                                updated_at: 1,
                                last_seen_at: 1,
                                elo_rating: 1000.0,
                                matches_played: 0,
                                wins: 0,
                                losses: 0,
                                draws: 0,
                                cumulative_score: 0,
                            },
                        ),
                        (
                            "b".to_owned(),
                            ArenaModelRecord {
                                model_id: "b".to_owned(),
                                model_name: "b".to_owned(),
                                provider: "x".to_owned(),
                                version: "1".to_owned(),
                                active: true,
                                created_at: 1,
                                updated_at: 1,
                                last_seen_at: 1,
                                elo_rating: 1000.0,
                                matches_played: 0,
                                wins: 0,
                                losses: 0,
                                draws: 0,
                                cumulative_score: 0,
                            },
                        ),
                    ]),
                    completed_matches: Vec::new(),
                }),
                pending_matches: Mutex::new(VecDeque::new()),
                in_flight_matches: DashMap::new(),
                total_matches_reported: AtomicU64::new(0),
                worker_runs: AtomicU64::new(0),
                worker_executed: AtomicU64::new(0),
                worker_idle: AtomicU64::new(0),
                worker_failures: AtomicU64::new(0),
                worker_last_success_at: AtomicU64::new(0),
                worker_last_failure_at: AtomicU64::new(0),
                worker_last_error: RwLock::new(None),
            }),
        };

        let queued = service
            .queue_match(QueueMatchBody {
                model_a_id: "a".to_owned(),
                model_b_id: "b".to_owned(),
                mode: Some("ctf".to_owned()),
                metadata: None,
            })
            .expect("queue should succeed");
        assert_eq!(queued.queued_count, 1);

        let executed = service
            .execute_next_match(ExecuteNextBody {
                max_ticks: Some(120),
                seed: Some(7),
            })
            .expect("execute should succeed");
        assert_eq!(executed.pending_after, 0);
        assert_eq!(executed.report.model_a.matches_played, 1);
        assert_eq!(executed.report.model_b.matches_played, 1);
        assert!(executed.sandbox.ticks_executed > 0);
        assert!(executed.sandbox.ticks_executed <= 120);
        assert_eq!(executed.sandbox.mode, "ctf");
        assert_eq!(executed.sandbox.objective_label, "captures");
    }

    #[test]
    fn worker_execute_next_returns_none_when_queue_empty() {
        let service = ArenaService::new_from_env();
        let outcome = service
            .worker_execute_next(Some(64), Some(1))
            .expect("worker call should not fail");
        assert!(outcome.is_none());
        let stats = service.worker_stats();
        assert_eq!(stats.runs, 1);
        assert_eq!(stats.idle, 1);
    }

    #[test]
    fn upload_wasm_rejects_invalid_base64() {
        let service = ArenaService {
            inner: Arc::new(ArenaInner {
                store_path: PathBuf::from("/tmp/test_arena_store_unused.json"),
                bot_sandbox: BotSandbox::new_from_env(),
                wasm_dir: PathBuf::from("/tmp/test_arena_wasm"),
                wasm_max_bytes: DEFAULT_ARENA_WASM_MAX_BYTES,
                persistent_store: RwLock::new(PersistentArenaStore {
                    models: HashMap::from([(
                        "model_x".to_owned(),
                        ArenaModelRecord {
                            model_id: "model_x".to_owned(),
                            model_name: "model_x".to_owned(),
                            provider: "x".to_owned(),
                            version: "1".to_owned(),
                            active: true,
                            created_at: 1,
                            updated_at: 1,
                            last_seen_at: 1,
                            elo_rating: 1000.0,
                            matches_played: 0,
                            wins: 0,
                            losses: 0,
                            draws: 0,
                            cumulative_score: 0,
                        },
                    )]),
                    completed_matches: Vec::new(),
                }),
                pending_matches: Mutex::new(VecDeque::new()),
                in_flight_matches: DashMap::new(),
                total_matches_reported: AtomicU64::new(0),
                worker_runs: AtomicU64::new(0),
                worker_executed: AtomicU64::new(0),
                worker_idle: AtomicU64::new(0),
                worker_failures: AtomicU64::new(0),
                worker_last_success_at: AtomicU64::new(0),
                worker_last_failure_at: AtomicU64::new(0),
                worker_last_error: RwLock::new(None),
            }),
        };

        let result = service.upload_model_wasm(UploadModelWasmBody {
            model_id: "model_x".to_owned(),
            wasm_base64: "!!!not-base64!!!".to_owned(),
            overwrite: Some(true),
        });
        assert!(result.is_err());
    }
}
