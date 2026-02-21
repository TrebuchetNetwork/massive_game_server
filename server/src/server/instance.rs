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
use crate::core::types::{CorePickupType, EntityId, MatchState, PlayerID};
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
use crate::operational::tuning::adaptive_quality::QualitySettings;
use crate::operational::monitoring::metrics;
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
use crate::systems::ai::bot_ai::BotAISystem;
use crate::systems::ai::optimized_bot_ai::OptimizedBotAI;
use crate::systems::respawn::{RespawnManager, WallRespawnManager};
use crate::world::map_generator::MapGenerator;
use crate::world::navigation::NavMesh;
use crate::world::partition::WorldPartitionManager; // Removed unused ImprovedWorldPartition
use flatbuffers::FlatBufferBuilder;
use futures::executor;
use parking_lot::RwLockReadGuard;
use std::borrow::Cow;
use tokio::task::JoinError;
use webrtc::data_channel::data_channel_state::RTCDataChannelState;

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
use tokio::time::sleep; // Add this import
use uuid::Uuid;
// In src/server/instance.rs
use tracing::{debug, error, info, trace, warn}; // Ensure all levels are available

use tokio::{task::JoinSet, time::timeout};

mod bot_management;
mod broadcast_prep;
mod constants;
mod game_modes;
mod join_stage;
mod navigation_mesh;
mod replay;
mod serialization;
mod types;
mod util;

