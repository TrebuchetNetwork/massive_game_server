mod persistence;
pub(crate) mod ratings;
mod replays;
mod routes;
mod scoring;
mod service;
mod types;

// ── Re-exports: public API remains identical to the old single-file module ──

pub use routes::{build_arena_routes, build_public_arena_routes};
pub use types::{
    ArenaLeaderboardResponse, ArenaLeagueEpochStandingView, ArenaLeagueEpochView, ArenaLeagueView,
    ArenaMatchReplayResponse, ArenaModelView, ArenaOverviewResponse, ArenaRatingMethodology,
    ArenaRatingRanking, ArenaRatingsResponse, ArenaReplayEvent, ArenaReplayEventFeedResponse,
    ArenaReplayListResponse, ArenaReplayView, ArenaSeasonRatingView, ArenaWorkerStatsResponse,
    ClaimMatchResponse, ExecuteNextMatchResponse, QueueMatchResponse, QueuedMatchView,
    ReportMatchResponse, SimulateTeamBattleResponse, SimulateWorldBattleResponse,
    UploadModelWasmResponse,
};

use crate::operational::bot_sandbox::BotSandbox;
use dashmap::DashMap;
use parking_lot::{Mutex, RwLock};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use tokio::sync::broadcast;
use tracing::info;
use types::{ArenaMatchReplayRecord, ArenaRedisStore, PersistentArenaStore, QueuedMatch};

// ── Constants ───────────────────────────────────────────────────────────────

const DEFAULT_ARENA_WASM_DIR: &str = "data/arena_bots";
const DEFAULT_ARENA_WASM_MAX_BYTES: usize = 2 * 1024 * 1024;
const DEFAULT_ARENA_REPLAY_HISTORY_CAP: usize = 256;
const DEFAULT_ARENA_REPLAY_EVENT_HISTORY_CAP: usize = 4_096;
const DEFAULT_ARENA_REPLAY_MATCH_HISTORY_CAP: usize = 256;
const DEFAULT_ARENA_REPLAY_STREAM_CHANNEL_CAP: usize = 1_024;
const MAX_ARENA_REPLAY_HISTORY_CAP: usize = 4096;
const MAX_ARENA_REPLAY_EVENT_HISTORY_CAP: usize = 32_768;
const MAX_ARENA_REPLAY_MATCH_HISTORY_CAP: usize = 4_096;
const DEFAULT_ARENA_IN_FLIGHT_TTL_SECS: u64 = 30 * 60;
const MAX_ARENA_IN_FLIGHT_TTL_SECS: u64 = 24 * 60 * 60;

fn arena_in_flight_ttl_secs() -> u64 {
    static TTL_SECS: OnceLock<u64> = OnceLock::new();
    *TTL_SECS.get_or_init(|| {
        std::env::var("MGS_ARENA_IN_FLIGHT_TTL_SECS")
            .ok()
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_ARENA_IN_FLIGHT_TTL_SECS)
            .clamp(60, MAX_ARENA_IN_FLIGHT_TTL_SECS)
    })
}

// ── ArenaService + ArenaInner ───────────────────────────────────────────────

#[derive(Clone)]
pub struct ArenaService {
    inner: Arc<ArenaInner>,
}

struct ArenaInner {
    store_path: PathBuf,
    redis_store: Option<ArenaRedisStore>,
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
    worker_total_duration_ms: AtomicU64,
    worker_total_ticks: AtomicU64,
    worker_warning_matches: AtomicU64,
    worker_runtime_fallback_matches: AtomicU64,
    worker_trap_warnings: AtomicU64,
    worker_timeout_warnings: AtomicU64,
    worker_draw_matches: AtomicU64,
    worker_max_duration_ms: AtomicU64,
    worker_min_duration_ms: AtomicU64,
    worker_model_win_distribution: RwLock<HashMap<String, u64>>,
    replay_sequence: AtomicU64,
    replay_history_capacity: usize,
    recent_replays: Mutex<VecDeque<types::ArenaReplayView>>,
    replay_event_sequence: AtomicU64,
    replay_event_history_capacity: usize,
    replay_match_history_capacity: usize,
    replay_events: Mutex<VecDeque<types::ArenaReplayEvent>>,
    replay_match_order: Mutex<VecDeque<String>>,
    replay_matches: RwLock<HashMap<String, ArenaMatchReplayRecord>>,
    replay_event_tx: broadcast::Sender<types::ArenaReplayEvent>,
}

