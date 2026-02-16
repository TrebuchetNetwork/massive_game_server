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
use crate::network::signaling::ChatMessage;
use crate::network::signaling::PickupState;
use crate::network::signaling::{
    next_chat_message_seq, ChatMessagesQueue, ClientState, ClientStatesMap, DataChannelsMap,
};
use crate::server::event_mapping::{
    event_instigator_id, event_position, event_target_id, event_value, event_weapon_type,
    map_game_event_type_to_fb,
};
use crate::server::pickup_pipeline::{
    apply_pickup_effect, collect_pickup_candidates, PickupCollectionCandidate,
};
use crate::systems::ai::bot_ai::BotAISystem;
use crate::systems::ai::optimized_bot_ai::OptimizedBotAI;
use crate::systems::respawn::{RespawnManager, WallRespawnManager};
use crate::world::map_generator::MapGenerator;
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
use itoa::Buffer as ItoaBuffer;
use once_cell::sync::OnceCell;
use parking_lot::RwLock as ParkingLotRwLock;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rayon::prelude::*;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::time::sleep; // Add this import
use uuid::Uuid;
// In src/server/instance.rs
use tracing::{debug, error, info, trace, warn}; // Ensure all levels are available

use tokio::{task::JoinSet, time::timeout};

const INITIAL_SNAPSHOT_MAX_PLAYERS: usize = 24;
const INITIAL_SNAPSHOT_MAX_WALLS: usize = 128;
const INITIAL_SNAPSHOT_MAX_PROJECTILES: usize = 200;
const INITIAL_SNAPSHOT_MAX_PICKUPS: usize = 24;
const INITIAL_SNAPSHOT_TAIL_MAX_PLAYERS: usize = 16;
const INITIAL_SNAPSHOT_TAIL_MAX_WALLS: usize = 96;
const INITIAL_SNAPSHOT_TAIL_MAX_PROJECTILES: usize = 120;
const INITIAL_SNAPSHOT_TAIL_MAX_PICKUPS: usize = 16;
const INITIAL_SNAPSHOT_TAIL_AGGRESSIVE_MAX_PLAYERS: usize = 12;
const INITIAL_SNAPSHOT_TAIL_AGGRESSIVE_MAX_WALLS: usize = 72;
const INITIAL_SNAPSHOT_TAIL_AGGRESSIVE_MAX_PROJECTILES: usize = 80;
const INITIAL_SNAPSHOT_TAIL_AGGRESSIVE_MAX_PICKUPS: usize = 12;
const INITIAL_SNAPSHOT_EXTREME_TAIL_MAX_PLAYERS: usize = 10;
const INITIAL_SNAPSHOT_EXTREME_TAIL_MAX_WALLS: usize = 56;
const INITIAL_SNAPSHOT_EXTREME_TAIL_MAX_PROJECTILES: usize = 56;
const INITIAL_SNAPSHOT_EXTREME_TAIL_MAX_PICKUPS: usize = 10;
const INITIAL_SNAPSHOT_SINGLE_MACHINE_BACKLOG_MAX_PLAYERS: usize = 14;
const INITIAL_SNAPSHOT_SINGLE_MACHINE_BACKLOG_MAX_WALLS: usize = 84;
const INITIAL_SNAPSHOT_SINGLE_MACHINE_BACKLOG_MAX_PROJECTILES: usize = 96;
const INITIAL_SNAPSHOT_SINGLE_MACHINE_BACKLOG_MAX_PICKUPS: usize = 14;
const MAX_CHAT_PER_BATCH: usize = 10;

#[derive(Clone, Copy, Debug)]
struct InitialSnapshotCaps {
    max_players: usize,
    max_walls: usize,
    max_projectiles: usize,
    max_pickups: usize,
}

impl InitialSnapshotCaps {
    const DEFAULT: Self = Self {
        max_players: INITIAL_SNAPSHOT_MAX_PLAYERS,
        max_walls: INITIAL_SNAPSHOT_MAX_WALLS,
        max_projectiles: INITIAL_SNAPSHOT_MAX_PROJECTILES,
        max_pickups: INITIAL_SNAPSHOT_MAX_PICKUPS,
    };

    const TAIL: Self = Self {
        max_players: INITIAL_SNAPSHOT_TAIL_MAX_PLAYERS,
        max_walls: INITIAL_SNAPSHOT_TAIL_MAX_WALLS,
        max_projectiles: INITIAL_SNAPSHOT_TAIL_MAX_PROJECTILES,
        max_pickups: INITIAL_SNAPSHOT_TAIL_MAX_PICKUPS,
    };

    const TAIL_AGGRESSIVE: Self = Self {
        max_players: INITIAL_SNAPSHOT_TAIL_AGGRESSIVE_MAX_PLAYERS,
        max_walls: INITIAL_SNAPSHOT_TAIL_AGGRESSIVE_MAX_WALLS,
        max_projectiles: INITIAL_SNAPSHOT_TAIL_AGGRESSIVE_MAX_PROJECTILES,
        max_pickups: INITIAL_SNAPSHOT_TAIL_AGGRESSIVE_MAX_PICKUPS,
    };

    const EXTREME_TAIL: Self = Self {
        max_players: INITIAL_SNAPSHOT_EXTREME_TAIL_MAX_PLAYERS,
        max_walls: INITIAL_SNAPSHOT_EXTREME_TAIL_MAX_WALLS,
        max_projectiles: INITIAL_SNAPSHOT_EXTREME_TAIL_MAX_PROJECTILES,
        max_pickups: INITIAL_SNAPSHOT_EXTREME_TAIL_MAX_PICKUPS,
    };

