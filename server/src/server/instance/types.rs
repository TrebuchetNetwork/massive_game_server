use super::constants::InitialSnapshotCaps;
use super::*;

/// Identifies the type of match being played.  Determines max player count,
/// match duration and bot-fill behaviour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MatchType {
    /// Full-size match (64+ players, 5-min rounds, desktop recommended).
    #[default]
    FullMatch,
    /// Quick match: 32 players, 5-min rounds, auto-fills bots after 15s queue.
    QuickMatch,
    /// Mobile Blitz: 16 players, 3-min rounds, small maps.
    MobileBlitz,
    /// Mobile Standard: 32 players, 5-min rounds, medium maps.
    MobileStandard,
}

impl MatchType {
    /// Parse a match type from a query-parameter string.
    pub fn from_query_str(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "quick" | "quick_match" | "quickmatch" => MatchType::QuickMatch,
            "mobile_blitz" | "mobileblitz" | "blitz" => MatchType::MobileBlitz,
            "mobile_standard" | "mobilestandard" | "mobile" => MatchType::MobileStandard,
            "full" | "full_match" | "fullmatch" | "desktop" => MatchType::FullMatch,
            _ => MatchType::FullMatch,
        }
    }

    /// Maximum players allowed for this match type.
    pub fn max_players(&self) -> usize {
        match self {
            MatchType::FullMatch => 400, // server default, may be overridden by config
            MatchType::QuickMatch => QUICK_MATCH_MAX_PLAYERS,
            MatchType::MobileBlitz => MOBILE_BLITZ_MAX_PLAYERS,
            MatchType::MobileStandard => MOBILE_STANDARD_MAX_PLAYERS,
        }
    }

    /// Round duration in seconds for this match type.
    pub fn duration_secs(&self) -> f32 {
        match self {
            MatchType::FullMatch => FULL_MATCH_DURATION_SECS,
            MatchType::QuickMatch => QUICK_MATCH_DURATION_SECS,
            MatchType::MobileBlitz => MOBILE_BLITZ_DURATION_SECS,
            MatchType::MobileStandard => MOBILE_STANDARD_DURATION_SECS,
        }
    }

    /// Delay in seconds before auto-filling bots (only applicable to QuickMatch).
    pub fn bot_fill_delay_secs(&self) -> Option<f32> {
        match self {
            MatchType::QuickMatch => Some(QUICK_MATCH_BOT_FILL_DELAY_SECS),
            _ => None,
        }
    }

    /// Minimum human player count before bot auto-fill kicks in (QuickMatch only).
    pub fn min_humans_for_bot_fill(&self) -> Option<usize> {
        match self {
            MatchType::QuickMatch => Some(QUICK_MATCH_MIN_HUMANS),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            MatchType::FullMatch => "FullMatch",
            MatchType::QuickMatch => "QuickMatch",
            MatchType::MobileBlitz => "MobileBlitz",
            MatchType::MobileStandard => "MobileStandard",
        }
    }
}

impl std::fmt::Display for MatchType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ServerFlagState {
    pub team_id: u8,                  // Which team this flag BELONGS to
    pub status: fb::FlagStatus,       // fb from flatbuffers_generated
    pub position: Vec2,               // Current position (at base, or where it was dropped)
    pub carrier_id: Option<PlayerID>, // ID of the player carrying this flag
    pub respawn_timer: f32,           // If dropped, time until it auto-returns
}