impl ArenaService {
    pub fn new_from_env() -> Self {
        let store_path = std::env::var("MGS_ARENA_STORE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("data/arena_store.json"));
        let redis_store = persistence::init_redis_store(
            std::env::var("MGS_ARENA_REDIS_URL")
                .ok()
                .or_else(|| std::env::var("MGS_REDIS_URL").ok()),
            std::env::var("MGS_REDIS_ARENA_STORE_KEY")
                .ok()
                .unwrap_or_else(|| "mgs:arena:store".to_owned()),
        );
        let wasm_dir = std::env::var("MGS_ARENA_WASM_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_ARENA_WASM_DIR));
        let wasm_max_bytes = std::env::var("MGS_ARENA_WASM_MAX_BYTES")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .filter(|value| *value > 1024)
            .unwrap_or(DEFAULT_ARENA_WASM_MAX_BYTES);
        let replay_history_capacity = std::env::var("MGS_ARENA_REPLAY_HISTORY_CAP")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(DEFAULT_ARENA_REPLAY_HISTORY_CAP)
            .clamp(16, MAX_ARENA_REPLAY_HISTORY_CAP);
        let replay_event_history_capacity = std::env::var("MGS_ARENA_REPLAY_EVENT_HISTORY_CAP")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(DEFAULT_ARENA_REPLAY_EVENT_HISTORY_CAP)
            .clamp(128, MAX_ARENA_REPLAY_EVENT_HISTORY_CAP);
        let replay_match_history_capacity = std::env::var("MGS_ARENA_REPLAY_MATCH_HISTORY_CAP")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(DEFAULT_ARENA_REPLAY_MATCH_HISTORY_CAP)
            .clamp(16, MAX_ARENA_REPLAY_MATCH_HISTORY_CAP);
        let replay_stream_channel_cap = std::env::var("MGS_ARENA_REPLAY_STREAM_CHANNEL_CAP")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(DEFAULT_ARENA_REPLAY_STREAM_CHANNEL_CAP)
            .clamp(64, 65_536);
        let (replay_event_tx, _) = broadcast::channel(replay_stream_channel_cap);
        let persistent_store =
            persistence::load_persistent_store(&store_path, redis_store.as_ref());
        let completed_count = persistent_store.completed_matches.len() as u64;

        info!(
            "Arena service initialized. store_path='{}', redis_backed={}, wasm_dir='{}', models={}, completed_matches={}, replay_history_capacity={}, replay_event_history_capacity={}, replay_match_history_capacity={}, replay_stream_channel_cap={}",
            store_path.display(),
            redis_store.is_some(),
            wasm_dir.display(),
            persistent_store.models.len(),
            completed_count,
            replay_history_capacity,
            replay_event_history_capacity,
            replay_match_history_capacity,
            replay_stream_channel_cap
        );

        Self {
            inner: Arc::new(ArenaInner {
                store_path,
                redis_store,
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
                worker_total_duration_ms: AtomicU64::new(0),
                worker_total_ticks: AtomicU64::new(0),
                worker_warning_matches: AtomicU64::new(0),
                worker_runtime_fallback_matches: AtomicU64::new(0),
                worker_trap_warnings: AtomicU64::new(0),
                worker_timeout_warnings: AtomicU64::new(0),
                worker_draw_matches: AtomicU64::new(0),
                worker_max_duration_ms: AtomicU64::new(0),
                worker_min_duration_ms: AtomicU64::new(0),
                worker_model_win_distribution: RwLock::new(HashMap::new()),
                replay_sequence: AtomicU64::new(0),
                replay_history_capacity,
                recent_replays: Mutex::new(VecDeque::new()),
                replay_event_sequence: AtomicU64::new(0),
                replay_event_history_capacity,
                replay_match_history_capacity,
                replay_events: Mutex::new(VecDeque::new()),
                replay_match_order: Mutex::new(VecDeque::new()),
                replay_matches: RwLock::new(HashMap::new()),
                replay_event_tx,
            }),
        }
    }

    pub fn worker_execute_next(
        &self,
        max_ticks: Option<u32>,
        seed: Option<u64>,
    ) -> Result<Option<ExecuteNextMatchResponse>, String> {
        use scoring::unix_now;
        use types::{ArenaError, ExecuteNextBody};

        self.inner.worker_runs.fetch_add(1, Ordering::Relaxed);
        match self.execute_next_match(ExecuteNextBody { max_ticks, seed }) {
            Ok(response) => {
                self.inner.worker_executed.fetch_add(1, Ordering::Relaxed);
                self.record_worker_success_metrics(&response);
                self.inner
                    .worker_last_success_at
                    .store(unix_now(), Ordering::Relaxed);
                *self.inner.worker_last_error.write() = None;
                Ok(Some(response))
            }
            Err(ArenaError::NotFound("no_pending_match", _)) => {
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
        use scoring::{safe_average, unix_now};

        let last_success_raw = self.inner.worker_last_success_at.load(Ordering::Relaxed);
        let last_failure_raw = self.inner.worker_last_failure_at.load(Ordering::Relaxed);
        let executed = self.inner.worker_executed.load(Ordering::Relaxed);
        let total_duration_ms = self.inner.worker_total_duration_ms.load(Ordering::Relaxed);
        let total_ticks = self.inner.worker_total_ticks.load(Ordering::Relaxed);
        let min_duration_raw = self.inner.worker_min_duration_ms.load(Ordering::Relaxed);
        ArenaWorkerStatsResponse {
            generated_at: unix_now(),
            pending_matches: self.inner.pending_matches.lock().len(),
            in_flight_matches: self.inner.in_flight_matches.len(),
            runs: self.inner.worker_runs.load(Ordering::Relaxed),
            executed,
            idle: self.inner.worker_idle.load(Ordering::Relaxed),
            failures: self.inner.worker_failures.load(Ordering::Relaxed),
            last_success_at: (last_success_raw > 0).then_some(last_success_raw),
            last_failure_at: (last_failure_raw > 0).then_some(last_failure_raw),
            last_error: self.inner.worker_last_error.read().clone(),
            total_match_duration_ms: total_duration_ms,
            avg_match_duration_ms: safe_average(total_duration_ms, executed),
            max_match_duration_ms: self.inner.worker_max_duration_ms.load(Ordering::Relaxed),
            min_match_duration_ms: (min_duration_raw > 0).then_some(min_duration_raw),
            total_ticks_executed: total_ticks,
            avg_ticks_executed: safe_average(total_ticks, executed),
            warning_matches: self.inner.worker_warning_matches.load(Ordering::Relaxed),
            runtime_fallback_matches: self
                .inner
                .worker_runtime_fallback_matches
                .load(Ordering::Relaxed),
            trap_warnings: self.inner.worker_trap_warnings.load(Ordering::Relaxed),
            timeout_warnings: self.inner.worker_timeout_warnings.load(Ordering::Relaxed),
            draw_matches: self.inner.worker_draw_matches.load(Ordering::Relaxed),
            model_win_distribution: self.inner.worker_model_win_distribution.read().clone(),
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::scoring::{update_elo_pair, MatchResult};
    use super::types::*;
    use super::*;
    use crate::operational::bot_sandbox::{
        BotSandbox, MAX_WORLD_BATTLE_ENTRANTS, MAX_WORLD_SQUAD_SIZE,
    };
    use crate::operational::validation::sanitize_model_id;
    use std::time::{SystemTime, UNIX_EPOCH};

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
                redis_store: None,
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
                worker_total_duration_ms: AtomicU64::new(0),
                worker_total_ticks: AtomicU64::new(0),
                worker_warning_matches: AtomicU64::new(0),
                worker_runtime_fallback_matches: AtomicU64::new(0),
                worker_trap_warnings: AtomicU64::new(0),
                worker_timeout_warnings: AtomicU64::new(0),
                worker_draw_matches: AtomicU64::new(0),
                worker_max_duration_ms: AtomicU64::new(0),
                worker_min_duration_ms: AtomicU64::new(0),
                worker_model_win_distribution: RwLock::new(HashMap::new()),
                replay_sequence: AtomicU64::new(0),
                replay_history_capacity: DEFAULT_ARENA_REPLAY_HISTORY_CAP,
                recent_replays: Mutex::new(VecDeque::new()),
                replay_event_sequence: AtomicU64::new(0),
                replay_event_history_capacity: DEFAULT_ARENA_REPLAY_EVENT_HISTORY_CAP,
                replay_match_history_capacity: DEFAULT_ARENA_REPLAY_MATCH_HISTORY_CAP,
                replay_events: Mutex::new(VecDeque::new()),
                replay_match_order: Mutex::new(VecDeque::new()),
                replay_matches: RwLock::new(HashMap::new()),
                replay_event_tx: broadcast::channel(DEFAULT_ARENA_REPLAY_STREAM_CHANNEL_CAP).0,
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
    fn sanitize_model_id_rejects_hidden_and_traversal_like_names() {
        assert!(sanitize_model_id("../etc/passwd").is_none());
        assert!(sanitize_model_id(".hidden-model").is_none());
        assert!(sanitize_model_id("model..double-dot").is_none());
        assert!(sanitize_model_id("bot-alpha_1").is_some());
    }

    #[test]
    fn execute_next_match_reports_result() {
        let service = ArenaService {
            inner: Arc::new(ArenaInner {
                store_path: PathBuf::from("/tmp/test_arena_store_unused.json"),
                redis_store: None,
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
                worker_total_duration_ms: AtomicU64::new(0),
                worker_total_ticks: AtomicU64::new(0),
                worker_warning_matches: AtomicU64::new(0),
                worker_runtime_fallback_matches: AtomicU64::new(0),
                worker_trap_warnings: AtomicU64::new(0),
                worker_timeout_warnings: AtomicU64::new(0),
                worker_draw_matches: AtomicU64::new(0),
                worker_max_duration_ms: AtomicU64::new(0),
                worker_min_duration_ms: AtomicU64::new(0),
                worker_model_win_distribution: RwLock::new(HashMap::new()),
                replay_sequence: AtomicU64::new(0),
                replay_history_capacity: DEFAULT_ARENA_REPLAY_HISTORY_CAP,
                recent_replays: Mutex::new(VecDeque::new()),
                replay_event_sequence: AtomicU64::new(0),
                replay_event_history_capacity: DEFAULT_ARENA_REPLAY_EVENT_HISTORY_CAP,
                replay_match_history_capacity: DEFAULT_ARENA_REPLAY_MATCH_HISTORY_CAP,
                replay_events: Mutex::new(VecDeque::new()),
                replay_match_order: Mutex::new(VecDeque::new()),
                replay_matches: RwLock::new(HashMap::new()),
                replay_event_tx: broadcast::channel(DEFAULT_ARENA_REPLAY_STREAM_CHANNEL_CAP).0,
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

        let replays = service.recent_replays(5);
        assert_eq!(replays.total_replays, 1);
        assert_eq!(replays.replays.len(), 1);
        assert_eq!(replays.replays[0].match_id, executed.report.match_id);

        let recent_events = service.recent_replay_events(128, None);
        assert!(recent_events.total_events >= 3);
        assert_eq!(recent_events.returned_events, recent_events.events.len());
        assert!(recent_events.newest_sequence.is_some());

        let match_events = service
            .replay_events_for_match(&executed.report.match_id, 512, None)
            .expect("match replay events should exist");
        assert_eq!(match_events.match_id, executed.report.match_id);
        assert!(match_events.total_events >= 3);
        assert_eq!(match_events.returned_events, match_events.events.len());
        assert!(match_events
            .events
            .iter()
            .any(|event| event.event_type == "tick"));
        assert!(match_events
            .events
            .iter()
            .any(|event| { event.event_type == "match_end" || event.event_type == "match_draw" }));

        let from_newest = service.replay_events_for_match(
            &executed.report.match_id,
            16,
            recent_events.newest_sequence,
        );
        assert!(from_newest.expect("query should succeed").events.is_empty());
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
        assert_eq!(stats.executed, 0);
        assert_eq!(stats.avg_match_duration_ms, 0.0);
        assert_eq!(stats.model_win_distribution.len(), 0);
    }

    #[test]
    fn worker_stats_track_extended_metrics() {
        let service = ArenaService {
            inner: Arc::new(ArenaInner {
                store_path: PathBuf::from("/tmp/test_arena_store_unused.json"),
                redis_store: None,
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
                worker_total_duration_ms: AtomicU64::new(0),
                worker_total_ticks: AtomicU64::new(0),
                worker_warning_matches: AtomicU64::new(0),
                worker_runtime_fallback_matches: AtomicU64::new(0),
                worker_trap_warnings: AtomicU64::new(0),
                worker_timeout_warnings: AtomicU64::new(0),
                worker_draw_matches: AtomicU64::new(0),
                worker_max_duration_ms: AtomicU64::new(0),
                worker_min_duration_ms: AtomicU64::new(0),
                worker_model_win_distribution: RwLock::new(HashMap::new()),
                replay_sequence: AtomicU64::new(0),
                replay_history_capacity: DEFAULT_ARENA_REPLAY_HISTORY_CAP,
                recent_replays: Mutex::new(VecDeque::new()),
                replay_event_sequence: AtomicU64::new(0),
                replay_event_history_capacity: DEFAULT_ARENA_REPLAY_EVENT_HISTORY_CAP,
                replay_match_history_capacity: DEFAULT_ARENA_REPLAY_MATCH_HISTORY_CAP,
                replay_events: Mutex::new(VecDeque::new()),
                replay_match_order: Mutex::new(VecDeque::new()),
                replay_matches: RwLock::new(HashMap::new()),
                replay_event_tx: broadcast::channel(DEFAULT_ARENA_REPLAY_STREAM_CHANNEL_CAP).0,
            }),
        };

        service
            .queue_match(QueueMatchBody {
                model_a_id: "a".to_owned(),
                model_b_id: "b".to_owned(),
                mode: Some("tdm".to_owned()),
                metadata: None,
            })
            .expect("queue should succeed");
        let outcome = service
            .worker_execute_next(Some(120), Some(42))
            .expect("worker execute should succeed")
            .expect("match should execute");

        let stats = service.worker_stats();
        assert_eq!(stats.runs, 1);
        assert_eq!(stats.executed, 1);
        assert!(stats.total_match_duration_ms >= outcome.sandbox.duration_ms);
        assert!(stats.avg_ticks_executed > 0.0);
        assert!(stats.max_match_duration_ms >= stats.min_match_duration_ms.unwrap_or(0));
        if let Some(winner) = outcome.sandbox.winner_model_id {
            assert_eq!(
                stats
                    .model_win_distribution
                    .get(&winner)
                    .copied()
                    .unwrap_or(0),
                1
            );
        } else {
            assert_eq!(stats.draw_matches, 1);
        }
    }

    #[test]
    fn upload_wasm_rejects_invalid_base64() {
        let service = ArenaService {
            inner: Arc::new(ArenaInner {
                store_path: PathBuf::from("/tmp/test_arena_store_unused.json"),
                redis_store: None,
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
                worker_total_duration_ms: AtomicU64::new(0),
                worker_total_ticks: AtomicU64::new(0),
                worker_warning_matches: AtomicU64::new(0),
                worker_runtime_fallback_matches: AtomicU64::new(0),
                worker_trap_warnings: AtomicU64::new(0),
                worker_timeout_warnings: AtomicU64::new(0),
                worker_draw_matches: AtomicU64::new(0),
                worker_max_duration_ms: AtomicU64::new(0),
                worker_min_duration_ms: AtomicU64::new(0),
                worker_model_win_distribution: RwLock::new(HashMap::new()),
                replay_sequence: AtomicU64::new(0),
                replay_history_capacity: DEFAULT_ARENA_REPLAY_HISTORY_CAP,
                recent_replays: Mutex::new(VecDeque::new()),
                replay_event_sequence: AtomicU64::new(0),
                replay_event_history_capacity: DEFAULT_ARENA_REPLAY_EVENT_HISTORY_CAP,
                replay_match_history_capacity: DEFAULT_ARENA_REPLAY_MATCH_HISTORY_CAP,
                replay_events: Mutex::new(VecDeque::new()),
                replay_match_order: Mutex::new(VecDeque::new()),
                replay_matches: RwLock::new(HashMap::new()),
                replay_event_tx: broadcast::channel(DEFAULT_ARENA_REPLAY_STREAM_CHANNEL_CAP).0,
            }),
        };

        let result = service.upload_model_wasm(UploadModelWasmBody {
            model_id: "model_x".to_owned(),
            wasm_base64: "!!!not-base64!!!".to_owned(),
            overwrite: Some(true),
        });
        assert!(result.is_err());
    }

    #[test]
    fn simulate_team_battle_runs_10v10() {
        let service = ArenaService {
            inner: Arc::new(ArenaInner {
                store_path: PathBuf::from("/tmp/test_arena_store_unused.json"),
                redis_store: None,
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
                worker_total_duration_ms: AtomicU64::new(0),
                worker_total_ticks: AtomicU64::new(0),
                worker_warning_matches: AtomicU64::new(0),
                worker_runtime_fallback_matches: AtomicU64::new(0),
                worker_trap_warnings: AtomicU64::new(0),
                worker_timeout_warnings: AtomicU64::new(0),
                worker_draw_matches: AtomicU64::new(0),
                worker_max_duration_ms: AtomicU64::new(0),
                worker_min_duration_ms: AtomicU64::new(0),
                worker_model_win_distribution: RwLock::new(HashMap::new()),
                replay_sequence: AtomicU64::new(0),
                replay_history_capacity: DEFAULT_ARENA_REPLAY_HISTORY_CAP,
                recent_replays: Mutex::new(VecDeque::new()),
                replay_event_sequence: AtomicU64::new(0),
                replay_event_history_capacity: DEFAULT_ARENA_REPLAY_EVENT_HISTORY_CAP,
                replay_match_history_capacity: DEFAULT_ARENA_REPLAY_MATCH_HISTORY_CAP,
                replay_events: Mutex::new(VecDeque::new()),
                replay_match_order: Mutex::new(VecDeque::new()),
                replay_matches: RwLock::new(HashMap::new()),
                replay_event_tx: broadcast::channel(DEFAULT_ARENA_REPLAY_STREAM_CHANNEL_CAP).0,
            }),
        };

        let result = service
            .simulate_team_battle(SimulateTeamBattleBody {
                model_a_id: "a".to_owned(),
                model_b_id: "b".to_owned(),
                mode: Some("tdm".to_owned()),
                team_size: Some(10),
                rounds: Some(2),
                max_ticks: Some(120),
                seed: Some(11),
            })
            .expect("team simulation should succeed");
        assert_eq!(result.simulation.team_size, 10);
        assert_eq!(result.simulation.rounds, 2);
        assert_eq!(result.simulation.total_engagements, 20);
        assert_eq!(result.simulation.mode, "tdm");
        assert_eq!(result.simulation.rounds_detail.len(), 2);

        let world = service
            .simulate_world_battle(SimulateWorldBattleBody {
                model_ids: vec!["b".to_owned(), "a".to_owned()],
                squad_size: Some(2),
                rounds: Some(2),
                max_ticks: Some(80),
                seed: Some(11),
            })
            .expect("world simulation should succeed");
        assert_eq!(world.simulation.mode, "world_ffa");
        assert_eq!(world.simulation.entrants, 2);
        assert_eq!(world.simulation.squad_size, 2);
        assert_eq!(world.simulation.rounds_detail.len(), 2);
        assert_eq!(world.simulation.rankings.len(), 2);

        let duplicate = service.simulate_world_battle(SimulateWorldBattleBody {
            model_ids: vec!["a".to_owned(), "a".to_owned()],
            squad_size: Some(1),
            rounds: Some(1),
            max_ticks: Some(20),
            seed: Some(11),
        });
        assert!(matches!(
            duplicate,
            Err(ArenaError::InvalidInput("duplicate_world_model", _))
        ));

        let oversized_squad = service.simulate_world_battle(SimulateWorldBattleBody {
            model_ids: vec!["a".to_owned(), "b".to_owned()],
            squad_size: Some(MAX_WORLD_SQUAD_SIZE + 1),
            rounds: Some(1),
            max_ticks: Some(20),
            seed: Some(11),
        });
        assert!(matches!(
            oversized_squad,
            Err(ArenaError::InvalidInput("invalid_world_squad_size", _))
        ));

        let oversized_roster = service.simulate_world_battle(SimulateWorldBattleBody {
            model_ids: (0..=MAX_WORLD_BATTLE_ENTRANTS)
                .map(|index| format!("model_{index}"))
                .collect(),
            squad_size: Some(1),
            rounds: Some(1),
            max_ticks: Some(20),
            seed: Some(11),
        });
        assert!(matches!(
            oversized_roster,
            Err(ArenaError::InvalidInput("invalid_world_entrants", _))
        ));
    }

    fn test_mixed_service(model_ids: &[&str]) -> ArenaService {
        let record = |model_id: &str| ArenaModelRecord {
            model_id: model_id.to_owned(),
            model_name: model_id.to_owned(),
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
        };
        ArenaService {
            inner: Arc::new(ArenaInner {
                store_path: PathBuf::from("/tmp/test_arena_store_unused.json"),
                redis_store: None,
                bot_sandbox: BotSandbox::new_from_env(),
                wasm_dir: PathBuf::from("/tmp/test_arena_wasm"),
                wasm_max_bytes: DEFAULT_ARENA_WASM_MAX_BYTES,
                persistent_store: RwLock::new(PersistentArenaStore {
                    models: model_ids
                        .iter()
                        .map(|model_id| ((*model_id).to_owned(), record(model_id)))
                        .collect(),
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
                worker_total_duration_ms: AtomicU64::new(0),
                worker_total_ticks: AtomicU64::new(0),
                worker_warning_matches: AtomicU64::new(0),
                worker_runtime_fallback_matches: AtomicU64::new(0),
                worker_trap_warnings: AtomicU64::new(0),
                worker_timeout_warnings: AtomicU64::new(0),
                worker_draw_matches: AtomicU64::new(0),
                worker_max_duration_ms: AtomicU64::new(0),
                worker_min_duration_ms: AtomicU64::new(0),
                worker_model_win_distribution: RwLock::new(HashMap::new()),
                replay_sequence: AtomicU64::new(0),
                replay_history_capacity: DEFAULT_ARENA_REPLAY_HISTORY_CAP,
                recent_replays: Mutex::new(VecDeque::new()),
                replay_event_sequence: AtomicU64::new(0),
                replay_event_history_capacity: DEFAULT_ARENA_REPLAY_EVENT_HISTORY_CAP,
                replay_match_history_capacity: DEFAULT_ARENA_REPLAY_MATCH_HISTORY_CAP,
                replay_events: Mutex::new(VecDeque::new()),
                replay_match_order: Mutex::new(VecDeque::new()),
                replay_matches: RwLock::new(HashMap::new()),
                replay_event_tx: broadcast::channel(DEFAULT_ARENA_REPLAY_STREAM_CHANNEL_CAP).0,
            }),
        }
    }

    #[test]
    fn simulate_mixed_team_battle_validates_and_attributes() {
        let service = test_mixed_service(&["a", "b", "c", "d"]);
        let body = |team_a: Vec<&str>, team_b: Vec<&str>| SimulateMixedTeamBattleBody {
            team_a_models: team_a.iter().map(|id| (*id).to_owned()).collect(),
            team_b_models: team_b.iter().map(|id| (*id).to_owned()).collect(),
            mode: Some("tdm".to_owned()),
            rounds: Some(1),
            max_ticks: Some(60),
            seed: Some(7),
        };

        let result = service
            .simulate_mixed_team_battle(body(vec!["a", "b"], vec!["c", "d"]))
            .expect("mixed simulation should succeed");
        let simulation = result.simulation;
        assert_eq!(simulation.mode, "mixed_team");
        assert_eq!(simulation.match_mode, "tdm");
        assert_eq!(simulation.team_size, 2);
        assert_eq!(simulation.team_a_models, vec!["a".to_owned(), "b".to_owned()]);
        assert_eq!(simulation.team_b_models, vec!["c".to_owned(), "d".to_owned()]);
        assert_eq!(simulation.fighters.len(), 4);
        assert!(
            simulation
                .fighters
                .iter()
                .all(|fighter| ["a", "b", "c", "d"].contains(&fighter.model_id.as_str()))
        );
        assert_eq!(simulation.draw, simulation.winner_side.is_none());

        let duplicate = service.simulate_mixed_team_battle(body(vec!["a", "a"], vec!["c", "d"]));
        assert!(matches!(
            duplicate,
            Err(ArenaError::InvalidInput("duplicate_mixed_model", _))
        ));

        let unknown = service.simulate_mixed_team_battle(body(vec!["a", "b"], vec!["c", "zzz"]));
        assert!(matches!(unknown, Err(ArenaError::NotFound("model_not_found", _))));

        let unequal = service.simulate_mixed_team_battle(body(vec!["a", "b"], vec!["c"]));
        assert!(matches!(
            unequal,
            Err(ArenaError::InvalidInput("invalid_mixed_squad_size", _))
        ));

        let empty = service.simulate_mixed_team_battle(body(vec![], vec!["c", "d"]));
        assert!(matches!(
            empty,
            Err(ArenaError::InvalidInput("invalid_mixed_squad_size", _))
        ));
    }

    #[test]
    fn load_persistent_store_falls_back_to_file_when_redis_is_invalid() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "mgs-arena-store-fallback-{}-{}.json",
            std::process::id(),
            nanos
        ));
        let store = PersistentArenaStore {
            models: HashMap::from([(
                "model-a".to_owned(),
                ArenaModelRecord {
                    model_id: "model-a".to_owned(),
                    model_name: "Arena Alpha".to_owned(),
                    provider: "test".to_owned(),
                    version: "1".to_owned(),
                    active: true,
                    created_at: 1,
                    updated_at: 1,
                    last_seen_at: 1,
                    elo_rating: 1000.0,
                    matches_played: 2,
                    wins: 1,
                    losses: 1,
                    draws: 0,
                    cumulative_score: 4,
                },
            )]),
            completed_matches: Vec::new(),
        };

        persistence::persist_store(&path, &store, None).expect("file persistence should succeed");

        let loaded = persistence::load_persistent_store(
            &path,
            persistence::init_redis_store(
                Some("redis://".to_owned()),
                "mgs:test:arena:store".to_owned(),
            )
            .as_ref(),
        );

        assert_eq!(loaded.models.len(), 1);
        assert!(loaded.models.contains_key("model-a"));

        let _ = std::fs::remove_file(path);
    }
}