    const SINGLE_MACHINE_BACKLOG: Self = Self {
        max_players: INITIAL_SNAPSHOT_SINGLE_MACHINE_BACKLOG_MAX_PLAYERS,
        max_walls: INITIAL_SNAPSHOT_SINGLE_MACHINE_BACKLOG_MAX_WALLS,
        max_projectiles: INITIAL_SNAPSHOT_SINGLE_MACHINE_BACKLOG_MAX_PROJECTILES,
        max_pickups: INITIAL_SNAPSHOT_SINGLE_MACHINE_BACKLOG_MAX_PICKUPS,
    };
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

struct ClientInfo {
    data_channel: Arc<crate::core::types::RTCDataChannel>,
    needs_initial_state: bool,
}

#[derive(Clone, Debug)]
pub struct BotController {
    pub player_id: PlayerID,
    pub target_position: Option<Vec2>,
    pub target_enemy_id: Option<PlayerID>,
    pub last_decision_time: Instant,
    pub behavior_state: BotBehaviorState,
    pub current_path: VecDeque<Vec2>,
    pub path_recalculation_timer: Instant,
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

#[derive(Debug)]
struct ProjectileResults {
    total_processed: usize,
    hits: Vec<(PlayerID, PlayerID, i32, ServerWeaponType)>, // (attacker, target, damage, weapon)
    wall_hits: Vec<(EntityId, i32)>,                        // (wall_id, damage)
    removed_projectile_ids: Vec<EntityId>,
    kept_projectiles: Vec<Projectile>,
    spatial_updates: Vec<(EntityId, f32, f32)>,
    wall_impacts: Vec<GameEvent>,
}

#[derive(Default)]
struct ProjectileChunkResults {
    to_remove: Vec<usize>,
    hits: Vec<(PlayerID, PlayerID, i32, ServerWeaponType)>,
    wall_hits: Vec<(EntityId, i32)>,
    spatial_updates: Vec<(EntityId, f32, f32)>,
    wall_impacts: Vec<GameEvent>,
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

#[derive(Debug)]
struct PlayerPhysicsResults {
    players_to_respawn: Vec<(PlayerID, u8)>, // (player_id, team_id)
    alive_count: usize,
}

// Helper functions (assuming these are already defined as per your project structure)
fn map_server_weapon_to_fb(server_weapon: ServerWeaponType) -> fb::WeaponType {
    match server_weapon {
        ServerWeaponType::Pistol => fb::WeaponType::Pistol,
        ServerWeaponType::Shotgun => fb::WeaponType::Shotgun,
        ServerWeaponType::Rifle => fb::WeaponType::Rifle,
        ServerWeaponType::Sniper => fb::WeaponType::Sniper,
        ServerWeaponType::Melee => fb::WeaponType::Melee,
    }
}

fn map_core_pickup_to_fb(core_type: &CorePickupType) -> (fb::PickupType, Option<fb::WeaponType>) {
    match core_type {
        CorePickupType::Health => (fb::PickupType::Health, None),
        CorePickupType::Ammo => (fb::PickupType::Ammo, None),
        CorePickupType::WeaponCrate(server_weapon_type) => (
            fb::PickupType::WeaponCrate,
            Some(map_server_weapon_to_fb(*server_weapon_type)),
        ),
        CorePickupType::SpeedBoost => (fb::PickupType::SpeedBoost, None),
        CorePickupType::DamageBoost => (fb::PickupType::DamageBoost, None),
        CorePickupType::Shield => (fb::PickupType::Shield, None),
    }
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

#[derive(Clone, Debug, Default)]
struct JoinStageTrace {
    join_sequence: u64,
    first_seen_ms: u64,
    first_channel_open_ms: Option<u64>,
    first_build_start_ms: Option<u64>,
    first_build_done_ms: Option<u64>,
    first_send_start_ms: Option<u64>,
    first_send_result_ms: Option<u64>,
    first_send_failure_ms: Option<u64>,
    first_send_done_ms: Option<u64>,
    build_attempts: u32,
    send_attempts: u32,
    retry_count: u32,
    retry_interval_total_ms: u64,
    retry_interval_samples: u32,
    last_retry_at_ms: Option<u64>,
    completed_ms: Option<u64>,
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

const JOIN_STAGE_WAVES: [(&str, &str, u64, Option<u64>); 4] = [
    ("wave_1_24", "1-24", 1, Some(24)),
    ("wave_25_48", "25-48", 25, Some(48)),
    ("wave_49_72", "49-72", 49, Some(72)),
    ("wave_73_plus", "73+", 73, None),
];

#[inline]
fn fb_safe_str<'b>(
    builder: &mut flatbuffers::FlatBufferBuilder<'b>,
    s: &str,
) -> flatbuffers::WIPOffset<&'b str> {
    // Rust strings are UTF-8. Flatbuffers create_string expects valid UTF-8.
    // The main concern could be embedded nulls if strings come from unsafe sources,
    // but Rust &str shouldn't have them.
    // For extreme safety or if data might come from FFI with potential nulls:
    // if s.contains('\0') {
    //     let cleaned_s: String = s.chars().filter(|&c| c != '\0').collect();
    //     return builder.create_string(&cleaned_s);
    // }
    builder.create_string(s)
}

#[inline]
fn fb_safe_entity_id<'b>(
    builder: &mut flatbuffers::FlatBufferBuilder<'b>,
    id: EntityId,
) -> flatbuffers::WIPOffset<&'b str> {
    let mut buf = ItoaBuffer::new();
    builder.create_string(buf.format(id))
}

fn create_fb_player_state_for_delta<'a>(
    builder: &mut flatbuffers::FlatBufferBuilder<'a>,
    pstate: &PlayerState,
    changed_fields: u16,
) -> flatbuffers::WIPOffset<fb::PlayerState<'a>> {
    let is_full_state = changed_fields == 0xFFFF || changed_fields == u8::MAX as u16;
    let has_position_delta = is_full_state || (changed_fields & FIELD_POSITION_ROTATION) != 0;
    let has_health_delta = is_full_state || (changed_fields & FIELD_HEALTH_ALIVE) != 0;
    let has_weapon_delta = is_full_state || (changed_fields & FIELD_WEAPON_AMMO) != 0;
    let has_score_delta = is_full_state || (changed_fields & FIELD_SCORE_STATS) != 0;
    let has_powerup_delta = is_full_state || (changed_fields & FIELD_POWERUPS) != 0;
    let has_shield_delta = is_full_state || (changed_fields & FIELD_SHIELD) != 0;
    let has_flag_delta = is_full_state || (changed_fields & FIELD_FLAG) != 0;

    let id_fb = fb_safe_str(builder, pstate.id.as_str());
    let username_fb = if is_full_state || has_score_delta {
        Some(fb_safe_str(builder, &pstate.username))
    } else {
        None
    };
    let weapon_fb = if has_weapon_delta {
        map_server_weapon_to_fb(pstate.weapon)
    } else {
        fb::WeaponType::Pistol
    };

    fb::PlayerState::create(
        builder,
        &fb::PlayerStateArgs {
            id: Some(id_fb),
            username: username_fb,
            x: if has_position_delta { pstate.x } else { 0.0 },
            y: if has_position_delta { pstate.y } else { 0.0 },
            rotation: if has_position_delta {
                pstate.rotation
            } else {
                0.0
            },
            velocity_x: if has_position_delta {
                pstate.velocity_x
            } else {
                0.0
            },
            velocity_y: if has_position_delta {
                pstate.velocity_y
            } else {
                0.0
            },
            health: if has_health_delta { pstate.health } else { 0 },
            max_health: if has_health_delta {
                pstate.max_health
            } else {
                0
            },
            alive: if has_health_delta {
                pstate.alive
            } else {
                false
            },
            respawn_timer: if has_health_delta {
                pstate.respawn_timer.unwrap_or(-1.0)
            } else {
                0.0
            },
            weapon: weapon_fb,
            ammo: if has_weapon_delta { pstate.ammo } else { 0 },
            reload_progress: if has_weapon_delta {
                pstate.reload_progress.unwrap_or(-1.0)
            } else {
                0.0
            },
            score: if has_score_delta { pstate.score } else { 0 },
            kills: if has_score_delta { pstate.kills } else { 0 },
            deaths: if has_score_delta { pstate.deaths } else { 0 },
            team_id: if has_score_delta {
                pstate.team_id as i8
            } else {
                0
            },
            speed_boost_remaining: if has_powerup_delta {
                pstate.speed_boost_remaining
            } else {
                0.0
            },
            damage_boost_remaining: if has_powerup_delta {
                pstate.damage_boost_remaining
            } else {
                0.0
            },
            shield_current: if has_shield_delta {
                pstate.shield_current
            } else {
                0
            },
            shield_max: if has_shield_delta {
                pstate.shield_max
            } else {
                0
            },
            is_carrying_flag_team_id: if has_flag_delta {
                pstate.is_carrying_flag_team_id as i8
            } else {
                0
            },
        },
    )
}

