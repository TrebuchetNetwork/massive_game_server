// massive_game_server/server/src/server/instance.rs
use crate::concurrent::atomic_snapshot::{
    AtomicPickupSnapshot, AtomicPlayerAoISnapshot, AtomicPlayerSnapshot, AtomicProjectileSnapshot,
    PickupSoASnapshot, PlayerAoISnapshot, PlayerSoASnapshot, ProjectileSoASnapshot,
};
use crate::concurrent::event_queue::PriorityEventQueue;
use crate::concurrent::spatial_index::ImprovedSpatialIndex;
use crate::concurrent::thread_pools::ThreadPoolSystem;
use crate::concurrent::wall_spatial_index::WallSpatialIndex;
use crate::core::config::ServerConfig;
use crate::core::constants::*; // Import all constants, including MIN_PLAYERS_TO_START
use crate::core::error::ServerError;
use crate::core::simd;
use crate::core::types::*;
use crate::core::types::{CorePickupType, EntityId, PlayerID};
use crate::entities::player::ImprovedPlayerManager;
use crate::flatbuffers_generated::game_protocol as fb;
use crate::network::quic::{
    connected_quic_peer_count, connected_quic_peer_ids, send_quic_packet_batch,
};
use crate::network::signaling::ChatMessage;
use crate::network::signaling::PickupState;
use crate::network::signaling::{
    next_chat_message_seq, ChatMessagesQueue, ClientState, ClientStatesMap, DataChannelsMap,
};
use crate::operational::monitoring::metrics;
use crate::operational::tuning::adaptive_quality::QualitySettings;
use crate::operational::tuning::auto_tuner::{AutoTuner, TuningSample};
use crate::server::ecs_bridge::EcsBridge;
use crate::server::event_mapping::{
    event_instigator_id, event_position, event_target_id, event_value, event_weapon_type,
    map_game_event_type_to_fb,
};
use crate::server::pickup_pipeline::{
    apply_pickup_effect, collect_pickup_candidates, PickupCollectionCandidate,
};
use crate::state_sync::interpolation::InterpolationBuffer;
use crate::systems::ai::optimized_bot_ai::OptimizedBotAI;
use crate::systems::respawn::{RespawnManager, WallRespawnManager};
use crate::world::map_generator::MapGenerator;
use crate::world::navigation::NavMesh;
use crate::world::partition::WorldPartitionManager; // Removed unused ImprovedWorldPartition
use std::borrow::Cow;

use bytes::Bytes;
use crossbeam_queue::SegQueue;
use dashmap::DashMap;
use hmac::{Hmac, Mac};
use itoa::Buffer as ItoaBuffer;
use once_cell::sync::OnceCell;
use parking_lot::RwLock as ParkingLotRwLock;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;
// In src/server/instance.rs
use tracing::{debug, error, info, trace, warn}; // Ensure all levels are available

use tokio::{task::JoinSet, time::timeout};

mod bot_management;
mod broadcast_dispatch;
mod broadcast_loop;
mod broadcast_prep;
mod broadcast_state;
mod combat_melee;
mod constants;
mod game_modes;
mod input_runtime;
mod join_stage;
mod match_info;
mod match_summary;
mod navigation_mesh;
mod physics;
mod replay;
mod serialization;
mod snapshot_publish;
mod tick;
mod types;
mod util;

use self::constants::*;
use self::serialization::*;
use self::types::*;
pub use self::types::{
    BotBehaviorState, BotController, JoinStageLatencyStats, JoinStageReport, JoinStageWaveSummary,
    KillCamData, KillCamSample, LiveReplayDisputeAuditProof, LiveReplayDisputeFilter,
    LiveReplayDisputeReport, LiveReplayDisputeRequest, LiveReplayFrame, LiveReplayKillFeedEntry,
    MatchEndSummary, PlayerMatchStats, QuicJoinSnapshot, ServerFlagState, ServerKillFeedEntry,
    ServerMatchInfo,
};
use self::util::*;

#[inline]
fn shortest_angle_diff_radians(a: f32, b: f32) -> f32 {
    let mut diff = (a - b) % (2.0 * std::f32::consts::PI);
    if diff > std::f32::consts::PI {
        diff -= 2.0 * std::f32::consts::PI;
    } else if diff < -std::f32::consts::PI {
        diff += 2.0 * std::f32::consts::PI;
    }
    diff
}

#[inline]
fn segment_first_hit_fraction_with_aabb(
    start_x: f32,
    start_y: f32,
    end_x: f32,
    end_y: f32,
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
) -> Option<f32> {
    let dx = end_x - start_x;
    let dy = end_y - start_y;
    let mut t_min = 0.0f32;
    let mut t_max = 1.0f32;

    if dx.abs() < f32::EPSILON {
        if start_x < min_x || start_x > max_x {
            return None;
        }
    } else {
        let inv_dx = 1.0 / dx;
        let mut t1 = (min_x - start_x) * inv_dx;
        let mut t2 = (max_x - start_x) * inv_dx;
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        t_min = t_min.max(t1);
        t_max = t_max.min(t2);
        if t_min > t_max {
            return None;
        }
    }

    if dy.abs() < f32::EPSILON {
        if start_y < min_y || start_y > max_y {
            return None;
        }
    } else {
        let inv_dy = 1.0 / dy;
        let mut t1 = (min_y - start_y) * inv_dy;
        let mut t2 = (max_y - start_y) * inv_dy;
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        t_min = t_min.max(t1);
        t_max = t_max.min(t2);
        if t_min > t_max {
            return None;
        }
    }

    if t_max < 0.0 || t_min > 1.0 {
        return None;
    }

    Some(t_min.clamp(0.0, 1.0))
}