use self::constants::*;
use self::serialization::*;
use self::util::*;
pub use self::types::{
    BotBehaviorState, BotController, JoinStageLatencyStats, JoinStageReport, JoinStageWaveSummary,
    LiveReplayDisputeAuditProof, LiveReplayDisputeFilter, LiveReplayDisputeReport,
    LiveReplayDisputeRequest, LiveReplayFrame, LiveReplayKillFeedEntry, QuicJoinSnapshot,
    ServerFlagState, ServerKillFeedEntry, ServerMatchInfo,
};
use self::types::*;


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

    pub data_channels_map: DataChannelsMap,
    pub client_states_map: ClientStatesMap,
    pub chat_messages_queue: ChatMessagesQueue,

    pub is_shutting_down: Arc<AtomicBool>,

    pub match_info: Arc<ParkingLotRwLock<ServerMatchInfo>>,
    pub kill_feed: Arc<ParkingLotRwLock<VecDeque<ServerKillFeedEntry>>>,

    pub destroyed_wall_ids_this_tick: Arc<ParkingLotRwLock<HashSet<EntityId>>>,
    pub updated_walls_this_tick: Arc<ParkingLotRwLock<HashMap<EntityId, Wall>>>, // To track respawned/updated walls

    pub player_aois: PlayerAoIs,

    pub respawn_manager: Arc<RespawnManager>,
    pub wall_respawn_manager: Arc<WallRespawnManager>,

    pub bot_players: Arc<DashMap<PlayerID, BotController>>,
    pub target_bot_count: Arc<AtomicU64>,
    pub bot_name_counter: Arc<AtomicU64>,
    human_priority_enabled: bool,
    reserved_human_slots: usize,
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

        info!(
            "Human-priority slots: enabled={}, reserved_human_slots={}",
            human_priority_enabled, reserved_human_slots
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
            player_aois,
            respawn_manager,
            wall_respawn_manager,
            bot_players: Arc::new(DashMap::new()),
            target_bot_count: Arc::new(AtomicU64::new(initial_target_bot_count)),
            bot_name_counter: Arc::new(AtomicU64::new(0)),
            human_priority_enabled,
            reserved_human_slots,
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

        let chosen_team = requested_team
            .filter(|team| *team == 1 || *team == 2)
            .unwrap_or_else(|| self.player_manager.assign_team_to_new_player());
        if !self.ensure_human_join_capacity_for_team(peer_id, Some(chosen_team)) {
            warn!(
                "[{}]: unable to ensure human join capacity for QUIC player",
                peer_id
            );
        }

        let spawn = self
            .respawn_manager
            .get_respawn_position(self, &player_id, Some(chosen_team), &[]);
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
        if let Some(mut player_state) = self.player_manager.get_player_state_mut(&inserted_player_id) {
            player_state.team_id = chosen_team;
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
    }

    fn prune_runtime_tracking_state(&self) {
        self.player_position_history
            .retain(|player_id, _| self.player_manager.get_player_state(player_id).is_some());
        self.aim_anomaly_states
            .retain(|player_id, _| self.player_manager.get_player_state(player_id).is_some());
    }

    fn record_player_position_sample(
        &self,
        player_id: &PlayerID,
        timestamp_ms: u64,
        x: f32,
        y: f32,
    ) {
        let mut history = self
            .player_position_history
            .entry(player_id.clone())
            .or_insert_with(|| InterpolationBuffer::new(MAX_POSITION_HISTORY_SAMPLES));
        history.push(timestamp_ms, Vec2::new(x, y));
    }

    fn get_rewound_player_position(
        &self,
        player_id: &PlayerID,
        target_timestamp_ms: u64,
    ) -> Option<(f32, f32)> {
        let history = self.player_position_history.get(player_id)?;
        let sample = history.sample_at(target_timestamp_ms)?;
        Some((sample.x, sample.y))
    }

    fn apply_aim_anomaly_detection(
        &self,
        player_id: &PlayerID,
        input: &PlayerInputData,
        player_state: &mut PlayerState,
        now: Instant,
    ) {
        let mut entry = self
            .aim_anomaly_states
            .entry(player_id.clone())
            .or_insert_with(|| AimAnomalyState {
                last_rotation: input.rotation,
                last_input_timestamp_ms: input.timestamp,
                suspicion_score: 0.0,
                last_warned_at: now,
            });

        let dt_ms = input
            .timestamp
            .saturating_sub(entry.last_input_timestamp_ms)
            .max(1);
        let dt_sec = dt_ms as f32 / 1000.0;
        let rotation_delta = shortest_angle_diff_radians(input.rotation, entry.last_rotation).abs();
        let rotation_speed = rotation_delta / dt_sec.max(0.001);

        if input.shooting && rotation_speed > AIMBOT_SUSPICION_ROTATION_RAD_PER_SEC {
            let overshoot = rotation_speed / AIMBOT_SUSPICION_ROTATION_RAD_PER_SEC - 1.0;
            entry.suspicion_score += AIMBOT_SUSPICION_SHOT_WEIGHT + overshoot * 0.2;
        } else {
            entry.suspicion_score =
                (entry.suspicion_score - AIMBOT_SUSPICION_DECAY_PER_SEC * dt_sec).max(0.0);
        }

        if entry.suspicion_score >= AIMBOT_SUSPICION_THRESHOLD
            && now.duration_since(entry.last_warned_at) >= Duration::from_secs(2)
        {
            entry.last_warned_at = now;
            player_state.violation_count = player_state.violation_count.saturating_add(1);
            warn!(
                "[{}]: Aim anomaly detected (rotation_speed={:.2} rad/s, suspicion={:.2}).",
                player_id.as_str(),
                rotation_speed,
                entry.suspicion_score
            );
        }

        entry.last_rotation = input.rotation;
        entry.last_input_timestamp_ms = input.timestamp;
    }

    fn sync_pickups_to_partition_index(
        world_partition_manager: &Arc<WorldPartitionManager>,
        pickups: &[Pickup],
    ) {
        for partition in world_partition_manager.get_partitions_for_processing() {
            partition.dynamic_objects.clear();
        }

        for pickup in pickups {
            let partition_idx =
                world_partition_manager.get_partition_index_for_point(pickup.x, pickup.y);
            if let Some(partition) = world_partition_manager.get_partition(partition_idx) {
                partition.add_dynamic_object(pickup.clone());
            }
        }
    }

    fn upsert_pickup_in_partition_index(&self, pickup: &Pickup) {
        let partition_idx = self
            .world_partition_manager
            .get_partition_index_for_point(pickup.x, pickup.y);
        if let Some(partition) = self.world_partition_manager.get_partition(partition_idx) {
            partition.add_dynamic_object(pickup.clone());
        }
    }

    fn generate_initial_pickups(
        map_walls: &[Wall],
        target_players: usize,
        seed: u64,
    ) -> Vec<Pickup> {
        let mut pickups: Vec<Pickup> = Vec::new();
        let mut rng = StdRng::seed_from_u64(seed);
        let pickup_types = [
            CorePickupType::Health,
            CorePickupType::Ammo,
            CorePickupType::WeaponCrate(ServerWeaponType::Shotgun),
            CorePickupType::WeaponCrate(ServerWeaponType::Rifle),
            CorePickupType::SpeedBoost,
            CorePickupType::DamageBoost,
            CorePickupType::Shield,
            CorePickupType::WeaponCrate(ServerWeaponType::Sniper),
        ];

        let strategic_locations = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(WORLD_MIN_X / 2.0, WORLD_MIN_Y / 2.0),
            Vec2::new(WORLD_MAX_X / 2.0, WORLD_MIN_Y / 2.0),
            Vec2::new(WORLD_MIN_X / 2.0, WORLD_MAX_Y / 2.0),
            Vec2::new(WORLD_MAX_X / 2.0, WORLD_MAX_Y / 2.0),
            Vec2::new(WORLD_MIN_X + 250.0, 0.0),
            Vec2::new(WORLD_MAX_X - 250.0, 0.0),
        ];
        let strategic_anchor_count = strategic_locations.len();
        let mut spawn_anchors = strategic_locations;
        let extra_anchor_count = (target_players / 12).clamp(0, 32);
        for _ in 0..extra_anchor_count {
            spawn_anchors.push(Vec2::new(
                rng.gen_range(WORLD_MIN_X + 120.0..WORLD_MAX_X - 120.0),
                rng.gen_range(WORLD_MIN_Y + 120.0..WORLD_MAX_Y - 120.0),
            ));
        }
        let desired_pickups = (8 + (target_players / 8)).clamp(8, 48);
        const PICKUP_SPACING_MIN: f32 = 70.0;
        const PICKUP_SPACING_MIN_SQ: f32 = PICKUP_SPACING_MIN * PICKUP_SPACING_MIN;

        for i in 0..desired_pickups {
            let base_pos = spawn_anchors[i % spawn_anchors.len()];
            let jitter = if i < strategic_anchor_count {
                50.0
            } else {
                110.0
            };
            let mut placed = false;
            for _attempt in 0..24 {
                let x_offset = rng.gen_range(-jitter..jitter);
                let y_offset = rng.gen_range(-jitter..jitter);
                let x = (base_pos.x + x_offset).clamp(WORLD_MIN_X + 50.0, WORLD_MAX_X - 50.0);
                let y = (base_pos.y + y_offset).clamp(WORLD_MIN_Y + 50.0, WORLD_MAX_Y - 50.0);

                let mut obstructed = false;
                for wall in map_walls {
                    if wall.is_destructible && wall.current_health <= 0 {
                        continue;
                    }
                    if x + PICKUP_COLLECTION_RADIUS > wall.x
                        && x - PICKUP_COLLECTION_RADIUS < wall.x + wall.width
                        && y + PICKUP_COLLECTION_RADIUS > wall.y
                        && y - PICKUP_COLLECTION_RADIUS < wall.y + wall.height
                    {
                        obstructed = true;
                        break;
                    }
                }
                if !obstructed {
                    let too_close_to_existing = pickups.iter().any(|existing| {
                        let dx = existing.x - x;
                        let dy = existing.y - y;
                        (dx * dx + dy * dy) < PICKUP_SPACING_MIN_SQ
                    });
                    if too_close_to_existing {
                        continue;
                    }

                    let pickup_type = pickup_types[i % pickup_types.len()].clone();
                    pickups.push(Pickup::new(generate_entity_id(), x, y, pickup_type));
                    placed = true;
                    break;
                }
            }
            if !placed {
                warn!(
                    "Could not place pickup {} near {:?} after 24 attempts.",
                    i, base_pos
                );
            }
        }
        pickups
    }

    pub fn spawn_initial_bots(&self, count: usize) {
        info!("Spawning {} initial bots...", count);
        // No longer reducing count here - use what's passed in
        let team_spawn_areas = MapGenerator::get_team_spawn_areas();
        let mut rng = rand::thread_rng();

        for i in 0..count {
            let bot_name_num = self.bot_name_counter.fetch_add(1, AtomicOrdering::SeqCst);
            let bot_names = [
                "Alpha", "Beta", "Gamma", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India",
                "Juliet", "Kilo", "Lima", "Mike", "November", "Oscar", "Papa", "Quebec", "Romeo",
                "Sierra", "Tango", "Uniform", "Victor", "Whiskey", "Xray", "Yankee", "Zulu",
            ];
            let bot_name = format!(
                "Bot {}",
                bot_names
                    .get(bot_name_num as usize % bot_names.len())
                    .unwrap_or(&"X")
            );
            let bot_player_id_str = format!("bot_{}", Uuid::new_v4());

            let team_id = (i % 2) + 1;

            let potential_spawns_for_team: Vec<Vec2> = team_spawn_areas
                .iter()
                .filter(|(_, sp_team_id)| *sp_team_id == team_id as u8)
                .map(|(pos, _)| *pos)
                .collect();

            let spawn_pos = if !potential_spawns_for_team.is_empty() {
                // Use team spawn point with some random offset
                let base_spawn =
                    potential_spawns_for_team[rng.gen_range(0..potential_spawns_for_team.len())];
                let offset_radius = 50.0; // Small offset to prevent stacking
                let angle = rng.gen_range(0.0..2.0 * std::f32::consts::PI);
                let offset_x = offset_radius * angle.cos();
                let offset_y = offset_radius * angle.sin();
                Vec2::new(
                    (base_spawn.x + offset_x)
                        .clamp(WORLD_MIN_X + PLAYER_RADIUS, WORLD_MAX_X - PLAYER_RADIUS),
                    (base_spawn.y + offset_y)
                        .clamp(WORLD_MIN_Y + PLAYER_RADIUS, WORLD_MAX_Y - PLAYER_RADIUS),
                )
            } else {
                // Fallback: use respawn manager
                self.respawn_manager.get_respawn_position(
                    self,
                    &Arc::new(bot_player_id_str.clone()),
                    Some(team_id as u8),
                    &[],
                )
            };

            if let Some(player_id_arc) = self.player_manager.add_player(
                bot_player_id_str.clone(),
                bot_name.clone(),
                spawn_pos.x,
                spawn_pos.y,
            ) {
                if let Some(mut p_state) = self.player_manager.get_player_state_mut(&player_id_arc)
                {
                    p_state.team_id = team_id as u8;
                }

                let bot_controller = BotController {
                    player_id: player_id_arc.clone(),
                    target_position: None,
                    target_enemy_id: None,
                    last_decision_time: Instant::now(),
                    behavior_state: BotBehaviorState::Idle,
                    current_path: VecDeque::new(),
                    path_recalculation_timer: Instant::now(),
                    last_position: Vec2::new(spawn_pos.x, spawn_pos.y),
                    stuck_timer: 0.0,
                    stuck_check_position: Vec2::new(spawn_pos.x, spawn_pos.y),
                };
                self.bot_players.insert(player_id_arc, bot_controller);
                debug!(
                    "Spawned bot: {} (ID: {}) on team {} at ({:.1}, {:.1})",
                    bot_name, bot_player_id_str, team_id, spawn_pos.x, spawn_pos.y
                );
            } else {
                error!("Failed to add bot {} to player manager.", bot_name);
            }
        }
    }

    fn apply_input_to_player_state(
        &self,
        player_state: &mut PlayerState,
        input: &PlayerInputData,
        current_server_time: Instant,
    ) {
        if !player_state.alive {
            player_state.velocity_x = 0.0;
            player_state.velocity_y = 0.0;
            return;
        }

        if input.sequence <= player_state.last_processed_input_sequence && input.sequence != 0 {
            // warn!("[{}]: Received out-of-order or duplicate input (seq: {}, last_processed: {}). Ignoring.", player_state.id, input.sequence, player_state.last_processed_input_sequence);
            return;
        }
        player_state.last_processed_input_sequence = input.sequence;
        let player_id_for_anti_cheat = player_state.id.clone();
        self.apply_aim_anomaly_detection(
            &player_id_for_anti_cheat,
            input,
            player_state,
            current_server_time,
        );
        player_state.mark_field_changed(FIELD_POSITION_ROTATION);

        // Calculate movement relative to player rotation
        let mut forward_intent = 0.0_f32;
        let mut strafe_intent = 0.0_f32;

        if input.move_forward {
            forward_intent += 1.0;
        }
        if input.move_backward {
            forward_intent -= 1.0;
        }
        if input.move_left {
            strafe_intent -= 1.0;
        }
        if input.move_right {
            strafe_intent += 1.0;
        }

        let effective_speed = if player_state.speed_boost_remaining > 0.0 {
            PLAYER_BASE_SPEED * MAX_PLAYER_SPEED_MULTIPLIER
        } else {
            PLAYER_BASE_SPEED
        };

        if forward_intent != 0.0 || strafe_intent != 0.0 {
            // Normalize movement vector
            let move_magnitude =
                (forward_intent * forward_intent + strafe_intent * strafe_intent).sqrt();
            forward_intent /= move_magnitude;
            strafe_intent /= move_magnitude;

            // Apply rotation to movement direction
            let cos_rot = player_state.rotation.cos();
            let sin_rot = player_state.rotation.sin();

            // Forward movement in the direction of rotation
            let forward_x = cos_rot * forward_intent;
            let forward_y = sin_rot * forward_intent;

            // Strafe movement perpendicular to rotation (90 degrees)
            let strafe_x = -sin_rot * strafe_intent;
            let strafe_y = cos_rot * strafe_intent;

            // Combine forward and strafe movement
            player_state.velocity_x = (forward_x + strafe_x) * effective_speed;
            player_state.velocity_y = (forward_y + strafe_y) * effective_speed;

            // Debug logging for bot movement
            if player_state.username.starts_with("Bot") {
                trace!("Bot {} velocity set to ({:.1}, {:.1}) from input forward={:.1} strafe={:.1} rot={:.2}",
                    player_state.username, player_state.velocity_x, player_state.velocity_y, forward_intent, strafe_intent, player_state.rotation);
            }
        } else {
            player_state.velocity_x = 0.0;
            player_state.velocity_y = 0.0;
        }
        player_state.mark_field_changed(FIELD_POSITION_ROTATION);

        if (input.rotation - player_state.rotation).abs() > 0.001 {
            player_state.rotation = input.rotation;
            player_state.mark_field_changed(FIELD_POSITION_ROTATION);
        }

        // Shooting logic for firearms
        if input.shooting
            && player_state.weapon != ServerWeaponType::Melee
            && player_state.can_shoot(current_server_time)
        {
            player_state.last_shot_time = Some(current_server_time);
            player_state.ammo -= 1;
            player_state.mark_field_changed(FIELD_WEAPON_AMMO);

            let spawn_offset = PLAYER_RADIUS + 5.0;
            let proj_spawn_x = player_state.x + player_state.rotation.cos() * spawn_offset;
            let proj_spawn_y = player_state.y + player_state.rotation.sin() * spawn_offset;

            let damage_multiplier = if player_state.damage_boost_remaining > 0.0 {
                1.5
            } else {
                1.0
            };

            self.global_game_events.push(
                GameEvent::WeaponFired {
                    player_id: player_state.id.clone(),
                    weapon: player_state.weapon,
                    position: Vec2 {
                        x: proj_spawn_x,
                        y: proj_spawn_y,
                    },
                },
                EventPriority::Normal,
            );

            match player_state.weapon {
                ServerWeaponType::Shotgun => {
                    for _ in 0..SHOTGUN_PELLET_COUNT {
                        // Changed i to _ as i is not used
                        let angle_offset =
                            SHOTGUN_SPREAD_ANGLE_RAD * (2.0 * (rand::random::<f32>()) - 1.0); // Simplified spread
                        let dir_x = player_state.rotation.cos() * angle_offset.cos()
                            - player_state.rotation.sin() * angle_offset.sin();
                        let dir_y = player_state.rotation.sin() * angle_offset.cos()
                            + player_state.rotation.cos() * angle_offset.sin();
                        self.projectiles_to_add.push(Projectile::new(
                            player_state.id.clone(),
                            player_state.weapon,
                            proj_spawn_x,
                            proj_spawn_y,
                            dir_x,
                            dir_y,
                            damage_multiplier,
                        ));
                    }
                }
                // ServerWeaponType::Melee is handled by the separate melee_attack check below
                _ => {
                    // Pistol, Rifle, Sniper
                    self.projectiles_to_add.push(Projectile::new(
                        player_state.id.clone(),
                        player_state.weapon,
                        proj_spawn_x,
                        proj_spawn_y,
                        player_state.rotation.cos(),
                        player_state.rotation.sin(),
                        damage_multiplier,
                    ));
                }
            }
        }

        // Melee Attack Logic (V key)
        if input.melee_attack && player_state.can_shoot(current_server_time) {
            // Using can_shoot for cooldown & alive check
            player_state.last_shot_time = Some(current_server_time); // Apply melee cooldown

            // Position for the melee event (e.g., slightly in front of the player)
            let melee_event_pos_x =
                player_state.x + player_state.rotation.cos() * (PLAYER_RADIUS + 1.0);
            let melee_event_pos_y =
                player_state.y + player_state.rotation.sin() * (PLAYER_RADIUS + 1.0);

            debug!("[{}] initiated Melee Attack (V key).", player_state.id);
            let melee_event = GameEvent::MeleeHit {
                attacker_id: player_state.id.clone(),
                target_id: None, // Target is resolved in game_logic_update's MeleeHit processing
                position: Vec2 {
                    x: melee_event_pos_x,
                    y: melee_event_pos_y,
                },
            };
            // Keep for client broadcast.
            self.global_game_events
                .push(melee_event.clone(), EventPriority::Normal);
            // Process melee hits without draining the global event queue.
            self.melee_hit_events.push(melee_event);
        }

        if input.reload {
            player_state.start_reload(current_server_time);
        }

        if input.change_weapon_slot != 0 {
            let new_weapon = match input.change_weapon_slot {
                1 => Some(ServerWeaponType::Pistol),
                2 => Some(ServerWeaponType::Shotgun),
                3 => Some(ServerWeaponType::Rifle),
                4 => Some(ServerWeaponType::Sniper),
                5 => Some(ServerWeaponType::Melee),
                _ => None,
            };
            if let Some(weapon) = new_weapon {
                if player_state.weapon != weapon {
                    player_state.weapon = weapon;
                    player_state.ammo = PlayerState::get_max_ammo_for_weapon(weapon);
                    player_state.reload_progress = None;
                    player_state.mark_field_changed(FIELD_WEAPON_AMMO);
                }
            }
        }
    }

    pub async fn process_network_input(&self) {
        let network_start = Instant::now();
        let current_server_time = Instant::now();

        // First, collect all player inputs with their IDs
        let mut all_inputs = Vec::new();
        self.player_manager
            .for_each_player_mut(|player_id, player_state| {
                player_state.clear_changed_fields();
                let inputs: Vec<PlayerInputData> = player_state.input_queue.drain(..).collect();
                if !inputs.is_empty() {
                    all_inputs.push((player_id.clone(), inputs));
                }
            });

        // Then process each player's inputs
        for (player_id, inputs) in all_inputs {
            if let Some(mut player_state_entry) =
                self.player_manager.get_player_state_mut(&player_id)
            {
                for input in inputs {
                    self.apply_input_to_player_state(
                        &mut *player_state_entry,
                        &input,
                        current_server_time,
                    );
                }
            }
        }
        metrics::record_subsystem_time("network", network_start.elapsed().as_secs_f64());
    }

    pub async fn run_ai_update(&self) {
        let delta_time = TICK_DURATION.as_secs_f32();
        // Use the optimized bot AI that processes bots in batches
        OptimizedBotAI::update_bots_batch(self, delta_time);
    }

    pub async fn run_physics_update(&self, delta_time: f32) {
        let physics_start_time = Instant::now();
        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);

        // Stage 1: Wall Respawns (example)
        let respawn_stage_start = Instant::now();
        let respawned_walls = if frame % 30 == 0 {
            //
            let templates = self.wall_respawn_manager.as_ref().check_respawns(); //
            if !templates.is_empty() {
                // CHANGED to debug!
                debug!(
                    "[Frame {}]: Respawning {} walls (took {:?})",
                    frame,
                    templates.len(),
                    respawn_stage_start.elapsed()
                );
                self.process_wall_respawns(templates).await //
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Update wall spatial index if walls were respawned, destroyed, or if it needs periodic rebuild
        let destroyed_walls_count = self.destroyed_wall_ids_this_tick.read().len();
        let needs_wall_index_rebuild = !respawned_walls.is_empty()
            || destroyed_walls_count > 0
            || self.wall_spatial_index.needs_rebuild(frame, 150); // Rebuild every 150 frames

        if needs_wall_index_rebuild {
            let index_rebuild_start = Instant::now();
            let active_walls = self.collect_active_walls_optimized();
            self.wall_spatial_index.rebuild(&active_walls, frame);
            debug!(
                "[Frame {}] Wall spatial index rebuilt in {:?} (respawned: {}, destroyed: {})",
                frame,
                index_rebuild_start.elapsed(),
                respawned_walls.len(),
                destroyed_walls_count
            );
        }

        // Stage 2: Collect Active Walls
        let collect_walls_start = Instant::now();
        let active_walls = self.get_active_walls_cached(frame).await; //
                                                                      // CHANGED to debug!
        debug!(
            "Frame {}: Collected {} active walls (took {:?})",
            frame,
            active_walls.len(),
            collect_walls_start.elapsed()
        );

        // Stage 3: Process Player Physics
        let player_physics_start = Instant::now();
        let player_updates = self
            .process_player_physics_parallel(&active_walls, delta_time)
            .await; //
                    // CHANGED to debug!
        debug!(
            "Frame {}: Processed {} player physics updates (took {:?})",
            frame,
            player_updates.players_to_respawn.len() + player_updates.alive_count,
            player_physics_start.elapsed()
        );

        // Stage 4: Apply Player Updates
        let apply_updates_start = Instant::now();
        self.apply_player_updates(player_updates, &active_walls).await; //
                                                         // CHANGED to debug!
        debug!(
            "Frame {}: Applied player updates (took {:?})",
            frame,
            apply_updates_start.elapsed()
        );

        // Stage 5: Process Projectiles
        let projectiles_start = Instant::now();
        let projectile_results = self
            .process_projectiles_optimized(&active_walls, delta_time)
            .await; //
                    // CHANGED to debug!
        debug!(
            "Frame {}: Processed {} projectiles, {} hits, {} removed (took {:?})",
            frame,
            projectile_results.total_processed,
            projectile_results.hits.len(),
            projectile_results.removed_projectile_ids.len(),
            projectiles_start.elapsed()
        );

        // Stage 6: Apply Projectile Results
        let apply_projectiles_start = Instant::now();
        self.apply_projectile_results(projectile_results).await; //
                                                                 // CHANGED to debug!
        debug!(
            "Frame {}: Applied projectile results (took {:?})",
            frame,
            apply_projectiles_start.elapsed()
        );

        // Stage 7: Process Pickups
        let pickups_start = Instant::now();
        self.process_pickup_respawns(delta_time).await; //
                                                        // CHANGED to debug!
        debug!(
            "Frame {}: Processed pickups (took {:?})",
            frame,
            pickups_start.elapsed()
        );

        // This overall timing can remain info if you want a less frequent summary,
        // but if it's per-frame, debug is better.
        // For a true summary, this should be outside this function, logged less often.
        // Let's make it debug for now.
        debug!(
            "Frame {}: TOTAL physics update took {:?}",
            frame,
            physics_start_time.elapsed()
        );
        metrics::record_subsystem_time("physics", physics_start_time.elapsed().as_secs_f64());

        // The specific log "Collected {} walls from {} partitions"
        // in `collect_active_walls_optimized` can also be changed to `debug!`.
        // In src/server/instance.rs, inside `collect_active_walls_optimized`:
        // Change:
        // info!("Collected {} walls from {} partitions", all_walls.len(), partitions.len()); //
        // To:
        // debug!("Collected {} walls from {} partitions", all_walls.len(), partitions.len());
    }

    // Helper methods:
    async fn process_wall_respawns(&self, templates: Vec<Wall>) -> Vec<EntityId> {
        let mut updated_walls_guard = self.updated_walls_this_tick.write();
        let mut respawned_ids = Vec::with_capacity(templates.len());

        for wall_template in templates {
            let partition_idx = self.world_partition_manager.get_partition_index_for_point(
                wall_template.x + wall_template.width / 2.0,
                wall_template.y + wall_template.height / 2.0,
            );

            if let Some(partition) = self.world_partition_manager.get_partition(partition_idx) {
                if partition.respawn_destructible_wall(wall_template.id) {
                    if let Some(respawned_wall_state) = partition.get_wall(wall_template.id) {
                        updated_walls_guard.insert(wall_template.id, respawned_wall_state);
                        respawned_ids.push(wall_template.id);
                    }
                }
            }
        }

        // After respawning walls, update all player AOIs
        if !respawned_ids.is_empty() {
            info!(
                "[Wall Respawn] Updating player AOIs for {} respawned walls",
                respawned_ids.len()
            );
            for mut aoi_entry in self.player_aois.iter_mut() {
                let aoi = aoi_entry.value_mut();
                for wall_id in &respawned_ids {
                    if !aoi.visible_walls.contains(wall_id) {
                        aoi.visible_walls.insert(*wall_id);
                        debug!(
                            "[Wall Respawn] Added respawned wall {} to player's AOI",
                            wall_id
                        );
                    }
                }
            }
        }

        respawned_ids
    }

    async fn get_active_walls_cached(&self, frame: u64) -> Arc<Vec<Wall>> {
        // Cache walls for a few frames since they don't change often
        static WALL_CACHE: OnceCell<Arc<ParkingLotRwLock<(u64, Arc<Vec<Wall>>)>>> = OnceCell::new();
        let cache =
            WALL_CACHE.get_or_init(|| Arc::new(ParkingLotRwLock::new((0, Arc::new(Vec::new())))));

        // Keep a read lock while deciding whether to upgrade, eliminating unlock/relock races.
        let cache_read = cache.upgradable_read();
        if cache_read.0 + 5 > frame {
            return cache_read.1.clone();
        }

        // Rebuild cache after atomically upgrading to write access.
        let mut cache_write = parking_lot::RwLockUpgradableReadGuard::upgrade(cache_read);
        if cache_write.0 + 5 > frame {
            return cache_write.1.clone();
        }

        let walls = Arc::new(self.collect_active_walls_optimized());
        cache_write.0 = frame;
        cache_write.1 = walls.clone();
        walls
    }

    // server/src/server/instance.rs

    // server/src/server/instance.rs
    // server/src/server/instance.rs
    fn collect_active_walls_optimized(&self) -> Vec<Wall> {
        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        // CACHED_WALLS is static and initialized in new()

        let cache_entry_arc = CACHED_WALLS
            .get()
            .expect("Wall cache should have been initialized in MassiveGameServer::new()");

        let structural_walls_from_cache = {
            // Read all structural walls
            let guard = cache_entry_arc.read();
            // Check if cache needs refresh based on frame number.
            // This simple check might need to be more sophisticated if walls change health often
            // outside of just being destroyed/respawned, but for now, let's assume
            // the cache primarily stores the structural layout.
            if guard.0 == frame || (guard.0 != u64::MAX && guard.0 >= frame.saturating_sub(10)) {
                debug!(
                    "[Frame {}] Using cached structural walls (cache frame {}, count {}).",
                    frame,
                    guard.0,
                    guard.1.len()
                );
                guard.1.clone()
            } else {
                // Cache is stale, need to rebuild it
                drop(guard); // Release read lock
                let mut write_guard = cache_entry_arc.write();
                // Double check after acquiring write lock
                if write_guard.0 == frame
                    || (write_guard.0 != u64::MAX && write_guard.0 >= frame.saturating_sub(10))
                {
                    debug!(
                        "[Frame {}] Cache updated by another thread. Using new structural walls.",
                        frame
                    );
                    write_guard.1.clone()
                } else {
                    info!(
                        "[Frame {}] Rebuilding structural wall cache (was for frame {}).",
                        frame, write_guard.0
                    );
                    let mut new_cache_walls = Vec::new();
                    let partitions = self.world_partition_manager.get_partitions_for_processing();
                    for partition in &partitions {
                        for entry in partition.all_walls_in_partition.iter() {
                            new_cache_walls.push(entry.value().clone());
                        }
                    }
                    info!(
                        "[Frame {}] Structural wall cache rebuilt with {} walls.",
                        frame,
                        new_cache_walls.len()
                    );
                    write_guard.0 = frame;
                    write_guard.1 = new_cache_walls.clone();
                    new_cache_walls
                }
            }
        };

        // Now filter these structural walls for "activeness"
        // IMPORTANT: For destructible walls, we need to check their CURRENT health from partitions, not cached health
        let mut active_walls = Vec::new();

        for cached_wall in structural_walls_from_cache {
            if !cached_wall.is_destructible {
                // Non-destructible walls are always active
                active_walls.push(cached_wall);
            } else {
                // For destructible walls, check current health from the partition
                let mut wall_is_active = false;
                let wall_center_x = cached_wall.x + cached_wall.width / 2.0;
                let wall_center_y = cached_wall.y + cached_wall.height / 2.0;
                let partition_idx = self
                    .world_partition_manager
                    .get_partition_index_for_point(wall_center_x, wall_center_y);

                if let Some(partition) = self.world_partition_manager.get_partition(partition_idx) {
                    if let Some(current_wall) = partition.get_wall(cached_wall.id) {
                        if current_wall.current_health > 0 {
                            // Use the current wall state, not the cached one
                            active_walls.push(current_wall);
                            wall_is_active = true;
                        }
                    }
                }

                if !wall_is_active {
                    debug!(
                        "[Frame {}] Filtering out destroyed wall {} (health: 0)",
                        frame, cached_wall.id
                    );
                }
            }
        }

        // This log will show the count of *active* walls
        debug!(
            "[Frame {}] Collected {} active walls.",
            frame,
            active_walls.len()
        );
        active_walls
    }

    fn update_client_state_after_initial(
        &self, // Assuming this is part of MassiveGameServer impl
        peer_id_str: &str,
        shared_data: &SharedBroadcastData,
        last_chat_message_seq_sent: u64,
    ) {
        let frame_num = self.frame_counter.load(AtomicOrdering::Relaxed);
        trace!(
            "[Frame {}] Client {}: Preparing to set initial ClientState in DashMap.",
            frame_num,
            peer_id_str
        );
        let mut client_state = ClientState::default(); // Create new state
        client_state.known_walls_sent = true; // Mark walls as sent
        client_state.last_update_sent_time = Instant::now();
        client_state.last_chat_message_seq_sent = last_chat_message_seq_sent;

        client_state.last_known_match_state = Some(shared_data.match_info_snapshot.match_state);
        client_state.last_known_match_time_remaining =
            Some(shared_data.match_info_snapshot.time_remaining);
        client_state.last_known_team_scores = shared_data.match_info_snapshot.team_scores.clone();
        client_state.match_info_pending = false;

        let snapshot_caps = shared_data.initial_snapshot_caps;
        let self_player_id_arc = self.player_manager.id_pool.get_or_create(peer_id_str);
        if let Some(self_pstate_guard) =
            Self::lookup_player_state_from_shared(shared_data, &self_player_id_arc)
        {
            client_state
                .last_known_player_states
                .insert(self_player_id_arc.clone(), self_pstate_guard.clone());
            client_state
                .last_known_players
                .insert(self_player_id_arc.clone());
        }

        let p_aoi = self.resolve_player_aoi_for_player(shared_data, &self_player_id_arc);
        for visible_player_id in p_aoi
            .visible_players
            .iter()
            .take(snapshot_caps.max_players.saturating_sub(1))
        {
            if let Some(pstate_guard) =
                Self::lookup_player_state_from_shared(shared_data, visible_player_id)
            {
                client_state
                    .last_known_player_states
                    .insert(visible_player_id.clone(), pstate_guard.clone());
            }
            client_state
                .last_known_players
                .insert(visible_player_id.clone());
        }
        client_state.last_known_projectile_ids = p_aoi
            .visible_projectiles
            .iter()
            .take(snapshot_caps.max_projectiles)
            .copied()
            .collect();
        for pickup_id in p_aoi.visible_pickups.iter().take(snapshot_caps.max_pickups) {
            if let Some(pickup) = Self::lookup_pickup_from_shared(shared_data, pickup_id) {
                client_state.last_known_pickup_states.insert(
                    *pickup_id,
                    PickupState {
                        is_active: pickup.is_active,
                    },
                );
            }
        }

        for wall_id in p_aoi.visible_walls.iter().take(AOI_MAX_VISIBLE_WALLS) {
            if let Some(wall_data) = shared_data.active_walls_by_id.get(wall_id) {
                client_state
                    .last_known_wall_states
                    .insert(*wall_id, (wall_data.current_health, wall_data.max_health));
            }
        }
        client_state.last_known_wall_ids = Some(
            p_aoi
                .visible_walls
                .iter()
                .take(AOI_MAX_VISIBLE_WALLS)
                .copied()
                .collect(),
        );

        let key_for_insert = peer_id_str.to_string();
        trace!("[Frame {}] Client {}: ABOUT TO INSERT initial ClientState into client_states_map. Key: {}", frame_num, peer_id_str, key_for_insert);
        self.client_states_map
            .write()
            .insert(key_for_insert.clone(), client_state);
        trace!("[Frame {}] Client {}: SUCCESSFULLY INSERTED initial ClientState into client_states_map. Key: {}", frame_num, peer_id_str, key_for_insert);
    }

    fn update_client_state_after_delta(
        &self,
        client_state: &mut ClientState,
        player_id: &PlayerID,
        shared_data: &SharedBroadcastData,
    ) {
        // Get the player's current AoI
        let player_aoi = self.resolve_player_aoi_for_player(shared_data, player_id);

        // Update last broadcast frame
        client_state.last_broadcast_frame = self.frame_counter.load(AtomicOrdering::Relaxed);

        // CRITICAL FIX 1: Update projectile tracking
        // Clear old projectile IDs and populate with current visible ones
        client_state.last_known_projectile_ids.clear();
        for projectile_id in &player_aoi.visible_projectiles {
            client_state
                .last_known_projectile_ids
                .insert(*projectile_id);
        }

        // CRITICAL FIX 2: Update pickup tracking
        // Clear old pickup states and populate with current visible ones
        client_state.last_known_pickup_states.clear();

        for pickup_id in &player_aoi.visible_pickups {
            if let Some(pickup) = Self::lookup_pickup_from_shared(shared_data, pickup_id) {
                client_state.last_known_pickup_states.insert(
                    *pickup_id,
                    PickupState {
                        is_active: pickup.is_active,
                    },
                );
            }
        }

        // Update visible players tracking (this was likely already working)
        client_state.last_known_players.clear();
        client_state.last_known_players.insert(player_id.clone());
        for visible_player_id in &player_aoi.visible_players {
            client_state
                .last_known_players
                .insert(visible_player_id.clone());
        }

        // Update visible wall tracking and wall state knowledge.
        client_state.last_known_wall_states.clear();
        for wall_id in player_aoi.visible_walls.iter().take(AOI_MAX_VISIBLE_WALLS) {
            if let Some(wall_data) = shared_data.active_walls_by_id.get(wall_id) {
                client_state
                    .last_known_wall_states
                    .insert(*wall_id, (wall_data.current_health, wall_data.max_health));
            }
        }
        client_state.last_known_wall_ids = Some(
            player_aoi
                .visible_walls
                .iter()
                .take(AOI_MAX_VISIBLE_WALLS)
                .copied()
                .collect(),
        );

        trace!(
            "Updated client state for {}: {} projectiles, {} pickups, {} players tracked",
            player_id.as_str(),
            client_state.last_known_projectile_ids.len(),
            client_state.last_known_pickup_states.len(),
            client_state.last_known_players.len()
        );
    }

    fn update_client_state_after_delta_with_shared(
        &self,
        client_state: &mut ClientState,
        shared_data: &SharedBroadcastData,
    ) {
        client_state.last_known_match_state = Some(shared_data.match_info_snapshot.match_state);
        client_state.last_known_match_time_remaining =
            Some(shared_data.match_info_snapshot.time_remaining);
        client_state.last_known_team_scores = shared_data.match_info_snapshot.team_scores.clone();
        client_state.match_info_pending = false;
        client_state.last_kill_feed_count_sent = shared_data.kill_feed_snapshot.len();
        for wall_id in &shared_data.destroyed_wall_ids {
            client_state.known_destroyed_wall_ids.insert(*wall_id);
        }
    }

    // Helper function to get default PlayerAoI
    fn get_empty_player_aoi() -> PlayerAoI {
        PlayerAoI {
            visible_players: HashSet::new(),
            visible_projectiles: HashSet::new(),
            visible_pickups: HashSet::new(),
            visible_walls: HashSet::new(),
            last_update: Instant::now(), // Added this field
        }
    }

    async fn process_player_physics_parallel(
        &self,
        walls: &[Wall],
        delta_time: f32,
    ) -> PlayerPhysicsResults {
        let wall_arc = Arc::new(walls.to_vec());
        let mut all_to_respawn = Vec::new();
        let mut total_alive = 0;
        let sample_timestamp_ms = self.get_server_timestamp_ms();

        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        if frame % 120 == 0 {
            self.prune_runtime_tracking_state();
        }

        // Process all players using for_each_player_mut
        self.player_manager
            .for_each_player_mut(|player_id, player_state| {
                // Update timers
                player_state.update_timers(delta_time);

                if player_state.alive {
                    total_alive += 1;
                    // Process movement with optimized collision
                    self.process_player_movement_optimized(player_state, &wall_arc, delta_time);
                    self.record_player_position_sample(
                        player_id,
                        sample_timestamp_ms,
                        player_state.x,
                        player_state.y,
                    );
                } else if player_state.respawn_timer == Some(0.0) {
                    all_to_respawn.push((player_id.clone(), player_state.team_id));
                }
            });

        PlayerPhysicsResults {
            players_to_respawn: all_to_respawn,
            alive_count: total_alive,
        }
    }

    fn process_player_movement_optimized(
        &self,
        player_state: &mut PlayerState,
        _walls: &[Wall],
        delta_time: f32,
    ) {
        let old_x = player_state.x;
        let old_y = player_state.y;

        // Debug logging for bot movement
        if player_state.username.starts_with("Bot")
            && (player_state.velocity_x != 0.0 || player_state.velocity_y != 0.0)
        {
            trace!(
                "Bot {} physics: pos({:.1},{:.1}) vel({:.1},{:.1}) dt={:.3}",
                player_state.username,
                old_x,
                old_y,
                player_state.velocity_x,
                player_state.velocity_y,
                delta_time
            );
        }

        // Apply velocity
        player_state.x += player_state.velocity_x * delta_time;
        player_state.y += player_state.velocity_y * delta_time;

        // Log position after velocity application
        if player_state.username.starts_with("Bot")
            && (old_x != player_state.x || old_y != player_state.y)
        {
            trace!(
                "Bot {} moved to ({:.1},{:.1})",
                player_state.username,
                player_state.x,
                player_state.y
            );
        }

        // Quick bounds check first
        let half_radius = PLAYER_RADIUS;
        if player_state.x < WORLD_MIN_X + half_radius
            || player_state.x > WORLD_MAX_X - half_radius
            || player_state.y < WORLD_MIN_Y + half_radius
            || player_state.y > WORLD_MAX_Y - half_radius
        {
            player_state.x = player_state
                .x
                .clamp(WORLD_MIN_X + half_radius, WORLD_MAX_X - half_radius);
            player_state.y = player_state
                .y
                .clamp(WORLD_MIN_Y + half_radius, WORLD_MAX_Y - half_radius);
            player_state.velocity_x = 0.0;
            player_state.velocity_y = 0.0;
            player_state.mark_field_changed(FIELD_POSITION_ROTATION);
            return;
        }

        // Use spatial index to query nearby walls
        let check_radius = PLAYER_RADIUS + 10.0; // Reduced from 50.0 since spatial index is efficient
        let nearby_walls =
            self.wall_spatial_index
                .query_radius(player_state.x, player_state.y, check_radius);

        // Check collision with nearby walls only
        for wall in nearby_walls.iter() {
            let closest_x = player_state.x.clamp(wall.x, wall.x + wall.width);
            let closest_y = player_state.y.clamp(wall.y, wall.y + wall.height);

            let dist_sq =
                (player_state.x - closest_x).powi(2) + (player_state.y - closest_y).powi(2);
            if dist_sq < PLAYER_RADIUS.powi(2) {
                // Collision detected - revert position
                player_state.x = old_x;
                player_state.y = old_y;
                player_state.velocity_x = 0.0;
                player_state.velocity_y = 0.0;
                player_state.mark_field_changed(FIELD_POSITION_ROTATION);
                return;
            }
        }

        // Prevent player stacking by rejecting moves that overlap nearby players.
        let min_player_distance = PLAYER_RADIUS * 2.0;
        let min_player_distance_sq = min_player_distance * min_player_distance;
        let nearby_players = self.spatial_index.query_nearby_players_with_positions(
            player_state.x,
            player_state.y,
            min_player_distance + 8.0,
        );
        for (other_player_id, other_x, other_y) in nearby_players {
            if other_player_id == player_state.id {
                continue;
            }
            let dist_sq = (player_state.x - other_x).powi(2) + (player_state.y - other_y).powi(2);
            if dist_sq < min_player_distance_sq {
                player_state.x = old_x;
                player_state.y = old_y;
                player_state.velocity_x = 0.0;
                player_state.velocity_y = 0.0;
                player_state.mark_field_changed(FIELD_POSITION_ROTATION);
                return;
            }
        }

        // Anti-cheat validation
        let max_speed_dist = PLAYER_BASE_SPEED * MAX_PLAYER_SPEED_MULTIPLIER * delta_time;
        // Fixed slack per tick allowed excessive burst distance; scale with expected movement instead.
        let adaptive_slack = (max_speed_dist * 0.35).clamp(1.0, MAX_POSITION_DELTA_SLACK);
        let max_dist = max_speed_dist + adaptive_slack;
        let actual_dist = ((player_state.x - player_state.last_valid_position.0).powi(2)
            + (player_state.y - player_state.last_valid_position.1).powi(2))
        .sqrt();

        if actual_dist > max_dist {
            player_state.violation_count += 1;
            if player_state.violation_count > POSITION_VALIDATION_VIOLATION_THRESHOLD {
                player_state.x = player_state.last_valid_position.0;
                player_state.y = player_state.last_valid_position.1;
                player_state.velocity_x = 0.0;
                player_state.velocity_y = 0.0;
                player_state.mark_field_changed(FIELD_POSITION_ROTATION);
            }
        } else {
            player_state.last_valid_position = (player_state.x, player_state.y);
            player_state.violation_count = player_state.violation_count.saturating_sub(1);
        }

        // Mark as changed if moved
        if (old_x - player_state.x).abs() > 0.01 || (old_y - player_state.y).abs() > 0.01 {
            player_state.mark_field_changed(FIELD_POSITION_ROTATION);
        }
    }

    /*async fn process_projectiles_optimized(&self, _walls: &[Wall], delta_time: f32) -> ProjectileResults {
        let mut projectiles_guard = self.projectiles.write();
        let mut results = ProjectileResults {
            total_processed: projectiles_guard.len(),
            hits: Vec::new(),
            wall_hits: Vec::new(),
            to_remove: Vec::new(),
        };

        let mut destroyed_wall_ids_guard = self.destroyed_wall_ids_this_tick.write();

        // Process projectiles
        for (idx, proj) in projectiles_guard.iter_mut().enumerate() {
            // Update position
            proj.x += proj.velocity_x * delta_time;
            proj.y += proj.velocity_y * delta_time;

            // Quick bounds check
            if proj.x < WORLD_MIN_X || proj.x > WORLD_MAX_X ||
               proj.y < WORLD_MIN_Y || proj.y > WORLD_MAX_Y ||
               proj.should_remove() {
                results.to_remove.push(idx);
                continue;
            }

            // Check wall collisions
            let proj_partition_idx = self.world_partition_manager.get_partition_index_for_point(proj.x, proj.y);
            if let Some(partition) = self.world_partition_manager.get_partition(proj_partition_idx) {
                let mut hit_wall = false;
                for mut wall_entry in partition.all_walls_in_partition.iter_mut() {
                    let wall = wall_entry.value_mut();
                    if wall.is_destructible && wall.current_health <= 0 { continue; }

                    if proj.x >= wall.x && proj.x <= wall.x + wall.width &&
                       proj.y >= wall.y && proj.y <= wall.y + wall.height {

                        if let Some(event) = crate::systems::physics::collision::handle_projectile_wall_collision(
                            proj, wall.id, wall, &self.wall_respawn_manager
                        ) {
                            self.global_game_events.push(event.clone(), EventPriority::Normal);
                            if let GameEvent::WallDestroyed { wall_id: destroyed_id, .. } = event {
                                destroyed_wall_ids_guard.insert(destroyed_id);
                            }
                        }
                        results.to_remove.push(idx);
                        hit_wall = true;
                        break;
                    }
                }

                if !hit_wall {
                    // Check player collisions
                    let nearby_players = self.spatial_index.query_nearby_players(proj.x, proj.y, 100.0);
                    for target_id in nearby_players {
                        if target_id == proj.owner_id { continue; }

                        if let Some(target_state) = self.player_manager.get_player_state(&target_id) {
                            if !target_state.alive { continue; }

                            let dist_sq = (target_state.x - proj.x).powi(2) + (target_state.y - proj.y).powi(2);
                            if dist_sq < PLAYER_RADIUS.powi(2) {
                                results.hits.push((
                                    proj.owner_id.clone(),
                                    target_id.clone(),
                                    proj.damage,
                                    proj.weapon_type
                                ));
                                results.to_remove.push(idx);
                                break;
                            }
                        }
                    }
                }
            }
        }

        // Remove projectiles in reverse order
        results.to_remove.sort_unstable();
        results.to_remove.dedup();
        for &idx in results.to_remove.iter().rev() {
            if idx < projectiles_guard.len() {
                projectiles_guard.swap_remove(idx);
            }
        }

        drop(projectiles_guard);
        drop(destroyed_wall_ids_guard);
        results
    }*/

    async fn apply_player_updates(&self, updates: PlayerPhysicsResults, active_walls: &[Wall]) {
        // Precompute enemy snapshots once for this respawn batch.
        let enemies_for_team_1 = self.get_enemy_positions_for_team(1);
        let enemies_for_team_2 = self.get_enemy_positions_for_team(2);
        let no_enemies: Vec<(Vec2, PlayerID)> = Vec::new();

        // Batch respawns
        for (player_id, team_id) in updates.players_to_respawn {
            let assigned_team = if team_id == 1 || team_id == 2 {
                Some(team_id)
            } else {
                None
            };
            let enemies = match assigned_team {
                Some(1) => enemies_for_team_1.as_slice(),
                Some(2) => enemies_for_team_2.as_slice(),
                _ => no_enemies.as_slice(),
            };
            let spawn_pos = self.respawn_manager.get_respawn_position_with_walls(
                &player_id,
                assigned_team,
                enemies,
                active_walls,
            );

            if let Some(mut p_state) = self.player_manager.get_player_state_mut(&player_id) {
                p_state.respawn(spawn_pos.x, spawn_pos.y);
                self.record_player_position_sample(
                    &player_id,
                    self.get_server_timestamp_ms(),
                    spawn_pos.x,
                    spawn_pos.y,
                );
                self.global_game_events.push(
                    GameEvent::PlayerJoined {
                        player_id: player_id.clone(),
                    },
                    EventPriority::High,
                );
            }
        }
    }

    fn drain_queued_projectiles_to_authoritative_state(&self) {
        let mut queued_projectiles = Vec::new();
        while let Some(proj) = self.projectiles_to_add.pop() {
            queued_projectiles.push(proj);
        }
        if queued_projectiles.is_empty() {
            return;
        }

        let mut projectiles_guard = self.projectiles.write();
        projectiles_guard.extend(queued_projectiles);
    }

    fn take_authoritative_projectiles_for_processing(&self) -> Vec<Projectile> {
        let mut guard = self.projectiles.write();
        std::mem::take(&mut *guard)
    }

    fn commit_authoritative_projectile_state(
        &self,
        kept_projectiles: Vec<Projectile>,
        removed_ids: &[EntityId],
    ) {
        for proj_id in removed_ids {
            self.spatial_index.remove_projectile(proj_id);
        }

        let mut guard = self.projectiles.write();
        *guard = kept_projectiles;
    }

    fn process_pickup_respawns_authoritative(&self, pickups: &mut [Pickup], delta_time: f32) {
        for pickup in pickups.iter_mut() {
            if !pickup.is_active {
                if let Some(timer) = &mut pickup.respawn_timer {
                    *timer -= delta_time;
                    if *timer <= 0.0 {
                        pickup.is_active = true;
                        pickup.respawn_timer = None;
                        self.upsert_pickup_in_partition_index(pickup);
                    }
                }
            }
        }
    }

    fn collect_pickup_collection_candidates(
        &self,
        pickups: &[Pickup],
    ) -> Vec<PickupCollectionCandidate> {
        let mut players = Vec::with_capacity(self.player_manager.player_count());
        self.player_manager
            .for_each_player(|player_id, player_state| {
                if player_state.alive {
                    players.push((player_id.clone(), player_state.x, player_state.y));
                }
            });
        collect_pickup_candidates(&players, pickups)
    }

    fn apply_pickup_collection_authoritative(
        &self,
        pickups: &mut [Pickup],
        pickup_candidates: &[PickupCollectionCandidate],
    ) {
        let pickup_radius_sq = PICKUP_COLLECTION_RADIUS * PICKUP_COLLECTION_RADIUS;
        for pickup_candidate in pickup_candidates {
            let Some(pickup) = pickups.get_mut(pickup_candidate.pickup_index) else {
                continue;
            };
            if !pickup.is_active {
                continue;
            }

            let pickup_x = pickup.x;
            let pickup_y = pickup.y;
            let pickup_id = pickup.id;
            let pickup_type = pickup.pickup_type.clone();

            let mut collected = false;
            if let Some(mut player_state_for_pickup) = self
                .player_manager
                .get_player_state_mut(&pickup_candidate.player_id)
            {
                if !player_state_for_pickup.alive {
                    continue;
                }

                let dx = player_state_for_pickup.x - pickup_x;
                let dy = player_state_for_pickup.y - pickup_y;
                if dx * dx + dy * dy > pickup_radius_sq {
                    continue;
                }

                collected = apply_pickup_effect(&mut player_state_for_pickup, &pickup_type);
            }

            if !collected {
                continue;
            }

            pickup.is_active = false;
            pickup.respawn_timer = Some(pickup.get_respawn_duration());
            let pickup_partition_state = pickup.clone();
            self.upsert_pickup_in_partition_index(&pickup_partition_state);
            self.global_game_events.push(
                GameEvent::PowerupCollected {
                    player_id: pickup_candidate.player_id.clone(),
                    pickup_id,
                    pickup_type,
                    position: Vec2::new(pickup_x, pickup_y),
                },
                EventPriority::Normal,
            );
        }
    }

    // In massive_game_server/server/src/server/instance.rs

    async fn process_projectiles_optimized(
        &self,
        _walls: &[Wall],
        delta_time: f32,
    ) -> ProjectileResults {
        use rayon::prelude::*;
        #[derive(Default)]
        struct PartitionWallAabbCache {
            ids: Vec<EntityId>,
            min_xs: Vec<f32>,
            max_xs: Vec<f32>,
            min_ys: Vec<f32>,
            max_ys: Vec<f32>,
            destructible: Vec<bool>,
        }

        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        trace!("[Frame {}] Starting optimized projectile processing", frame);

        // Take authoritative projectile state for parallel processing.
        let mut all_projectiles = self.take_authoritative_projectiles_for_processing();

        let total_projectiles = all_projectiles.len();
        trace!(
            "[Frame {}] Processing {} projectiles",
            frame,
            total_projectiles
        );

        if total_projectiles == 0 {
            return ProjectileResults {
                total_processed: 0,
                hits: Vec::new(),
                wall_hits: Vec::new(),
                removed_projectile_ids: Vec::new(),
                kept_projectiles: Vec::new(),
                spatial_updates: Vec::new(),
                wall_impacts: Vec::new(),
            };
        }

        // Build per-partition wall AABB caches once per tick and share across rayon workers.
        let partition_wall_caches: Arc<Vec<PartitionWallAabbCache>> = {
            let partitions = self.world_partition_manager.get_partitions_for_processing();
            let mut caches = Vec::with_capacity(partitions.len());
            for partition in partitions {
                let mut cache = PartitionWallAabbCache::default();
                for wall_entry in partition.all_walls_in_partition.iter() {
                    let wall = wall_entry.value();
                    if wall.is_destructible && wall.current_health <= 0 {
                        continue;
                    }
                    cache.ids.push(wall.id);
                    cache.min_xs.push(wall.x);
                    cache.max_xs.push(wall.x + wall.width);
                    cache.min_ys.push(wall.y);
                    cache.max_ys.push(wall.y + wall.height);
                    cache.destructible.push(wall.is_destructible);
                }
                caches.push(cache);
            }
            Arc::new(caches)
        };
        let lag_compensation_target_ms = self
            .get_server_timestamp_ms()
            .saturating_sub(self.lag_compensation_ms);

        // Process projectiles in parallel chunks
        let chunk_size = 50.max(total_projectiles / rayon::current_num_threads());

        let partition_wall_caches_ref = Arc::clone(&partition_wall_caches);
        let chunk_results: Vec<ProjectileChunkResults> = all_projectiles
            .par_chunks_mut(chunk_size)
            .enumerate()
            .map(|(chunk_idx, chunk)| {
                let mut local_results = ProjectileChunkResults::default();
                let chunk_start_idx = chunk_idx * chunk_size;
                let mut target_ids: Vec<PlayerID> = Vec::with_capacity(32);
                let mut target_xs: Vec<f32> = Vec::with_capacity(32);
                let mut target_ys: Vec<f32> = Vec::with_capacity(32);
                let mut candidate_partition_indices: Vec<usize> = Vec::with_capacity(16);

                for (local_idx, proj) in chunk.iter_mut().enumerate() {
                    let global_idx = chunk_start_idx + local_idx;

                    // Update position
                    let old_x = proj.x;
                    let old_y = proj.y;
                    proj.x += proj.velocity_x * delta_time;
                    proj.y += proj.velocity_y * delta_time;

                    local_results
                        .spatial_updates
                        .push((proj.id, proj.x, proj.y));

                    // Check bounds
                    if proj.x < WORLD_MIN_X
                        || proj.x > WORLD_MAX_X
                        || proj.y < WORLD_MIN_Y
                        || proj.y > WORLD_MAX_Y
                    {
                        local_results.to_remove.push(global_idx);
                        continue;
                    }

                    // Check lifetime
                    if proj.should_remove() {
                        local_results.to_remove.push(global_idx);
                        continue;
                    }

                    // Continuous wall collision detection across all partitions touched by
                    // the projectile segment this tick.
                    candidate_partition_indices.clear();
                    self.world_partition_manager
                        .collect_partition_indices_for_bounds(
                            old_x.min(proj.x),
                            old_x.max(proj.x),
                            old_y.min(proj.y),
                            old_y.max(proj.y),
                            &mut candidate_partition_indices,
                        );

                    let mut earliest_wall_hit_t: Option<f32> = None;
                    let mut earliest_wall_id: EntityId = 0;
                    let mut earliest_wall_destructible = false;

                    for partition_idx in &candidate_partition_indices {
                        let Some(wall_cache) = partition_wall_caches_ref.get(*partition_idx) else {
                            continue;
                        };

                        for wall_idx in 0..wall_cache.ids.len() {
                            let Some(hit_t) = segment_first_hit_fraction_with_aabb(
                                old_x,
                                old_y,
                                proj.x,
                                proj.y,
                                wall_cache.min_xs[wall_idx],
                                wall_cache.max_xs[wall_idx],
                                wall_cache.min_ys[wall_idx],
                                wall_cache.max_ys[wall_idx],
                            ) else {
                                continue;
                            };

                            let is_earlier_hit = match earliest_wall_hit_t {
                                Some(existing_t) => hit_t < existing_t,
                                None => true,
                            };
                            if is_earlier_hit {
                                earliest_wall_hit_t = Some(hit_t);
                                earliest_wall_id = wall_cache.ids[wall_idx];
                                earliest_wall_destructible = wall_cache.destructible[wall_idx];
                            }
                        }
                    }

                    if let Some(hit_t) = earliest_wall_hit_t {
                        let hit_x = old_x + (proj.x - old_x) * hit_t;
                        let hit_y = old_y + (proj.y - old_y) * hit_t;
                        proj.x = hit_x;
                        proj.y = hit_y;

                        if earliest_wall_destructible {
                            local_results
                                .wall_hits
                                .push((earliest_wall_id, proj.damage));
                            local_results.wall_impacts.push(GameEvent::WallImpact {
                                position: Vec2::new(hit_x, hit_y),
                                wall_id: earliest_wall_id,
                                damage: proj.damage,
                            });
                        }

                        local_results.to_remove.push(global_idx);
                        continue;
                    }

                    // Check player collisions using spatial index.
                    let nearby_players = self.spatial_index.query_nearby_players_with_positions(
                        proj.x,
                        proj.y,
                        PLAYER_RADIUS + 20.0, // Small buffer for fast projectiles
                    );
                    target_ids.clear();
                    target_xs.clear();
                    target_ys.clear();

                    for (target_id, target_x, target_y) in nearby_players {
                        if target_id == proj.owner_id {
                            continue;
                        }
                        let (validated_target_x, validated_target_y) = self
                            .get_rewound_player_position(&target_id, lag_compensation_target_ms)
                            .unwrap_or((target_x, target_y));
                        target_ids.push(target_id);
                        target_xs.push(validated_target_x);
                        target_ys.push(validated_target_y);
                    }

                    if !target_ids.is_empty() {
                        let radius_sq = PLAYER_RADIUS * PLAYER_RADIUS;
                        if let Some(target_idx) = simd::first_index_within_segment_radius(
                            &target_xs, &target_ys, old_x, old_y, proj.x, proj.y, radius_sq,
                        ) {
                            if let Some(target_id) = target_ids.get(target_idx) {
                                let seg_dx = proj.x - old_x;
                                let seg_dy = proj.y - old_y;
                                let seg_len_sq = seg_dx * seg_dx + seg_dy * seg_dy;
                                let target_x = target_xs[target_idx];
                                let target_y = target_ys[target_idx];
                                let t = if seg_len_sq > f32::EPSILON {
                                    (((target_x - old_x) * seg_dx + (target_y - old_y) * seg_dy)
                                        / seg_len_sq)
                                        .clamp(0.0, 1.0)
                                } else {
                                    0.0
                                };
                                let hit_x = old_x + seg_dx * t;
                                let hit_y = old_y + seg_dy * t;

                                proj.x = hit_x;
                                proj.y = hit_y;
                                local_results.hits.push((
                                    proj.owner_id.clone(),
                                    target_id.clone(),
                                    proj.damage,
                                    proj.weapon_type,
                                ));
                                local_results.to_remove.push(global_idx);
                            }
                        }
                    }
                }

                local_results
            })
            .collect();

        let mut merged_results = ProjectileChunkResults::default();
        for mut chunk_result in chunk_results {
            merged_results.to_remove.append(&mut chunk_result.to_remove);
            merged_results.hits.append(&mut chunk_result.hits);
            merged_results.wall_hits.append(&mut chunk_result.wall_hits);
            merged_results
                .spatial_updates
                .append(&mut chunk_result.spatial_updates);
            merged_results
                .wall_impacts
                .append(&mut chunk_result.wall_impacts);
        }

        // Remove dead projectiles
        merged_results.to_remove.sort_unstable();
        merged_results.to_remove.dedup();
        let mut remove_iter = merged_results.to_remove.into_iter().peekable();
        let mut kept_projectiles = Vec::with_capacity(all_projectiles.len());
        let mut removed_ids = Vec::new();

        for (idx, proj) in all_projectiles.into_iter().enumerate() {
            if remove_iter
                .peek()
                .is_some_and(|remove_idx| *remove_idx == idx)
            {
                let _ = remove_iter.next();
                removed_ids.push(proj.id);
            } else {
                kept_projectiles.push(proj);
            }
        }

        trace!(
            "[Frame {}] Projectile processing complete: {} processed, {} hits, {} wall hits, {} removed",
            frame,
            total_projectiles,
            merged_results.hits.len(),
            merged_results.wall_hits.len(),
            removed_ids.len()
        );

        ProjectileResults {
            total_processed: total_projectiles,
            hits: merged_results.hits,
            wall_hits: merged_results.wall_hits,
            removed_projectile_ids: removed_ids,
            kept_projectiles,
            spatial_updates: merged_results.spatial_updates,
            wall_impacts: merged_results.wall_impacts,
        }
    }

    fn apply_wall_damage_authoritative(&self, wall_hits: &[(EntityId, i32)]) -> usize {
        if wall_hits.is_empty() {
            return 0;
        }

        let mut wall_damage_by_id: HashMap<EntityId, i32> = HashMap::new();
        for (wall_id, damage) in wall_hits {
            *wall_damage_by_id.entry(*wall_id).or_insert(0) += *damage;
        }

        let partitions_for_lookup = self.world_partition_manager.get_partitions_for_processing();
        let mut wall_partition_lookup: HashMap<EntityId, usize> = HashMap::new();
        for (partition_idx, partition) in partitions_for_lookup.iter().enumerate() {
            for wall_entry in partition.all_walls_in_partition.iter() {
                wall_partition_lookup.insert(*wall_entry.key(), partition_idx);
            }
        }

        let mut destroyed_count = 0usize;
        for (wall_id, total_damage) in wall_damage_by_id {
            if let Some(partition_idx) = wall_partition_lookup.get(&wall_id).copied() {
                if let Some(partition) = partitions_for_lookup.get(partition_idx) {
                    if let Some((destroyed, pos)) =
                        partition.damage_destructible_wall(wall_id, total_damage)
                    {
                        if destroyed {
                            destroyed_count += 1;
                            self.global_game_events.push(
                                GameEvent::WallDestroyed {
                                    wall_id,
                                    position: pos,
                                },
                                EventPriority::High,
                            );
                            self.destroyed_wall_ids_this_tick.write().insert(wall_id);
                            self.wall_respawn_manager.wall_destroyed(wall_id);
                        }
                    }
                }
            }
        }

        destroyed_count
    }

    async fn apply_projectile_results(&self, results: ProjectileResults) {
        let ProjectileResults {
            total_processed: _,
            hits,
            wall_hits,
            removed_projectile_ids,
            kept_projectiles,
            spatial_updates,
            wall_impacts,
        } = results;

        for wall_impact in wall_impacts {
            self.global_game_events
                .push(wall_impact, EventPriority::Normal);
        }
        if !spatial_updates.is_empty() {
            self.spatial_index
                .batch_update_projectiles(&spatial_updates);
        }
        self.commit_authoritative_projectile_state(kept_projectiles, &removed_projectile_ids);

        let destroyed_walls = self.apply_wall_damage_authoritative(&wall_hits);
        if destroyed_walls > 0 {
            trace!(
                "Applied authoritative wall damage from projectile results (destroyed_walls={}).",
                destroyed_walls
            );
        }

        // Process hits - reuse existing game logic
        for (attacker_id, target_id, damage, weapon) in hits {
            if let Some(mut target_state_entry) =
                self.player_manager.get_player_state_mut(&target_id)
            {
                if target_state_entry.alive {
                    let died = target_state_entry.apply_damage(damage);
                    let target_pos = Vec2::new(target_state_entry.x, target_state_entry.y);

                    self.global_game_events.push(
                        GameEvent::PlayerDamaged {
                            target_id: target_id.clone(),
                            attacker_id: Some(attacker_id.clone()),
                            damage,
                            weapon,
                            position: target_pos,
                        },
                        EventPriority::Normal,
                    );

                    if died {
                        // Store flag carry state before clearing it
                        let victim_was_carrying_flag_id =
                            target_state_entry.is_carrying_flag_team_id;
                        let victim_username = target_state_entry.username.clone();

                        // Clear flag carry state on the victim
                        if victim_was_carrying_flag_id != 0 {
                            target_state_entry.is_carrying_flag_team_id = 0;
                            target_state_entry.mark_field_changed(FIELD_FLAG);
                        }

                        // Handle death (existing logic from run_physics_update)
                        if attacker_id != target_id {
                            // Get team information for friendly fire check
                            let attacker_team = self
                                .player_manager
                                .get_player_state(&attacker_id)
                                .map(|p| p.team_id)
                                .unwrap_or(0);
                            let victim_team = target_state_entry.team_id;

                            if let Some(mut attacker_state_entry) =
                                self.player_manager.get_player_state_mut(&attacker_id)
                            {
                                attacker_state_entry.kills += 1;

                                // Check for friendly fire
                                if attacker_team != 0
                                    && victim_team != 0
                                    && attacker_team == victim_team
                                {
                                    // Friendly fire: double negative score
                                    attacker_state_entry.score -= 200;
                                    info!(
                                        "Friendly fire penalty: {} killed teammate {}, -200 score",
                                        attacker_state_entry.username, victim_username
                                    );
                                } else {
                                    // Normal kill: positive score
                                    attacker_state_entry.score += 100;
                                }

                                attacker_state_entry.mark_field_changed(FIELD_SCORE_STATS);
                            }
                        }

                        // Update team scores for TeamDeathmatch
                        {
                            let match_info_guard = self.match_info.read();
                            if match_info_guard.game_mode == fb::GameModeType::TeamDeathmatch {
                                drop(match_info_guard);

                                // Get attacker and victim team IDs
                                let attacker_team = self
                                    .player_manager
                                    .get_player_state(&attacker_id)
                                    .map(|p| p.team_id)
                                    .unwrap_or(0);
                                let victim_team = target_state_entry.team_id;

                                // Award point to attacker's team if it's a valid team kill
                                if attacker_team != 0
                                    && victim_team != 0
                                    && attacker_team != victim_team
                                {
                                    let mut match_info_write = self.match_info.write();
                                    let team_score = match_info_write
                                        .team_scores
                                        .entry(attacker_team)
                                        .or_insert(0);
                                    *team_score += 1;
                                    info!("Team {} scored! New score: {} (kill by player on victim from team {})",
                                          attacker_team, *team_score, victim_team);
                                }
                            }
                        }

                        self.global_game_events.push(
                            GameEvent::PlayerKilled {
                                victim_id: target_id.clone(),
                                killer_id: attacker_id.clone(),
                                weapon,
                                position: target_pos,
                            },
                            EventPriority::High,
                        );

                        // Update kill feed
                        let killer_username = self
                            .player_manager
                            .get_player_state(&attacker_id)
                            .map_or_else(|| "World".to_string(), |p| p.username.clone());

                        self.push_kill_feed_entry(
                            killer_username.clone(),
                            victim_username.clone(),
                            weapon,
                        );

                        // Handle flag dropping if victim was carrying a flag
                        if victim_was_carrying_flag_id != 0 {
                            let mut match_info_guard = self.match_info.write();

                            // Drop the flag
                            if let Some(flag_state) = match_info_guard
                                .flag_states
                                .get_mut(&victim_was_carrying_flag_id)
                            {
                                flag_state.status = fb::FlagStatus::Dropped;
                                flag_state.position = target_pos;
                                flag_state.carrier_id = None;
                                flag_state.respawn_timer = 30.0;

                                // Push flag dropped event after releasing match_info lock
                                drop(match_info_guard);

                                self.global_game_events.push(
                                    GameEvent::FlagDropped {
                                        player_id: target_id.clone(),
                                        flag_team_id: victim_was_carrying_flag_id,
                                        position: target_pos,
                                    },
                                    EventPriority::High,
                                );

                                info!("(Projectile Kill) Flag of team {} dropped at ({:.1}, {:.1}) by {} killing {}",
                                      victim_was_carrying_flag_id, target_pos.x, target_pos.y, killer_username, victim_username);
                            }
                        }
                    }
                }
            }
        }
    }

    async fn process_pickup_respawns(&self, delta_time: f32) {
        let mut pickups_guard = self.pickups.write();
        self.process_pickup_respawns_authoritative(pickups_guard.as_mut_slice(), delta_time);
    }

    fn get_enemy_positions_for_team(&self, team_id: u8) -> Vec<(Vec2, PlayerID)> {
        let mut enemies = Vec::with_capacity(50);
        self.player_manager.for_each_player(|id, state| {
            if state.alive && state.team_id != team_id && state.team_id != 0 {
                enemies.push((Vec2::new(state.x, state.y), id.clone()));
            }
        });
        enemies
    }

    pub fn collect_all_walls_current_state(&self) -> Vec<Wall> {
        let mut all_walls = Vec::new();
        for partition_arc in self.world_partition_manager.get_partitions_for_processing() {
            partition_arc
                .all_walls_in_partition
                .iter()
                .for_each(|wall_entry| {
                    let wall = wall_entry.value();
                    // Send ALL walls including destroyed ones - client needs to render them as rubble/obstacles
                    all_walls.push(wall.clone());
                });
        }
        all_walls
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

    fn publish_player_soa_snapshot_if_enabled(&self) {
        if !join_soa_snapshot_enabled() {
            return;
        }

        let mut owned_states = Vec::with_capacity(self.player_manager.player_count());
        self.player_manager
            .for_each_player(|player_id, player_state| {
                owned_states.push((player_id.clone(), player_state.clone()));
            });
        self.player_soa_snapshot
            .publish(PlayerSoASnapshot::from_owned_player_states(owned_states));
    }

    fn publish_entity_soa_snapshots_if_enabled(&self) {
        if !join_entity_soa_snapshot_enabled() {
            return;
        }

        let projectiles_guard = self.projectiles.read();
        self.projectile_soa_snapshot
            .publish(ProjectileSoASnapshot::from_projectiles_slice(
                &projectiles_guard,
            ));
        drop(projectiles_guard);

        let pickups_guard = self.pickups.read();
        self.pickup_soa_snapshot
            .publish(PickupSoASnapshot::from_pickups_slice(&pickups_guard));
    }

    fn publish_player_aoi_snapshot_if_enabled(&self) {
        if !join_authoritative_aoi_snapshot_enabled() {
            return;
        }

        let mut owned_aois = Vec::with_capacity(self.player_aois.len());
        for aoi_entry in self.player_aois.iter() {
            let player_id = self.player_manager.id_pool.get_or_create(aoi_entry.key());
            owned_aois.push((player_id, aoi_entry.value().clone()));
        }
        self.player_aoi_snapshot
            .publish(PlayerAoISnapshot::from_owned_player_aois(owned_aois));
    }

    fn publish_authoritative_lock_free_snapshots(&self) {
        self.publish_player_soa_snapshot_if_enabled();
        self.publish_entity_soa_snapshots_if_enabled();
        self.publish_player_aoi_snapshot_if_enabled();
    }

    fn rebuild_player_soa_snapshot_from_authoritative_state(&self) -> Arc<PlayerSoASnapshot> {
        let mut owned_states = Vec::with_capacity(self.player_manager.player_count());
        self.player_manager
            .for_each_player(|player_id, player_state| {
                owned_states.push((player_id.clone(), player_state.clone()));
            });
        let snapshot = Arc::new(PlayerSoASnapshot::from_owned_player_states(owned_states));
        self.player_soa_snapshot.publish_arc(snapshot.clone());
        snapshot
    }

    fn rebuild_projectile_soa_snapshot_from_authoritative_state(
        &self,
    ) -> Arc<ProjectileSoASnapshot> {
        let projectiles_guard = self.projectiles.read();
        let snapshot = Arc::new(ProjectileSoASnapshot::from_projectiles_slice(
            &projectiles_guard,
        ));
        drop(projectiles_guard);
        self.projectile_soa_snapshot.publish_arc(snapshot.clone());
        snapshot
    }

    fn rebuild_pickup_soa_snapshot_from_authoritative_state(&self) -> Arc<PickupSoASnapshot> {
        let pickups_guard = self.pickups.read();
        let snapshot = Arc::new(PickupSoASnapshot::from_pickups_slice(&pickups_guard));
        drop(pickups_guard);
        self.pickup_soa_snapshot.publish_arc(snapshot.clone());
        snapshot
    }

    async fn process_client_broadcast(
        peer_id_str: &str,
        client_info: &ClientInfo,
        shared_data: &SharedBroadcastData,
        server: &Arc<MassiveGameServer>, // Correctly takes &Arc<MassiveGameServer>
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let frame = server.frame_counter.load(AtomicOrdering::Relaxed);

        trace!(
            "[Frame {}] Starting broadcast for client {}",
            frame,
            peer_id_str
        );

        let player_id_arc = server.player_manager.id_pool.get_or_create(peer_id_str);
        // Use the authoritative per-broadcast snapshot view to decide whether this
        // client can be serialized in this frame.
        let player_exists =
            Self::lookup_player_state_from_shared(shared_data, &player_id_arc).is_some();

        if !player_exists {
            trace!(
                "[Frame {}] Player {} absent from shared snapshot, deferring broadcast.",
                frame,
                peer_id_str
            );
            return Ok(());
        }

        if client_info.needs_initial_state {
            server.ensure_join_trace(peer_id_str, client_info.data_channel.is_open());
        }

        if client_info.needs_initial_state && !client_info.data_channel.is_open() {
            trace!(
                "[Frame {}] Client {} data channel not open yet, deferring initial state build.",
                frame,
                peer_id_str
            );
            return Ok(());
        }

        let mut client_state_for_delta: Option<ClientState> = None;
        let mut last_chat_message_seq_sent = server
            .client_states_map
            .read()
            .get(peer_id_str)
            .map(|state| state.last_chat_message_seq_sent)
            .unwrap_or_default();
        let state_result = if client_info.needs_initial_state {
            let cached_initial_state = server
                .client_states_map
                .read()
                .get(peer_id_str)
                .and_then(|state| state.pending_initial_state_bytes.clone());

            if let Some(cached_bytes) = cached_initial_state {
                trace!(
                    "[Frame {}] Reusing cached initial state for {} ({} bytes)",
                    frame,
                    peer_id_str,
                    cached_bytes.len()
                );
                Ok(cached_bytes)
            } else {
                server.mark_join_build_start(peer_id_str);
                trace!(
                    "[Frame {}] Building initial state for {}",
                    frame,
                    peer_id_str
                );
                let initial_result = server.build_initial_state_optimized(peer_id_str, shared_data);
                if let Ok(initial_bytes) = initial_result.as_ref() {
                    server.mark_join_build_done(peer_id_str);
                    // Cache built initial bytes immediately so retries avoid re-serialization.
                    let mut client_states = server.client_states_map.write();
                    let state_entry = client_states
                        .entry(peer_id_str.to_string())
                        .or_insert_with(ClientState::default);
                    state_entry.pending_initial_state_bytes = Some(initial_bytes.clone());
                    state_entry.known_walls_sent = false;
                }
                initial_result
            }
        } else {
            trace!("[Frame {}] Building delta state for {}", frame, peer_id_str);
            let client_state_snapshot = server.client_states_map
                .read() // Acquire read lock
                .get(peer_id_str)
                .map(|cs_state_ref| cs_state_ref.clone()) // Clone the ClientState from the &ClientState
                .unwrap_or_else(|| {
                    debug!(
                        "[Frame {}] ClientState not found for {} during delta build, using default.",
                        server.frame_counter.load(AtomicOrdering::Relaxed),
                        peer_id_str
                    );
                    ClientState::default()
                });
            let delta_result = server.build_delta_state_optimized(
                peer_id_str,
                &client_state_snapshot,
                shared_data,
            );
            last_chat_message_seq_sent = client_state_snapshot.last_chat_message_seq_sent;
            client_state_for_delta = Some(client_state_snapshot);
            delta_result
        };

        let bytes_to_send = match state_result {
            Ok(b) => {
                trace!(
                    "[Frame {}] State built successfully for {} ({} bytes)",
                    frame,
                    peer_id_str,
                    b.len()
                );
                b
            }
            Err(_e) => {
                error!(
                    "[Frame {}] Failed to build state for {}: {:?}",
                    frame, peer_id_str, _e
                );
                return Err(format!("Failed to build state for client {}", peer_id_str).into());
            }
        };

        trace!(
            "[Frame {}] Prepared state payload {} bytes for client {}",
            frame,
            bytes_to_send.len(),
            peer_id_str
        );

        let pending_chat_packets =
            collect_pending_chat_packets(last_chat_message_seq_sent, &shared_data.chat_packets);
        let mut outbound_packets: Vec<Bytes> = Vec::with_capacity(1 + pending_chat_packets.len());
        outbound_packets.push(bytes_to_send.clone());
        outbound_packets.extend(
            pending_chat_packets
                .iter()
                .map(|packet| packet.bytes.clone()),
        );

        const DELTA_SEND_TIMEOUT_MS: u64 = 50;
        const INITIAL_SEND_TIMEOUT_MS: u64 = 200;
        const INITIAL_SEND_TIMEOUT_TAIL_MS: u64 = 320;
        const INITIAL_SEND_TIMEOUT_AGGRESSIVE_TAIL_MS: u64 = 420;
        const INITIAL_SEND_TIMEOUT_EXTREME_TAIL_MS: u64 = 540;
        let base_send_timeout_ms = if client_info.needs_initial_state {
            if shared_data.extreme_tail_join_mode {
                INITIAL_SEND_TIMEOUT_EXTREME_TAIL_MS
            } else if shared_data.aggressive_tail_join_mode {
                INITIAL_SEND_TIMEOUT_AGGRESSIVE_TAIL_MS
            } else if shared_data.tail_join_mode {
                INITIAL_SEND_TIMEOUT_TAIL_MS
            } else {
                INITIAL_SEND_TIMEOUT_MS
            }
        } else {
            DELTA_SEND_TIMEOUT_MS
        };
        let send_timeout_ms = base_send_timeout_ms.saturating_add(
            ((outbound_packets.len().saturating_sub(1) as u64) * 12).min(INITIAL_SEND_TIMEOUT_MS),
        );

        if client_info.needs_initial_state {
            server.mark_join_send_start(peer_id_str);
        }
        let sent_packets = server
            .send_packet_batch_optimized(
                &client_info.data_channel,
                &outbound_packets,
                send_timeout_ms,
            )
            .await;
        let send_succeeded = sent_packets > 0;
        let sent_chat_packets_count = sent_packets
            .saturating_sub(1)
            .min(pending_chat_packets.len());

        let mut final_chat_message_seq_sent = last_chat_message_seq_sent;
        if sent_chat_packets_count > 0 {
            for packet in pending_chat_packets.iter().take(sent_chat_packets_count) {
                if packet.seq > final_chat_message_seq_sent {
                    final_chat_message_seq_sent = packet.seq;
                }
            }
        }
        if sent_chat_packets_count < pending_chat_packets.len() {
            final_chat_message_seq_sent = server
                .send_chat_messages_optimized(
                    &client_info.data_channel,
                    final_chat_message_seq_sent,
                    &shared_data.chat_packets,
                )
                .await;
        }

        if !send_succeeded {
            if client_info.data_channel.is_open() {
                warn!(
                    "[Frame {}] Send failed for client {} (timeout {}ms, batch packets {}).",
                    frame,
                    peer_id_str,
                    send_timeout_ms,
                    outbound_packets.len()
                );
            } else {
                trace!(
                    "[Frame {}] Send skipped for {} because data channel is not open.",
                    frame,
                    peer_id_str
                );
            }
        } else {
            trace!(
                "[Frame {}] Sent {} packet(s) to client {} in one dispatch path.",
                frame,
                sent_packets,
                peer_id_str
            );
        }

        if client_info.needs_initial_state && !send_succeeded {
            server.mark_join_send_failure(peer_id_str);
            trace!(
                "[Frame {}] Initial state send not completed for {}, retrying on next broadcast.",
                frame,
                peer_id_str
            );
            return Ok(());
        }
        if !client_info.needs_initial_state && !send_succeeded {
            trace!(
                "[Frame {}] Delta state send did not complete for {}; preserving previous snapshot.",
                frame,
                peer_id_str
            );
            return Ok(());
        }

        if client_info.needs_initial_state && send_succeeded {
            server.mark_join_send_done(peer_id_str);
        }

        trace!(
            "[Frame {}] Updating client state for {}",
            frame,
            peer_id_str
        );
        if client_info.needs_initial_state {
            server.update_client_state_after_initial(
                peer_id_str,
                shared_data,
                final_chat_message_seq_sent,
            );
        } else {
            let mut client_state = client_state_for_delta.unwrap_or_default();
            client_state.last_chat_message_seq_sent = final_chat_message_seq_sent;

            server.update_client_state_after_delta(&mut client_state, &player_id_arc, shared_data);
            server.update_client_state_after_delta_with_shared(&mut client_state, shared_data);

            server
                .client_states_map
                .write()
                .insert(peer_id_str.to_string(), client_state);
        }

        trace!(
            "[Frame {}] Broadcast processing complete for client {}",
            frame,
            peer_id_str
        );
        Ok(())
    }

    async fn process_quic_client_broadcast(
        peer_id_str: &str,
        needs_initial_state: bool,
        shared_data: &SharedBroadcastData,
        server: &Arc<MassiveGameServer>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let frame = server.frame_counter.load(AtomicOrdering::Relaxed);
        let player_id_arc = server.player_manager.id_pool.get_or_create(peer_id_str);
        let player_exists =
            Self::lookup_player_state_from_shared(shared_data, &player_id_arc).is_some();
        if !player_exists {
            return Ok(());
        }

        let mut client_state_for_delta: Option<ClientState> = None;
        let mut last_chat_message_seq_sent = server
            .client_states_map
            .read()
            .get(peer_id_str)
            .map(|state| state.last_chat_message_seq_sent)
            .unwrap_or_default();
        let state_result = if needs_initial_state {
            let cached_initial_state = server
                .client_states_map
                .read()
                .get(peer_id_str)
                .and_then(|state| state.pending_initial_state_bytes.clone());

            if let Some(cached_bytes) = cached_initial_state {
                Ok(cached_bytes)
            } else {
                let initial_result = server.build_initial_state_optimized(peer_id_str, shared_data);
                if let Ok(initial_bytes) = initial_result.as_ref() {
                    let mut client_states = server.client_states_map.write();
                    let state_entry = client_states
                        .entry(peer_id_str.to_string())
                        .or_insert_with(ClientState::default);
                    state_entry.pending_initial_state_bytes = Some(initial_bytes.clone());
                    state_entry.known_walls_sent = false;
                }
                initial_result
            }
        } else {
            let client_state_snapshot = server
                .client_states_map
                .read()
                .get(peer_id_str)
                .cloned()
                .unwrap_or_default();
            let delta_result = server.build_delta_state_optimized(
                peer_id_str,
                &client_state_snapshot,
                shared_data,
            );
            last_chat_message_seq_sent = client_state_snapshot.last_chat_message_seq_sent;
            client_state_for_delta = Some(client_state_snapshot);
            delta_result
        };

        let bytes_to_send = match state_result {
            Ok(bytes) => bytes,
            Err(err) => {
                return Err(
                    format!(
                        "[Frame {}] failed building QUIC payload for {}: {}",
                        frame, peer_id_str, err
                    )
                    .into(),
                );
            }
        };

        let pending_chat_packets =
            collect_pending_chat_packets(last_chat_message_seq_sent, &shared_data.chat_packets);
        let mut outbound_packets = Vec::with_capacity(1 + pending_chat_packets.len());
        outbound_packets.push(bytes_to_send);
        outbound_packets.extend(pending_chat_packets.iter().map(|packet| packet.bytes.clone()));

        let sent_packets = send_quic_packet_batch(peer_id_str, &outbound_packets);
        let send_succeeded = sent_packets > 0;
        if !send_succeeded {
            trace!(
                "[Frame {}] QUIC send skipped/failed for {}",
                frame,
                peer_id_str
            );
            return Ok(());
        }

        let sent_chat_packets_count = sent_packets
            .saturating_sub(1)
            .min(pending_chat_packets.len());
        let mut final_chat_message_seq_sent = last_chat_message_seq_sent;
        for packet in pending_chat_packets.iter().take(sent_chat_packets_count) {
            if packet.seq > final_chat_message_seq_sent {
                final_chat_message_seq_sent = packet.seq;
            }
        }

        if needs_initial_state {
            server.update_client_state_after_initial(
                peer_id_str,
                shared_data,
                final_chat_message_seq_sent,
            );
        } else {
            let mut client_state = client_state_for_delta.unwrap_or_default();
            client_state.last_chat_message_seq_sent = final_chat_message_seq_sent;
            server.update_client_state_after_delta(&mut client_state, &player_id_arc, shared_data);
            server.update_client_state_after_delta_with_shared(&mut client_state, shared_data);
            server
                .client_states_map
                .write()
                .insert(peer_id_str.to_string(), client_state);
        }

        Ok(())
    }

    fn get_server_timestamp_us(&self) -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64
    }

    pub fn get_server_timestamp_ms(&self) -> u64 {
        self.get_server_timestamp_us() / 1000
    }

    fn resolve_player_aoi_for_player(
        &self,
        shared_data: &SharedBroadcastData,
        player_id: &PlayerID,
    ) -> PlayerAoI {
        if shared_data.use_aoi_snapshot {
            shared_data
                .player_aois_snapshot
                .get(player_id)
                .cloned()
                .unwrap_or_else(Self::get_empty_player_aoi)
        } else {
            self.player_aois
                .get(player_id.as_str())
                .map(|entry| entry.value().clone())
                .unwrap_or_else(Self::get_empty_player_aoi)
        }
    }

    #[inline]
    fn lookup_player_state_from_shared<'a>(
        shared_data: &'a SharedBroadcastData,
        player_id: &PlayerID,
    ) -> Option<&'a PlayerState> {
        if shared_data.use_soa_snapshot {
            shared_data.player_soa_snapshot.get_state(player_id)
        } else {
            shared_data.player_states_snapshot.get(player_id)
        }
    }

    #[inline]
    fn lookup_projectile_from_shared<'a>(
        shared_data: &'a SharedBroadcastData,
        projectile_id: &EntityId,
    ) -> Option<&'a Projectile> {
        if shared_data.use_entity_soa_snapshot {
            shared_data
                .projectiles_soa_snapshot
                .get_state(projectile_id)
        } else {
            shared_data.projectiles_snapshot.get(projectile_id)
        }
    }

    #[inline]
    fn lookup_pickup_from_shared<'a>(
        shared_data: &'a SharedBroadcastData,
        pickup_id: &EntityId,
    ) -> Option<&'a Pickup> {
        if shared_data.use_entity_soa_snapshot {
            shared_data.pickups_soa_snapshot.get_state(pickup_id)
        } else {
            shared_data.pickups_snapshot.get(pickup_id)
        }
    }

    // Extracted melee processing logic
    fn process_melee_hits(&self, melee_hit_events: Vec<GameEvent>) {
        for event in melee_hit_events {
            if let GameEvent::MeleeHit {
                attacker_id,
                position: _attack_pos,
                ..
            } = event
            {
                let melee_range_sq = 50.0 * 50.0;
                let melee_arc_angle_rad = std::f32::consts::FRAC_PI_3;
                let melee_damage = 30;

                // Get attacker info
                let (
                    attacker_pos_x,
                    attacker_pos_y,
                    attacker_rot,
                    attacker_team_id,
                    attacker_username,
                ) = {
                    if let Some(attacker_state_guard) =
                        self.player_manager.get_player_state(&attacker_id)
                    {
                        (
                            attacker_state_guard.x,
                            attacker_state_guard.y,
                            attacker_state_guard.rotation,
                            attacker_state_guard.team_id,
                            attacker_state_guard.username.clone(),
                        )
                    } else {
                        continue; // Attacker not found
                    }
                };

                // Use spatial index for nearby players
                let melee_check_radius = 70.0;
                let nearby_player_ids = self.spatial_index.query_nearby_players(
                    attacker_pos_x,
                    attacker_pos_y,
                    melee_check_radius,
                );

                // Process each potential target
                for target_id_arc_nearby in nearby_player_ids {
                    if target_id_arc_nearby == attacker_id {
                        continue;
                    }

                    // Collect all the data we need from the target before applying damage
                    let target_hit_data = {
                        if let Some(mut target_state_entry) = self
                            .player_manager
                            .get_player_state_mut(&target_id_arc_nearby)
                        {
                            let target_state = &mut *target_state_entry;

                            if !target_state.alive
                                || (target_state.team_id != 0
                                    && attacker_team_id != 0
                                    && target_state.team_id == attacker_team_id)
                            {
                                continue; // Skip dead or same-team targets
                            }

                            let dx = target_state.x - attacker_pos_x;
                            let dy = target_state.y - attacker_pos_y;
                            let dist_sq = dx * dx + dy * dy;

                            if dist_sq >= melee_range_sq {
                                continue; // Out of range
                            }

                            let angle_to_target = dy.atan2(dx);
                            let mut angle_diff = (angle_to_target - attacker_rot)
                                .rem_euclid(2.0 * std::f32::consts::PI);
                            if angle_diff > std::f32::consts::PI {
                                angle_diff = 2.0 * std::f32::consts::PI - angle_diff;
                            }

                            if angle_diff > melee_arc_angle_rad / 2.0 {
                                continue; // Outside melee arc
                            }

                            info!("[Melee] {} attempting to hit {} (dist_sq: {:.1}, angle_diff: {:.2} rad).",
                                  attacker_id.as_str(), target_id_arc_nearby.as_str(), dist_sq, angle_diff);

                            // Apply damage and collect necessary data
                            let died = target_state.apply_damage(melee_damage);
                            let target_position = Vec2::new(target_state.x, target_state.y);
                            let target_username = target_state.username.clone();
                            let victim_was_carrying_flag_id = if died {
                                target_state.is_carrying_flag_team_id
                            } else {
                                0
                            };

                            if died {
                                // Reset flag carry state on the victim
                                target_state.is_carrying_flag_team_id = 0;
                                target_state.mark_field_changed(FIELD_FLAG);
                            }

                            Some((
                                died,
                                target_position,
                                target_username,
                                victim_was_carrying_flag_id,
                            ))
                        } else {
                            None
                        }
                    };

                    // Now process the hit results without holding any mutable borrows
                    if let Some((
                        died,
                        target_position,
                        target_username,
                        victim_was_carrying_flag_id,
                    )) = target_hit_data
                    {
                        // Push damage event
                        self.global_game_events.push(
                            GameEvent::PlayerDamaged {
                                target_id: target_id_arc_nearby.clone(),
                                attacker_id: Some(attacker_id.clone()),
                                damage: melee_damage,
                                weapon: ServerWeaponType::Melee,
                                position: target_position,
                            },
                            EventPriority::Normal,
                        );

                        if died {
                            // Update attacker stats
                            if attacker_id != target_id_arc_nearby {
                                // Get victim team for friendly fire check
                                let victim_team = self
                                    .player_manager
                                    .get_player_state(&target_id_arc_nearby)
                                    .map(|p| p.team_id)
                                    .unwrap_or(0);

                                if let Some(mut attacker_mut_state_entry) =
                                    self.player_manager.get_player_state_mut(&attacker_id)
                                {
                                    let attacker_mut_state = &mut *attacker_mut_state_entry;
                                    attacker_mut_state.kills += 1;

                                    // Check for friendly fire
                                    if attacker_team_id != 0
                                        && victim_team != 0
                                        && attacker_team_id == victim_team
                                    {
                                        // Friendly fire: double negative score
                                        attacker_mut_state.score -= 200;
                                        info!("Friendly fire penalty (melee): {} killed teammate {}, -200 score",
                                              attacker_username, target_username);
                                    } else {
                                        // Normal kill: positive score
                                        attacker_mut_state.score += 100;
                                    }

                                    attacker_mut_state.mark_field_changed(FIELD_SCORE_STATS);
                                }
                            }

                            // Push kill event
                            self.global_game_events.push(
                                GameEvent::PlayerKilled {
                                    victim_id: target_id_arc_nearby.clone(),
                                    killer_id: attacker_id.clone(),
                                    weapon: ServerWeaponType::Melee,
                                    position: target_position,
                                },
                                EventPriority::High,
                            );

                            // Update kill feed
                            self.push_kill_feed_entry(
                                attacker_username.clone(),
                                target_username,
                                ServerWeaponType::Melee,
                            );

                            // Handle flag dropping if victim was carrying a flag
                            if victim_was_carrying_flag_id != 0 {
                                let mut match_info_guard = self.match_info.write();

                                // Award score to attacker's team if applicable
                                if let Some(attacker_state_for_score) =
                                    self.player_manager.get_player_state(&attacker_id)
                                {
                                    if attacker_state_for_score.team_id != 0
                                        && attacker_state_for_score.team_id
                                            != victim_was_carrying_flag_id
                                    {
                                        let team_score_mut_ref = match_info_guard
                                            .team_scores
                                            .entry(attacker_state_for_score.team_id)
                                            .or_insert(0);
                                        *team_score_mut_ref += 1;
                                        info!("Team {} scored +1 via melee kill on flag carrier by {}",
                                              attacker_state_for_score.team_id, attacker_id.as_str());
                                    }
                                }

                                // Drop the flag
                                if let Some(flag_state) = match_info_guard
                                    .flag_states
                                    .get_mut(&victim_was_carrying_flag_id)
                                {
                                    flag_state.status = fb::FlagStatus::Dropped;
                                    flag_state.position = target_position;
                                    flag_state.carrier_id = None;
                                    flag_state.respawn_timer = 30.0;

                                    // Push flag dropped event after releasing match_info lock
                                    drop(match_info_guard);

                                    self.global_game_events.push(
                                        GameEvent::FlagDropped {
                                            player_id: target_id_arc_nearby.clone(),
                                            flag_team_id: victim_was_carrying_flag_id,
                                            position: target_position,
                                        },
                                        EventPriority::High,
                                    );

                                    info!(
                                        "(Melee Kill) Flag of team {} dropped at ({:.1}, {:.1})",
                                        victim_was_carrying_flag_id,
                                        target_position.x,
                                        target_position.y
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /*async fn process_client_broadcast_old(
        &self,
        peer_id_str: String,
        data_channel: Arc<crate::core::types::RTCDataChannel>,
        shared_data: &SharedBroadcastData,
    ) {
        // Get or create client state efficiently
        let mut client_state_needs_update = false;
        let client_state_copy = self.client_states_map
            .get(&peer_id_str)
            .map(|cs| cs.clone())
            .unwrap_or_else(|| {
                client_state_needs_update = true;
                ClientState::default()
            });

        // Process based on client state
        if !client_state_copy.known_walls_sent {
            // Build and send initial state
            let message_bytes = self.build_initial_state_optimized(
                &peer_id_str,
                shared_data
            ).await;

            if let Ok(bytes) = message_bytes {
                let _ = data_channel.send(&bytes).await;
            }

            // Update client state
            self.update_client_state_after_initial(&peer_id_str, shared_data);
        } else {
            // Build and send delta state
            let message_bytes = self.build_delta_state_optimized(
                &peer_id_str,
                &client_state_copy,
                shared_data
            ).await;

            if let Ok(bytes) = message_bytes {
                let _ = data_channel.send(&bytes).await;
            }

            // Send chat messages efficiently
            self.send_chat_messages_optimized(
                &peer_id_str,
                &data_channel,
                &client_state_copy,
                &shared_data.chat_messages
            ).await;

            // Update client state
            self.update_client_state_after_delta(&peer_id_str, shared_data);
        }
    }*/

    // Complete replacement for build_delta_state_optimized method:
    // In server/src/server/instance.rs
    fn build_delta_state_optimized(
        &self,
        peer_id_str: &str,
        client_state: &ClientState,
        shared_data: &SharedBroadcastData,
    ) -> Result<Bytes, Box<dyn std::error::Error + Send + Sync>> {
        let mut builder = flatbuffers::FlatBufferBuilder::with_capacity(16384);
        let build_start = Instant::now();
        let player_id = self.player_manager.id_pool.get_or_create(peer_id_str);

        trace!("[{}] DeltaBuilder: Started", peer_id_str);

        // Resolve AoI from authoritative per-frame snapshot when enabled, otherwise
        // use the legacy live AoI map path.
        let player_aoi = self.resolve_player_aoi_for_player(shared_data, &player_id);

        // Build player deltas (only changed or newly visible)
        let mut players_fb_vec = Vec::new();
        let mut player_fields_mask_vec: Vec<u8> = Vec::new();
        let mut removed_player_ids_vec = Vec::new();
        let encode_changed_mask = |mask: u16| -> u8 {
            if mask == 0xFFFF {
                u8::MAX
            } else {
                (mask & 0x00FF) as u8
            }
        };

        // Add self player only if changed or not known yet
        if let Some(self_state) = Self::lookup_player_state_from_shared(shared_data, &player_id) {
            let is_new = !client_state.last_known_players.contains(&player_id);
            if is_new || self_state.changed_fields > 0 {
                let mask = if is_new {
                    0xFFFF
                } else {
                    self_state.changed_fields
                };
                players_fb_vec.push(create_fb_player_state_for_delta(
                    &mut builder,
                    &self_state,
                    mask,
                ));
                player_fields_mask_vec.push(encode_changed_mask(mask));
            }
        }

        // Add visible players only if changed or newly visible
        for visible_player_id in &player_aoi.visible_players {
            if visible_player_id != &player_id {
                if let Some(player_state) =
                    Self::lookup_player_state_from_shared(shared_data, visible_player_id)
                {
                    let is_new = !client_state.last_known_players.contains(visible_player_id);
                    if is_new || player_state.changed_fields > 0 {
                        let mask = if is_new {
                            0xFFFF
                        } else {
                            player_state.changed_fields
                        };
                        players_fb_vec.push(create_fb_player_state_for_delta(
                            &mut builder,
                            &player_state,
                            mask,
                        ));
                        player_fields_mask_vec.push(encode_changed_mask(mask));
                    }
                }
            }
        }

        // Find removed players
        for known_player_id in &client_state.last_known_players {
            if !player_aoi.visible_players.contains(known_player_id)
                && known_player_id != &player_id
            {
                removed_player_ids_vec.push(builder.create_string(known_player_id.as_str()));
            }
        }

        let players_fb = builder.create_vector(&players_fb_vec);
        let changed_player_fields_fb = if !player_fields_mask_vec.is_empty() {
            Some(builder.create_vector(&player_fields_mask_vec))
        } else {
            None
        };
        let removed_players_fb = builder.create_vector(&removed_player_ids_vec);

        // Build projectile deltas
        let mut new_projectiles_vec = Vec::new();
        let mut removed_projectile_ids_vec = Vec::new();

        for proj_id in &player_aoi.visible_projectiles {
            if !client_state.last_known_projectile_ids.contains(proj_id) {
                if let Some(proj) = Self::lookup_projectile_from_shared(shared_data, proj_id) {
                    let id_str = fb_safe_entity_id(&mut builder, proj.id);
                    let owner_str = builder.create_string(proj.owner_id.as_str());

                    let proj_fb = fb::ProjectileState::create(
                        &mut builder,
                        &fb::ProjectileStateArgs {
                            id: Some(id_str),
                            x: proj.x,
                            y: proj.y,
                            owner_id: Some(owner_str),
                            weapon_type: map_server_weapon_to_fb(proj.weapon_type),
                            velocity_x: proj.velocity_x, // not vx
                            velocity_y: proj.velocity_y, // not vy
                        },
                    );
                    new_projectiles_vec.push(proj_fb);
                }
            }
        }

        for known_proj_id in &client_state.last_known_projectile_ids {
            if !player_aoi.visible_projectiles.contains(known_proj_id) {
                let id_str = fb_safe_entity_id(&mut builder, *known_proj_id);
                removed_projectile_ids_vec.push(id_str);
            }
        }

        let projectiles_fb = builder.create_vector(&new_projectiles_vec);
        let removed_projectiles_fb = builder.create_vector(&removed_projectile_ids_vec);

        // Build pickup deltas
        let mut pickups_delta_vec = Vec::new();
        let mut deactivated_pickup_ids_vec = Vec::new();

        for pickup_id in &player_aoi.visible_pickups {
            if let Some(pickup) = Self::lookup_pickup_from_shared(shared_data, pickup_id) {
                let should_send = if let Some(last_known_state) =
                    client_state.last_known_pickup_states.get(pickup_id)
                {
                    last_known_state.is_active != pickup.is_active
                } else {
                    true
                };

                if should_send {
                    let (pickup_type_fb, weapon_type_fb) =
                        map_core_pickup_to_fb(&pickup.pickup_type);
                    let id_str = fb_safe_entity_id(&mut builder, pickup.id);

                    let pickup_fb = fb::Pickup::create(
                        &mut builder,
                        &fb::PickupArgs {
                            id: Some(id_str),
                            x: pickup.x,
                            y: pickup.y,
                            pickup_type: pickup_type_fb,
                            weapon_type: weapon_type_fb.unwrap_or(fb::WeaponType::Pistol),
                            is_active: pickup.is_active,
                        },
                    );
                    pickups_delta_vec.push(pickup_fb);
                }
            }
        }

        for (known_pickup_id, _) in &client_state.last_known_pickup_states {
            if !player_aoi.visible_pickups.contains(known_pickup_id) {
                let id_str = fb_safe_entity_id(&mut builder, *known_pickup_id);
                deactivated_pickup_ids_vec.push(id_str);
            }
        }

        let pickups_fb = builder.create_vector(&pickups_delta_vec);
        let deactivated_pickups_fb = builder.create_vector(&deactivated_pickup_ids_vec);

        // Build events (single-machine/tail mode can lower per-client event budget).
        let game_events_fb = if shared_data.max_delta_events_per_client == 0 {
            None
        } else {
            let events_vec: Vec<_> = shared_data
                .events
                .iter()
                .take(shared_data.max_delta_events_per_client)
                .map(|event| build_game_event_fb(&mut builder, event))
                .collect();
            if events_vec.is_empty() {
                None
            } else {
                Some(builder.create_vector(&events_vec))
            }
        };

        // Build kill feed
        let kill_feed_vec: Vec<_> = shared_data
            .kill_feed_snapshot
            .iter()
            .skip(client_state.last_kill_feed_count_sent)
            .map(|entry| {
                let killer_name_fb = builder.create_string(&entry.killer_name);
                let victim_name_fb = builder.create_string(&entry.victim_name);
                fb::KillFeedEntry::create(
                    &mut builder,
                    &fb::KillFeedEntryArgs {
                        killer_name: Some(killer_name_fb),
                        victim_name: Some(victim_name_fb),
                        weapon: map_server_weapon_to_fb(entry.weapon),
                        timestamp: entry.timestamp as f32,
                        killer_position: None,
                        victim_position: None,
                        is_headshot: false,
                    },
                )
            })
            .collect();
        let kill_feed_fb = builder.create_vector(&kill_feed_vec);

        // Build match info if changed
        let match_info_fb = {
            let match_snapshot = &shared_data.match_info_snapshot;
            let team_scores_changed =
                client_state.last_known_team_scores != match_snapshot.team_scores;
            let time_changed = client_state
                .last_known_match_time_remaining
                .map_or(true, |t| (t - match_snapshot.time_remaining).abs() > 0.5);
            let state_changed = client_state
                .last_known_match_state
                .map_or(true, |s| s != match_snapshot.match_state);
            if client_state.match_info_pending
                || state_changed
                || time_changed
                || team_scores_changed
            {
                let team_scores_vec: Vec<_> = match_snapshot
                    .team_scores
                    .iter()
                    .map(|(team_id, score)| {
                        fb::TeamScoreEntry::create(
                            &mut builder,
                            &fb::TeamScoreEntryArgs {
                                team_id: *team_id as i8,
                                score: *score,
                            },
                        )
                    })
                    .collect();
                let team_scores_fb = builder.create_vector(&team_scores_vec);
                Some(fb::MatchInfo::create(
                    &mut builder,
                    &fb::MatchInfoArgs {
                        time_remaining: match_snapshot.time_remaining,
                        match_state: match_snapshot.match_state,
                        winner_id: None,
                        winner_name: None,
                        game_mode: match_snapshot.game_mode,
                        team_scores: Some(team_scores_fb),
                    },
                ))
            } else {
                None
            }
        };

        // Build destroyed wall IDs
        let destroyed_walls_vec: Vec<_> = shared_data
            .destroyed_wall_ids
            .iter()
            .filter(|id| !client_state.known_destroyed_wall_ids.contains(*id))
            .map(|id| fb_safe_entity_id(&mut builder, *id))
            .collect();
        let destroyed_wall_ids_fb = if !destroyed_walls_vec.is_empty() {
            Some(builder.create_vector(&destroyed_walls_vec))
        } else {
            None
        };

        // Build updated walls (respawned walls)
        let mut updated_walls_vec = Vec::new();
        let mut updated_wall_ids_sent = HashSet::new();

        // First, send walls explicitly updated this tick (damage/respawn) within AoI.
        for (wall_id, wall_data) in shared_data.updated_walls.iter() {
            if player_aoi.visible_walls.contains(wall_id) {
                let id_fb = fb_safe_entity_id(&mut builder, wall_data.id);
                let wall_fb = fb::Wall::create(
                    &mut builder,
                    &fb::WallArgs {
                        id: Some(id_fb),
                        x: wall_data.x,
                        y: wall_data.y,
                        width: wall_data.width,
                        height: wall_data.height,
                        is_destructible: wall_data.is_destructible,
                        current_health: wall_data.current_health,
                        max_health: wall_data.max_health,
                    },
                );
                updated_walls_vec.push(wall_fb);
                updated_wall_ids_sent.insert(*wall_id);
                if updated_walls_vec.len() >= AOI_MAX_VISIBLE_WALLS {
                    break;
                }
            }
        }

        // Then stream newly visible or changed walls so larger/dynamic maps load progressively.
        if updated_walls_vec.len() < AOI_MAX_VISIBLE_WALLS {
            for visible_wall_id in &player_aoi.visible_walls {
                if updated_wall_ids_sent.contains(visible_wall_id) {
                    continue;
                }

                let wall_data = match shared_data.active_walls_by_id.get(visible_wall_id) {
                    Some(wall) => wall,
                    None => continue,
                };

                let should_send = client_state
                    .last_known_wall_states
                    .get(visible_wall_id)
                    .map_or(true, |(known_health, known_max_health)| {
                        *known_health != wall_data.current_health
                            || *known_max_health != wall_data.max_health
                    });

                if !should_send {
                    continue;
                }

                let id_fb = fb_safe_entity_id(&mut builder, wall_data.id);
                let wall_fb = fb::Wall::create(
                    &mut builder,
                    &fb::WallArgs {
                        id: Some(id_fb),
                        x: wall_data.x,
                        y: wall_data.y,
                        width: wall_data.width,
                        height: wall_data.height,
                        is_destructible: wall_data.is_destructible,
                        current_health: wall_data.current_health,
                        max_health: wall_data.max_health,
                    },
                );
                updated_walls_vec.push(wall_fb);
                updated_wall_ids_sent.insert(*visible_wall_id);
                if updated_walls_vec.len() >= AOI_MAX_VISIBLE_WALLS {
                    break;
                }
            }
        }

        let updated_walls_fb = if !updated_walls_vec.is_empty() {
            Some(builder.create_vector(&updated_walls_vec))
        } else {
            None
        };

        // Build delta state message with correct field names
        let delta_state_args = fb::DeltaStateMessageArgs {
            players: Some(players_fb),
            projectiles: Some(projectiles_fb),
            removed_projectiles: Some(removed_projectiles_fb),
            pickups: Some(pickups_fb),
            deactivated_pickup_ids: Some(deactivated_pickups_fb),
            game_events: game_events_fb,
            timestamp: shared_data.timestamp_ms,
            last_processed_input_sequence: 0, // Get from player state if needed
            changed_player_fields: changed_player_fields_fb,
            kill_feed: Some(kill_feed_fb),
            match_info: match_info_fb,
            destroyed_wall_ids: destroyed_wall_ids_fb,
            flag_states: None,
            removed_player_ids: Some(removed_players_fb),
            updated_walls: updated_walls_fb,
        };

        let delta_state = fb::DeltaStateMessage::create(&mut builder, &delta_state_args);

        // Wrap in GameMessage
        let game_msg = fb::GameMessage::create(
            &mut builder,
            &fb::GameMessageArgs {
                msg_type: fb::MessageType::DeltaState,
                actual_message_type: fb::MessagePayload::DeltaStateMessage,
                actual_message: Some(delta_state.as_union_value()),
                protocol_version: GAME_PROTOCOL_VERSION,
            },
        );

        builder.finish(game_msg, None);
        let (buffer, root_index) = builder.collapse();
        let bytes = Bytes::from(buffer).slice(root_index..);

        trace!(
            "[{}] DeltaBuilder: Completed in {:?}",
            peer_id_str,
            build_start.elapsed()
        );
        Ok(bytes)
    }

    /*fn build_projectile_deltas_optimized(
        &self,
        builder: &mut FlatBufferBuilder,
        player_aoi: &PlayerAoI,
        last_known_projectiles: &HashSet<EntityId>,
    ) -> (Vec<flatbuffers::WIPOffset<fb::ProjectileState>>, Vec<flatbuffers::WIPOffset<&str>>) {
        let mut new_projectiles = Vec::new();
        let mut removed_projectile_ids = Vec::new();

        let projectiles_guard = self.projectiles.read();

        // Find new projectiles
        for projectile_id in &player_aoi.visible_projectiles {
            if !last_known_projectiles.contains(projectile_id) {
                if let Some(proj) = projectiles_guard.iter().find(|p| p.id == *projectile_id) {
                    let id_str = builder.create_string(&proj.id.to_string());
                    let owner_str = builder.create_string(&proj.owner_id.as_str());

                    let proj_fb = fb::ProjectileState::create(builder, &fb::ProjectileStateArgs {
                        id: Some(id_str),
                        x: proj.x,
                        y: proj.y,
                        vx: proj.vx,
                        vy: proj.vy,
                        damage: proj.damage as u8,
                        owner_id: Some(owner_str),
                        projectile_type: proj.projectile_type as u8,
                    });

                    new_projectiles.push(proj_fb);
                }
            }
        }

        drop(projectiles_guard);

        // Find removed projectiles
        for known_proj_id in last_known_projectiles {
            if !player_aoi.visible_projectiles.contains(known_proj_id) {
                let id_str = builder.create_string(&known_proj_id.to_string());
                removed_projectile_ids.push(id_str);
            }
        }

        (new_projectiles, removed_projectile_ids)
    }*/

    /*fn build_player_deltas_optimized(
        &self,
        builder: &mut FlatBufferBuilder,
        self_player_id: &Arc<String>,
        player_aoi: &PlayerAoI,
        last_known_players: &HashSet<Arc<String>>,
    ) -> (Vec<flatbuffers::WIPOffset<fb::PlayerState>>, Vec<flatbuffers::WIPOffset<&str>>) {
        let mut players_vec = Vec::new();
        let mut removed_player_ids = Vec::new();

        // Add self player
        if let Some(self_state) = self.player_manager.get_player_state_by_string(self_player_id) {
            let self_state_fb = self.create_player_state_fb(builder, self_player_id, &self_state);
            players_vec.push(self_state_fb);
        }

        // Add visible players
        for other_player_id in &player_aoi.visible_players {
            if other_player_id != self_player_id {
                if let Some(player_state) = self.player_manager.get_player_state_by_string(other_player_id) {
                    let player_state_fb = self.create_player_state_fb(builder, other_player_id, &player_state);
                    players_vec.push(player_state_fb);
                }
            }
        }

        // Find removed players
        for known_player_id in last_known_players {
            if known_player_id != self_player_id && !player_aoi.visible_players.contains(known_player_id) {
                let id_str = builder.create_string(known_player_id);
                removed_player_ids.push(id_str);
            }
        }

        (players_vec, removed_player_ids)
    }*/

    pub async fn broadcast_world_updates_optimized(self: Arc<Self>) {
        const BROADCAST_INTERVAL_FRAMES: u64 = 1;
        const MIN_BROADCAST_CONCURRENCY: usize = 8;
        const MAX_BROADCAST_CONCURRENCY: usize = 64;
        const MASS_JOIN_THROTTLE_PENDING_INITIAL_MIN: usize = 8;
        const MASS_JOIN_MEDIUM_PENDING_INITIAL_MIN: usize = 24;
        const MASS_JOIN_HEAVY_PENDING_INITIAL_MIN: usize = 48;
        const MASS_JOIN_DELTA_SKIP_MODULUS: u64 = 2;
        const MASS_JOIN_INITIAL_PER_FRAME_LIGHT: usize = 24;
        const MASS_JOIN_INITIAL_PER_FRAME_MEDIUM: usize = 20;
        const MASS_JOIN_INITIAL_PER_FRAME_HEAVY: usize = 16;
        const MASS_JOIN_MAX_DELTA_PER_FRAME_MEDIUM: usize = 20;
        const MASS_JOIN_MAX_DELTA_PER_FRAME_HEAVY: usize = 10;
        const MASS_JOIN_CONCURRENCY_CAP: usize = 48;
        const TAIL_JOIN_CONNECTED_CLIENTS_MIN: usize = 70;
        const TAIL_JOIN_PENDING_INITIAL_OPEN_MIN: usize = 3;
        const TAIL_JOIN_INITIAL_PER_FRAME_BOOST: usize = 32;
        const TAIL_JOIN_MAX_DELTA_PER_FRAME: usize = 4;
        const TAIL_JOIN_DELTA_SKIP_MODULUS: u64 = 4;
        const TAIL_JOIN_CONCURRENCY_CAP: usize = 36;
        const TAIL_JOIN_AGGRESSIVE_CONNECTED_CLIENTS_MIN: usize = 70;
        const TAIL_JOIN_AGGRESSIVE_PENDING_INITIAL_OPEN_MIN: usize = 6;
        const TAIL_JOIN_AGGRESSIVE_INITIAL_PER_FRAME_BOOST: usize = 56;
        const TAIL_JOIN_AGGRESSIVE_MAX_DELTA_PER_FRAME: usize = 1;
        const TAIL_JOIN_AGGRESSIVE_DELTA_SKIP_MODULUS: u64 = 7;
        const TAIL_JOIN_AGGRESSIVE_CONCURRENCY_CAP: usize = 28;
        // Dedicated policy for the 70+ tail wave where join timeout risk is highest.
        const TAIL_WAVE_70_PLUS_CLIENTS_MIN: usize = 70;
        const TAIL_WAVE_70_PLUS_PENDING_INITIAL_OPEN_MIN: usize = 2;
        const TAIL_WAVE_70_PLUS_INITIAL_PER_FRAME_BOOST: usize = 72;
        const TAIL_WAVE_70_PLUS_MAX_DELTA_PER_FRAME: usize = 1;
        const TAIL_WAVE_70_PLUS_DELTA_SKIP_MODULUS: u64 = 9;
        const TAIL_WAVE_70_PLUS_CONCURRENCY_CAP: usize = 24;
        const EXTREME_TAIL_WAVE_CLIENTS_MIN: usize = 90;
        const EXTREME_TAIL_WAVE_PENDING_INITIAL_OPEN_MIN: usize = 6;
        const EXTREME_TAIL_WAVE_INITIAL_PER_FRAME_BOOST: usize = 96;
        const EXTREME_TAIL_WAVE_MAX_DELTA_PER_FRAME: usize = 0;
        const EXTREME_TAIL_WAVE_DELTA_SKIP_MODULUS: u64 = 15;
        const EXTREME_TAIL_WAVE_CONCURRENCY_CAP: usize = 18;
        const SINGLE_MACHINE_TAIL_CONNECTED_CLIENTS_MIN: usize = 56;
        const SINGLE_MACHINE_TAIL_PENDING_INITIAL_OPEN_MIN: usize = 2;
        const SINGLE_MACHINE_AGGRESSIVE_CONNECTED_CLIENTS_MIN: usize = 64;
        const SINGLE_MACHINE_AGGRESSIVE_PENDING_INITIAL_OPEN_MIN: usize = 4;
        const SINGLE_MACHINE_INITIAL_PER_FRAME_BOOST: usize = 36;
        const SINGLE_MACHINE_MAX_DELTA_PER_FRAME: usize = 4;
        const SINGLE_MACHINE_DELTA_SKIP_MODULUS: u64 = 4;
        const SINGLE_MACHINE_CONCURRENCY_CAP: usize = 20;
        const MAX_DELTA_EVENTS_DEFAULT: usize = 50;
        const MAX_DELTA_EVENTS_TAIL: usize = 12;
        const MAX_DELTA_EVENTS_AGGRESSIVE: usize = 6;
        const MAX_DELTA_EVENTS_EXTREME_TAIL: usize = 2;
        const MAX_DELTA_EVENTS_SINGLE_MACHINE_BACKLOG: usize = 12;

        let current_frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        let last_broadcast = self.last_broadcast_frame.load(AtomicOrdering::Relaxed);
        let single_machine_opt = single_machine_optimization_enabled();

        if current_frame < last_broadcast + BROADCAST_INTERVAL_FRAMES && current_frame != 0 {
            trace!(
                "[Frame {}] Skipping broadcast (interval). Last broadcast: {}",
                current_frame,
                last_broadcast
            );
            return;
        }

        let quic_peer_ids = connected_quic_peer_ids();
        if !quic_peer_ids.is_empty() {
            let mut client_states_guard = self.client_states_map.write();
            for peer_id in &quic_peer_ids {
                client_states_guard
                    .entry(peer_id.clone())
                    .or_insert_with(ClientState::default);
            }
        }

        let connected_clients_total = self
            .data_channels_map
            .len()
            .saturating_add(quic_peer_ids.len());
        if connected_clients_total == 0 {
            if current_frame % 30 == 0 {
                // Log every 30 frames
                // Debug: List all keys in the map to see if there's a mismatch
                info!(
                    "[Frame {}] No connected clients in WebRTC/QUIC maps. Checking map contents...",
                    current_frame
                );
                info!(
                    "[Frame {}] Map ptr in broadcast: {:p}",
                    current_frame,
                    Arc::as_ptr(&self.data_channels_map)
                );
                for entry in self.data_channels_map.iter() {
                    info!(
                        "[Frame {}] Found entry in map: key={}",
                        current_frame,
                        entry.key()
                    );
                }
                info!(
                    "[Frame {}] Total entries found: {} (quic={})",
                    current_frame,
                    self.data_channels_map.len(),
                    quic_peer_ids.len()
                );
            }
            return;
        }

        debug!(
            "[Frame {}] Starting broadcast to {} clients. Last broadcast frame: {}",
            current_frame, connected_clients_total, last_broadcast
        );
        self.last_broadcast_frame
            .store(current_frame, AtomicOrdering::Relaxed);

        let client_entries: Vec<_> = {
            let client_states_guard = self.client_states_map.read();
            self.data_channels_map
                .iter()
                .map(|entry| {
                    let peer_id = entry.key().clone();
                    let data_channel = Arc::clone(entry.value());
                    let needs_initial = !client_states_guard
                        .get(&peer_id)
                        .map_or(false, |cs_state| cs_state.known_walls_sent);
                    let channel_open = data_channel.is_open();
                    (peer_id, data_channel, needs_initial, channel_open)
                })
                .collect()
        };
        let connected_clients_open = client_entries
            .iter()
            .filter(|(_, _, _, channel_open)| *channel_open)
            .count();
        if connected_clients_open == 0 && quic_peer_ids.is_empty() {
            trace!(
                "[Frame {}] Skipping broadcast fanout because no data channels are open (tracked={}).",
                current_frame,
                connected_clients_total
            );
            return;
        }

        let mut initial_entries_open: Vec<(String, Arc<crate::core::types::RTCDataChannel>, bool)> =
            Vec::new();
        let mut delta_entries: Vec<(String, Arc<crate::core::types::RTCDataChannel>, bool)> =
            Vec::new();
        let mut pending_initial_closed_count = 0usize;
        let mut pending_delta_closed_count = 0usize;

        for (peer_id, data_channel, needs_initial, channel_open) in client_entries {
            if needs_initial {
                self.ensure_join_trace(&peer_id, channel_open);
                if channel_open {
                    initial_entries_open.push((peer_id, data_channel, true));
                } else {
                    pending_initial_closed_count += 1;
                }
            } else if channel_open {
                delta_entries.push((peer_id, data_channel, false));
            } else {
                pending_delta_closed_count += 1;
            }
        }

        let quic_entries: Vec<(String, bool)> = {
            let client_states_guard = self.client_states_map.read();
            quic_peer_ids
                .iter()
                .filter(|peer_id| !self.data_channels_map.contains_key(peer_id.as_str()))
                .map(|peer_id| {
                    let needs_initial = !client_states_guard
                        .get(peer_id.as_str())
                        .map_or(false, |cs_state| cs_state.known_walls_sent);
                    (peer_id.clone(), needs_initial)
                })
                .collect()
        };

        let pending_initial_open_count = initial_entries_open.len();
        let pending_initial_total_count = pending_initial_open_count + pending_initial_closed_count;
        let tail_connected_clients_min = if single_machine_opt {
            SINGLE_MACHINE_TAIL_CONNECTED_CLIENTS_MIN
        } else {
            TAIL_JOIN_CONNECTED_CLIENTS_MIN
        };
        let tail_pending_initial_open_min = if single_machine_opt {
            SINGLE_MACHINE_TAIL_PENDING_INITIAL_OPEN_MIN
        } else {
            TAIL_JOIN_PENDING_INITIAL_OPEN_MIN
        };
        let aggressive_connected_clients_min = if single_machine_opt {
            SINGLE_MACHINE_AGGRESSIVE_CONNECTED_CLIENTS_MIN
        } else {
            TAIL_JOIN_AGGRESSIVE_CONNECTED_CLIENTS_MIN
        };
        let aggressive_pending_initial_open_min = if single_machine_opt {
            SINGLE_MACHINE_AGGRESSIVE_PENDING_INITIAL_OPEN_MIN
        } else {
            TAIL_JOIN_AGGRESSIVE_PENDING_INITIAL_OPEN_MIN
        };

        let tail_policy_enabled = join_tail_policy_enabled();
        let tail_join_mode = tail_policy_enabled
            && connected_clients_total >= tail_connected_clients_min
            && pending_initial_open_count >= tail_pending_initial_open_min;
        let aggressive_tail_join_mode = tail_policy_enabled
            && connected_clients_total >= aggressive_connected_clients_min
            && pending_initial_open_count >= aggressive_pending_initial_open_min;
        let tail_wave_70_plus_mode = tail_policy_enabled
            && connected_clients_total >= TAIL_WAVE_70_PLUS_CLIENTS_MIN
            && pending_initial_open_count >= TAIL_WAVE_70_PLUS_PENDING_INITIAL_OPEN_MIN;
        let extreme_tail_join_mode = tail_policy_enabled
            && connected_clients_total >= EXTREME_TAIL_WAVE_CLIENTS_MIN
            && pending_initial_open_count >= EXTREME_TAIL_WAVE_PENDING_INITIAL_OPEN_MIN;
        let initial_snapshot_caps = if extreme_tail_join_mode {
            InitialSnapshotCaps::EXTREME_TAIL
        } else if aggressive_tail_join_mode {
            InitialSnapshotCaps::TAIL_AGGRESSIVE
        } else if tail_join_mode {
            InitialSnapshotCaps::TAIL
        } else if single_machine_opt
            && pending_initial_total_count >= MASS_JOIN_THROTTLE_PENDING_INITIAL_MIN
        {
            InitialSnapshotCaps::SINGLE_MACHINE_BACKLOG
        } else {
            InitialSnapshotCaps::DEFAULT
        };

        let mut max_delta_events_per_client = if extreme_tail_join_mode {
            MAX_DELTA_EVENTS_EXTREME_TAIL
        } else if aggressive_tail_join_mode {
            MAX_DELTA_EVENTS_AGGRESSIVE
        } else if tail_join_mode {
            MAX_DELTA_EVENTS_TAIL
        } else if single_machine_opt
            && pending_initial_total_count >= MASS_JOIN_THROTTLE_PENDING_INITIAL_MIN
        {
            MAX_DELTA_EVENTS_SINGLE_MACHINE_BACKLOG
        } else {
            MAX_DELTA_EVENTS_DEFAULT
        };
        let soa_adaptive_fallback_active = join_soa_adaptive_fallback_enabled()
            && connected_clients_total >= MASS_JOIN_MEDIUM_PENDING_INITIAL_MIN
            && (pending_initial_total_count >= MASS_JOIN_THROTTLE_PENDING_INITIAL_MIN
                || aggressive_tail_join_mode
                || tail_join_mode);

        // Keep budget decisions tied to total backlog, but only schedule actionable
        // initial sends (open data channels).
        let mut max_initial_per_frame =
            if pending_initial_total_count >= MASS_JOIN_HEAVY_PENDING_INITIAL_MIN {
                MASS_JOIN_INITIAL_PER_FRAME_HEAVY
            } else if pending_initial_total_count >= MASS_JOIN_MEDIUM_PENDING_INITIAL_MIN {
                MASS_JOIN_INITIAL_PER_FRAME_MEDIUM
            } else {
                MASS_JOIN_INITIAL_PER_FRAME_LIGHT
            };
        if tail_join_mode {
            // 70+ client wave: allocate more slots to initial delivery to drain backlog sooner.
            max_initial_per_frame = max_initial_per_frame.max(TAIL_JOIN_INITIAL_PER_FRAME_BOOST);
        }
        if aggressive_tail_join_mode {
            max_initial_per_frame =
                max_initial_per_frame.max(TAIL_JOIN_AGGRESSIVE_INITIAL_PER_FRAME_BOOST);
        }
        if tail_wave_70_plus_mode {
            max_initial_per_frame =
                max_initial_per_frame.max(TAIL_WAVE_70_PLUS_INITIAL_PER_FRAME_BOOST);
        }
        if extreme_tail_join_mode {
            max_initial_per_frame =
                max_initial_per_frame.max(EXTREME_TAIL_WAVE_INITIAL_PER_FRAME_BOOST);
        }
        if single_machine_opt
            && pending_initial_total_count >= MASS_JOIN_THROTTLE_PENDING_INITIAL_MIN
        {
            max_initial_per_frame =
                max_initial_per_frame.max(SINGLE_MACHINE_INITIAL_PER_FRAME_BOOST);
        }

        let scheduled_initial_entries =
            if pending_initial_open_count > max_initial_per_frame && max_initial_per_frame > 0 {
                let start_index = (current_frame as usize) % pending_initial_open_count;
                let mut selected = Vec::with_capacity(max_initial_per_frame);
                for offset in 0..max_initial_per_frame {
                    let idx = (start_index + offset) % pending_initial_open_count;
                    selected.push(initial_entries_open[idx].clone());
                }
                selected
            } else {
                initial_entries_open
            };

        let include_active_walls_snapshot = !scheduled_initial_entries.is_empty();
        let throttle_delta_broadcasts = tail_join_mode
            || tail_wave_70_plus_mode
            || pending_initial_total_count >= MASS_JOIN_THROTTLE_PENDING_INITIAL_MIN;
        let mut max_delta_per_frame =
            if pending_initial_total_count >= MASS_JOIN_HEAVY_PENDING_INITIAL_MIN {
                MASS_JOIN_MAX_DELTA_PER_FRAME_HEAVY
            } else if pending_initial_total_count >= MASS_JOIN_MEDIUM_PENDING_INITIAL_MIN {
                MASS_JOIN_MAX_DELTA_PER_FRAME_MEDIUM
            } else {
                usize::MAX
            };
        if tail_join_mode {
            max_delta_per_frame = max_delta_per_frame.min(TAIL_JOIN_MAX_DELTA_PER_FRAME);
        }
        if aggressive_tail_join_mode {
            max_delta_per_frame = max_delta_per_frame.min(TAIL_JOIN_AGGRESSIVE_MAX_DELTA_PER_FRAME);
        }
        if tail_wave_70_plus_mode {
            max_delta_per_frame = max_delta_per_frame.min(TAIL_WAVE_70_PLUS_MAX_DELTA_PER_FRAME);
        }
        if extreme_tail_join_mode {
            max_delta_per_frame = max_delta_per_frame.min(EXTREME_TAIL_WAVE_MAX_DELTA_PER_FRAME);
        }
        if single_machine_opt
            && pending_initial_total_count >= MASS_JOIN_THROTTLE_PENDING_INITIAL_MIN
        {
            max_delta_per_frame = max_delta_per_frame.min(SINGLE_MACHINE_MAX_DELTA_PER_FRAME);
        }
        let mut delta_skip_modulus = if extreme_tail_join_mode {
            EXTREME_TAIL_WAVE_DELTA_SKIP_MODULUS
        } else if tail_wave_70_plus_mode {
            TAIL_WAVE_70_PLUS_DELTA_SKIP_MODULUS
        } else if aggressive_tail_join_mode {
            TAIL_JOIN_AGGRESSIVE_DELTA_SKIP_MODULUS
        } else if tail_join_mode {
            TAIL_JOIN_DELTA_SKIP_MODULUS
        } else if single_machine_opt
            && pending_initial_total_count >= MASS_JOIN_THROTTLE_PENDING_INITIAL_MIN
        {
            SINGLE_MACHINE_DELTA_SKIP_MODULUS
        } else {
            MASS_JOIN_DELTA_SKIP_MODULUS
        };

        let quality = self.current_quality_settings();
        max_delta_events_per_client =
            ((max_delta_events_per_client as f32) * quality.max_projectiles_scale)
                .round()
                .clamp(1.0, MAX_DELTA_EVENTS_DEFAULT as f32) as usize;
        delta_skip_modulus = delta_skip_modulus.max(quality.delta_skip_modulus);

        let mut scheduled_client_entries = scheduled_initial_entries;
        let mut scheduled_delta_count = 0usize;
        for (peer_id, data_channel, needs_initial) in delta_entries {
            if throttle_delta_broadcasts && current_frame % delta_skip_modulus != 0 {
                continue;
            }
            if scheduled_delta_count >= max_delta_per_frame {
                continue;
            }
            scheduled_client_entries.push((peer_id, data_channel, needs_initial));
            scheduled_delta_count += 1;
        }

        debug!(
            "[Frame {}] Join scheduler: tracked_clients_total={}, tracked_clients_open={}, pending_initial_total={}, pending_initial_open={}, pending_initial_closed={}, pending_delta_closed={}, tail_policy_enabled={}, tail_join_mode={}, aggressive_tail_join_mode={}, tail_wave_70_plus_mode={}, extreme_tail_join_mode={}, single_machine_opt={}, soa_fallback_active={}, initial_budget={}, delta_budget={}, delta_skip_modulus={}, delta_event_budget={}, snapshot_caps={{players:{}, walls:{}, projectiles:{}, pickups:{}}}, scheduled_initial={}, scheduled_delta={}",
            current_frame,
            connected_clients_total,
            connected_clients_open,
            pending_initial_total_count,
            pending_initial_open_count,
            pending_initial_closed_count,
            pending_delta_closed_count,
            tail_policy_enabled,
            tail_join_mode,
            aggressive_tail_join_mode,
            tail_wave_70_plus_mode,
            extreme_tail_join_mode,
            single_machine_opt,
            soa_adaptive_fallback_active,
            max_initial_per_frame,
            max_delta_per_frame,
            delta_skip_modulus,
            max_delta_events_per_client,
            initial_snapshot_caps.max_players,
            initial_snapshot_caps.max_walls,
            initial_snapshot_caps.max_projectiles,
            initial_snapshot_caps.max_pickups,
            scheduled_client_entries
                .iter()
                .filter(|(_, _, needs_initial)| *needs_initial)
                .count(),
            scheduled_delta_count
        );

        let mut scheduled_peer_ids: Vec<String> = scheduled_client_entries
            .iter()
            .map(|(peer_id, _, _)| peer_id.clone())
            .collect();
        for (peer_id, _) in &quic_entries {
            if !scheduled_peer_ids.iter().any(|existing| existing == peer_id) {
                scheduled_peer_ids.push(peer_id.clone());
            }
        }

        let shared_broadcast_data = Arc::new(
            self.prepare_shared_broadcast_data(
                include_active_walls_snapshot,
                initial_snapshot_caps,
                tail_join_mode,
                aggressive_tail_join_mode,
                extreme_tail_join_mode,
                soa_adaptive_fallback_active,
                max_delta_events_per_client,
                &scheduled_peer_ids,
            )
            .await,
        );
        trace!("[Frame {}] Prepared shared broadcast data. Events: {}, Destroyed Walls: {}, ChatPackets: {}, KF: {}, use_soa_snapshot={}, use_entity_soa_snapshot={}, soa_fallback_active={}",
            current_frame, shared_broadcast_data.events.len(), shared_broadcast_data.destroyed_wall_ids.len(),
            shared_broadcast_data.chat_packets.len(), shared_broadcast_data.kill_feed_snapshot.len(),
            shared_broadcast_data.use_soa_snapshot, shared_broadcast_data.use_entity_soa_snapshot, shared_broadcast_data.soa_fallback_active);

        let mut broadcast_concurrency = (self
            .config
            .thread_pools
            .networking_threads
            .saturating_mul(4))
        .clamp(MIN_BROADCAST_CONCURRENCY, MAX_BROADCAST_CONCURRENCY);
        if pending_initial_total_count >= MASS_JOIN_THROTTLE_PENDING_INITIAL_MIN {
            broadcast_concurrency = broadcast_concurrency.min(MASS_JOIN_CONCURRENCY_CAP);
        }
        if tail_join_mode {
            broadcast_concurrency = broadcast_concurrency.min(TAIL_JOIN_CONCURRENCY_CAP);
        }
        if aggressive_tail_join_mode {
            broadcast_concurrency = broadcast_concurrency.min(TAIL_JOIN_AGGRESSIVE_CONCURRENCY_CAP);
        }
        if tail_wave_70_plus_mode {
            broadcast_concurrency = broadcast_concurrency.min(TAIL_WAVE_70_PLUS_CONCURRENCY_CAP);
        }
        if extreme_tail_join_mode {
            broadcast_concurrency = broadcast_concurrency.min(EXTREME_TAIL_WAVE_CONCURRENCY_CAP);
        }
        if single_machine_opt
            && pending_initial_total_count >= MASS_JOIN_THROTTLE_PENDING_INITIAL_MIN
        {
            broadcast_concurrency = broadcast_concurrency.min(SINGLE_MACHINE_CONCURRENCY_CAP);
        }

        if broadcast_work_stealing_enabled() {
            let runtime_handle = tokio::runtime::Handle::current();
            let server_ref = Arc::clone(&self);
            let shared_data_ref = Arc::clone(&shared_broadcast_data);
            let frame_for_log = current_frame;

            self.thread_pools.network_pool.install(move || {
                scheduled_client_entries.into_par_iter().for_each(
                    |(peer_id_str, data_channel_arc, needs_initial)| {
                        let server_ref = Arc::clone(&server_ref);
                        let shared_data_ref = Arc::clone(&shared_data_ref);
                        let runtime_handle = runtime_handle.clone();

                        let client_info = ClientInfo {
                            data_channel: data_channel_arc,
                            needs_initial_state: needs_initial,
                        };

                        let result = runtime_handle.block_on(async {
                            Self::process_client_broadcast(
                                &peer_id_str,
                                &client_info,
                                shared_data_ref.as_ref(),
                                &server_ref,
                            )
                            .await
                        });
                        if let Err(err) = result {
                            error!(
                                "[Frame {}] Work-stealing broadcast failed for {}: {}",
                                frame_for_log, peer_id_str, err
                            );
                        }
                    },
                );
            });
        } else {
            let mut fanout_tasks = JoinSet::new();
            for (peer_id_str, data_channel_arc, needs_initial) in scheduled_client_entries {
                let server_ref = Arc::clone(&self);
                let shared_data_ref = Arc::clone(&shared_broadcast_data);

                fanout_tasks.spawn(async move {
                    let client_info = ClientInfo {
                        data_channel: data_channel_arc,
                        needs_initial_state: needs_initial,
                    };

                    trace!(
                        "[Frame {}] Processing client: {}, Needs Initial: {}",
                        current_frame,
                        peer_id_str,
                        client_info.needs_initial_state
                    );

                    if let Err(e) = Self::process_client_broadcast(
                        &peer_id_str,
                        &client_info,
                        shared_data_ref.as_ref(),
                        &server_ref,
                    )
                    .await
                    {
                        error!(
                            "[Frame {}] Error processing broadcast for client {}: {:?}",
                            current_frame, peer_id_str, e
                        );
                    }
                });

                if fanout_tasks.len() >= broadcast_concurrency {
                    if let Some(join_result) = fanout_tasks.join_next().await {
                        if let Err(join_err) = join_result {
                            error!(
                                "[Frame {}] Broadcast fanout task join error: {}",
                                current_frame, join_err
                            );
                        }
                    }
                }
            }

            while let Some(join_result) = fanout_tasks.join_next().await {
                if let Err(join_err) = join_result {
                    error!(
                        "[Frame {}] Broadcast fanout task join error: {}",
                        current_frame, join_err
                    );
                }
            }
        }

        if !quic_entries.is_empty() {
            let mut quic_tasks = JoinSet::new();
            for (peer_id_str, needs_initial) in quic_entries {
                let server_ref = Arc::clone(&self);
                let shared_data_ref = Arc::clone(&shared_broadcast_data);

                quic_tasks.spawn(async move {
                    if let Err(err) = Self::process_quic_client_broadcast(
                        &peer_id_str,
                        needs_initial,
                        shared_data_ref.as_ref(),
                        &server_ref,
                    )
                    .await
                    {
                        error!(
                            "[Frame {}] Error processing QUIC broadcast for {}: {}",
                            current_frame, peer_id_str, err
                        );
                    }
                });

                if quic_tasks.len() >= broadcast_concurrency {
                    if let Some(join_result) = quic_tasks.join_next().await {
                        if let Err(join_err) = join_result {
                            error!(
                                "[Frame {}] QUIC broadcast task join error: {}",
                                current_frame, join_err
                            );
                        }
                    }
                }
            }

            while let Some(join_result) = quic_tasks.join_next().await {
                if let Err(join_err) = join_result {
                    error!(
                        "[Frame {}] QUIC broadcast task join error: {}",
                        current_frame, join_err
                    );
                }
            }
        }

        debug!(
            "[Frame {}] Broadcast processing loop complete.",
            current_frame
        );
    }

    fn build_initial_state_optimized(
        &self,
        peer_id_str: &str,
        shared_data: &SharedBroadcastData, // Used for timestamp, match_info, kill_feed
    ) -> Result<Bytes, Box<dyn std::error::Error + Send + Sync>> {
        const MAX_MESSAGE_SIZE_BYTES: usize = 160000; // Slightly less than 64KB

        let mut builder = flatbuffers::FlatBufferBuilder::with_capacity(32768);
        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        let snapshot_caps = shared_data.initial_snapshot_caps;
        debug!(
            "[Frame {}] Client {}: Building InitialStateMessage.",
            frame, peer_id_str
        );

        let self_player_id_arc = self.player_manager.id_pool.get_or_create(peer_id_str);

        // 1. Walls: Reuse per-broadcast active wall snapshot when available.
        let active_walls_to_send: Cow<'_, [Wall]> = if shared_data.active_walls_snapshot.is_empty()
        {
            let mut fallback_walls: Vec<Wall> =
                shared_data.active_walls_by_id.values().cloned().collect();
            fallback_walls.sort_by_key(|wall| wall.id);
            Cow::Owned(fallback_walls)
        } else {
            Cow::Borrowed(shared_data.active_walls_snapshot.as_slice())
        };

        debug!(
            "[Frame {} Client {}] InitialState: Collected {} active walls.",
            frame,
            peer_id_str,
            active_walls_to_send.len()
        );

        let mut walls_fb_vec =
            Vec::with_capacity(active_walls_to_send.len().min(snapshot_caps.max_walls));
        for wall_data in active_walls_to_send.iter().take(snapshot_caps.max_walls) {
            let id_fb = fb_safe_entity_id(&mut builder, wall_data.id);
            walls_fb_vec.push(fb::Wall::create(
                &mut builder,
                &fb::WallArgs {
                    id: Some(id_fb),
                    x: wall_data.x,
                    y: wall_data.y,
                    width: wall_data.width,
                    height: wall_data.height,
                    is_destructible: wall_data.is_destructible,
                    current_health: wall_data.current_health,
                    max_health: wall_data.max_health,
                },
            ));
        }
        let walls_fb = builder.create_vector(&walls_fb_vec);
        debug!(
            "[Frame {} Client {}] InitialState: Serialized {} walls.",
            frame,
            peer_id_str,
            walls_fb_vec.len()
        );

        // 2. Player States (Self + AoI)
        let mut players_fb_vec = Vec::new();
        let mut player_aoi_data_for_initial_state = Self::get_empty_player_aoi();

        if let Some(self_pstate_guard) =
            Self::lookup_player_state_from_shared(shared_data, &self_player_id_arc)
        {
            players_fb_vec.push(create_fb_player_state_for_delta(
                &mut builder,
                self_pstate_guard,
                0xFFFF,
            ));
            player_aoi_data_for_initial_state =
                self.resolve_player_aoi_for_player(shared_data, &self_player_id_arc);
        } else {
            warn!(
                "[Frame {} Client {}] InitialState: Self player state not found!",
                frame, peer_id_str
            );
        }

        for visible_player_id in player_aoi_data_for_initial_state
            .visible_players
            .iter()
            .take(
                snapshot_caps
                    .max_players
                    .saturating_sub(players_fb_vec.len()),
            )
        {
            if visible_player_id != &self_player_id_arc {
                // Already added self
                if let Some(pstate_guard) =
                    Self::lookup_player_state_from_shared(shared_data, visible_player_id)
                {
                    players_fb_vec.push(create_fb_player_state_for_delta(
                        &mut builder,
                        pstate_guard,
                        0xFFFF,
                    ));
                }
            }
        }
        let players_fb = builder.create_vector(&players_fb_vec);
        debug!(
            "[Frame {} Client {}] InitialState: Serialized {} player states.",
            frame,
            peer_id_str,
            players_fb_vec.len()
        );

        // 3. Projectiles (from AoI)
        let mut projectiles_fb_vec = Vec::new();
        for proj_id in player_aoi_data_for_initial_state
            .visible_projectiles
            .iter()
            .take(snapshot_caps.max_projectiles)
        {
            if let Some(proj) = Self::lookup_projectile_from_shared(shared_data, proj_id) {
                let id_fb = fb_safe_entity_id(&mut builder, proj.id);
                let owner_id_fb = fb_safe_str(&mut builder, proj.owner_id.as_str());
                projectiles_fb_vec.push(fb::ProjectileState::create(
                    &mut builder,
                    &fb::ProjectileStateArgs {
                        id: Some(id_fb),
                        x: proj.x,
                        y: proj.y,
                        owner_id: Some(owner_id_fb),
                        weapon_type: map_server_weapon_to_fb(proj.weapon_type),
                        velocity_x: proj.velocity_x,
                        velocity_y: proj.velocity_y,
                    },
                ));
            }
        }
        let projectiles_fb = builder.create_vector(&projectiles_fb_vec);
        debug!(
            "[Frame {} Client {}] InitialState: Serialized {} projectiles.",
            frame,
            peer_id_str,
            projectiles_fb_vec.len()
        );

        // 4. Pickups (Active ones from AoI)
        let mut pickups_fb_vec = Vec::new();
        for pickup_id in player_aoi_data_for_initial_state
            .visible_pickups
            .iter()
            .take(snapshot_caps.max_pickups)
        {
            if let Some(pickup) = Self::lookup_pickup_from_shared(shared_data, pickup_id) {
                if pickup.is_active {
                    // Only send active pickups
                    let (fb_pickup_type, fb_weapon_type_opt) =
                        map_core_pickup_to_fb(&pickup.pickup_type);
                    let id_fb = fb_safe_entity_id(&mut builder, pickup.id);
                    pickups_fb_vec.push(fb::Pickup::create(
                        &mut builder,
                        &fb::PickupArgs {
                            id: Some(id_fb),
                            x: pickup.x,
                            y: pickup.y,
                            pickup_type: fb_pickup_type,
                            weapon_type: fb_weapon_type_opt.unwrap_or(fb::WeaponType::Pistol),
                            is_active: pickup.is_active,
                        },
                    ));
                }
            }
        }
        let pickups_fb = builder.create_vector(&pickups_fb_vec);
        debug!(
            "[Frame {} Client {}] InitialState: Serialized {} active pickups.",
            frame,
            peer_id_str,
            pickups_fb_vec.len()
        );

        // 5. Match Info (from shared_data snapshot)
        let match_snapshot = &shared_data.match_info_snapshot;
        let fb_team_scores_vec: Vec<_> = match_snapshot
            .team_scores
            .iter()
            .map(|(team_id, score)| {
                fb::TeamScoreEntry::create(
                    &mut builder,
                    &fb::TeamScoreEntryArgs {
                        team_id: *team_id as i8,
                        score: *score,
                    },
                )
            })
            .collect();
        let team_scores_fb = builder.create_vector(&fb_team_scores_vec);

        let match_info_fb = fb::MatchInfo::create(
            &mut builder,
            &fb::MatchInfoArgs {
                time_remaining: match_snapshot.time_remaining,
                match_state: match_snapshot.match_state,
                winner_id: None, // Typically not known at initial state
                winner_name: None,
                game_mode: match_snapshot.game_mode,
                team_scores: Some(team_scores_fb),
            },
        );

        // 6. Flag States (from shared_data snapshot)
        let fb_flag_states_vec: Vec<_> = match_snapshot
            .flag_states
            .values()
            .map(|fs| {
                let carrier_id_fb = fs
                    .carrier_id
                    .as_ref()
                    .map(|id| fb_safe_str(&mut builder, id.as_str()));
                let pos_fb = fb::Vec2::create(
                    &mut builder,
                    &fb::Vec2Args {
                        x: fs.position.x,
                        y: fs.position.y,
                    },
                );
                fb::FlagState::create(
                    &mut builder,
                    &fb::FlagStateArgs {
                        team_id: fs.team_id as i8,
                        status: fs.status,
                        position: Some(pos_fb),
                        carrier_id: carrier_id_fb,
                        respawn_timer: fs.respawn_timer,
                    },
                )
            })
            .collect();
        let flag_states_fb = builder.create_vector(&fb_flag_states_vec);

        // 7. Map Name
        let map_name_fb = fb_safe_str(&mut builder, &self.map_name);

        // 8. Timestamp (from shared_data)
        let timestamp_initial = shared_data.timestamp_ms;

        // 9. Player ID for the message
        let player_id_fb_initial = fb_safe_str(&mut builder, peer_id_str);

        // Create InitialStateMessage
        let initial_state_args = fb::InitialStateMessageArgs {
            player_id: Some(player_id_fb_initial),
            walls: Some(walls_fb),
            players: Some(players_fb),
            projectiles: Some(projectiles_fb),
            pickups: Some(pickups_fb),
            match_info: Some(match_info_fb),
            flag_states: Some(flag_states_fb),
            timestamp: timestamp_initial,
            map_name: Some(map_name_fb),
        };
        let initial_state_msg = fb::InitialStateMessage::create(&mut builder, &initial_state_args);

        // Wrap in GameMessage
        let game_msg_args = fb::GameMessageArgs {
            msg_type: fb::MessageType::InitialState,
            actual_message_type: fb::MessagePayload::InitialStateMessage,
            actual_message: Some(initial_state_msg.as_union_value()),
            protocol_version: GAME_PROTOCOL_VERSION,
        };
        let game_msg = fb::GameMessage::create(&mut builder, &game_msg_args);
        builder.finish(game_msg, None);

        let finished_len = builder.finished_data().len();
        debug!(
            "[Frame {} Client {}] InitialStateMessage built. Size: {} bytes.",
            frame, peer_id_str, finished_len
        );

        if finished_len > MAX_MESSAGE_SIZE_BYTES {
            return Err("Initial state too large".into());
        }

        let (buffer, root_index) = builder.collapse();
        Ok(Bytes::from(buffer).slice(root_index..))
    }

    pub(crate) fn build_match_info_only_bytes(&self) -> Bytes {
        let mut builder = flatbuffers::FlatBufferBuilder::with_capacity(2048);
        let match_info_guard = self.match_info.read();
        let team_scores_vec: Vec<_> = match_info_guard
            .team_scores
            .iter()
            .map(|(team_id, score)| {
                fb::TeamScoreEntry::create(
                    &mut builder,
                    &fb::TeamScoreEntryArgs {
                        team_id: *team_id as i8,
                        score: *score,
                    },
                )
            })
            .collect();
        let team_scores_fb = builder.create_vector(&team_scores_vec);
        let match_info_fb = fb::MatchInfo::create(
            &mut builder,
            &fb::MatchInfoArgs {
                time_remaining: match_info_guard.time_remaining,
                match_state: match_info_guard.match_state,
                winner_id: None,
                winner_name: None,
                game_mode: match_info_guard.game_mode,
                team_scores: Some(team_scores_fb),
            },
        );
        drop(match_info_guard);

        let delta_state_args = fb::DeltaStateMessageArgs {
            players: None,
            projectiles: None,
            removed_projectiles: None,
            pickups: None,
            deactivated_pickup_ids: None,
            game_events: None,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            last_processed_input_sequence: 0,
            changed_player_fields: None,
            kill_feed: None,
            match_info: Some(match_info_fb),
            destroyed_wall_ids: None,
            flag_states: None,
            removed_player_ids: None,
            updated_walls: None,
        };
        let delta_state_msg = fb::DeltaStateMessage::create(&mut builder, &delta_state_args);
        let game_msg = fb::GameMessage::create(
            &mut builder,
            &fb::GameMessageArgs {
                msg_type: fb::MessageType::DeltaState,
                actual_message_type: fb::MessagePayload::DeltaStateMessage,
                actual_message: Some(delta_state_msg.as_union_value()),
                protocol_version: GAME_PROTOCOL_VERSION,
            },
        );
        builder.finish(game_msg, None);
        let (buffer, root_index) = builder.collapse();
        Bytes::from(buffer).slice(root_index..)
    }

    pub(crate) async fn send_match_info_only(
        &self,
        peer_id_str: &str,
        data_channel: &Arc<crate::core::types::RTCDataChannel>,
    ) {
        let data_bytes = self.build_match_info_only_bytes();
        let sent_packets = self
            .send_packet_batch_optimized(data_channel, &[data_bytes], 80)
            .await;
        if sent_packets == 0 {
            warn!("[{}] Failed to send match_info-only delta.", peer_id_str);
        } else {
            info!(
                "[{}] Sent match_info-only delta to unblock client.",
                peer_id_str
            );
        }
    }

    pub async fn process_game_tick(self: Arc<Self>, dt: f32) -> Result<(), ServerError> {
        let tick_started = Instant::now();
        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        let has_connected_clients =
            !self.data_channels_map.is_empty() || connected_quic_peer_count() > 0;

        // Stage 1: Input & AI (Potentially parallelizable)
        let stage1_start = Instant::now();
        let mut set = JoinSet::new();

        set.spawn({
            let server_clone = Arc::clone(&self);
            async move {
                let task_name = "network_input";
                trace!("[Frame {}] Starting task: {}", frame, task_name);
                let result = timeout(Duration::from_millis(NET_IO_TIMEOUT_MS), async {
                    server_clone.process_network_input().await;
                })
                .await;
                if result.is_err() {
                    if frame % 60 == 0 {
                        warn!(
                            "[Frame {}] Task '{}' timed out after {}ms",
                            frame, task_name, NET_IO_TIMEOUT_MS
                        );
                    }
                }
                trace!("[Frame {}] Finished task: {}", frame, task_name);
            }
        });

        let ai_stride = if has_connected_clients {
            AI_UPDATE_STRIDE
        } else {
            AI_UPDATE_STRIDE * 3
        };
        if frame % ai_stride == 0 {
            set.spawn({
                let server_clone = Arc::clone(&self);
                async move {
                    let task_name = "ai_update";
                    trace!("[Frame {}] Starting task: {}", frame, task_name);
                    let result = timeout(Duration::from_millis(AI_TIMEOUT_MS), async {
                        server_clone.run_ai_update().await;
                    })
                    .await;
                    if result.is_err() {
                        if frame % 60 == 0 {
                            warn!(
                                "[Frame {}] Task '{}' timed out after {}ms",
                                frame, task_name, AI_TIMEOUT_MS
                            );
                        }
                    }
                    trace!("[Frame {}] Finished task: {}", frame, task_name);
                }
            });
        }

        while let Some(res) = set.join_next().await {
            if let Err(e) = res {
                error!("[Frame {}] Task join error in Stage 1: {}", frame, e);
            }
        }
        let stage1_elapsed = stage1_start.elapsed();
        trace!(
            "[Frame {}] Stage 1 (Input/AI) took: {:?}",
            frame,
            stage1_elapsed
        );

        // Stage 2: Physics & Game Logic (Sequential, mutation-heavy)
        let stage2_start = Instant::now();
        self.maybe_refresh_navigation_mesh();

        let physics_start = Instant::now();
        self.run_physics_update(dt).await;
        let physics_elapsed = physics_start.elapsed();
        trace!(
            "[Frame {}] Physics update took: {:?}",
            frame,
            physics_elapsed
        );

        let game_logic_start = Instant::now();
        self.run_game_logic_update(dt).await;
        let game_logic_elapsed = game_logic_start.elapsed();
        trace!(
            "[Frame {}] Game logic update took: {:?}",
            frame,
            game_logic_elapsed
        );

        let stage2_elapsed = stage2_start.elapsed();
        if stage2_elapsed > Duration::from_millis(SLOW_TICK_LOG_MS) && frame % 60 == 0 {
            warn!(
                ?frame,
                ms = stage2_elapsed.as_micros() as f64 / 1000.0,
                physics_ms = physics_elapsed.as_micros() as f64 / 1000.0,
                game_logic_ms = game_logic_elapsed.as_micros() as f64 / 1000.0,
                "Stage 2 (Physics/Logic) exceeded soft budget {}ms",
                SLOW_TICK_LOG_MS
            );
        }

        let should_rebuild_ecs = self.ecs_bridge.is_enabled()
            && (self.ecs_bridge.is_authoritative()
                || frame % self.ecs_bridge.rebuild_stride_frames() == 0);
        if should_rebuild_ecs {
            let projectiles_snapshot = self.projectiles.read().clone();
            let pickups_snapshot = self.pickups.read().clone();
            let ecs_stats = self.ecs_bridge.rebuild_snapshot(
                self.player_manager.as_ref(),
                &projectiles_snapshot,
                &pickups_snapshot,
            );
            if ecs_stats.skipped_contention {
                trace!("[Frame {}] ECS snapshot rebuild skipped due to lock contention.", frame);
            } else {
                trace!(
                    "[Frame {}] ECS snapshot rebuilt: players={}, projectiles={}, pickups={}",
                    frame,
                    ecs_stats.players,
                    ecs_stats.projectiles,
                    ecs_stats.pickups
                );
            }

            if self.ecs_bridge.is_authoritative() {
                let mut projectiles = self.projectiles.write();
                let mut pickups = self.pickups.write();
                let reconciled = self.ecs_bridge.apply_authoritative_reconciliation(
                    self.player_manager.as_ref(),
                    projectiles.as_mut_slice(),
                    pickups.as_mut_slice(),
                );
                if reconciled.skipped_contention {
                    trace!(
                        "[Frame {}] ECS authoritative reconciliation skipped due to lock contention.",
                        frame
                    );
                } else {
                    trace!(
                        "[Frame {}] ECS authoritative reconciliation applied: players={}, projectiles={}, pickups={}",
                        frame,
                        reconciled.players,
                        reconciled.projectiles,
                        reconciled.pickups
                    );
                }
            }
        }

        // Stage 3: State Sync & Broadcast
        let stage3_start = Instant::now();

        let sync_start = Instant::now();
        self.synchronize_state(has_connected_clients).await;
        // AoI is refreshed during synchronize_state, so publish its snapshot afterwards to keep
        // broadcast reads on the latest authoritative frame.
        self.publish_player_aoi_snapshot_if_enabled();
        let sync_elapsed = sync_start.elapsed();
        trace!(
            "[Frame {}] State synchronization took: {:?}",
            frame,
            sync_elapsed
        );

        let broadcast_start_time = Instant::now();
        let broadcast_elapsed_duration;
        let broadcast_timed_out_flag;
        if has_connected_clients {
            let server_for_broadcast_call = Arc::clone(&self);
            let broadcast_future = server_for_broadcast_call.broadcast_world_updates_optimized();

            let timed_broadcast_future =
                tokio::time::timeout(Duration::from_millis(FAN_OUT_TIMEOUT_MS), broadcast_future);

            let b_start_inner = Instant::now();
            broadcast_timed_out_flag = timed_broadcast_future.await.is_err();
            broadcast_elapsed_duration = b_start_inner.elapsed();
        } else {
            broadcast_elapsed_duration = broadcast_start_time.elapsed();
            broadcast_timed_out_flag = false;
        }

        trace!(
            "[Frame {}] Broadcast took: {:?} (timed_out: {})",
            frame,
            broadcast_elapsed_duration,
            broadcast_timed_out_flag
        );

        if broadcast_timed_out_flag {
            if frame % 60 == 0 {
                error!(
                    "[Frame {}] Broadcast stage timed out after {}ms (actual: {:?})",
                    frame, FAN_OUT_TIMEOUT_MS, broadcast_elapsed_duration
                );
            }
        }
        let _stage3_elapsed = stage3_start.elapsed();
        self.capture_live_replay_frame(frame);

        // Stage 4: Cleanup
        self.destroyed_wall_ids_this_tick.write().clear();
        self.updated_walls_this_tick.write().clear();
        trace!("[Frame {}] Tick-local cleanup complete.", frame);

        let total_tick_processing_elapsed = tick_started.elapsed();

        if total_tick_processing_elapsed > Duration::from_millis(TARGET_TICK_MS + 4) {
            if frame % 10 == 0 {
                warn!(
                    "Frame {} timing breakdown:\n\
                     Total: {:.2}ms\n\
                     - Input/AI (Stage 1): {:.2}ms\n\
                     - Physics (Stage 2a): {:.2}ms\n\
                     - Game Logic (Stage 2b): {:.2}ms\n\
                     - State Sync (Stage 3a): {:.2}ms\n\
                     - Broadcast (Stage 3b): {:.2}ms (timed_out: {})\n\
                     (Target Tick: {}ms)",
                    frame,
                    total_tick_processing_elapsed.as_secs_f32() * 1000.0,
                    stage1_elapsed.as_secs_f32() * 1000.0,
                    physics_elapsed.as_secs_f32() * 1000.0,
                    game_logic_elapsed.as_secs_f32() * 1000.0,
                    sync_elapsed.as_secs_f32() * 1000.0,
                    broadcast_elapsed_duration.as_secs_f32() * 1000.0,
                    broadcast_timed_out_flag,
                    TARGET_TICK_MS
                );
            }
        }

        if total_tick_processing_elapsed > Duration::from_millis(TARGET_TICK_MS) {
            if frame % 60 == 0 {
                warn!(
                    ?frame,
                    ms = total_tick_processing_elapsed.as_micros() as f64 / 1000.0,
                    target = TARGET_TICK_MS,
                    "Tick processing WORK exceeded hard budget (game_loop will log wall-clock overrun)"
                );
            }
        }

        Ok(())
    }

    pub(crate) async fn send_packet_batch_optimized(
        &self,
        data_channel: &Arc<crate::core::types::RTCDataChannel>,
        packets: &[Bytes],
        timeout_ms: u64,
    ) -> usize {
        send_packet_batch_over_channel(data_channel, packets, timeout_ms).await
    }

    async fn send_chat_messages_optimized(
        &self,
        data_channel: &Arc<crate::core::types::RTCDataChannel>,
        last_seq_sent: u64,
        chat_packets: &[SerializedChatPacket],
    ) -> u64 {
        const CHAT_PACKET_TIMEOUT_MS: u64 = 30;

        let packets_to_send: Vec<&SerializedChatPacket> = chat_packets
            .iter()
            .filter(|packet| packet.seq > last_seq_sent)
            .take(MAX_CHAT_PER_BATCH)
            .collect();
        if packets_to_send.is_empty() {
            return last_seq_sent;
        }

        // Bytes clones are ref-counted and avoid re-serializing chat payloads per client.
        let serialized_packets: Vec<Bytes> = packets_to_send
            .iter()
            .map(|packet| packet.bytes.clone())
            .collect();
        let sent_packets = self
            .send_packet_batch_optimized(data_channel, &serialized_packets, CHAT_PACKET_TIMEOUT_MS)
            .await;

        let mut max_seq_in_batch = last_seq_sent;
        for packet in packets_to_send.iter().take(sent_packets) {
            if packet.seq > max_seq_in_batch {
                max_seq_in_batch = packet.seq;
            }
        }
        max_seq_in_batch
    }
}