fn build_chat_game_message_bytes(chat_entry: &ChatMessage) -> Bytes {
    let mut chat_builder = flatbuffers::FlatBufferBuilder::with_capacity(256);

    let player_id_fb = chat_builder.create_string(chat_entry.player_id.as_str());
    let username_fb = chat_builder.create_string(&chat_entry.username);
    let message_fb = chat_builder.create_string(&chat_entry.message);

    let chat_payload_offset = fb::ChatMessage::create(
        &mut chat_builder,
        &fb::ChatMessageArgs {
            seq: chat_entry.seq,
            player_id: Some(player_id_fb),
            username: Some(username_fb),
            message: Some(message_fb),
            timestamp: chat_entry.timestamp,
        },
    );

    let game_message_offset = fb::GameMessage::create(
        &mut chat_builder,
        &fb::GameMessageArgs {
            msg_type: fb::MessageType::Chat,
            actual_message_type: fb::MessagePayload::ChatMessage,
            actual_message: Some(chat_payload_offset.as_union_value()),
            protocol_version: GAME_PROTOCOL_VERSION,
        },
    );

    chat_builder.finish(game_message_offset, None);
    let (buffer, root_index) = chat_builder.collapse();
    Bytes::from(buffer).slice(root_index..)
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

#[derive(Clone)]
struct SerializedChatPacket {
    seq: u64,
    bytes: Bytes,
}

fn build_game_event_fb<'a>(
    builder: &mut flatbuffers::FlatBufferBuilder<'a>,
    event: &GameEvent,
) -> flatbuffers::WIPOffset<fb::GameEvent<'a>> {
    let event_pos = event_position(event);
    let pos_fb = fb::Vec2::create(
        builder,
        &fb::Vec2Args {
            x: event_pos.x,
            y: event_pos.y,
        },
    );
    let instigator_id_fb = event_instigator_id(event).map(|id| builder.create_string(id.as_str()));
    let target_id_fb = event_target_id(event).map(|id| builder.create_string(&id));
    let weapon_type_fb =
        event_weapon_type(event).map_or(fb::WeaponType::Pistol, map_server_weapon_to_fb);

    fb::GameEvent::create(
        builder,
        &fb::GameEventArgs {
            event_type: map_game_event_type_to_fb(event),
            position: Some(pos_fb),
            instigator_id: instigator_id_fb,
            target_id: target_id_fb,
            weapon_type: weapon_type_fb,
            value: event_value(event).unwrap_or(0.0),
        },
    )
}

// Shared data that's the same for all clients
#[derive(Clone)]
struct SharedBroadcastData {
    timestamp_ms: u64,
    events: Vec<GameEvent>,
    destroyed_wall_ids: Vec<EntityId>,
    updated_walls: HashMap<EntityId, Wall>,
    active_walls_by_id: HashMap<EntityId, Wall>,
    active_walls_snapshot: Vec<Wall>,
    player_aois_snapshot: Arc<HashMap<PlayerID, PlayerAoI>>,
    player_soa_snapshot: Arc<PlayerSoASnapshot>,
    player_states_snapshot: HashMap<PlayerID, PlayerState>,
    projectiles_soa_snapshot: Arc<ProjectileSoASnapshot>,
    pickups_soa_snapshot: Arc<PickupSoASnapshot>,
    projectiles_snapshot: Arc<HashMap<EntityId, Projectile>>,
    pickups_snapshot: Arc<HashMap<EntityId, Pickup>>,
    chat_packets: Vec<SerializedChatPacket>,
    match_info_snapshot: MatchInfoSnapshot,
    kill_feed_snapshot: Vec<ServerKillFeedEntry>,
    max_delta_events_per_client: usize,
    initial_snapshot_caps: InitialSnapshotCaps,
    tail_join_mode: bool,
    aggressive_tail_join_mode: bool,
    extreme_tail_join_mode: bool,
    use_aoi_snapshot: bool,
    soa_fallback_active: bool,
    use_soa_snapshot: bool,
    use_entity_soa_snapshot: bool,
}

// Lightweight match info snapshot
#[derive(Clone)]
struct MatchInfoSnapshot {
    time_remaining: f32,
    match_state: fb::MatchStateType,
    game_mode: fb::GameModeType,
    team_scores: HashMap<u8, i32>,
    flag_states: HashMap<u8, ServerFlagState>,
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
}