const PROGRESSIVE_WALL_STAGE1_HEALTH_RATIO: f32 = 0.50;
const PROGRESSIVE_WALL_STAGE2_HEALTH_RATIO: f32 = 0.25;
const PROGRESSIVE_WALL_MIN_FRAGMENT_LENGTH: f32 = 12.0;
const COMMANDER_MAX_WAYPOINTS_PER_TEAM: usize = 3;
const COMMANDER_WAYPOINT_TTL_MS: u64 = 20_000;
const COMMANDER_SUPPLY_DROP_COOLDOWN_MS: u64 = 60_000;
const COMMANDER_SUPPLY_DROP_PICKUPS: usize = 6;

fn env_bool_value(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|raw| {
            let normalized = raw.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "yes" || normalized == "on"
        })
        .unwrap_or(false)
}

fn single_machine_optimization_enabled() -> bool {
    static SINGLE_MACHINE_OPT: OnceCell<bool> = OnceCell::new();
    *SINGLE_MACHINE_OPT.get_or_init(|| {
        env_bool_value("MGS_SINGLE_MACHINE_OPT") || env_bool_value("MGS_SINGLE_MACHINE_MODE")
    })
}

fn join_tail_policy_enabled() -> bool {
    static ENABLED: OnceCell<bool> = OnceCell::new();
    *ENABLED.get_or_init(|| !env_bool_value("MGS_JOIN_DISABLE_TAIL_POLICY"))
}

fn join_packet_batching_enabled() -> bool {
    static ENABLED: OnceCell<bool> = OnceCell::new();
    *ENABLED.get_or_init(|| !env_bool_value("MGS_JOIN_DISABLE_PACKET_BATCHING"))
}

fn join_soa_snapshot_enabled() -> bool {
    static ENABLED: OnceCell<bool> = OnceCell::new();
    *ENABLED.get_or_init(|| !env_bool_value("MGS_JOIN_DISABLE_SOA_SNAPSHOT"))
}

fn join_soa_adaptive_fallback_enabled() -> bool {
    static ENABLED: OnceCell<bool> = OnceCell::new();
    *ENABLED.get_or_init(|| !env_bool_value("MGS_JOIN_DISABLE_SOA_ADAPTIVE_FALLBACK"))
}

fn join_entity_soa_snapshot_enabled() -> bool {
    static ENABLED: OnceCell<bool> = OnceCell::new();
    *ENABLED.get_or_init(|| !env_bool_value("MGS_JOIN_DISABLE_ENTITY_SOA_SNAPSHOT"))
}

fn join_initial_state_chunking_enabled() -> bool {
    static ENABLED: OnceCell<bool> = OnceCell::new();
    *ENABLED.get_or_init(|| !env_bool_value("MGS_JOIN_DISABLE_INITIAL_CHUNKING"))
}

fn join_authoritative_aoi_snapshot_enabled() -> bool {
    static ENABLED: OnceCell<bool> = OnceCell::new();
    *ENABLED.get_or_init(|| {
        !env_bool_value("MGS_JOIN_DISABLE_AUTHORITATIVE_AOI_SNAPSHOT")
            || env_bool_value("MGS_JOIN_ENABLE_AUTHORITATIVE_AOI_SNAPSHOT")
    })
}

fn broadcast_work_stealing_enabled() -> bool {
    static ENABLED: OnceCell<bool> = OnceCell::new();
    *ENABLED.get_or_init(|| {
        env_bool_value("MGS_BROADCAST_WORK_STEALING")
            || env_bool_value("MGS_BROADCAST_RAYON_FANOUT")
    })
}

async fn send_packet_batch_over_channel(
    data_channel: &Arc<crate::core::types::RTCDataChannel>,
    packets: &[Bytes],
    timeout_ms: u64,
) -> usize {
    crate::server::packet_batch::send_packet_batch_over_channel(
        data_channel,
        packets,
        timeout_ms,
        join_packet_batching_enabled(),
    )
    .await
}

fn collect_pending_chat_packets(
    last_seq_sent: u64,
    chat_packets: &[SerializedChatPacket],
) -> Vec<SerializedChatPacket> {
    chat_packets
        .iter()
        .filter(|packet| packet.seq > last_seq_sent)
        .take(MAX_CHAT_PER_BATCH)
        .cloned()
        .collect()
}

#[cfg(test)]
mod packet_batch_tests {
    use super::*;

