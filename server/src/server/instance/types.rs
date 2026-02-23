use super::constants::InitialSnapshotCaps;
use super::*;

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
    pub flag_states: HashMap<u8, ServerFlagState>, // team_id of flag -> state
}

impl Default for ServerMatchInfo {
    fn default() -> Self {
        ServerMatchInfo {
            time_remaining: 300.0, // 5 minutes
            match_state: fb::MatchStateType::Waiting,
            game_mode: fb::GameModeType::CaptureTheFlag, // Changed to CTF mode
            team_scores: HashMap::new(),
            flag_states: HashMap::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
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

pub(super) struct ClientInfo {
    pub(super) data_channel: Arc<crate::core::types::RTCDataChannel>,
    pub(super) needs_initial_state: bool,
}

#[derive(Clone, Debug)]
pub struct BotController {
    pub player_id: PlayerID,
    pub target_position: Option<Vec2>,
    pub target_enemy_id: Option<PlayerID>,
    pub last_decision_time: Instant,
    pub ai_update_accumulator_secs: f32,
    pub behavior_state: BotBehaviorState,
    pub current_path: VecDeque<Vec2>,
    pub path_recalculation_timer: Instant,
    pub last_weapon_switch_time: Instant,
    // Stuck detection fields
    pub last_position: Vec2,
    pub stuck_timer: f32,
    pub stuck_check_position: Vec2,
}

#[derive(Clone, Debug)]
pub struct ServerKillFeedEntry {
    pub killer_name: String,
    pub victim_name: String,
    pub weapon: ServerWeaponType,
    pub timestamp: u64,
}

#[derive(Clone, Debug, Serialize)]
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
    pub weapon_kills: Vec<i32>,
    pub kd_ratio: f32,
}

#[derive(Clone, Debug, Serialize)]
pub struct MatchEndSummary {
    pub generated_at_ms: u64,
    pub reason: String,
    pub map_name: String,
    pub game_mode: String,
    pub match_duration: f32,
    pub winning_team: u8,
    pub players: Vec<PlayerMatchStats>,
    pub mvp_kills: Option<String>,
    pub mvp_damage: Option<String>,
    pub mvp_objectives: Option<String>,
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
    pub(super) last_input_timestamp_ms: u64,
    pub(super) suspicion_score: f32,
    pub(super) last_warned_at: Instant,
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