const MAX_KILL_FEED_HISTORY: usize = 10;
const MAX_CHAT_MESSAGES_HISTORY: usize = 50;
const MAX_MELEE_EVENTS_PER_TICK: usize = 200;
static CACHED_WALLS: OnceCell<Arc<ParkingLotRwLock<(u64, Vec<Wall>)>>> = OnceCell::new();

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
        };

        info!("MassiveGameServer initialized successfully.");
        server
    }

    pub fn request_shutdown(&self) {
        self.is_shutting_down.store(true, AtomicOrdering::SeqCst);
    }

    pub fn is_shutdown_requested(&self) -> bool {
        self.is_shutting_down.load(AtomicOrdering::Acquire)
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
        self.apply_player_updates(player_updates).await; //
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

        // Process all players using for_each_player_mut
        self.player_manager
            .for_each_player_mut(|player_id, player_state| {
                // Update timers
                player_state.update_timers(delta_time);

                if player_state.alive {
                    total_alive += 1;
                    // Process movement with optimized collision
                    self.process_player_movement_optimized(player_state, &wall_arc, delta_time);
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

    async fn apply_player_updates(&self, updates: PlayerPhysicsResults) {
        // Batch respawns
        for (player_id, team_id) in updates.players_to_respawn {
            let enemies = self.get_enemy_positions_for_team(team_id);
            let spawn_pos = self.respawn_manager.get_respawn_position(
                self,
                &player_id,
                Some(team_id),
                &enemies,
            );

            if let Some(mut p_state) = self.player_manager.get_player_state_mut(&player_id) {
                p_state.respawn(spawn_pos.x, spawn_pos.y);
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
                        target_ids.push(target_id);
                        target_xs.push(target_x);
                        target_ys.push(target_y);
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

    fn update_match_state_authoritative(&self, delta_time: f32) {
        let mut match_info_guard = self.match_info.write();
        let player_count = self.player_manager.player_count();

        match match_info_guard.match_state {
            fb::MatchStateType::Waiting => {
                if player_count >= MIN_PLAYERS_TO_START {
                    match_info_guard.match_state = fb::MatchStateType::Active;
                    match_info_guard.time_remaining = 300.0;
                    info!("Match starting! Mode: {:?}", match_info_guard.game_mode);
                    if match_info_guard.game_mode == fb::GameModeType::CaptureTheFlag {
                        self.initialize_ctf_flags(&mut match_info_guard);
                    }
                    self.player_manager.for_each_player_mut(|_id, p_state| {
                        p_state.score = 0;
                        p_state.kills = 0;
                        p_state.deaths = 0;
                        p_state.is_carrying_flag_team_id = 0;
                        p_state.mark_field_changed(FIELD_SCORE_STATS | FIELD_FLAG);
                    });
                    self.kill_feed.write().clear();
                }
            }
            fb::MatchStateType::Active => {
                match_info_guard.time_remaining -= delta_time;
                if match_info_guard.time_remaining <= 0.0 {
                    match_info_guard.match_state = fb::MatchStateType::Ended;
                    info!("Match ended! (Time up)");
                    if match_info_guard.game_mode == fb::GameModeType::TeamDeathmatch
                        || match_info_guard.game_mode == fb::GameModeType::CaptureTheFlag
                    {
                        let team1_score =
                            match_info_guard.team_scores.get(&1).cloned().unwrap_or(0);
                        let team2_score =
                            match_info_guard.team_scores.get(&2).cloned().unwrap_or(0);

                        // Determine and announce the winner
                        if team1_score > team2_score {
                            info!(
                                "Team 1 wins with {} points vs Team 2's {} points!",
                                team1_score, team2_score
                            );
                        } else if team2_score > team1_score {
                            info!(
                                "Team 2 wins with {} points vs Team 1's {} points!",
                                team2_score, team1_score
                            );
                        } else if team1_score == team2_score && team1_score > 0 {
                            info!(
                                "Match ended in a draw! Both teams scored {} points.",
                                team1_score
                            );
                        } else {
                            info!("Match ended with no winner (0-0).");
                        }
                    }
                }
            }
            fb::MatchStateType::Ended => {
                match_info_guard.time_remaining -= delta_time;
                if match_info_guard.time_remaining <= -10.0 {
                    match_info_guard.match_state = fb::MatchStateType::Waiting;
                    self.reset_match_state(&mut match_info_guard);
                    info!("Match reset to Waiting.");
                }
            }
            _ => {}
        }
    }

    fn process_ctf_logic_authoritative(&self, delta_time: f32) {
        let mut match_info_write_guard = self.match_info.write();
        if match_info_write_guard.game_mode == fb::GameModeType::CaptureTheFlag
            && match_info_write_guard.match_state == fb::MatchStateType::Active
        {
            for flag_state in match_info_write_guard.flag_states.values_mut() {
                if flag_state.status == fb::FlagStatus::Dropped && flag_state.respawn_timer > 0.0 {
                    flag_state.respawn_timer -= delta_time;
                    if flag_state.respawn_timer <= 0.0 {
                        flag_state.respawn_timer = 0.0;
                        flag_state.status = fb::FlagStatus::AtBase;
                        flag_state.position = Self::get_flag_base_position(flag_state.team_id);
                        flag_state.carrier_id = None;
                        self.global_game_events.push(
                            GameEvent::FlagReturned {
                                player_id: Arc::new("server".to_string()),
                                flag_team_id: flag_state.team_id,
                                position: flag_state.position,
                            },
                            EventPriority::High,
                        );
                        info!("Flag of team {} auto-returned to base.", flag_state.team_id);
                    }
                }
            }

            let mut player_snapshots: HashMap<PlayerID, PlayerState> = HashMap::new();
            self.player_manager.for_each_player(|id, state| {
                player_snapshots.insert(id.clone(), state.clone());
            });

            for (player_id_arc, player_state_snapshot) in &player_snapshots {
                if !player_state_snapshot.alive {
                    continue;
                }

                if player_state_snapshot.is_carrying_flag_team_id == 0 {
                    for flag_state in match_info_write_guard.flag_states.values_mut() {
                        // Check if flag can be interacted with
                        let can_interact = match flag_state.status {
                            fb::FlagStatus::AtBase => true,
                            fb::FlagStatus::Dropped => {
                                // Enemy can pick up after timer expires, own team can return immediately
                                if flag_state.team_id == player_state_snapshot.team_id {
                                    true // Own team can always return their dropped flag
                                } else {
                                    flag_state.respawn_timer <= 0.0 // Enemy must wait for timer
                                }
                            }
                            _ => false,
                        };

                        if can_interact {
                            let dx = player_state_snapshot.x - flag_state.position.x;
                            let dy = player_state_snapshot.y - flag_state.position.y;
                            if (dx * dx + dy * dy)
                                < (PICKUP_COLLECTION_RADIUS * PICKUP_COLLECTION_RADIUS)
                            {
                                if flag_state.team_id != player_state_snapshot.team_id {
                                    // Enemy picking up flag
                                    flag_state.status = fb::FlagStatus::Carried;
                                    flag_state.carrier_id = Some(player_id_arc.clone());
                                    if let Some(mut p_state_mut_entry) =
                                        self.player_manager.get_player_state_mut(player_id_arc)
                                    {
                                        let p_state_mut = &mut *p_state_mut_entry;
                                        p_state_mut.is_carrying_flag_team_id = flag_state.team_id;
                                        p_state_mut.mark_field_changed(FIELD_FLAG);
                                    }
                                    self.global_game_events.push(
                                        GameEvent::FlagGrabbed {
                                            player_id: player_id_arc.clone(),
                                            flag_team_id: flag_state.team_id,
                                            position: flag_state.position,
                                        },
                                        EventPriority::High,
                                    );
                                    info!(
                                        "Player {} grabbed flag of team {}",
                                        player_state_snapshot.username, flag_state.team_id
                                    );
                                    break;
                                } else if flag_state.status == fb::FlagStatus::Dropped
                                    && flag_state.team_id == player_state_snapshot.team_id
                                {
                                    // Own team returning flag
                                    flag_state.status = fb::FlagStatus::AtBase;
                                    flag_state.position =
                                        Self::get_flag_base_position(flag_state.team_id);
                                    flag_state.carrier_id = None;
                                    flag_state.respawn_timer = 0.0;
                                    self.global_game_events.push(
                                        GameEvent::FlagReturned {
                                            player_id: player_id_arc.clone(),
                                            flag_team_id: flag_state.team_id,
                                            position: flag_state.position,
                                        },
                                        EventPriority::High,
                                    );
                                    info!(
                                        "Player {} returned own team {}'s flag.",
                                        player_state_snapshot.username, flag_state.team_id
                                    );
                                    break;
                                }
                            }
                        }
                    }
                }

                if player_state_snapshot.is_carrying_flag_team_id != 0
                    && player_state_snapshot.is_carrying_flag_team_id
                        != player_state_snapshot.team_id
                {
                    let own_player_team_id = player_state_snapshot.team_id;

                    let own_flag_at_base = match_info_write_guard
                        .flag_states
                        .get(&own_player_team_id)
                        .map_or(false, |ofs| ofs.status == fb::FlagStatus::AtBase);

                    if own_flag_at_base {
                        let own_flag_base_pos = Self::get_flag_base_position(own_player_team_id);
                        let dx = player_state_snapshot.x - own_flag_base_pos.x;
                        let dy = player_state_snapshot.y - own_flag_base_pos.y;

                        if (dx * dx + dy * dy)
                            < (PICKUP_COLLECTION_RADIUS * PICKUP_COLLECTION_RADIUS)
                        {
                            let captured_flag_team_id =
                                player_state_snapshot.is_carrying_flag_team_id;

                            if let Some(captured_flag) = match_info_write_guard
                                .flag_states
                                .get_mut(&captured_flag_team_id)
                            {
                                captured_flag.status = fb::FlagStatus::AtBase;
                                captured_flag.position =
                                    Self::get_flag_base_position(captured_flag_team_id);
                                captured_flag.carrier_id = None;
                            }

                            if let Some(mut p_state_mut_entry) =
                                self.player_manager.get_player_state_mut(player_id_arc)
                            {
                                let p_state_mut = &mut *p_state_mut_entry;
                                p_state_mut.is_carrying_flag_team_id = 0;
                                p_state_mut.mark_field_changed(FIELD_FLAG);
                                p_state_mut.score += 100;
                                p_state_mut.mark_field_changed(FIELD_SCORE_STATS);
                            }

                            let team_score_mut_ref = match_info_write_guard
                                .team_scores
                                .entry(own_player_team_id)
                                .or_insert(0);
                            *team_score_mut_ref += 1;
                            let current_score = *team_score_mut_ref;

                            self.global_game_events.push(
                                GameEvent::FlagCaptured {
                                    capturer_id: player_id_arc.clone(),
                                    captured_flag_team_id,
                                    capturing_team_id: own_player_team_id,
                                    position: own_flag_base_pos,
                                },
                                EventPriority::High,
                            );
                            info!(
                                "Player {} captured team {}'s flag for team {}! (Score: {})",
                                player_state_snapshot.username,
                                captured_flag_team_id,
                                own_player_team_id,
                                current_score
                            );

                            if current_score >= 3 {
                                match_info_write_guard.match_state = fb::MatchStateType::Ended;
                                info!(
                                    "Team {} wins by capturing {} flags!",
                                    own_player_team_id, current_score
                                );
                            }
                        }
                    }
                }
            }
        }
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

    fn get_server_timestamp_us(&self) -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64
    }

    fn get_server_timestamp_ms(&self) -> u64 {
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

    fn ensure_join_trace(&self, peer_id: &str, channel_open: bool) {
        let now_ms = self.get_server_timestamp_us();
        let mut trace = self
            .join_stage_traces
            .entry(peer_id.to_owned())
            .or_insert_with(|| JoinStageTrace {
                join_sequence: self
                    .join_sequence_counter
                    .fetch_add(1, AtomicOrdering::Relaxed)
                    + 1,
                first_seen_ms: now_ms,
                ..JoinStageTrace::default()
            });
        if channel_open && trace.first_channel_open_ms.is_none() {
            trace.first_channel_open_ms = Some(now_ms);
        }
    }

    pub fn note_join_enqueued(&self, peer_id: &str) {
        self.ensure_join_trace(peer_id, false);
    }

    pub fn note_join_channel_open(&self, peer_id: &str) {
        self.ensure_join_trace(peer_id, true);
    }

    fn mark_join_build_start(&self, peer_id: &str) {
        self.ensure_join_trace(peer_id, false);
        let now_ms = self.get_server_timestamp_us();
        if let Some(mut trace) = self.join_stage_traces.get_mut(peer_id) {
            if trace.first_build_start_ms.is_none() {
                trace.first_build_start_ms = Some(now_ms);
            }
            trace.build_attempts = trace.build_attempts.saturating_add(1);
        }
    }

    fn mark_join_build_done(&self, peer_id: &str) {
        let now_ms = self.get_server_timestamp_us();
        if let Some(mut trace) = self.join_stage_traces.get_mut(peer_id) {
            if trace.first_build_done_ms.is_none() {
                trace.first_build_done_ms = Some(now_ms);
            }
        }
    }

    fn mark_join_send_start(&self, peer_id: &str) {
        self.ensure_join_trace(peer_id, true);
        let now_ms = self.get_server_timestamp_us();
        if let Some(mut trace) = self.join_stage_traces.get_mut(peer_id) {
            if trace.first_send_start_ms.is_none() {
                trace.first_send_start_ms = Some(now_ms);
            }
            trace.send_attempts = trace.send_attempts.saturating_add(1);
        }
    }

    fn mark_join_send_failure(&self, peer_id: &str) {
        let now_ms = self.get_server_timestamp_us();
        if let Some(mut trace) = self.join_stage_traces.get_mut(peer_id) {
            if trace.first_send_failure_ms.is_none() {
                trace.first_send_failure_ms = Some(now_ms);
            }
            if trace.first_send_result_ms.is_none() {
                trace.first_send_result_ms = Some(now_ms);
            }
            trace.retry_count = trace.retry_count.saturating_add(1);
            if let Some(previous_retry_ms) = trace.last_retry_at_ms {
                if now_ms > previous_retry_ms {
                    trace.retry_interval_total_ms = trace
                        .retry_interval_total_ms
                        .saturating_add(now_ms.saturating_sub(previous_retry_ms));
                    trace.retry_interval_samples = trace.retry_interval_samples.saturating_add(1);
                }
            }
            trace.last_retry_at_ms = Some(now_ms);
        }
    }

    fn mark_join_send_done(&self, peer_id: &str) {
        let now_ms = self.get_server_timestamp_us();
        if let Some(mut trace) = self.join_stage_traces.get_mut(peer_id) {
            if trace.first_send_done_ms.is_none() {
                trace.first_send_done_ms = Some(now_ms);
            }
            if trace.first_send_result_ms.is_none() {
                trace.first_send_result_ms = Some(now_ms);
            }
            if trace.completed_ms.is_none() {
                trace.completed_ms = Some(now_ms);
            }
        }
    }

    pub fn reset_join_stage_report(&self) {
        self.join_stage_traces.clear();
        self.join_sequence_counter.store(0, AtomicOrdering::Relaxed);
    }

    pub fn join_stage_report(&self) -> JoinStageReport {
        let traces: Vec<JoinStageTrace> = self
            .join_stage_traces
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        let total_tracked_clients = traces.len();
        let total_completed_clients = traces
            .iter()
            .filter(|trace| trace.completed_ms.is_some())
            .count();

        let mut waves = HashMap::new();
        for (wave_key, wave_label, wave_start, wave_end) in JOIN_STAGE_WAVES {
            let wave_traces: Vec<&JoinStageTrace> = traces
                .iter()
                .filter(|trace| trace.join_sequence >= wave_start)
                .filter(|trace| wave_end.map_or(true, |end| trace.join_sequence <= end))
                .collect();

            let open_channel_wait_ms: Vec<f64> = wave_traces
                .iter()
                .filter_map(
                    |trace| match (trace.first_seen_ms, trace.first_channel_open_ms) {
                        (seen, Some(opened)) if opened >= seen => {
                            Some(opened.saturating_sub(seen) as f64 / 1000.0)
                        }
                        _ => None,
                    },
                )
                .collect();
            let queue_wait_ms: Vec<f64> = wave_traces
                .iter()
                .filter_map(|trace| {
                    trace.first_build_start_ms.map(|build_start| {
                        let queue_start =
                            trace.first_channel_open_ms.unwrap_or(trace.first_seen_ms);
                        build_start.saturating_sub(queue_start) as f64 / 1000.0
                    })
                })
                .collect();
            let snapshot_build_ms: Vec<f64> = wave_traces
                .iter()
                .filter_map(
                    |trace| match (trace.first_build_start_ms, trace.first_build_done_ms) {
                        (Some(build_start), Some(build_done)) if build_done >= build_start => {
                            Some(build_done.saturating_sub(build_start) as f64 / 1000.0)
                        }
                        _ => None,
                    },
                )
                .collect();
            let send_ack_ms: Vec<f64> = wave_traces
                .iter()
                .filter_map(
                    |trace| match (trace.first_send_start_ms, trace.completed_ms) {
                        (Some(send_start), Some(completed)) if completed >= send_start => {
                            Some(completed.saturating_sub(send_start) as f64 / 1000.0)
                        }
                        _ => None,
                    },
                )
                .collect();
            let send_result_ms: Vec<f64> = wave_traces
                .iter()
                .filter_map(
                    |trace| match (trace.first_send_start_ms, trace.first_send_result_ms) {
                        (Some(send_start), Some(result_ms)) if result_ms >= send_start => {
                            Some(result_ms.saturating_sub(send_start) as f64 / 1000.0)
                        }
                        _ => None,
                    },
                )
                .collect();
            let retry_interval_ms: Vec<f64> = wave_traces
                .iter()
                .filter_map(|trace| {
                    if trace.retry_interval_samples == 0 {
                        None
                    } else {
                        Some(
                            trace.retry_interval_total_ms as f64
                                / trace.retry_interval_samples as f64
                                / 1000.0,
                        )
                    }
                })
                .collect();

            let retry_count_avg = if wave_traces.is_empty() {
                0.0
            } else {
                wave_traces
                    .iter()
                    .map(|trace| trace.retry_count as f64)
                    .sum::<f64>()
                    / wave_traces.len() as f64
            };
            let build_attempts_avg = if wave_traces.is_empty() {
                0.0
            } else {
                wave_traces
                    .iter()
                    .map(|trace| trace.build_attempts as f64)
                    .sum::<f64>()
                    / wave_traces.len() as f64
            };
            let send_attempts_avg = if wave_traces.is_empty() {
                0.0
            } else {
                wave_traces
                    .iter()
                    .map(|trace| trace.send_attempts as f64)
                    .sum::<f64>()
                    / wave_traces.len() as f64
            };

            let requested_slots = if let Some(end) = wave_end {
                end.saturating_sub(wave_start).saturating_add(1)
            } else {
                total_tracked_clients.max((wave_start.saturating_sub(1)) as usize) as u64
                    - wave_start.saturating_sub(1)
            };

            waves.insert(
                wave_key.to_owned(),
                JoinStageWaveSummary {
                    label: wave_label.to_owned(),
                    start_sequence: wave_start,
                    end_sequence: wave_end,
                    requested_slots,
                    tracked_clients: wave_traces.len(),
                    completed_clients: wave_traces
                        .iter()
                        .filter(|trace| trace.completed_ms.is_some())
                        .count(),
                    open_channel_wait_ms: summarize_join_stage_latencies(&open_channel_wait_ms),
                    queue_wait_ms: summarize_join_stage_latencies(&queue_wait_ms),
                    snapshot_build_ms: summarize_join_stage_latencies(&snapshot_build_ms),
                    send_result_ms: summarize_join_stage_latencies(&send_result_ms),
                    send_ack_ms: summarize_join_stage_latencies(&send_ack_ms),
                    retry_interval_ms: summarize_join_stage_latencies(&retry_interval_ms),
                    retry_count_avg: round_metric(retry_count_avg),
                    build_attempts_avg: round_metric(build_attempts_avg),
                    send_attempts_avg: round_metric(send_attempts_avg),
                },
            );
        }

        JoinStageReport {
            generated_at_ms: self.get_server_timestamp_ms(),
            total_tracked_clients,
            total_completed_clients,
            waves,
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

    fn initialize_ctf_flags(&self, match_info: &mut ServerMatchInfo) {
        match_info.flag_states.clear();
        let team1_flag_pos = Self::get_flag_base_position(1);
        match_info.flag_states.insert(
            1,
            ServerFlagState {
                team_id: 1,
                status: fb::FlagStatus::AtBase,
                position: team1_flag_pos,
                carrier_id: None,
                respawn_timer: 0.0,
            },
        );
        let team2_flag_pos = Self::get_flag_base_position(2);
        match_info.flag_states.insert(
            2,
            ServerFlagState {
                team_id: 2,
                status: fb::FlagStatus::AtBase,
                position: team2_flag_pos,
                carrier_id: None,
                respawn_timer: 0.0,
            },
        );
        info!(
            "CTF Flags initialized. T1 at {:?}, T2 at {:?}",
            team1_flag_pos, team2_flag_pos
        );
    }

    pub fn get_flag_base_position(team_id: u8) -> Vec2 {
        if team_id == 1 {
            Vec2::new(WORLD_MIN_X + 100.0, 0.0)
        } else if team_id == 2 {
            Vec2::new(WORLD_MAX_X - 100.0, 0.0)
        } else {
            Vec2::new(0.0, 0.0)
        }
    }

    fn reset_match_state(&self, match_info: &mut ServerMatchInfo) {
        match_info.time_remaining = 300.0;
        // Don't clear team scores - preserve them between rounds
        // match_info.team_scores.clear();
        match_info.flag_states.clear();
        if match_info.match_state == fb::MatchStateType::Waiting
            && match_info.game_mode == fb::GameModeType::CaptureTheFlag
        {
            self.initialize_ctf_flags(match_info);
        }
        self.player_manager.for_each_player_mut(|_id, pstate| {
            // Reset individual player stats but keep their contribution to team score
            pstate.score = 0;
            pstate.kills = 0;
            pstate.deaths = 0;
            pstate.is_carrying_flag_team_id = 0;
            pstate.mark_field_changed(FIELD_SCORE_STATS | FIELD_FLAG);
        });
        self.kill_feed.write().clear();
    }

    async fn prepare_shared_broadcast_data(
        &self,
        include_active_walls_snapshot: bool,
        initial_snapshot_caps: InitialSnapshotCaps,
        tail_join_mode: bool,
        aggressive_tail_join_mode: bool,
        extreme_tail_join_mode: bool,
        _disable_soa_snapshot_for_backlog: bool,
        max_delta_events_per_client: usize,
        scheduled_peer_ids: &[String],
    ) -> SharedBroadcastData {
        let current_timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Collect events efficiently
        let mut events = Vec::with_capacity(100);
        while let Some(event) = self.global_game_events.pop() {
            events.push(event);
            if events.len() >= 100 {
                break;
            }
        }

        // Snapshot destroyed walls
        let destroyed_wall_ids = self
            .destroyed_wall_ids_this_tick
            .read()
            .iter()
            .cloned()
            .collect();

        // Snapshot updated walls
        let updated_walls = self.updated_walls_this_tick.read().clone();

        // Snapshot active walls once per broadcast and index them for fast dynamic wall streaming.
        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        let active_walls_cached = self.get_active_walls_cached(frame).await;
        let mut active_walls_by_id = HashMap::with_capacity(active_walls_cached.len());
        for wall in active_walls_cached.iter() {
            active_walls_by_id.insert(wall.id, wall.clone());
        }
        let active_walls_snapshot = if include_active_walls_snapshot {
            active_walls_cached.iter().cloned().collect()
        } else {
            Vec::new()
        };

        let use_aoi_snapshot = join_authoritative_aoi_snapshot_enabled();
        let player_aois_snapshot = if use_aoi_snapshot {
            // Resolve AoI from authoritative lock-free snapshot and only keep entries
            // for peers scheduled this frame.
            let authoritative_aoi_snapshot = self.player_aoi_snapshot.load();
            if authoritative_aoi_snapshot.is_empty() && !scheduled_peer_ids.is_empty() {
                debug!(
                    "[Frame {}] Authoritative AoI snapshot is empty while {} peers are scheduled.",
                    frame,
                    scheduled_peer_ids.len()
                );
            }

            let mut scheduled_aoi_snapshot = HashMap::with_capacity(scheduled_peer_ids.len());
            for peer_id in scheduled_peer_ids {
                let player_id = self.player_manager.id_pool.get_or_create(peer_id);
                if let Some(aoi) = authoritative_aoi_snapshot.get_aoi(&player_id) {
                    scheduled_aoi_snapshot.insert(player_id, aoi.clone());
                }
            }
            Arc::new(scheduled_aoi_snapshot)
        } else {
            Arc::new(HashMap::new())
        };

        let configured_soa_snapshot = join_soa_snapshot_enabled();
        let use_soa_snapshot = configured_soa_snapshot;
        let mut soa_fallback_active = false;
        let (player_soa_snapshot, player_states_snapshot) = if use_soa_snapshot {
            let mut snapshot = self.player_soa_snapshot.load();
            if snapshot.is_empty() && !scheduled_peer_ids.is_empty() {
                debug!(
                    "[Frame {}] Player SoA snapshot is empty while {} peers are scheduled.",
                    frame,
                    scheduled_peer_ids.len()
                );
                snapshot = self.rebuild_player_soa_snapshot_from_authoritative_state();
                soa_fallback_active = true;
            }
            (snapshot, HashMap::new())
        } else {
            let mut by_id = HashMap::with_capacity(self.player_manager.player_count());
            self.player_manager
                .for_each_player(|player_id, player_state| {
                    by_id.insert(player_id.clone(), player_state.clone());
                });
            (Arc::new(PlayerSoASnapshot::default()), by_id)
        };

        // Snapshot projectiles/pickups once per tick (reused for all client delta builds).
        let configured_entity_soa_snapshot = join_entity_soa_snapshot_enabled();
        let use_entity_soa_snapshot = configured_entity_soa_snapshot;

        let (projectiles_soa_snapshot, projectiles_snapshot) = {
            if use_entity_soa_snapshot {
                let mut snapshot = self.projectile_soa_snapshot.load();
                if snapshot.is_empty() && !scheduled_peer_ids.is_empty() {
                    debug!(
                        "[Frame {}] Projectile SoA snapshot is empty while {} peers are scheduled.",
                        frame,
                        scheduled_peer_ids.len()
                    );
                    snapshot = self.rebuild_projectile_soa_snapshot_from_authoritative_state();
                    soa_fallback_active = true;
                }
                (snapshot, Arc::new(HashMap::new()))
            } else {
                let projectiles_guard = self.projectiles.read();
                let mut by_id = HashMap::with_capacity(projectiles_guard.len());
                for projectile in projectiles_guard.iter() {
                    by_id.insert(projectile.id, projectile.clone());
                }
                (Arc::new(ProjectileSoASnapshot::default()), Arc::new(by_id))
            }
        };

        let (pickups_soa_snapshot, pickups_snapshot) = {
            if use_entity_soa_snapshot {
                let mut snapshot = self.pickup_soa_snapshot.load();
                if snapshot.is_empty() && !scheduled_peer_ids.is_empty() {
                    debug!(
                        "[Frame {}] Pickup SoA snapshot is empty while {} peers are scheduled.",
                        frame,
                        scheduled_peer_ids.len()
                    );
                    snapshot = self.rebuild_pickup_soa_snapshot_from_authoritative_state();
                    soa_fallback_active = true;
                }
                (snapshot, Arc::new(HashMap::new()))
            } else {
                let pickups_guard = self.pickups.read();
                let mut by_id = HashMap::with_capacity(pickups_guard.len());
                for pickup in pickups_guard.iter() {
                    by_id.insert(pickup.id, pickup.clone());
                }
                (Arc::new(PickupSoASnapshot::default()), Arc::new(by_id))
            }
        };

        // Snapshot serialized chat packets once per broadcast.
        let chat_packets = self
            .chat_messages_queue
            .read()
            .await
            .iter()
            .map(|chat_entry| SerializedChatPacket {
                seq: chat_entry.seq,
                bytes: build_chat_game_message_bytes(chat_entry),
            })
            .collect();

        // Snapshot match info (read once)
        let match_info_guard = self.match_info.read();
        let match_info_snapshot = MatchInfoSnapshot {
            time_remaining: match_info_guard.time_remaining,
            match_state: match_info_guard.match_state,
            game_mode: match_info_guard.game_mode,
            team_scores: match_info_guard.team_scores.clone(),
            flag_states: match_info_guard.flag_states.clone(),
        };
        drop(match_info_guard);

        // Snapshot kill feed
        let kill_feed_snapshot = self.kill_feed.read().iter().cloned().collect();

        SharedBroadcastData {
            timestamp_ms: current_timestamp_ms,
            events,
            destroyed_wall_ids,
            updated_walls,
            active_walls_by_id,
            active_walls_snapshot,
            player_aois_snapshot,
            player_soa_snapshot,
            player_states_snapshot,
            projectiles_soa_snapshot,
            pickups_soa_snapshot,
            projectiles_snapshot,
            pickups_snapshot,
            chat_packets,
            match_info_snapshot,
            kill_feed_snapshot,
            max_delta_events_per_client,
            initial_snapshot_caps,
            tail_join_mode,
            aggressive_tail_join_mode,
            extreme_tail_join_mode,
            use_aoi_snapshot,
            soa_fallback_active,
            use_soa_snapshot,
            use_entity_soa_snapshot,
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

    fn manage_bot_population(&self) {
        // Ensure this method is defined within the impl block
        let human_player_count = self
            .player_manager
            .player_count()
            .saturating_sub(self.bot_players.len());
        let current_bot_count = self.bot_players.len();

        // Corrected line: Directly use the usize value from config
        let max_players_in_match = self.config.max_players_per_match;
        let effective_bot_capacity = max_players_in_match.saturating_sub(self.reserved_human_slots);

        let desired_bot_count = if human_player_count >= effective_bot_capacity {
            0
        } else {
            (effective_bot_capacity - human_player_count).min(
                self.target_bot_count
                    .load(std::sync::atomic::Ordering::Relaxed) as usize,
            ) // Also consider target_bot_count
        };

        if current_bot_count > desired_bot_count {
            let bots_to_remove_count = current_bot_count - desired_bot_count;
            debug!("[Bot Management] Max players: {}, Humans: {}, Current Bots: {}, Desired Bots: {}. Removing {} bots.",
                max_players_in_match, human_player_count, current_bot_count, desired_bot_count, bots_to_remove_count);
            self.remove_bots(bots_to_remove_count);
        } else if current_bot_count < desired_bot_count {
            let bots_to_add_count = desired_bot_count - current_bot_count;
            debug!("[Bot Management] Max players: {}, Humans: {}, Current Bots: {}, Desired Bots: {}. Adding {} bots.",
                max_players_in_match, human_player_count, current_bot_count, desired_bot_count, bots_to_add_count);
            self.spawn_additional_bots(bots_to_add_count);
        }
    }

    fn team_player_counts(&self) -> (usize, usize) {
        let mut team1_count = 0usize;
        let mut team2_count = 0usize;
        self.player_manager.for_each_player(|_, state| {
            if state.team_id == 1 {
                team1_count += 1;
            } else if state.team_id == 2 {
                team2_count += 1;
            }
        });
        (team1_count, team2_count)
    }

    pub fn ensure_human_join_capacity(&self, joining_peer_id: &str) -> bool {
        self.ensure_human_join_capacity_for_team(joining_peer_id, None)
    }

    pub fn ensure_human_join_capacity_for_team(
        &self,
        joining_peer_id: &str,
        joining_team: Option<u8>,
    ) -> bool {
        if !self.human_priority_enabled {
            return self.player_manager.player_count() < self.config.max_players_per_match;
        }
        if self.player_manager.player_count() < self.config.max_players_per_match {
            return true;
        }
        let selected_bot = match joining_team {
            Some(team) if team == 1 || team == 2 => self.select_balanced_bot_for_human_join(team),
            _ => self.select_lowest_performing_bot(),
        };
        let Some(bot_id) = selected_bot else {
            warn!(
                "[Human Priority] No bot available to evict for human '{}'; server remains full.",
                joining_peer_id
            );
            return false;
        };
        self.evict_bot_for_human(&bot_id, joining_peer_id, joining_team)
    }

    fn bot_eviction_candidates(&self) -> Vec<(PlayerID, i64, u8, String)> {
        let mut candidates = Vec::with_capacity(self.bot_players.len());

        for entry in self.bot_players.iter() {
            let bot_id = entry.key().clone();
            let (rating, team_id, username) = self
                .player_manager
                .get_player_state(&bot_id)
                .map(|state| {
                    let score = state.score as i64;
                    let kills = state.kills as i64 * 25;
                    let deaths_penalty = state.deaths as i64 * 10;
                    let health_bonus = state.health.max(0) as i64;
                    (
                        score + kills + health_bonus - deaths_penalty,
                        state.team_id,
                        state.username.clone(),
                    )
                })
                .unwrap_or((i64::MIN, 0, bot_id.as_str().to_owned()));
            candidates.push((bot_id, rating, team_id, username));
        }

        candidates
    }

    fn select_lowest_performing_bot(&self) -> Option<PlayerID> {
        self.bot_eviction_candidates()
            .into_iter()
            .min_by_key(|(_, rating, _, _)| *rating)
            .map(|(bot_id, _, _, _)| bot_id)
    }

    fn select_balanced_bot_for_human_join(&self, joining_team: u8) -> Option<PlayerID> {
        let candidates = self.bot_eviction_candidates();
        if candidates.is_empty() {
            return None;
        }
        if joining_team != 1 && joining_team != 2 {
            return candidates
                .into_iter()
                .min_by_key(|(_, rating, _, _)| *rating)
                .map(|(bot_id, _, _, _)| bot_id);
        }

        let (team1_count, team2_count) = self.team_player_counts();
        candidates
            .into_iter()
            .map(|(bot_id, rating, bot_team, _)| {
                let mut projected_team1 =
                    team1_count as i64 + if joining_team == 1 { 1 } else { 0 };
                let mut projected_team2 =
                    team2_count as i64 + if joining_team == 2 { 1 } else { 0 };
                if bot_team == 1 {
                    projected_team1 -= 1;
                } else if bot_team == 2 {
                    projected_team2 -= 1;
                }
                let imbalance = (projected_team1 - projected_team2).abs();
                (bot_id, imbalance, rating)
            })
            .min_by(|lhs, rhs| lhs.1.cmp(&rhs.1).then_with(|| lhs.2.cmp(&rhs.2)))
            .map(|(bot_id, _, _)| bot_id)
    }

    fn enqueue_system_chat_message(&self, message: String) {
        let entry = ChatMessage {
            seq: next_chat_message_seq(),
            player_id: self.player_manager.id_pool.get_or_create("system"),
            username: "System".to_owned(),
            message: message.chars().take(160).collect(),
            timestamp: self.get_server_timestamp_ms(),
        };

        if let Ok(mut chat_q_guard) = self.chat_messages_queue.try_write() {
            chat_q_guard.push_back(entry);
            if chat_q_guard.len() > MAX_CHAT_MESSAGES_HISTORY {
                chat_q_guard.pop_front();
            }
            return;
        }

        let queue = self.chat_messages_queue.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let mut chat_q_guard = queue.write().await;
                chat_q_guard.push_back(entry);
                if chat_q_guard.len() > MAX_CHAT_MESSAGES_HISTORY {
                    chat_q_guard.pop_front();
                }
            });
        } else {
            warn!(
                "[System Chat] Dropped announcement without runtime: {}",
                message
            );
        }
    }

    fn push_kill_feed_entry(
        &self,
        killer_name: String,
        victim_name: String,
        weapon: ServerWeaponType,
    ) {
        let mut kill_feed_guard = self.kill_feed.write();
        kill_feed_guard.push_back(ServerKillFeedEntry {
            killer_name,
            victim_name,
            weapon,
            timestamp: self.frame_counter.load(AtomicOrdering::Relaxed),
        });
        if kill_feed_guard.len() > MAX_KILL_FEED_HISTORY {
            kill_feed_guard.pop_front();
        }
    }

    fn evict_bot_for_human(
        &self,
        bot_id: &PlayerID,
        joining_peer_id: &str,
        joining_team: Option<u8>,
    ) -> bool {
        let bot_snapshot = self
            .bot_eviction_candidates()
            .into_iter()
            .find(|(candidate_bot_id, _, _, _)| candidate_bot_id == bot_id)
            .map(|(_, _, team_id, username)| (team_id, username));

        if self.bot_players.remove(bot_id).is_none() {
            return false;
        }

        self.player_manager.remove_player(bot_id.as_str());
        self.data_channels_map.remove(bot_id.as_str());
        self.client_states_map.write().remove(bot_id.as_str());
        self.player_aois.remove(bot_id.as_str());

        info!(
            "[Human Priority] Evicted bot '{}' to free a slot for human '{}'.",
            bot_id, joining_peer_id
        );

        if joining_peer_id != "bot_population_manager" {
            let (bot_team, bot_name) =
                bot_snapshot.unwrap_or((0, format!("Bot {}", bot_id.as_str())));
            let mut announcement = format!(
                "{} was rotated out to free a slot for {}.",
                bot_name, joining_peer_id
            );
            if let Some(team) = joining_team {
                if (team == 1 || team == 2) && (bot_team == 1 || bot_team == 2) {
                    announcement.push_str(&format!(
                        " Team balance: joiner T{}, removed bot T{}.",
                        team, bot_team
                    ));
                }
            }
            self.enqueue_system_chat_message(announcement);
            let joiner_short = &joining_peer_id[..joining_peer_id.len().min(6)];
            self.push_kill_feed_entry(
                format!("Human {}", joiner_short),
                bot_name,
                ServerWeaponType::Melee,
            );
        }

        true
    }

    fn spawn_additional_bots(&self, count_to_add: usize) {
        if count_to_add == 0 {
            return;
        }
        info!(
            "[Bot Management] Attempting to spawn {} additional bots...",
            count_to_add
        );

        let team_spawn_areas = crate::world::map_generator::MapGenerator::get_team_spawn_areas();
        let mut rng = rand::thread_rng();
        let bot_names = [
            "Alpha", "Beta", "Gamma", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India",
            "Juliet", "Kilo", "Lima", "Mike", "November", "Oscar", "Papa", "Quebec", "Romeo",
            "Sierra", "Tango", "Uniform", "Victor", "Whiskey", "Xray", "Yankee", "Zulu",
        ];

        for _i in 0..count_to_add {
            // _i as it's not directly used for bot naming index here
            let current_total_players = self.player_manager.player_count();
            if current_total_players >= self.config.max_players_per_match {
                info!("[Bot Management] Max player limit ({}) reached, stopping additional bot spawn. Current players: {}", self.config.max_players_per_match, current_total_players);
                break;
            }

            let bot_name_num = self.bot_name_counter.fetch_add(1, AtomicOrdering::SeqCst);
            let bot_base_name = bot_names
                .get(bot_name_num as usize % bot_names.len())
                .unwrap_or(&"Extra");
            let bot_name = format!(
                "Bot {}{}",
                bot_base_name,
                if bot_name_num >= bot_names.len() as u64 {
                    (bot_name_num / bot_names.len() as u64).to_string()
                } else {
                    "".to_string()
                }
            );

            let bot_player_id_str = format!("bot_{}", uuid::Uuid::new_v4());

            let mut team1_player_count = 0; // Count players (human + bot) on team 1
            let mut team2_player_count = 0; // Count players (human + bot) on team 2
            self.player_manager.for_each_player(|_id, p_state| {
                if p_state.team_id == 1 {
                    team1_player_count += 1;
                } else if p_state.team_id == 2 {
                    team2_player_count += 1;
                }
            });

            let team_id = if team1_player_count <= team2_player_count {
                1
            } else {
                2
            };

            // Get spawn points for the selected team
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
                if let Some(mut p_state_entry) =
                    self.player_manager.get_player_state_mut(&player_id_arc)
                {
                    let p_state = &mut *p_state_entry;
                    p_state.team_id = team_id;
                    p_state.mark_field_changed(FIELD_SCORE_STATS | FIELD_FLAG);
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
                debug!("[Bot Management] Spawned additional bot: {} (ID: {}) on team {} at ({:.1}, {:.1}). Total players: {}", bot_name, bot_player_id_str, team_id, spawn_pos.x, spawn_pos.y, self.player_manager.player_count());
            } else {
                error!(
                    "[Bot Management] Failed to add bot {} to player manager.",
                    bot_name
                );
            }
        }
    }

    fn remove_bots(&self, count: usize) {
        let mut removed_count = 0;
        while removed_count < count {
            let Some(bot_key) = self.select_lowest_performing_bot() else {
                break;
            };
            if self.evict_bot_for_human(&bot_key, "bot_population_manager", None) {
                removed_count += 1;
            } else {
                break;
            }
        }
    }

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

        let connected_clients_total = self.data_channels_map.len();
        if connected_clients_total == 0 {
            if current_frame % 30 == 0 {
                // Log every 30 frames
                // Debug: List all keys in the map to see if there's a mismatch
                info!("[Frame {}] No connected clients in data_channels_map. Checking map contents...", current_frame);
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
                    "[Frame {}] Total entries found: {}",
                    current_frame,
                    self.data_channels_map.len()
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
        if connected_clients_open == 0 {
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

        let max_delta_events_per_client = if extreme_tail_join_mode {
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
        let delta_skip_modulus = if extreme_tail_join_mode {
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

        let scheduled_peer_ids: Vec<String> = scheduled_client_entries
            .iter()
            .map(|(peer_id, _, _)| peer_id.clone())
            .collect();

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
        let has_connected_clients = !self.data_channels_map.is_empty();

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

fn round_metric(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn percentile_sorted(values_sorted: &[f64], percentile: f64) -> f64 {
    if values_sorted.is_empty() {
        return 0.0;
    }
    if values_sorted.len() == 1 {
        return values_sorted[0];
    }
    let clamped = percentile.clamp(0.0, 1.0);
    let idx = (values_sorted.len() - 1) as f64 * clamped;
    let lower = idx.floor() as usize;
    let upper = idx.ceil() as usize;
    if lower == upper {
        values_sorted[lower]
    } else {
        let weight = idx - lower as f64;
        values_sorted[lower] + (values_sorted[upper] - values_sorted[lower]) * weight
    }
}

fn summarize_join_stage_latencies(values: &[f64]) -> JoinStageLatencyStats {
    if values.is_empty() {
        return JoinStageLatencyStats::default();
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.total_cmp(right));
    let avg = sorted.iter().sum::<f64>() / sorted.len() as f64;
    JoinStageLatencyStats {
        count: sorted.len(),
        avg_ms: round_metric(avg),
        p95_ms: round_metric(percentile_sorted(&sorted, 0.95)),
        max_ms: round_metric(*sorted.last().unwrap_or(&0.0)),
    }
}
