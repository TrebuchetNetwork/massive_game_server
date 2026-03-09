use crate::operational::bot_sandbox::{BotMatchOutcome, TeamBattleOutcome};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Public response / view structs ──────────────────────────────────────────

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
pub struct SimulateTeamBattleResponse {
    pub generated_at: u64,
    pub simulation: TeamBattleOutcome,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArenaReplayView {
    pub replay_id: String,
    pub match_id: String,
    pub executed_at: u64,
    pub mode: String,
    pub model_a_id: String,
    pub model_b_id: String,
    pub winner_model_id: Option<String>,
    pub draw: bool,
    pub model_a_score: i32,
    pub model_b_score: i32,
    pub objective_label: String,
    pub objective_a: i32,
    pub objective_b: i32,
    pub model_a_runtime: String,
    pub model_b_runtime: String,
    pub ticks_executed: u32,
    pub duration_ms: u64,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArenaReplayListResponse {
    pub generated_at: u64,
    pub total_replays: usize,
    pub replays: Vec<ArenaReplayView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArenaReplayEvent {
    pub sequence: u64,
    pub emitted_at: u64,
    pub match_id: String,
    pub mode: String,
    pub event_type: String,
    pub tick: Option<u32>,
    pub action_model_a: Option<String>,
    pub action_model_b: Option<String>,
    pub health_model_a: Option<i32>,
    pub health_model_b: Option<i32>,
    pub score_model_a: Option<i32>,
    pub score_model_b: Option<i32>,
    pub objective_a: Option<i32>,
    pub objective_b: Option<i32>,
    pub winner_model_id: Option<String>,
    pub draw: Option<bool>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArenaReplayEventFeedResponse {
    pub generated_at: u64,
    pub total_events: usize,
    pub returned_events: usize,
    pub newest_sequence: Option<u64>,
    pub events: Vec<ArenaReplayEvent>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArenaMatchReplayResponse {
    pub generated_at: u64,
    pub match_id: String,
    pub mode: String,
    pub model_a_id: String,
    pub model_b_id: String,
    pub seed: u64,
    pub max_ticks: u32,
    pub ticks_executed: u32,
    pub duration_ms: u64,
    pub winner_model_id: Option<String>,
    pub draw: bool,
    pub truncated: bool,
    pub total_frames: usize,
    pub total_events: usize,
    pub returned_events: usize,
    pub completed_at: u64,
    pub warnings: Vec<String>,
    pub events: Vec<ArenaReplayEvent>,
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
    pub total_match_duration_ms: u64,
    pub avg_match_duration_ms: f64,
    pub max_match_duration_ms: u64,
    pub min_match_duration_ms: Option<u64>,
    pub total_ticks_executed: u64,
    pub avg_ticks_executed: f64,
    pub warning_matches: u64,
    pub runtime_fallback_matches: u64,
    pub trap_warnings: u64,
    pub timeout_warnings: u64,
    pub draw_matches: u64,
    pub model_win_distribution: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UploadModelWasmResponse {
    pub model_id: String,
    pub wasm_path: String,
    pub bytes_written: usize,
    pub overwritten: bool,
}

// ── Internal types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct PersistentArenaStore {
    pub(super) models: HashMap<String, ArenaModelRecord>,
    pub(super) completed_matches: Vec<CompletedMatchRecord>,
}

#[derive(Clone)]
pub(super) struct ArenaRedisStore {
    pub(super) client: redis::Client,
    pub(super) key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ArenaModelRecord {
    pub(super) model_id: String,
    pub(super) model_name: String,
    pub(super) provider: String,
    pub(super) version: String,
    pub(super) active: bool,
    pub(super) created_at: u64,
    pub(super) updated_at: u64,
    pub(super) last_seen_at: u64,
    pub(super) elo_rating: f64,
    pub(super) matches_played: u64,
    pub(super) wins: u64,
    pub(super) losses: u64,
    pub(super) draws: u64,
    pub(super) cumulative_score: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CompletedMatchRecord {
    pub(super) match_id: String,
    pub(super) model_a_id: String,
    pub(super) model_b_id: String,
    pub(super) winner_model_id: Option<String>,
    pub(super) draw: bool,
    pub(super) model_a_score: i32,
    pub(super) model_b_score: i32,
    pub(super) duration_ms: Option<u64>,
    pub(super) completed_at: u64,
}

#[derive(Debug, Clone)]
pub(super) struct QueuedMatch {
    pub(super) match_id: String,
    pub(super) model_a_id: String,
    pub(super) model_b_id: String,
    pub(super) mode: String,
    pub(super) queued_at: u64,
    #[allow(dead_code)]
    pub(super) metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub(super) struct ArenaMatchReplayRecord {
    pub(super) match_id: String,
    pub(super) mode: String,
    pub(super) model_a_id: String,
    pub(super) model_b_id: String,
    pub(super) seed: u64,
    pub(super) max_ticks: u32,
    pub(super) ticks_executed: u32,
    pub(super) duration_ms: u64,
    pub(super) winner_model_id: Option<String>,
    pub(super) draw: bool,
    pub(super) warnings: Vec<String>,
    pub(super) truncated: bool,
    pub(super) total_frames: usize,
    pub(super) completed_at: u64,
    pub(super) events: Vec<ArenaReplayEvent>,
}

// ── Request / query types (used by routes) ──────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RegisterModelBody {
    pub(super) model_id: Option<String>,
    pub(super) model_name: String,
    pub(super) provider: Option<String>,
    pub(super) version: Option<String>,
    pub(super) active: Option<bool>,
    pub(super) initial_elo: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ModelHeartbeatBody {
    pub(super) model_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct UploadModelWasmBody {
    pub(super) model_id: String,
    pub(super) wasm_base64: String,
    pub(super) overwrite: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct QueueMatchBody {
    pub(super) model_a_id: String,
    pub(super) model_b_id: String,
    pub(super) mode: Option<String>,
    pub(super) metadata: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct QueueRoundRobinBody {
    pub(super) mode: Option<String>,
    pub(super) include_inactive: Option<bool>,
    pub(super) max_pairs: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ReportMatchBody {
    pub(super) match_id: String,
    pub(super) model_a_id: Option<String>,
    pub(super) model_b_id: Option<String>,
    pub(super) winner_model_id: Option<String>,
    pub(super) draw: Option<bool>,
    pub(super) model_a_score: Option<i32>,
    pub(super) model_b_score: Option<i32>,
    pub(super) duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(super) struct LeaderboardQuery {
    pub(super) limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(super) struct PendingQuery {
    pub(super) limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(super) struct ReplayQuery {
    pub(super) limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(super) struct ReplayEventsQuery {
    pub(super) limit: Option<usize>,
    pub(super) after_sequence: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(super) struct ReplayStreamQuery {
    pub(super) after_sequence: Option<u64>,
    pub(super) backlog: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(super) struct ExecuteNextBody {
    pub(super) max_ticks: Option<u32>,
    pub(super) seed: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(super) struct SimulateTeamBattleBody {
    pub(super) model_a_id: String,
    pub(super) model_b_id: String,
    pub(super) mode: Option<String>,
    pub(super) team_size: Option<u32>,
    pub(super) rounds: Option<u32>,
    pub(super) max_ticks: Option<u32>,
    pub(super) seed: Option<u64>,
}

// ── API envelope types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub(super) struct ApiErrorBody {
    pub(super) code: &'static str,
    pub(super) message: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ApiResponse<T>
where
    T: Serialize,
{
    pub(super) ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<ApiErrorBody>,
}

// ── ArenaError ──────────────────────────────────────────────────────────────

#[derive(Debug)]
pub(super) enum ArenaError {
    InvalidInput(&'static str, String),
    NotFound(&'static str, String),
    Conflict(&'static str, String),
    Internal(String),
}

impl ArenaError {
    pub(super) fn code(&self) -> &'static str {
        match self {
            ArenaError::InvalidInput(code, _) => code,
            ArenaError::NotFound(code, _) => code,
            ArenaError::Conflict(code, _) => code,
            ArenaError::Internal(_) => "internal_error",
        }
    }

    pub(super) fn message(&self) -> String {
        match self {
            ArenaError::InvalidInput(_, msg)
            | ArenaError::NotFound(_, msg)
            | ArenaError::Conflict(_, msg)
            | ArenaError::Internal(msg) => msg.clone(),
        }
    }
}