#[derive(Clone, Debug, PartialEq)]
pub struct ServerMatchInfo {
    pub time_remaining: f32,
    pub match_state: fb::MatchStateType, // fb from flatbuffers_generated
    pub game_mode: fb::GameModeType,     // fb from flatbuffers_generated
    pub team_scores: HashMap<u8, i32>,   // team_id -> score
    pub ctf_overtime_round: u8,
    pub map_event_count: u32,
    pub map_event_elapsed_secs: f32,
    pub map_event_interval_secs: f32,
    pub hot_zone_active: bool,
    pub hot_zone_event_count: u32,
    pub hot_zone_elapsed_secs: f32,
    pub hot_zone_center: Vec2,
    pub hot_zone_radius: f32,
    pub ffa_bounty_player_id: Option<PlayerID>,
    pub ffa_bounty_ping_elapsed_secs: f32,
    pub late_phase_supply_warning_triggered: bool,
    pub late_phase_zone_surge_triggered: bool,
    pub late_phase_final_stand_triggered: bool,
    pub flag_states: HashMap<u8, ServerFlagState>, // team_id of flag -> state
    /// One entry per dynamic-mode phase this match (a fixed-mode match has
    /// exactly one). Lets the end-of-match summary attribute kill tempo to
    /// each game mode instead of the mode the match happened to end in.
    pub mode_phases: Vec<ModePhaseMarker>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModePhaseMarker {
    pub game_mode: fb::GameModeType,
    pub started_at_secs: f32,
    pub kills_at_start: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MatchPhaseSummary {
    pub game_mode: String,
    pub started_at_secs: f32,
    pub duration_secs: f32,
    pub kills: u32,
    pub kills_per_minute: f32,
}

impl Default for ServerMatchInfo {
    fn default() -> Self {
        ServerMatchInfo {
            time_remaining: FULL_MATCH_DURATION_SECS, // overridden per match_type when match starts
            match_state: fb::MatchStateType::Waiting,
            game_mode: fb::GameModeType::CaptureTheFlag, // Changed to CTF mode
            team_scores: HashMap::new(),
            ctf_overtime_round: 0,
            map_event_count: 0,
            map_event_elapsed_secs: 0.0,
            map_event_interval_secs: 75.0,
            hot_zone_active: false,
            hot_zone_event_count: 0,
            hot_zone_elapsed_secs: 0.0,
            hot_zone_center: Vec2::new(0.0, 0.0),
            hot_zone_radius: HOT_ZONE_RADIUS,
            ffa_bounty_player_id: None,
            ffa_bounty_ping_elapsed_secs: 0.0,
            late_phase_supply_warning_triggered: false,
            late_phase_zone_surge_triggered: false,
            late_phase_final_stand_triggered: false,
            flag_states: HashMap::new(),
            mode_phases: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BotBehaviorState {
    Idle,
    MovingToPosition,
    Engaging,
    SeekingPickup,
    Defending,
    MovingToObjective,
    Flanking,
    Patrolling,
}

impl BotBehaviorState {
    pub const fn as_u8(self) -> u8 {
        match self {
            BotBehaviorState::Idle => 0,
            BotBehaviorState::MovingToPosition => 1,
            BotBehaviorState::Engaging => 2,
            BotBehaviorState::SeekingPickup => 3,
            BotBehaviorState::Defending => 4,
            BotBehaviorState::MovingToObjective => 5,
            BotBehaviorState::Flanking => 6,
            BotBehaviorState::Patrolling => 7,
        }
    }
}

pub(super) struct ClientInfo {
    pub(super) data_channel: Arc<crate::core::types::RTCDataChannel>,
    pub(super) needs_initial_state: bool,
}

pub(crate) struct SnapshotState {
    pub(crate) last_broadcast_frame: Arc<AtomicU64>,
    pub(crate) player_last_sync_positions: Arc<DashMap<PlayerID, (f32, f32)>>,
    pub(crate) player_soa_snapshot: Arc<AtomicPlayerSnapshot>,
    pub(crate) player_aoi_snapshot: Arc<AtomicPlayerAoISnapshot>,
    pub(crate) projectile_soa_snapshot: Arc<AtomicProjectileSnapshot>,
    pub(crate) pickup_soa_snapshot: Arc<AtomicPickupSnapshot>,
}

pub(super) struct RuntimeTrackingState {
    pub(super) join_stage_traces: Arc<DashMap<String, JoinStageTrace>>,
    pub(super) join_sequence_counter: Arc<AtomicU64>,
    pub(super) player_position_history: Arc<DashMap<PlayerID, InterpolationBuffer<Vec2>>>,
    pub(super) aim_anomaly_states: Arc<DashMap<PlayerID, AimAnomalyState>>,
    pub(super) auto_tuner: Arc<ParkingLotRwLock<AutoTuner>>,
    pub(super) dynamic_quality_settings: Arc<ParkingLotRwLock<QualitySettings>>,
    pub(super) match_degraded: Arc<AtomicBool>,
}

pub(super) struct NavMeshState {
    pub(super) enabled: bool,
    pub(super) rebuild_interval_frames: u64,
    pub(super) cell_wall_limit: usize,
    pub(super) current: Arc<ArcSwapOption<NavMesh>>,
    pub(super) last_rebuild_frame: Arc<AtomicU64>,
}

pub(super) struct ReplayState {
    pub(super) enabled: bool,
    pub(super) frames: Arc<ParkingLotRwLock<VecDeque<LiveReplayFrame>>>,
    pub(super) capacity: usize,
    pub(super) player_cap: usize,
    pub(super) dispute_persist_enabled: bool,
    pub(super) dispute_store_path: Arc<PathBuf>,
    pub(super) dispute_redis_url: Option<String>,
    pub(super) dispute_redis_key: String,
    pub(super) dispute_signing_key: Option<Arc<Vec<u8>>>,
    pub(super) dispute_chain_head: Arc<ParkingLotRwLock<Option<String>>>,
    pub(super) dispute_audits: Arc<ParkingLotRwLock<VecDeque<LiveReplayDisputeAuditProof>>>,
    pub(super) dispute_audit_capacity: usize,
    pub(super) match_persist_enabled: bool,
    pub(super) match_store_dir: Arc<PathBuf>,
    pub(super) match_redis_url: Option<String>,
    pub(super) match_redis_key: String,
    pub(super) match_retention: usize,
    pub(super) latest_match_end_summary: Arc<ParkingLotRwLock<Option<MatchEndSummary>>>,
    pub(super) recent_killcams: Arc<DashMap<PlayerID, KillCamData>>,
}

pub(super) struct QueueState {
    pub(super) direct_packets: Arc<DashMap<String, VecDeque<Bytes>>>,
    pub(super) direct_packet_queue_cap: usize,
    pub(super) quick_match_queue_start: Arc<ParkingLotRwLock<Option<Instant>>>,
}

#[derive(Clone, Debug)]
pub struct BotController {
    pub player_id: PlayerID,
    /// Verified weekly arena model backing this bot in exhibition mode.
    /// `None` always means the generic built-in controller and must retain a
    /// generic player identity.
    pub arena_model_id: Option<String>,
    pub arena_model_rank: Option<usize>,
    pub arena_slot: i32,
    pub arena_action: Option<crate::operational::bot_sandbox::ExhibitionBotAction>,
    /// Ally currently protected by a SUPPORT action. A support action only
    /// protects this one lowest-health teammate and never the supporter.
    pub arena_support_target_id: Option<PlayerID>,
    /// ATTACK and CHARGE may deal damage once per strategy tick. This latch is
    /// consumed by the first successful hit against the selected target.
    pub arena_damage_pending: bool,
    /// Strategy-runtime tick which produced `arena_action`. It is part of the
    /// deterministic exhibition damage jitter seed.
    pub arena_action_tick: u32,
    pub target_position: Option<Vec2>,
    pub target_enemy_id: Option<PlayerID>,
    pub last_decision_time: Instant,
    /// Tick-based decision timing: the frame_count at which the last decision was made.
    pub last_decision_tick: u64,
    pub ai_update_accumulator_secs: f32,
    pub behavior_state: BotBehaviorState,
    pub current_path: VecDeque<Vec2>,
    pub path_recalculation_timer: Instant,
    pub last_weapon_switch_time: Instant,
    /// Tick-based weapon switch timing: the frame_count of the last weapon switch.
    pub last_weapon_switch_tick: u64,
    // Stuck detection fields
    pub last_position: Vec2,
    pub stuck_timer: f32,
    pub stuck_check_position: Vec2,
    /// Personality profile that influences weapon preferences, engagement ranges, and retreat behavior.
    pub personality: crate::systems::ai::optimized_bot_ai::BotPersonality,
    /// Frame at which the current A* path was last computed.
    pub path_compute_tick: u64,
    /// The target position used when the current path was computed, so we can
    /// detect when the target moves significantly and recompute.
    pub last_path_target: Option<Vec2>,
}

#[derive(Clone, Debug)]
pub struct ServerKillFeedEntry {
    pub killer_name: String,
    pub victim_name: String,
    pub weapon: ServerWeaponType,
    pub is_headshot: bool,
    pub kill_context: KillContext,
    pub timestamp: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayerMatchStats {
    pub player_id: String,
    pub player_name: String,
    pub team_id: u8,
    pub kills: i32,
    pub deaths: i32,
    pub score: i32,
    pub damage_dealt: i32,
    pub damage_taken: i32,
    pub flag_captures: i32,
    pub flag_returns: i32,
    pub hot_zone_kills: i32,
    pub hot_zone_time_seconds: f32,
    pub weapon_kills: Vec<i32>,
    pub kd_ratio: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MatchEndSummary {
    pub generated_at_ms: u64,
    pub reason: String,
    pub map_name: String,
    pub game_mode: String,
    pub match_duration: f32,
    pub winning_team: u8,
    /// Combined kills across all participants, for excitement scoring.
    #[serde(default)]
    pub total_kills: u32,
    /// Kill tempo over the played duration; the primary excitement signal.
    #[serde(default)]
    pub kills_per_minute: f32,
    /// Closeness at the end: winning minus runner-up score (team score in
    /// team modes, top-two player scores otherwise). Small margin = tense.
    #[serde(default)]
    pub final_score_margin: i32,
    /// Per-mode phases of the match with kill tempo attributed to each, so
    /// dynamic-transition matches (FFA -> TDM -> CTF) can be compared mode
    /// by mode instead of by the mode the match ended in.
    #[serde(default)]
    pub phases: Vec<MatchPhaseSummary>,
    /// True when this match ran the co-op gauntlet configuration, so
    /// telemetry can separate gauntlet TDM from regular TDM and the client
    /// can flavor its victory screen.
    #[serde(default)]
    pub coop_gauntlet: bool,
    pub players: Vec<PlayerMatchStats>,
    pub mvp_kills: Option<String>,
    pub mvp_damage: Option<String>,
    pub mvp_objectives: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedMatchReplaySnapshotRecord {
    pub generated_at_ms: u64,
    pub reason: String,
    pub map_name: String,
    pub file_name: String,
    pub frame_count: usize,
    pub compressed_bytes: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct KillCamSample {
    pub x: f32,
    pub y: f32,
    pub rotation: f32,
    pub shooting: bool,
    pub timestamp_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct KillCamData {
    pub victim_id: String,
    pub victim_name: String,
    pub killer_id: String,
    pub killer_name: String,
    pub weapon: String,
    pub generated_at_ms: u64,
    pub samples: Vec<KillCamSample>,
}

#[derive(Debug)]
pub(super) struct ProjectileResults {
    pub(super) total_processed: usize,
    pub(super) hits: Vec<(PlayerID, PlayerID, i32, ServerWeaponType, f32, f32)>, // (attacker, target, damage, weapon, hit_x, hit_y)
    pub(super) wall_hits: Vec<(EntityId, i32)>, // (wall_id, damage)
    pub(super) removed_projectile_ids: Vec<EntityId>,
    pub(super) kept_projectiles: Vec<Projectile>,
    pub(super) spatial_updates: Vec<(EntityId, f32, f32)>,
    pub(super) wall_impacts: Vec<GameEvent>,
}

#[derive(Default)]
pub(super) struct ProjectileChunkResults {
    pub(super) to_remove: Vec<usize>,
    pub(super) hits: Vec<(PlayerID, PlayerID, i32, ServerWeaponType, f32, f32)>,
    pub(super) wall_hits: Vec<(EntityId, i32)>,
    pub(super) spatial_updates: Vec<(EntityId, f32, f32)>,
    pub(super) wall_impacts: Vec<GameEvent>,
}

#[derive(Debug, Clone)]
pub(super) struct AimAnomalyState {
    pub(super) last_rotation: f32,
    // Server-side receive time, not the client-supplied input timestamp —
    // a cheating client can lie about its own timestamp to shrink the
    // computed rotation speed and evade detection; wall-clock Instant
    // captured on receipt can't be spoofed the same way.
    pub(super) last_seen_at_for_anomaly: Instant,
    pub(super) suspicion_score: f32,
    pub(super) last_warned_at: Instant,
}

#[derive(Clone, Debug)]
pub(super) struct ProgressiveWallFragmentState {
    pub(super) stage: u8,
    pub(super) parent_wall: Wall,
    pub(super) child_walls: Vec<Wall>,
}

#[derive(Default, Debug)]
pub(super) struct ProgressiveDestructibleState {
    pub(super) fragmented_walls: HashMap<EntityId, ProgressiveWallFragmentState>,
    pub(super) child_to_parent: HashMap<EntityId, EntityId>,
}

#[derive(Clone, Debug)]
pub(super) struct CommanderWaypoint {
    pub(super) position: Vec2,
    pub(super) expires_at_ms: u64,
}

#[derive(Default, Debug)]
pub(super) struct CommanderRuntimeState {
    pub(super) team_commanders: HashMap<u8, PlayerID>,
    pub(super) team_waypoints: HashMap<u8, VecDeque<CommanderWaypoint>>,
    pub(super) team_attack_bias: HashMap<u8, f32>,
    pub(super) team_supply_drop_ready_ms: HashMap<u8, u64>,
}

#[derive(Debug)]
pub(super) struct PlayerPhysicsResults {
    pub(super) players_to_respawn: Vec<(PlayerID, u8)>, // (player_id, team_id)
    pub(super) alive_count: usize,
}

#[derive(Clone, Debug, Default)]
pub(super) struct JoinStageTrace {
    pub(super) join_sequence: u64,
    pub(super) first_seen_ms: u64,
    pub(super) first_channel_open_ms: Option<u64>,
    pub(super) first_build_start_ms: Option<u64>,
    pub(super) first_build_done_ms: Option<u64>,
    pub(super) first_send_start_ms: Option<u64>,
    pub(super) first_send_result_ms: Option<u64>,
    pub(super) first_send_failure_ms: Option<u64>,
    pub(super) first_send_done_ms: Option<u64>,
    pub(super) build_attempts: u32,
    pub(super) send_attempts: u32,
    pub(super) retry_count: u32,
    pub(super) retry_interval_total_ms: u64,
    pub(super) retry_interval_samples: u32,
    pub(super) last_retry_at_ms: Option<u64>,
    pub(super) completed_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct JoinStageLatencyStats {
    pub count: usize,
    pub avg_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct JoinStageWaveSummary {
    pub label: String,
    pub start_sequence: u64,
    pub end_sequence: Option<u64>,
    pub requested_slots: u64,
    pub tracked_clients: usize,
    pub completed_clients: usize,
    pub open_channel_wait_ms: JoinStageLatencyStats,
    pub queue_wait_ms: JoinStageLatencyStats,
    pub snapshot_build_ms: JoinStageLatencyStats,
    pub send_result_ms: JoinStageLatencyStats,
    pub send_ack_ms: JoinStageLatencyStats,
    pub retry_interval_ms: JoinStageLatencyStats,
    pub retry_count_avg: f64,
    pub build_attempts_avg: f64,
    pub send_attempts_avg: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct JoinStageReport {
    pub generated_at_ms: u64,
    pub total_tracked_clients: usize,
    pub total_completed_clients: usize,
    pub waves: HashMap<String, JoinStageWaveSummary>,
}

#[derive(Clone)]
pub(super) struct SerializedChatPacket {
    pub(super) seq: u64,
    pub(super) bytes: Bytes,
}

#[derive(Clone)]
pub(super) struct SharedBroadcastData {
    pub(super) timestamp_ms: u64,
    pub(super) events: Vec<GameEvent>,
    pub(super) destroyed_wall_ids: Vec<EntityId>,
    pub(super) updated_walls: HashMap<EntityId, Wall>,
    pub(super) active_walls_by_id: HashMap<EntityId, Wall>,
    pub(super) active_walls_snapshot: Vec<Wall>,
    pub(super) player_aois_snapshot: Arc<HashMap<PlayerID, PlayerAoI>>,
    pub(super) player_soa_snapshot: Arc<PlayerSoASnapshot>,
    pub(super) player_states_snapshot: HashMap<PlayerID, PlayerState>,
    pub(super) projectiles_soa_snapshot: Arc<ProjectileSoASnapshot>,
    pub(super) pickups_soa_snapshot: Arc<PickupSoASnapshot>,
    pub(super) projectiles_snapshot: Arc<HashMap<EntityId, Projectile>>,
    pub(super) pickups_snapshot: Arc<HashMap<EntityId, Pickup>>,
    pub(super) chat_packets: Vec<SerializedChatPacket>,
    pub(super) match_info_snapshot: MatchInfoSnapshot,
    pub(super) kill_feed_snapshot: Vec<ServerKillFeedEntry>,
    pub(super) max_delta_events_per_client: usize,
    pub(super) initial_snapshot_caps: InitialSnapshotCaps,
    pub(super) tail_join_mode: bool,
    pub(super) aggressive_tail_join_mode: bool,
    pub(super) extreme_tail_join_mode: bool,
    pub(super) use_aoi_snapshot: bool,
    pub(super) soa_fallback_active: bool,
    pub(super) use_soa_snapshot: bool,
    pub(super) use_entity_soa_snapshot: bool,
}

#[derive(Clone)]
pub(super) struct MatchInfoSnapshot {
    pub(super) time_remaining: f32,
    pub(super) match_state: fb::MatchStateType,
    pub(super) game_mode: fb::GameModeType,
    pub(super) team_scores: HashMap<u8, i32>,
    pub(super) flag_states: HashMap<u8, ServerFlagState>,
    pub(super) team1_commander_id: Option<String>,
    pub(super) team2_commander_id: Option<String>,
    pub(super) team1_commander_waypoint: Option<Vec2>,
    pub(super) team2_commander_waypoint: Option<Vec2>,
    pub(super) team1_commander_attack_bias: f32,
    pub(super) team2_commander_attack_bias: f32,
}

#[derive(Clone, Debug, Serialize)]
pub struct LiveReplayFrame {
    pub frame: u64,
    pub timestamp_ms: u64,
    pub players: usize,
    pub projectiles: usize,
    pub pickups: usize,
    pub events: usize,
    pub sampled_players: Vec<LiveReplayPlayerSample>,
    pub kill_feed_size: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct LiveReplayPlayerSample {
    pub player_id: String,
    pub username: String,
    pub x: f32,
    pub y: f32,
    pub velocity_x: f32,
    pub velocity_y: f32,
    pub health: i32,
    pub alive: bool,
    pub team_id: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct LiveReplayDisputeRequest {
    pub from_frame: Option<u64>,
    pub to_frame: Option<u64>,
    pub limit: Option<usize>,
    pub player_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveReplayDisputeFilter {
    pub from_frame: Option<u64>,
    pub to_frame: Option<u64>,
    pub player_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LiveReplayDisputeAuditProof {
    pub dispute_id: String,
    pub persisted: bool,
    pub storage_path: Option<String>,
    pub payload_sha256: String,
    pub chain_hash_sha256: String,
    pub chain_prev_hash_sha256: Option<String>,
    pub signature_hmac_sha256: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LiveReplayDisputeReport {
    pub generated_at_ms: u64,
    pub total_captured_frames: usize,
    pub selected_frames: Vec<LiveReplayFrame>,
    pub relevant_kill_feed: Vec<LiveReplayKillFeedEntry>,
    pub filter: LiveReplayDisputeFilter,
    pub audit: Option<LiveReplayDisputeAuditProof>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LiveReplayKillFeedEntry {
    pub killer_name: String,
    pub victim_name: String,
    pub weapon: String,
    pub is_headshot: bool,
    pub kill_context: String,
    pub timestamp: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct PersistedLiveReplayDisputeRecord {
    pub(super) dispute_id: String,
    pub(super) generated_at_ms: u64,
    pub(super) total_captured_frames: usize,
    pub(super) selected_frame_count: usize,
    pub(super) selected_from_frame: Option<u64>,
    pub(super) selected_to_frame: Option<u64>,
    pub(super) kill_feed_event_count: usize,
    pub(super) filter: LiveReplayDisputeFilter,
    pub(super) payload_sha256: String,
    pub(super) chain_hash_sha256: String,
    pub(super) chain_prev_hash_sha256: Option<String>,
    pub(super) signature_hmac_sha256: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct QuicJoinSnapshot {
    pub peer_id: String,
    pub username: String,
    pub team_id: u8,
    pub spawn_x: f32,
    pub spawn_y: f32,
    pub created: bool,
}