    #[test]
    fn collect_pending_chat_packets_applies_seq_filter_and_cap() {
        let chat_packets: Vec<SerializedChatPacket> = (1..=64)
            .map(|seq| SerializedChatPacket {
                seq,
                bytes: Bytes::from_static(b"x"),
            })
            .collect();

        let pending = collect_pending_chat_packets(24, &chat_packets);
        assert_eq!(pending.len(), MAX_CHAT_PER_BATCH);
        assert_eq!(pending.first().map(|p| p.seq), Some(25));
        assert_eq!(pending.last().map(|p| p.seq), Some(34));
    }

    #[test]
    fn segment_first_hit_fraction_detects_entry_time() {
        let hit_t = segment_first_hit_fraction_with_aabb(0.0, 0.0, 10.0, 0.0, 4.0, 6.0, -1.0, 1.0);
        assert_eq!(hit_t, Some(0.4));
    }

    #[test]
    fn segment_first_hit_fraction_returns_none_for_miss() {
        let hit_t = segment_first_hit_fraction_with_aabb(0.0, 0.0, 3.0, 0.0, 4.0, 6.0, -1.0, 1.0);
        assert_eq!(hit_t, None);
    }
}

pub struct MassiveGameServer {
    pub config: Arc<ServerConfig>,
    pub thread_pools: Arc<ThreadPoolSystem>,
    pub player_manager: Arc<ImprovedPlayerManager>,
    pub world_partition_manager: Arc<WorldPartitionManager>,
    pub spatial_index: Arc<ImprovedSpatialIndex>,
    pub wall_spatial_index: Arc<WallSpatialIndex>,

    pub projectiles_to_add: Arc<SegQueue<Projectile>>,
    pub global_game_events: Arc<PriorityEventQueue>,
    pub melee_hit_events: Arc<SegQueue<GameEvent>>,

    pub active_connections: Arc<DashMap<String, NetworkConnection>>,

    pub frame_counter: Arc<AtomicU64>,
    pub tick_durations_history: Arc<ParkingLotRwLock<VecDeque<Duration>>>,
    pub projectiles: Arc<ParkingLotRwLock<Vec<Projectile>>>,
    pub pickups: Arc<ParkingLotRwLock<Vec<Pickup>>>,
    pub zones: Arc<Vec<Zone>>,

    pub data_channels_map: DataChannelsMap,
    pub client_states_map: ClientStatesMap,
    pub chat_messages_queue: ChatMessagesQueue,

    pub is_shutting_down: Arc<AtomicBool>,

    pub match_info: Arc<ParkingLotRwLock<ServerMatchInfo>>,
    pub kill_feed: Arc<ParkingLotRwLock<VecDeque<ServerKillFeedEntry>>>,

    pub destroyed_wall_ids_this_tick: Arc<ParkingLotRwLock<HashSet<EntityId>>>,
    pub updated_walls_this_tick: Arc<ParkingLotRwLock<HashMap<EntityId, Wall>>>, // To track respawned/updated walls
    progressive_destructible_enabled: bool,
    progressive_destructible_state: Arc<ParkingLotRwLock<ProgressiveDestructibleState>>,

    pub player_aois: PlayerAoIs,

    pub respawn_manager: Arc<RespawnManager>,
    pub wall_respawn_manager: Arc<WallRespawnManager>,

    pub bot_players: Arc<DashMap<PlayerID, BotController>>,
    pub target_bot_count: Arc<AtomicU64>,
    pub bot_name_counter: Arc<AtomicU64>,
    human_priority_enabled: bool,
    reserved_human_slots: usize,
    spectator_slot_cap: usize,
    pub map_name: String,

    pub last_broadcast_frame: Arc<AtomicU64>,
    pub player_last_sync_positions: Arc<DashMap<PlayerID, (f32, f32)>>,
    pub player_soa_snapshot: Arc<AtomicPlayerSnapshot>,
    pub player_aoi_snapshot: Arc<AtomicPlayerAoISnapshot>,
    pub projectile_soa_snapshot: Arc<AtomicProjectileSnapshot>,
    pub pickup_soa_snapshot: Arc<AtomicPickupSnapshot>,
    join_stage_traces: Arc<DashMap<String, JoinStageTrace>>,
    join_sequence_counter: Arc<AtomicU64>,
    player_position_history: Arc<DashMap<PlayerID, InterpolationBuffer<Vec2>>>,
    aim_anomaly_states: Arc<DashMap<PlayerID, AimAnomalyState>>,
    lag_compensation_ms: u64,
    auto_tuner: Arc<ParkingLotRwLock<AutoTuner>>,
    dynamic_quality_settings: Arc<ParkingLotRwLock<QualitySettings>>,
    ecs_bridge: Arc<EcsBridge>,
    navmesh_enabled: bool,
    navmesh_rebuild_interval_frames: u64,
    navmesh_cell_wall_limit: usize,
    navmesh: Arc<ParkingLotRwLock<Option<NavMesh>>>,
    navmesh_last_rebuild_frame: Arc<AtomicU64>,
    live_replay_enabled: bool,
    live_replay_frames: Arc<ParkingLotRwLock<VecDeque<LiveReplayFrame>>>,
    live_replay_capacity: usize,
    live_replay_player_cap: usize,
    live_replay_dispute_persist_enabled: bool,
    live_replay_dispute_store_path: Arc<PathBuf>,
    live_replay_dispute_signing_key: Option<Arc<Vec<u8>>>,
    live_replay_dispute_chain_head: Arc<ParkingLotRwLock<Option<String>>>,
    live_replay_dispute_audits: Arc<ParkingLotRwLock<VecDeque<LiveReplayDisputeAuditProof>>>,
    live_replay_dispute_audit_capacity: usize,
    live_replay_match_persist_enabled: bool,
    live_replay_match_store_dir: Arc<PathBuf>,
    live_replay_match_retention: usize,
    latest_match_end_summary: Arc<ParkingLotRwLock<Option<MatchEndSummary>>>,
    recent_killcams: Arc<DashMap<PlayerID, KillCamData>>,
    direct_packets: Arc<DashMap<String, VecDeque<Bytes>>>,
    direct_packet_queue_cap: usize,
    commander_mode_enabled: bool,
    commander_runtime_state: Arc<ParkingLotRwLock<CommanderRuntimeState>>,
}

