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
mod broadcast_dispatch;
mod broadcast_loop;
mod broadcast_prep;
mod broadcast_state;
mod combat_melee;
mod constants;
mod game_modes;
mod join_stage;
mod match_info;
mod navigation_mesh;
mod physics;
mod replay;
mod serialization;
mod tick;
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

    pub(crate) async fn send_packet_batch_optimized(
        &self,
        data_channel: &Arc<crate::core::types::RTCDataChannel>,
        packets: &[Bytes],
        timeout_ms: u64,
    ) -> usize {
        send_packet_batch_over_channel(data_channel, packets, timeout_ms).await
    }

}