impl MassiveGameServer {
    pub fn new(
        config: Arc<ServerConfig>,
        thread_pools: Arc<ThreadPoolSystem>,
        data_channels_map: DataChannelsMap,
        client_states_map: ClientStatesMap,
        chat_messages_queue: ChatMessagesQueue,
        player_aois: PlayerAoIs,
    ) -> Self {
        info!("Initializing MassiveGameServer...");

        let spatial_index = Arc::new(ImprovedSpatialIndex::new(
            WORLD_MAX_X - WORLD_MIN_X,
            WORLD_MAX_Y - WORLD_MIN_Y,
            WORLD_MIN_X,
            WORLD_MIN_Y,
            SPATIAL_INDEX_CELL_SIZE,
        ));
        info!("Spatial index initialized.");

        let player_manager = Arc::new(ImprovedPlayerManager::new(
            config.num_player_shards,
            spatial_index.clone(),
        ));
        info!(
            "Player manager initialized with {} shards.",
            config.num_player_shards
        );

        let force_10v10_map = std::env::var("MGS_FORCE_10V10_MAP")
            .ok()
            .map(|raw| {
                let normalized = raw.trim().to_ascii_lowercase();
                normalized == "1"
                    || normalized == "true"
                    || normalized == "yes"
                    || normalized == "on"
            })
            .unwrap_or(false);
        let map_target_players = std::env::var("MGS_MAP_TARGET_PLAYERS")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(config.max_players_per_match.max(20));
        let map_seed = std::env::var("MGS_MAP_SEED")
            .ok()
            .and_then(|raw| raw.parse::<u64>().ok())
            .unwrap_or_else(|| {
                if force_10v10_map {
                    10_010
                } else {
                    100_000u64.wrapping_add(map_target_players.max(10) as u64)
                }
            });

        let (all_map_walls, map_name) = if force_10v10_map {
            (
                MapGenerator::generate_10v10_map_with_seed(map_seed),
                "Massive Arena 10v10".to_string(),
            )
        } else {
            MapGenerator::generate_dynamic_map_with_seed(map_target_players, map_seed)
        };
        info!(
            "Generated {} walls for map '{}' (target players: {}, force_10v10: {}, seed: {}).",
            all_map_walls.len(),
            map_name,
            map_target_players,
            force_10v10_map,
            map_seed
        );
        let zones = MapGenerator::generate_environment_zones_with_seed(map_seed);
        info!("Generated {} environmental zones.", zones.len());

        let world_partition_manager = Arc::new(WorldPartitionManager::new(
            config.world_partition_grid_dim,
            WORLD_MAX_X - WORLD_MIN_X,
            WORLD_MAX_Y - WORLD_MIN_Y,
            WORLD_MIN_X,
            WORLD_MIN_Y,
            1024,
        ));
        info!(
            "World partition manager initialized with {}x{} grid.",
            config.world_partition_grid_dim, config.world_partition_grid_dim
        );

        for wall in &all_map_walls {
            let wall_center_x = wall.x + wall.width / 2.0;
            let wall_center_y = wall.y + wall.height / 2.0;
            let partition_idx =
                world_partition_manager.get_partition_index_for_point(wall_center_x, wall_center_y);

            if let Some(partition) = world_partition_manager.get_partition(partition_idx) {
                partition.add_wall_on_load(wall.clone());
            } else {
                error!(
                    "Could not find partition with index {} for wall {}",
                    partition_idx, wall.id
                );
            }
        }
        info!("Distributed walls to partitions.");

        // ---- FORCE CACHE INITIALIZATION HERE ----
        CACHED_WALLS.get_or_init(|| {
            let mut initial_walls_vec = Vec::new();
            // This logic is now directly using the `world_partition_manager` available here
            for partition in world_partition_manager.get_partitions_for_processing() {
                for entry in partition.all_walls_in_partition.iter() {
                    initial_walls_vec.push(entry.value().clone());
                }
            }
            info!("[Wall Cache Initial Population] Populating wall cache in new() with {} structural walls.", initial_walls_vec.len());
            Arc::new(ParkingLotRwLock::new((0, initial_walls_vec))) // Store with frame 0
        });

        let respawn_manager = Arc::new(RespawnManager::new());
        let wall_respawn_manager = Arc::new(WallRespawnManager::new());

        let destructible_walls_vec: Vec<Wall> = all_map_walls
            .iter()
            .filter(|w| w.is_destructible)
            .cloned()
            .collect();
        wall_respawn_manager.register_all_walls(&destructible_walls_vec);
        info!(
            "Registered {} destructible walls with WallRespawnManager.",
            destructible_walls_vec.len()
        );

        let initial_pickups = Self::generate_initial_pickups(
            &all_map_walls,
            map_target_players,
            map_seed ^ 0x9E3779B97F4A7C15,
        );
        info!(
            "Generated {} initial pickups for target player count {}.",
            initial_pickups.len(),
            map_target_players
        );
        Self::sync_pickups_to_partition_index(&world_partition_manager, &initial_pickups);

        // Initialize wall spatial index
        let wall_spatial_index = Arc::new(WallSpatialIndex::new());

        // Build initial wall spatial index from ACTIVE walls only
        let mut active_walls_for_index = Vec::new();
        for partition in world_partition_manager.get_partitions_for_processing() {
            for wall_entry in partition.all_walls_in_partition.iter() {
                let wall = wall_entry.value();
                // Only include non-destructible walls and active destructible walls
                if !wall.is_destructible || (wall.is_destructible && wall.current_health > 0) {
                    active_walls_for_index.push(wall.clone());
                }
            }
        }
        wall_spatial_index.rebuild(&active_walls_for_index, 0);
        info!(
            "Wall spatial index initialized with {} active walls.",
            wall_spatial_index.size()
        );

        let initial_target_bot_count = std::env::var("MGS_TARGET_BOT_COUNT")
            .ok()
            .and_then(|raw| raw.parse::<u64>().ok())
            .unwrap_or(20);
        let human_priority_enabled = std::env::var("MGS_HUMAN_PRIORITY_ENABLED")
            .ok()
            .map(|raw| {
                let normalized = raw.trim().to_ascii_lowercase();
                normalized == "1"
                    || normalized == "true"
                    || normalized == "yes"
                    || normalized == "on"
            })
            .unwrap_or(true);
        let reserved_human_slots = if human_priority_enabled {
            std::env::var("MGS_RESERVED_HUMAN_SLOTS")
                .ok()
                .and_then(|raw| raw.parse::<usize>().ok())
                .unwrap_or(2)
                .min(config.max_players_per_match)
        } else {
            0
        };
        let spectator_slot_cap = std::env::var("MGS_SPECTATOR_CAP")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(20)
            .clamp(0, 256);
        let lag_compensation_ms = std::env::var("MGS_LAG_COMPENSATION_MS")
            .ok()
            .and_then(|raw| raw.parse::<u64>().ok())
            .unwrap_or(DEFAULT_LAG_COMPENSATION_MS)
            .min(250);
        let auto_tuner_target_ms = (1000.0f32 / config.tick_rate.max(1) as f32).max(1.0);
        let live_replay_enabled = env_bool_value("MGS_LIVE_REPLAY_ENABLED");
        let live_replay_capacity = std::env::var("MGS_LIVE_REPLAY_CAPACITY")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(3600)
            .clamp(120, 100_000);
        let live_replay_player_cap = std::env::var("MGS_LIVE_REPLAY_PLAYER_CAP")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(64)
            .clamp(8, 512);
        let live_replay_dispute_persist_enabled = std::env::var("MGS_LIVE_REPLAY_DISPUTE_PERSIST")
            .ok()
            .map(|raw| {
                let normalized = raw.trim().to_ascii_lowercase();
                normalized == "1"
                    || normalized == "true"
                    || normalized == "yes"
                    || normalized == "on"
            })
            .unwrap_or(live_replay_enabled);
        let live_replay_dispute_store_path = std::env::var("MGS_LIVE_REPLAY_DISPUTE_STORE_PATH")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("data/live_replay/disputes.jsonl"));
        let live_replay_dispute_signing_key = std::env::var("MGS_LIVE_REPLAY_DISPUTE_SIGNING_KEY")
            .ok()
            .map(|raw| raw.into_bytes())
            .filter(|bytes| !bytes.is_empty())
            .map(Arc::new);
        let live_replay_dispute_audit_capacity =
            std::env::var("MGS_LIVE_REPLAY_DISPUTE_AUDIT_CAPACITY")
                .ok()
                .and_then(|raw| raw.parse::<usize>().ok())
                .unwrap_or(512)
                .clamp(16, 4096);
        let live_replay_match_persist_enabled = std::env::var("MGS_LIVE_REPLAY_MATCH_PERSIST")
            .ok()
            .map(|raw| {
                let normalized = raw.trim().to_ascii_lowercase();
                normalized == "1"
                    || normalized == "true"
                    || normalized == "yes"
                    || normalized == "on"
            })
            .unwrap_or(live_replay_enabled);
        let live_replay_match_store_dir = std::env::var("MGS_LIVE_REPLAY_MATCH_STORE_DIR")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("data/live_replay/matches"));
        let live_replay_match_retention = std::env::var("MGS_LIVE_REPLAY_MATCH_RETENTION")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(100)
            .clamp(1, 2_000);
        let direct_packet_queue_cap = std::env::var("MGS_DIRECT_PACKET_QUEUE_CAP")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(64)
            .clamp(8, 512);
        let live_replay_dispute_chain_head = if live_replay_dispute_persist_enabled {
            load_dispute_chain_head(live_replay_dispute_store_path.as_path())
        } else {
            None
        };
        let navmesh_enabled = env_bool_value("MGS_NAVMESH_ENABLED");
        let navmesh_rebuild_interval_frames = std::env::var("MGS_NAVMESH_REBUILD_INTERVAL_FRAMES")
            .ok()
            .and_then(|raw| raw.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(180);
        let navmesh_cell_wall_limit = std::env::var("MGS_NAVMESH_CELL_WALL_LIMIT")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(16)
            .clamp(0, 2048);
        let progressive_destructible_enabled =
            !env_bool_value("MGS_DISABLE_PROGRESSIVE_DESTRUCTIBLE");
        let commander_mode_enabled = !env_bool_value("MGS_DISABLE_COMMANDER_MODE");

        info!(
            "Human-priority slots: enabled={}, reserved_human_slots={}",
            human_priority_enabled, reserved_human_slots
        );
        info!(
            "Phase 6 features: progressive_destructible_enabled={}, commander_mode_enabled={}",
            progressive_destructible_enabled, commander_mode_enabled
        );

        let server = MassiveGameServer {
            config,
            thread_pools,
            player_manager,
            world_partition_manager,
            spatial_index,
            wall_spatial_index,
            projectiles_to_add: Arc::new(SegQueue::new()),
            global_game_events: Arc::new(PriorityEventQueue::new()),
            melee_hit_events: Arc::new(SegQueue::new()),
            active_connections: Arc::new(DashMap::new()),
            frame_counter: Arc::new(AtomicU64::new(0)),
            tick_durations_history: Arc::new(ParkingLotRwLock::new(VecDeque::with_capacity(1000))),
            projectiles: Arc::new(ParkingLotRwLock::new(Vec::new())),
            pickups: Arc::new(ParkingLotRwLock::new(initial_pickups)),
            zones: Arc::new(zones),
            data_channels_map,
            client_states_map,
            chat_messages_queue,
            is_shutting_down: Arc::new(AtomicBool::new(false)),
            match_info: Arc::new(ParkingLotRwLock::new(ServerMatchInfo::default())),
            kill_feed: Arc::new(ParkingLotRwLock::new(VecDeque::with_capacity(
                MAX_KILL_FEED_HISTORY + 5,
            ))),
            destroyed_wall_ids_this_tick: Arc::new(ParkingLotRwLock::new(HashSet::new())),
            updated_walls_this_tick: Arc::new(ParkingLotRwLock::new(HashMap::new())),
            progressive_destructible_enabled,
            progressive_destructible_state: Arc::new(ParkingLotRwLock::new(
                ProgressiveDestructibleState::default(),
            )),
            player_aois,
            respawn_manager,
            wall_respawn_manager,
            bot_players: Arc::new(DashMap::new()),
            target_bot_count: Arc::new(AtomicU64::new(initial_target_bot_count)),
            bot_name_counter: Arc::new(AtomicU64::new(0)),
            human_priority_enabled,
            reserved_human_slots,
            spectator_slot_cap,
            map_name,
            last_broadcast_frame: Arc::new(AtomicU64::new(0)),
            player_last_sync_positions: Arc::new(DashMap::new()),
            player_soa_snapshot: Arc::new(AtomicPlayerSnapshot::new()),
            player_aoi_snapshot: Arc::new(AtomicPlayerAoISnapshot::new()),
            projectile_soa_snapshot: Arc::new(AtomicProjectileSnapshot::new()),
            pickup_soa_snapshot: Arc::new(AtomicPickupSnapshot::new()),
            join_stage_traces: Arc::new(DashMap::new()),
            join_sequence_counter: Arc::new(AtomicU64::new(0)),
            player_position_history: Arc::new(DashMap::new()),
            aim_anomaly_states: Arc::new(DashMap::new()),
            lag_compensation_ms,
            auto_tuner: Arc::new(ParkingLotRwLock::new(AutoTuner::new(auto_tuner_target_ms))),
            dynamic_quality_settings: Arc::new(ParkingLotRwLock::new(QualitySettings::default())),
            ecs_bridge: Arc::new(EcsBridge::new_from_env()),
            navmesh_enabled,
            navmesh_rebuild_interval_frames,
            navmesh_cell_wall_limit,
            navmesh: Arc::new(ParkingLotRwLock::new(None)),
            navmesh_last_rebuild_frame: Arc::new(AtomicU64::new(0)),
            live_replay_enabled,
            live_replay_frames: Arc::new(ParkingLotRwLock::new(VecDeque::with_capacity(
                live_replay_capacity,
            ))),
            live_replay_capacity,
            live_replay_player_cap,
            live_replay_dispute_persist_enabled,
            live_replay_dispute_store_path: Arc::new(live_replay_dispute_store_path),
            live_replay_dispute_signing_key,
            live_replay_dispute_chain_head: Arc::new(ParkingLotRwLock::new(
                live_replay_dispute_chain_head,
            )),
            live_replay_dispute_audits: Arc::new(ParkingLotRwLock::new(VecDeque::with_capacity(
                live_replay_dispute_audit_capacity,
            ))),
            live_replay_dispute_audit_capacity,
            live_replay_match_persist_enabled,
            live_replay_match_store_dir: Arc::new(live_replay_match_store_dir),
            live_replay_match_retention,
            latest_match_end_summary: Arc::new(ParkingLotRwLock::new(None)),
            recent_killcams: Arc::new(DashMap::new()),
            direct_packets: Arc::new(DashMap::new()),
            direct_packet_queue_cap,
            commander_mode_enabled,
            commander_runtime_state: Arc::new(ParkingLotRwLock::new(
                CommanderRuntimeState::default(),
            )),
        };

        server.maybe_refresh_navigation_mesh();
        info!("MassiveGameServer initialized successfully.");
        server
    }

    pub fn request_shutdown(&self) {
        self.is_shutting_down.store(true, AtomicOrdering::SeqCst);
    }

    pub fn is_shutdown_requested(&self) -> bool {
        self.is_shutting_down.load(AtomicOrdering::Acquire)
    }

    pub fn current_quality_settings(&self) -> QualitySettings {
        *self.dynamic_quality_settings.read()
    }

    pub fn spectator_count(&self) -> usize {
        let mut count = 0usize;
        self.player_manager.for_each_player(|_, player_state| {
            if player_state.is_spectator {
                count += 1;
            }
        });
        count
    }

    pub fn can_accept_spectator_join(&self) -> bool {
        self.spectator_slot_cap > 0 && self.spectator_count() < self.spectator_slot_cap
    }

    pub fn is_player_spectator(&self, player_id: &PlayerID) -> bool {
        self.player_manager
            .get_player_state(player_id)
            .map(|player_state| player_state.is_spectator)
            .unwrap_or(false)
    }

    pub fn is_peer_spectator(&self, peer_id: &str) -> bool {
        let player_id = self.player_manager.id_pool.get_or_create(peer_id);
        self.is_player_spectator(&player_id)
    }

    pub fn participant_count(&self) -> usize {
        self.player_manager
            .player_count()
            .saturating_sub(self.spectator_count())
    }

    pub(super) fn enqueue_direct_packet_for_peer(&self, peer_id: &str, packet: Bytes) {
        let mut queue = self
            .direct_packets
            .entry(peer_id.to_owned())
            .or_insert_with(VecDeque::new);
        while queue.len() >= self.direct_packet_queue_cap {
            let _ = queue.pop_front();
        }
        queue.push_back(packet);
    }

    pub(super) fn drain_direct_packets_for_peer(
        &self,
        peer_id: &str,
        max_packets: usize,
    ) -> Vec<Bytes> {
        if max_packets == 0 {
            return Vec::new();
        }
        let mut drained = Vec::new();
        if let Some(mut queue_entry) = self.direct_packets.get_mut(peer_id) {
            for _ in 0..max_packets {
                let Some(packet) = queue_entry.pop_front() else {
                    break;
                };
                drained.push(packet);
            }
            if queue_entry.is_empty() {
                drop(queue_entry);
                self.direct_packets.remove(peer_id);
            }
        }
        drained
    }

    pub(super) fn enqueue_direct_packet_for_all_players(&self, packet: Bytes) {
        let mut peers = Vec::new();
        for entry in self.data_channels_map.iter() {
            peers.push(entry.key().clone());
        }
        for peer_id in connected_quic_peer_ids() {
            if !peers.iter().any(|known| known == &peer_id) {
                peers.push(peer_id);
            }
        }
        for peer_id in peers {
            self.enqueue_direct_packet_for_peer(&peer_id, packet.clone());
        }
    }

    pub fn record_tick_metrics(&self, frame_duration: Duration) {
        {
            let mut history = self.tick_durations_history.write();
            history.push_back(frame_duration);
            while history.len() > 1000 {
                let _ = history.pop_front();
            }
        }

        let connected_players = self
            .data_channels_map
            .len()
            .saturating_add(connected_quic_peer_count());
        metrics::record_frame_metrics(frame_duration.as_secs_f64(), connected_players);
        let mut tuner = self.auto_tuner.write();
        let quality = tuner.ingest_sample(TuningSample {
            frame_time_ms: frame_duration.as_secs_f32() * 1000.0,
            connected_players,
        });
        *self.dynamic_quality_settings.write() = quality;
    }

    pub fn register_quic_player(
        &self,
        peer_id: &str,
        username_override: Option<&str>,
        requested_team: Option<u8>,
    ) -> Option<QuicJoinSnapshot> {
        let peer_id = peer_id.trim();
        if peer_id.is_empty() {
            return None;
        }

        self.note_join_channel_open(peer_id);
        self.client_states_map
            .write()
            .entry(peer_id.to_string())
            .or_insert_with(ClientState::default);

        let player_id = self.player_manager.id_pool.get_or_create(peer_id);
        if let Some(player_state) = self.player_manager.get_player_state(&player_id) {
            return Some(QuicJoinSnapshot {
                peer_id: peer_id.to_string(),
                username: player_state.username.clone(),
                team_id: player_state.team_id,
                spawn_x: player_state.x,
                spawn_y: player_state.y,
                created: false,
            });
        }

        let requested_spectator = requested_team == Some(0);
        if requested_spectator && !self.can_accept_spectator_join() {
            warn!(
                "[{}]: spectator join rejected due to spectator slot cap (cap={}).",
                peer_id, self.spectator_slot_cap
            );
            return None;
        }

        let chosen_team = if requested_spectator {
            0
        } else {
            requested_team
                .filter(|team| *team == 1 || *team == 2)
                .unwrap_or_else(|| self.player_manager.assign_team_to_new_player())
        };
        if !requested_spectator
            && !self.ensure_human_join_capacity_for_team(peer_id, Some(chosen_team))
        {
            warn!(
                "[{}]: unable to ensure human join capacity for QUIC player",
                peer_id
            );
        }

        let spawn = if requested_spectator {
            Vec2::new(0.0, 0.0)
        } else {
            self.respawn_manager
                .get_respawn_position(self, &player_id, Some(chosen_team), &[])
        };
        let fallback_username = format!("QPlayer_{}", &peer_id[..peer_id.len().min(6)]);
        let username = username_override
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .unwrap_or(fallback_username);

        let inserted_player_id = self
            .player_manager
            .add_player(peer_id.to_string(), username.clone(), spawn.x, spawn.y)
            .unwrap_or(player_id);
        if let Some(mut player_state) = self
            .player_manager
            .get_player_state_mut(&inserted_player_id)
        {
            player_state.team_id = chosen_team;
            player_state.is_spectator = requested_spectator;
            if requested_spectator {
                player_state.health = player_state.max_health;
                player_state.respawn_timer = None;
            }
            player_state.mark_field_changed(FIELD_SCORE_STATS | FIELD_FLAG);
        }
        self.update_player_aoi(&inserted_player_id, spawn.x, spawn.y);

        Some(QuicJoinSnapshot {
            peer_id: peer_id.to_string(),
            username,
            team_id: chosen_team,
            spawn_x: spawn.x,
            spawn_y: spawn.y,
            created: true,
        })
    }

    pub fn enqueue_quic_input(&self, peer_id: &str, input: PlayerInputData) -> bool {
        let player_id = self.player_manager.id_pool.get_or_create(peer_id);
        if let Some(mut player_state) = self.player_manager.get_player_state_mut(&player_id) {
            player_state.input_queue.push_back(input);
            return true;
        }
        false
    }

    pub fn remove_quic_player(&self, peer_id: &str) {
        self.player_manager.remove_player(peer_id);
        self.client_states_map.write().remove(peer_id);
        self.player_aois.remove(peer_id);
        self.data_channels_map.remove(peer_id);
        self.direct_packets.remove(peer_id);
    }

    pub async fn run_game_logic_update(&self, delta_time: f32) {
        // Drain projectile ingress queue into authoritative projectile state.
        self.drain_queued_projectiles_to_authoritative_state();
        self.update_match_state_authoritative(delta_time);

        // Run pickup collection as read-discovery + authoritative write-apply stages.
        let pickup_collection_candidates = {
            let pickups_guard = self.pickups.read();
            self.collect_pickup_collection_candidates(pickups_guard.as_slice())
        };
        {
            let mut pickups_guard = self.pickups.write();
            self.apply_pickup_collection_authoritative(
                pickups_guard.as_mut_slice(),
                &pickup_collection_candidates,
            );
        }

        self.process_ctf_logic_authoritative(delta_time);

        // Melee Event Processing - Fix 1 (drain dedicated queue only)
        let mut melee_hit_events_to_process = Vec::with_capacity(32);
        let mut processed = 0;
        while processed < MAX_MELEE_EVENTS_PER_TICK {
            if let Some(event_popped) = self.melee_hit_events.pop() {
                melee_hit_events_to_process.push(event_popped);
                processed += 1;
            } else {
                break;
            }
        }
        if processed == MAX_MELEE_EVENTS_PER_TICK {
            warn!(
                "Melee event backlog capped at {} events this tick. Remaining will be processed next tick.",
                MAX_MELEE_EVENTS_PER_TICK
            );
        }

        // Process melee hits (extracted logic)
        self.process_melee_hits(melee_hit_events_to_process);
        // End of Fix 1 for Melee

        self.manage_bot_population();
        // Publish authoritative lock-free read snapshots after all game-logic mutations.
        self.publish_authoritative_lock_free_snapshots();
        // self.destroyed_wall_ids_this_tick.write().clear(); // Moved to process_game_tick
    }

    pub(crate) async fn send_packet_batch_optimized(
        &self,
        data_channel: &Arc<crate::core::types::RTCDataChannel>,
        packets: &[Bytes],
        timeout_ms: u64,
    ) -> usize {
        send_packet_batch_over_channel(data_channel, packets, timeout_ms).await
    }
}
