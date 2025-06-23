// massive_game_server/server/src/server/instance.rs
use crate::core::types::*;
use crate::core::config::ServerConfig;
use crate::core::constants::*; // Import all constants, including MIN_PLAYERS_TO_START
use crate::core::error::ServerError;
use crate::concurrent::thread_pools::ThreadPoolSystem;
use crate::concurrent::spatial_index::ImprovedSpatialIndex;
use crate::concurrent::event_queue::PriorityEventQueue;
use crate::concurrent::wall_spatial_index::WallSpatialIndex;
use crate::world::partition::{WorldPartitionManager}; // Removed unused ImprovedWorldPartition
use crate::entities::player::{ImprovedPlayerManager};
use crate::flatbuffers_generated::game_protocol as fb;
use crate::network::signaling::{DataChannelsMap, ClientStatesMap, ChatMessagesQueue, ClientState, handle_dc_send_error};
use crate::world::map_generator::MapGenerator;
use crate::systems::respawn::{RespawnManager, WallRespawnManager};
use crate::systems::ai::bot_ai::BotAISystem;
use crate::systems::ai::optimized_bot_ai::OptimizedBotAI;
use crate::network::signaling::ChatMessage;
use tokio::task::JoinError;
use futures::executor;
use std::cell::RefCell;
use webrtc::data_channel::data_channel_state::RTCDataChannelState;
use parking_lot::RwLockReadGuard;
use crate::core::types::{EntityId, PlayerID, CorePickupType, MatchState};
use crate::network::signaling::PickupState;
use flatbuffers::FlatBufferBuilder;


use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use dashmap::DashMap;
use parking_lot::RwLock as ParkingLotRwLock;
use crossbeam_queue::SegQueue;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering, AtomicBool};
use std::collections::{VecDeque, HashSet, HashMap};
use uuid::Uuid;
use bytes::Bytes;
use rand::Rng;
use tokio::time::sleep; // Add this import
use once_cell::sync::OnceCell;
use rayon::prelude::*;
    // In src/server/instance.rs
use tracing::{debug, error, warn, info, trace}; // Ensure all levels are available
    

use tokio::{task::JoinSet, time::timeout};



#[derive(Clone, Debug, PartialEq)]
pub struct ServerFlagState {
    pub team_id: u8, // Which team this flag BELONGS to
    pub status: fb::FlagStatus, // fb from flatbuffers_generated
    pub position: Vec2, // Current position (at base, or where it was dropped)
    pub carrier_id: Option<PlayerID>, // ID of the player carrying this flag
    pub respawn_timer: f32, // If dropped, time until it auto-returns
}

#[derive(Clone, Debug, PartialEq)]
pub struct ServerMatchInfo {
    pub time_remaining: f32,
    pub match_state: fb::MatchStateType, // fb from flatbuffers_generated
    pub game_mode: fb::GameModeType,   // fb from flatbuffers_generated
    pub team_scores: HashMap<u8, i32>, // team_id -> score
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
    Patrolling
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
    wall_hits: Vec<(EntityId, i32)>, // (wall_id, damage)
    to_remove: Vec<usize>, // Projectile indices to remove
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
        CorePickupType::WeaponCrate(server_weapon_type) => {
            (fb::PickupType::WeaponCrate, Some(map_server_weapon_to_fb(*server_weapon_type)))
        }
        CorePickupType::SpeedBoost => (fb::PickupType::SpeedBoost, None),
        CorePickupType::DamageBoost => (fb::PickupType::DamageBoost, None),
        CorePickupType::Shield => (fb::PickupType::Shield, None),
    }
}

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

fn create_fb_player_state_for_delta<'a>(
    builder: &mut flatbuffers::FlatBufferBuilder<'a>,
    pstate: &PlayerState,
    _changed_fields: u16, // This can be used if we implement partial updates later
) -> flatbuffers::WIPOffset<fb::PlayerState<'a>> {
    let id_fb = fb_safe_str(builder, pstate.id.as_str());
    let username_fb = fb_safe_str(builder, &pstate.username);
    let weapon_fb = map_server_weapon_to_fb(pstate.weapon);

    fb::PlayerState::create(
        builder,
        &fb::PlayerStateArgs {
            id: Some(id_fb), username: Some(username_fb), x: pstate.x, y: pstate.y,
            rotation: pstate.rotation, velocity_x: pstate.velocity_x, velocity_y: pstate.velocity_y,
            health: pstate.health, max_health: pstate.max_health, alive: pstate.alive,
            respawn_timer: pstate.respawn_timer.unwrap_or(-1.0), weapon: weapon_fb, ammo: pstate.ammo,
            reload_progress: pstate.reload_progress.unwrap_or(-1.0), score: pstate.score,
            kills: pstate.kills, deaths: pstate.deaths,
            team_id: pstate.team_id as i8,
            speed_boost_remaining: pstate.speed_boost_remaining,
            damage_boost_remaining: pstate.damage_boost_remaining,
            shield_current: pstate.shield_current, shield_max: pstate.shield_max,
            is_carrying_flag_team_id: pstate.is_carrying_flag_team_id as i8,
        },
    )
}


fn build_game_event_fb<'a>(
    builder: &mut flatbuffers::FlatBufferBuilder<'a>,
    event: &GameEvent,
) -> flatbuffers::WIPOffset<fb::GameEvent<'a>> {
    let event_pos = event_position(event);
    let pos_fb = fb::Vec2::create(builder, &fb::Vec2Args { x: event_pos.x, y: event_pos.y });
    let instigator_id_fb = event_instigator_id(event).map(|id| builder.create_string(id.as_str()));
    let target_id_fb = event_target_id(event).map(|id| builder.create_string(&id));
    let weapon_type_fb = event_weapon_type(event).map_or(fb::WeaponType::Pistol, map_server_weapon_to_fb);
    
    fb::GameEvent::create(builder, &fb::GameEventArgs {
        event_type: map_game_event_type_to_fb(event),
        position: Some(pos_fb),
        instigator_id: instigator_id_fb,
        target_id: target_id_fb,
        weapon_type: weapon_type_fb,
        value: event_value(event).unwrap_or(0.0),
    })
}

// Shared data that's the same for all clients
#[derive(Clone)]
struct SharedBroadcastData {
    timestamp_ms: u64,
    events: Vec<GameEvent>,
    destroyed_wall_ids: Vec<EntityId>,
    updated_walls: HashMap<EntityId, Wall>,
    chat_messages: Vec<ChatMessage>,
    match_info_snapshot: MatchInfoSnapshot,
    kill_feed_snapshot: Vec<ServerKillFeedEntry>,
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

    pub last_broadcast_frame: Arc<AtomicU64>,
    pub player_last_sync_positions: Arc<DashMap<PlayerID, (f32, f32)>>,
}

const MAX_KILL_FEED_HISTORY: usize = 10;
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
            WORLD_MAX_X - WORLD_MIN_X, WORLD_MAX_Y - WORLD_MIN_Y,
            WORLD_MIN_X, WORLD_MIN_Y, SPATIAL_INDEX_CELL_SIZE,
        ));
        info!("Spatial index initialized.");

        let player_manager = Arc::new(ImprovedPlayerManager::new(
            config.num_player_shards,
            spatial_index.clone(),
        ));
        info!("Player manager initialized with {} shards.", config.num_player_shards);

        let all_map_walls = MapGenerator::generate_10v10_map();
        info!("Generated {} walls for the map.", all_map_walls.len());

        let world_partition_manager = Arc::new(WorldPartitionManager::new(
            config.world_partition_grid_dim,
            WORLD_MAX_X - WORLD_MIN_X,
            WORLD_MAX_Y - WORLD_MIN_Y,
            WORLD_MIN_X,
            WORLD_MIN_Y,
            1024, 
        ));
        info!("World partition manager initialized with {}x{} grid.", config.world_partition_grid_dim, config.world_partition_grid_dim);

        for wall in &all_map_walls {
            let wall_center_x = wall.x + wall.width / 2.0;
            let wall_center_y = wall.y + wall.height / 2.0;
            let partition_idx = world_partition_manager.get_partition_index_for_point(wall_center_x, wall_center_y);

            if let Some(partition) = world_partition_manager.get_partition(partition_idx) {
                partition.add_wall_on_load(wall.clone());
            } else {
                error!("Could not find partition with index {} for wall {}", partition_idx, wall.id);
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

        let destructible_walls_vec: Vec<Wall> = all_map_walls.iter()
            .filter(|w| w.is_destructible)
            .cloned()
            .collect();
        wall_respawn_manager.register_all_walls(&destructible_walls_vec);
        info!("Registered {} destructible walls with WallRespawnManager.", destructible_walls_vec.len());

        let initial_pickups = Self::generate_initial_pickups(&all_map_walls);
        info!("Generated {} initial pickups.", initial_pickups.len());

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
        info!("Wall spatial index initialized with {} active walls.", wall_spatial_index.size());

        let server = MassiveGameServer {
            config,
            thread_pools,
            player_manager,
            world_partition_manager,
            spatial_index,
            wall_spatial_index,
            projectiles_to_add: Arc::new(SegQueue::new()),
            global_game_events: Arc::new(PriorityEventQueue::new()),
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
            kill_feed: Arc::new(ParkingLotRwLock::new(VecDeque::with_capacity(MAX_KILL_FEED_HISTORY + 5))),
            destroyed_wall_ids_this_tick: Arc::new(ParkingLotRwLock::new(HashSet::new())),
            updated_walls_this_tick: Arc::new(ParkingLotRwLock::new(HashMap::new())),
            player_aois,
            respawn_manager,
            wall_respawn_manager,
            bot_players: Arc::new(DashMap::new()),
            target_bot_count: Arc::new(AtomicU64::new(20)), // Increased to 20 bots for active gameplay
            bot_name_counter: Arc::new(AtomicU64::new(0)),
            last_broadcast_frame: Arc::new(AtomicU64::new(0)),
            player_last_sync_positions: Arc::new(DashMap::new()),
        };

        info!("MassiveGameServer initialized successfully.");
        server
    }

    fn generate_initial_pickups(map_walls: &[Wall]) -> Vec<Pickup> {
        let mut pickups = Vec::new();
        let mut rng = rand::thread_rng();
        let pickup_types = [
            CorePickupType::Health, CorePickupType::Ammo,
            CorePickupType::WeaponCrate(ServerWeaponType::Shotgun),
            CorePickupType::WeaponCrate(ServerWeaponType::Rifle),
            CorePickupType::SpeedBoost, CorePickupType::DamageBoost, CorePickupType::Shield,
            CorePickupType::WeaponCrate(ServerWeaponType::Sniper),
        ];

        let strategic_locations = [
            Vec2::new(0.0, 0.0),
            Vec2::new(WORLD_MIN_X / 2.0, WORLD_MIN_Y / 2.0),
            Vec2::new(WORLD_MAX_X / 2.0, WORLD_MIN_Y / 2.0),
            Vec2::new(WORLD_MIN_X / 2.0, WORLD_MAX_Y / 2.0),
            Vec2::new(WORLD_MAX_X / 2.0, WORLD_MAX_Y / 2.0),
            Vec2::new(WORLD_MIN_X + 250.0, 0.0),
            Vec2::new(WORLD_MAX_X - 250.0, 0.0),
        ];

        let num_pickups_to_spawn = strategic_locations.len().min(pickup_types.len());

        for i in 0..num_pickups_to_spawn {
            let base_pos = strategic_locations[i % strategic_locations.len()];
            let mut placed = false;
            for _attempt in 0..10 {
                let x_offset = rng.gen_range(-50.0..50.0);
                let y_offset = rng.gen_range(-50.0..50.0);
                let x = (base_pos.x + x_offset).clamp(WORLD_MIN_X + 50.0, WORLD_MAX_X - 50.0);
                let y = (base_pos.y + y_offset).clamp(WORLD_MIN_Y + 50.0, WORLD_MAX_Y - 50.0);

                let mut obstructed = false;
                for wall in map_walls {
                        if wall.is_destructible && wall.current_health <= 0 { continue; }
                    if x + PICKUP_COLLECTION_RADIUS > wall.x && x - PICKUP_COLLECTION_RADIUS < wall.x + wall.width &&
                       y + PICKUP_COLLECTION_RADIUS > wall.y && y - PICKUP_COLLECTION_RADIUS < wall.y + wall.height {
                        obstructed = true; break;
                    }
                }
                if !obstructed {
                    let pickup_type = pickup_types[i % pickup_types.len()].clone();
                    pickups.push(Pickup::new(Uuid::new_v4().as_u128() as u64, x, y, pickup_type));
                    placed = true; break;
                }
            }
            if !placed { warn!("Could not place pickup {} near {:?} after 10 attempts.", i, base_pos); }
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
            let bot_names = ["Alpha", "Beta", "Gamma", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India", "Juliet", "Kilo", "Lima", "Mike", "November", "Oscar", "Papa", "Quebec", "Romeo", "Sierra", "Tango", "Uniform", "Victor", "Whiskey", "Xray", "Yankee", "Zulu"];
            let bot_name = format!("Bot {}", bot_names.get(bot_name_num as usize % bot_names.len()).unwrap_or(&"X"));
            let bot_player_id_str = format!("bot_{}", Uuid::new_v4());

            let team_id = (i % 2) + 1;

            let potential_spawns_for_team: Vec<Vec2> = team_spawn_areas.iter()
                .filter(|(_, sp_team_id)| *sp_team_id == team_id as u8)
                .map(|(pos, _)| *pos)
                .collect();

            let spawn_pos = if !potential_spawns_for_team.is_empty() {
                // Use team spawn point with some random offset
                let base_spawn = potential_spawns_for_team[rng.gen_range(0..potential_spawns_for_team.len())];
                let offset_radius = 50.0; // Small offset to prevent stacking
                let angle = rng.gen_range(0.0..2.0 * std::f32::consts::PI);
                let offset_x = offset_radius * angle.cos();
                let offset_y = offset_radius * angle.sin();
                Vec2::new(
                    (base_spawn.x + offset_x).clamp(WORLD_MIN_X + PLAYER_RADIUS, WORLD_MAX_X - PLAYER_RADIUS),
                    (base_spawn.y + offset_y).clamp(WORLD_MIN_Y + PLAYER_RADIUS, WORLD_MAX_Y - PLAYER_RADIUS)
                )
            } else {
                // Fallback: use respawn manager
                self.respawn_manager.get_respawn_position(self, &Arc::new(bot_player_id_str.clone()), Some(team_id as u8), &[])
            };

            if let Some(player_id_arc) = self.player_manager.add_player(bot_player_id_str.clone(), bot_name.clone(), spawn_pos.x, spawn_pos.y) {
                if let Some(mut p_state) = self.player_manager.get_player_state_mut(&player_id_arc) {
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
                debug!("Spawned bot: {} (ID: {}) on team {} at ({:.1}, {:.1})", bot_name, bot_player_id_str, team_id, spawn_pos.x, spawn_pos.y);
            } else {
                error!("Failed to add bot {} to player manager.", bot_name);
            }
        }
    }

    fn apply_input_to_player_state(&self, player_state: &mut PlayerState, input: &PlayerInputData, current_server_time: Instant) {
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

        if input.move_forward { forward_intent += 1.0; }
        if input.move_backward { forward_intent -= 1.0; }
        if input.move_left { strafe_intent -= 1.0; }
        if input.move_right { strafe_intent += 1.0; }

        let effective_speed = if player_state.speed_boost_remaining > 0.0 { PLAYER_BASE_SPEED * MAX_PLAYER_SPEED_MULTIPLIER } else { PLAYER_BASE_SPEED };

        if forward_intent != 0.0 || strafe_intent != 0.0 {
            // Normalize movement vector
            let move_magnitude = (forward_intent * forward_intent + strafe_intent * strafe_intent).sqrt();
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
        if input.shooting && player_state.weapon != ServerWeaponType::Melee && player_state.can_shoot(current_server_time) {
            player_state.last_shot_time = Some(current_server_time);
            player_state.ammo -= 1;
            player_state.mark_field_changed(FIELD_WEAPON_AMMO);

            let spawn_offset = PLAYER_RADIUS + 5.0;
            let proj_spawn_x = player_state.x + player_state.rotation.cos() * spawn_offset;
            let proj_spawn_y = player_state.y + player_state.rotation.sin() * spawn_offset;

            let damage_multiplier = if player_state.damage_boost_remaining > 0.0 { 1.5 } else { 1.0 };

            self.global_game_events.push(
                GameEvent::WeaponFired { player_id: player_state.id.clone(), weapon: player_state.weapon, position: Vec2{x: proj_spawn_x, y: proj_spawn_y}},
                EventPriority::Normal
            );

            match player_state.weapon {
                ServerWeaponType::Shotgun => {
                    for _ in 0..SHOTGUN_PELLET_COUNT { // Changed i to _ as i is not used
                        let angle_offset = SHOTGUN_SPREAD_ANGLE_RAD * (2.0 * (rand::random::<f32>()) - 1.0); // Simplified spread
                        let dir_x = player_state.rotation.cos() * angle_offset.cos() - player_state.rotation.sin() * angle_offset.sin();
                        let dir_y = player_state.rotation.sin() * angle_offset.cos() + player_state.rotation.cos() * angle_offset.sin();
                        self.projectiles_to_add.push(Projectile::new(
                            player_state.id.clone(),
                            player_state.weapon,
                            proj_spawn_x, proj_spawn_y,
                            dir_x, dir_y,
                            damage_multiplier,
                        ));
                    }
                }
                // ServerWeaponType::Melee is handled by the separate melee_attack check below
                _ => { // Pistol, Rifle, Sniper
                    self.projectiles_to_add.push(Projectile::new(
                        player_state.id.clone(),
                        player_state.weapon,
                        proj_spawn_x, proj_spawn_y,
                        player_state.rotation.cos(), player_state.rotation.sin(),
                        damage_multiplier,
                    ));
                }
            }
        }

        // Melee Attack Logic (V key)
        if input.melee_attack && player_state.can_shoot(current_server_time) { // Using can_shoot for cooldown & alive check
            player_state.last_shot_time = Some(current_server_time); // Apply melee cooldown

            // Position for the melee event (e.g., slightly in front of the player)
            let melee_event_pos_x = player_state.x + player_state.rotation.cos() * (PLAYER_RADIUS + 1.0);
            let melee_event_pos_y = player_state.y + player_state.rotation.sin() * (PLAYER_RADIUS + 1.0);

            debug!("[{}] initiated Melee Attack (V key).", player_state.id);
            self.global_game_events.push(
                GameEvent::MeleeHit {
                    attacker_id: player_state.id.clone(),
                    target_id: None, // Target is resolved in game_logic_update's MeleeHit processing
                    position: Vec2 { x: melee_event_pos_x, y: melee_event_pos_y }
                },
                EventPriority::Normal
            );
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
        self.player_manager.for_each_player_mut(|player_id, player_state| {
            player_state.clear_changed_fields();
            let inputs: Vec<PlayerInputData> = player_state.input_queue.drain(..).collect();
            if !inputs.is_empty() {
                all_inputs.push((player_id.clone(), inputs));
            }
        });
        
        // Then process each player's inputs
        for (player_id, inputs) in all_inputs {
            if let Some(mut player_state_entry) = self.player_manager.get_player_state_mut(&player_id) {
                for input in inputs {
                    self.apply_input_to_player_state(&mut *player_state_entry, &input, current_server_time);
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
    let respawned_walls = if frame % 30 == 0 { // 
        let templates = self.wall_respawn_manager.as_ref().check_respawns(); // 
        if !templates.is_empty() {
            // CHANGED to debug!
            debug!("[Frame {}]: Respawning {} walls (took {:?})", frame, templates.len(), respawn_stage_start.elapsed());
            self.process_wall_respawns(templates).await // 
        } else { Vec::new() }
    } else { Vec::new() };
    
    // Update wall spatial index if walls were respawned, destroyed, or if it needs periodic rebuild
    let destroyed_walls_count = self.destroyed_wall_ids_this_tick.read().len();
    let needs_wall_index_rebuild = !respawned_walls.is_empty() || 
                                   destroyed_walls_count > 0 ||
                                   self.wall_spatial_index.needs_rebuild(frame, 150); // Rebuild every 150 frames
    
    if needs_wall_index_rebuild {
        let index_rebuild_start = Instant::now();
        let active_walls = self.collect_active_walls_optimized();
        self.wall_spatial_index.rebuild(&active_walls, frame);
        debug!("[Frame {}] Wall spatial index rebuilt in {:?} (respawned: {}, destroyed: {})", 
            frame, index_rebuild_start.elapsed(), respawned_walls.len(), destroyed_walls_count);
    }
        
        // Stage 2: Collect Active Walls
        let collect_walls_start = Instant::now();
        let active_walls = self.get_active_walls_cached(frame).await; // 
        // CHANGED to debug!
        debug!("Frame {}: Collected {} active walls (took {:?})", frame, active_walls.len(), collect_walls_start.elapsed());
    
        // Stage 3: Process Player Physics
        let player_physics_start = Instant::now();
        let player_updates = self.process_player_physics_parallel(&active_walls, delta_time).await; // 
        // CHANGED to debug!
        debug!("Frame {}: Processed {} player physics updates (took {:?})", frame, player_updates.players_to_respawn.len() + player_updates.alive_count, player_physics_start.elapsed());
    
        // Stage 4: Apply Player Updates
        let apply_updates_start = Instant::now();
        self.apply_player_updates(player_updates).await; // 
        // CHANGED to debug!
        debug!("Frame {}: Applied player updates (took {:?})", frame, apply_updates_start.elapsed());
        
        // Stage 5: Process Projectiles
        let projectiles_start = Instant::now();
        let projectile_results = self.process_projectiles_optimized(&active_walls, delta_time).await; // 
        // CHANGED to debug!
        debug!("Frame {}: Processed {} projectiles, {} hits, {} removed (took {:?})", frame, projectile_results.total_processed, projectile_results.hits.len(), projectile_results.to_remove.len(), projectiles_start.elapsed());
    
        // Stage 6: Apply Projectile Results
        let apply_projectiles_start = Instant::now();
        self.apply_projectile_results(projectile_results).await; // 
        // CHANGED to debug!
        debug!("Frame {}: Applied projectile results (took {:?})", frame, apply_projectiles_start.elapsed());
        
        // Stage 7: Process Pickups
        let pickups_start = Instant::now();
        self.process_pickup_respawns(delta_time).await; // 
        // CHANGED to debug!
        debug!("Frame {}: Processed pickups (took {:?})", frame, pickups_start.elapsed());
        
        // This overall timing can remain info if you want a less frequent summary,
        // but if it's per-frame, debug is better.
        // For a true summary, this should be outside this function, logged less often.
        // Let's make it debug for now.
        debug!("Frame {}: TOTAL physics update took {:?}", frame, physics_start_time.elapsed());
    
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
                wall_template.y + wall_template.height / 2.0
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
            info!("[Wall Respawn] Updating player AOIs for {} respawned walls", respawned_ids.len());
            for mut aoi_entry in self.player_aois.iter_mut() {
                let aoi = aoi_entry.value_mut();
                for wall_id in &respawned_ids {
                    if !aoi.visible_walls.contains(wall_id) {
                        aoi.visible_walls.insert(*wall_id);
                        debug!("[Wall Respawn] Added respawned wall {} to player's AOI", wall_id);
                    }
                }
            }
        }
        
        respawned_ids
    }
    
    async fn get_active_walls_cached(&self, frame: u64) -> Arc<Vec<Wall>> {
        // Cache walls for a few frames since they don't change often
        static WALL_CACHE: OnceCell<Arc<ParkingLotRwLock<(u64, Arc<Vec<Wall>>)>>> = OnceCell::new();
        let cache = WALL_CACHE.get_or_init(|| Arc::new(ParkingLotRwLock::new((0, Arc::new(Vec::new())))));
        
        let cache_read = cache.read();
        if cache_read.0 + 5 > frame { // Cache for 5 frames
            return cache_read.1.clone();
        }
        drop(cache_read);
        
        // Rebuild cache
        let mut cache_write = cache.write();
        if cache_write.0 + 5 > frame { // Double-check after acquiring write lock
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

    let cache_entry_arc = CACHED_WALLS.get().expect("Wall cache should have been initialized in MassiveGameServer::new()");

    let structural_walls_from_cache = { // Read all structural walls
        let guard = cache_entry_arc.read();
         // Check if cache needs refresh based on frame number.
         // This simple check might need to be more sophisticated if walls change health often
         // outside of just being destroyed/respawned, but for now, let's assume
         // the cache primarily stores the structural layout.
        if guard.0 == frame || (guard.0 != u64::MAX && guard.0 >= frame.saturating_sub(10)) {
             debug!("[Frame {}] Using cached structural walls (cache frame {}, count {}).", frame, guard.0, guard.1.len());
            guard.1.clone()
        } else {
            // Cache is stale, need to rebuild it
            drop(guard); // Release read lock
            let mut write_guard = cache_entry_arc.write();
            // Double check after acquiring write lock
            if write_guard.0 == frame || (write_guard.0 != u64::MAX && write_guard.0 >= frame.saturating_sub(10)) {
                debug!("[Frame {}] Cache updated by another thread. Using new structural walls.", frame);
                write_guard.1.clone()
            } else {
                info!("[Frame {}] Rebuilding structural wall cache (was for frame {}).", frame, write_guard.0);
                let mut new_cache_walls = Vec::new();
                let partitions = self.world_partition_manager.get_partitions_for_processing();
                for partition in &partitions {
                    for entry in partition.all_walls_in_partition.iter() {
                        new_cache_walls.push(entry.value().clone());
                    }
                }
                info!("[Frame {}] Structural wall cache rebuilt with {} walls.", frame, new_cache_walls.len());
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
            let partition_idx = self.world_partition_manager.get_partition_index_for_point(wall_center_x, wall_center_y);
            
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
                debug!("[Frame {}] Filtering out destroyed wall {} (health: 0)", frame, cached_wall.id);
            }
        }
    }

    // This log will show the count of *active* walls
    debug!("[Frame {}] Collected {} active walls.", frame, active_walls.len());
    active_walls
}



    fn update_client_state_after_initial(
        &self, // Assuming this is part of MassiveGameServer impl
        peer_id_str: &str,
        shared_data: &SharedBroadcastData,
    ) {
        let frame_num = self.frame_counter.load(AtomicOrdering::Relaxed);
        trace!("[Frame {}] Client {}: Preparing to set initial ClientState in DashMap.", frame_num, peer_id_str);
        let mut client_state = ClientState::default(); // Create new state
        client_state.known_walls_sent = true; // Mark walls as sent
        client_state.last_update_sent_time = Instant::now();
    
        client_state.last_known_match_state = Some(shared_data.match_info_snapshot.match_state);
        client_state.last_known_match_time_remaining = Some(shared_data.match_info_snapshot.time_remaining);
        client_state.last_known_team_scores = shared_data.match_info_snapshot.team_scores.clone();
    
        let self_player_id_arc = self.player_manager.id_pool.get_or_create(peer_id_str);
        if let Some(self_pstate_guard) = self.player_manager.get_player_state(&self_player_id_arc) {
            client_state.last_known_player_states.insert(self_player_id_arc.clone(), (*self_pstate_guard).clone());
        }
    
        if let Some(aoi_entry) = self.player_aois.get(peer_id_str) {
            let p_aoi = aoi_entry.value();
            for visible_player_id in p_aoi.visible_players.iter() {
                if let Some(pstate_guard) = self.player_manager.get_player_state(visible_player_id) {
                    client_state.last_known_player_states.insert(visible_player_id.clone(), (*pstate_guard).clone());
                }
            }
            client_state.last_known_projectile_ids = p_aoi.visible_projectiles.clone();
            let pickups_guard = self.pickups.read();
            for pickup_id in p_aoi.visible_pickups.iter() {
                if let Some(pickup) = pickups_guard.iter().find(|p| p.id == *pickup_id) {
                    client_state.last_known_pickup_states.insert(*pickup_id, PickupState { is_active: pickup.is_active });

                }
            }
        }
    
        let key_for_insert = peer_id_str.to_string();
        trace!("[Frame {}] Client {}: ABOUT TO INSERT initial ClientState into client_states_map. Key: {}", frame_num, peer_id_str, key_for_insert);
        self.client_states_map.write().insert(key_for_insert.clone(), client_state);
        trace!("[Frame {}] Client {}: SUCCESSFULLY INSERTED initial ClientState into client_states_map. Key: {}", frame_num, peer_id_str, key_for_insert);
    }

    pub(crate) async fn send_chat_messages_static(
        _peer_id_str: &str,
        data_channel: &Arc<crate::core::types::RTCDataChannel>,
        client_state: &mut ClientState,
        chat_messages: &[ChatMessage],
        _chat_messages_queue: &ChatMessagesQueue,
    ) {
        let last_seq_sent = client_state.last_chat_message_seq_sent;
        let mut max_seq_in_batch = last_seq_sent;
        
        let messages_to_send: Vec<&ChatMessage> = chat_messages
            .iter()
            .filter(|msg| msg.seq > last_seq_sent)
            .take(10) // Limit messages per update
            .collect();
        
        for chat_entry in messages_to_send {
            let mut chat_builder = flatbuffers::FlatBufferBuilder::with_capacity(256);
            
            let player_id_fb = chat_builder.create_string(chat_entry.player_id.as_str());
            let username_fb = chat_builder.create_string(&chat_entry.username);
            let message_fb = chat_builder.create_string(&chat_entry.message);
            
            let chat_payload_offset = fb::ChatMessage::create(&mut chat_builder, &fb::ChatMessageArgs {
                seq: chat_entry.seq,
                player_id: Some(player_id_fb),
                username: Some(username_fb),
                message: Some(message_fb),
                timestamp: chat_entry.timestamp,
            });
            
            let game_message_offset = fb::GameMessage::create(&mut chat_builder, &fb::GameMessageArgs {
                msg_type: fb::MessageType::Chat,
                actual_message_type: fb::MessagePayload::ChatMessage,
                actual_message: Some(chat_payload_offset.as_union_value()),
            });
            
            chat_builder.finish(game_message_offset, None);
            let chat_msg_bytes = Bytes::from(chat_builder.finished_data().to_vec());
            
            let _ = data_channel.send(&chat_msg_bytes).await;
            
            if chat_entry.seq > max_seq_in_batch {
                max_seq_in_batch = chat_entry.seq;
            }
        }
        
        client_state.last_chat_message_seq_sent = max_seq_in_batch;
    }

    fn update_client_state_after_delta(
        &self,
        client_state: &mut ClientState,
        player_id: &PlayerID,
    ) {
        // Get the player's current AoI
        let player_aoi = match self.player_aois.get(player_id.as_str()) {
            Some(aoi_entry) => aoi_entry.clone(),
            None => {
                debug!("No AoI found for player {} when updating client state", player_id.as_str());
                return;
            }
        };
        
        // Update last broadcast frame
        client_state.last_broadcast_frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        
        // CRITICAL FIX 1: Update projectile tracking
        // Clear old projectile IDs and populate with current visible ones
        client_state.last_known_projectile_ids.clear();
        for projectile_id in &player_aoi.visible_projectiles {
            client_state.last_known_projectile_ids.insert(*projectile_id);
        }
        
        // CRITICAL FIX 2: Update pickup tracking
        // Clear old pickup states and populate with current visible ones
        client_state.last_known_pickup_states.clear();
        
        // Get current pickup states from the world
        let pickups_guard = self.pickups.read();
        for pickup_id in &player_aoi.visible_pickups {
            if let Some(pickup) = pickups_guard.iter().find(|p| p.id == *pickup_id) {
                client_state.last_known_pickup_states.insert(
                    *pickup_id,
                    PickupState {
                        is_active: pickup.is_active,
                    }
                );
            }
        }
        drop(pickups_guard);
        
        // Update visible players tracking (this was likely already working)
        client_state.last_known_players.clear();
        for visible_player_id in &player_aoi.visible_players {
            client_state.last_known_players.insert(visible_player_id.clone());
        }
        
        // Update visible walls tracking if you have it
        if let Some(ref mut last_known_walls) = client_state.last_known_wall_ids {
            last_known_walls.clear();
            for wall_id in &player_aoi.visible_walls {
                last_known_walls.insert(*wall_id);
            }
        }
        
        trace!(
            "Updated client state for {}: {} projectiles, {} pickups, {} players tracked",
            player_id.as_str(),
            client_state.last_known_projectile_ids.len(),
            client_state.last_known_pickup_states.len(),
            client_state.last_known_players.len()
        );
    }
    
    // Update client state after delta
    fn update_client_state_after_delta_static(
        peer_id_str: &str,
        mut client_state: ClientState, 
        shared_data: &SharedBroadcastData,
        client_states_map: &ClientStatesMap,
        frame_num: u64, 
    ) {
        trace!("[Frame {}] Client {}: Preparing to update ClientState in RwLock<HashMap> (delta). Last seq sent: {}", 
            frame_num, peer_id_str, client_state.last_chat_message_seq_sent);
    
        // Update the state
        client_state.last_update_sent_time = Instant::now();
        
        for wall_id in &shared_data.destroyed_wall_ids {
            client_state.known_destroyed_wall_ids.insert(*wall_id);
        }
        
        client_state.last_known_match_state = Some(shared_data.match_info_snapshot.match_state);
        client_state.last_known_match_time_remaining = Some(shared_data.match_info_snapshot.time_remaining);
        client_state.last_known_team_scores = shared_data.match_info_snapshot.team_scores.clone();
        client_state.last_kill_feed_count_sent = shared_data.kill_feed_snapshot.len();
        
        let key_for_insert = peer_id_str.to_string();
        trace!("[Frame {}] Client {}: ABOUT TO INSERT ClientState into RwLock<HashMap>. Key: {}", 
            frame_num, peer_id_str, key_for_insert);
        
        // Acquire write lock only when needed
        client_states_map.write().insert(key_for_insert.clone(), client_state);
        
        trace!("[Frame {}] Client {}: SUCCESSFULLY INSERTED ClientState into RwLock<HashMap>. Key: {}", 
            frame_num, peer_id_str, key_for_insert);
    }

    
    


    // Helper function to get default PlayerAoI
    fn get_empty_player_aoi() -> PlayerAoI {
        PlayerAoI {
            visible_players: HashSet::new(),
            visible_projectiles: HashSet::new(),
            visible_pickups: HashSet::new(),
            visible_walls: HashSet::new(),
            last_update: Instant::now(),  // Added this field
        }
    }
    
    async fn process_player_physics_parallel(&self, walls: &[Wall], delta_time: f32) -> PlayerPhysicsResults {
        let wall_arc = Arc::new(walls.to_vec());
        let mut all_to_respawn = Vec::new();
        let mut total_alive = 0;
        
        // Process all players using for_each_player_mut
        self.player_manager.for_each_player_mut(|player_id, player_state| {
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
    
    fn process_player_movement_optimized(&self, player_state: &mut PlayerState, _walls: &[Wall], delta_time: f32) {
        let old_x = player_state.x;
        let old_y = player_state.y;
        
        // Debug logging for bot movement
        if player_state.username.starts_with("Bot") && (player_state.velocity_x != 0.0 || player_state.velocity_y != 0.0) {
            trace!("Bot {} physics: pos({:.1},{:.1}) vel({:.1},{:.1}) dt={:.3}", 
                player_state.username, old_x, old_y, player_state.velocity_x, player_state.velocity_y, delta_time);
        }
        
        // Apply velocity
        player_state.x += player_state.velocity_x * delta_time;
        player_state.y += player_state.velocity_y * delta_time;
        
        // Log position after velocity application
        if player_state.username.starts_with("Bot") && (old_x != player_state.x || old_y != player_state.y) {
            trace!("Bot {} moved to ({:.1},{:.1})", player_state.username, player_state.x, player_state.y);
        }
        
        // Quick bounds check first
        let half_radius = PLAYER_RADIUS;
        if player_state.x < WORLD_MIN_X + half_radius || 
           player_state.x > WORLD_MAX_X - half_radius ||
           player_state.y < WORLD_MIN_Y + half_radius || 
           player_state.y > WORLD_MAX_Y - half_radius {
            
            player_state.x = player_state.x.clamp(WORLD_MIN_X + half_radius, WORLD_MAX_X - half_radius);
            player_state.y = player_state.y.clamp(WORLD_MIN_Y + half_radius, WORLD_MAX_Y - half_radius);
            player_state.velocity_x = 0.0;
            player_state.velocity_y = 0.0;
            player_state.mark_field_changed(FIELD_POSITION_ROTATION);
            return;
        }
        
        // Use spatial index to query nearby walls
        let check_radius = PLAYER_RADIUS + 10.0; // Reduced from 50.0 since spatial index is efficient
        let nearby_walls = self.wall_spatial_index.query_radius(player_state.x, player_state.y, check_radius);
        
        // Check collision with nearby walls only
        for wall in nearby_walls.iter() {
            let closest_x = player_state.x.clamp(wall.x, wall.x + wall.width);
            let closest_y = player_state.y.clamp(wall.y, wall.y + wall.height);
            
            let dist_sq = (player_state.x - closest_x).powi(2) + (player_state.y - closest_y).powi(2);
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
        let max_dist = PLAYER_BASE_SPEED * MAX_PLAYER_SPEED_MULTIPLIER * delta_time + MAX_POSITION_DELTA_SLACK;
        let actual_dist = ((player_state.x - player_state.last_valid_position.0).powi(2) + 
                          (player_state.y - player_state.last_valid_position.1).powi(2)).sqrt();
        
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
                &enemies
            );
            
            if let Some(mut p_state) = self.player_manager.get_player_state_mut(&player_id) {
                p_state.respawn(spawn_pos.x, spawn_pos.y);
                self.global_game_events.push(
                    GameEvent::PlayerJoined { player_id: player_id.clone() },
                    EventPriority::High
                );
            }
        }
    }

    // In massive_game_server/server/src/server/instance.rs

    async fn process_projectiles_optimized(&self, walls: &[Wall], delta_time: f32) -> ProjectileResults {
        use rayon::prelude::*;
        use std::sync::Mutex;
        
        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        trace!("[Frame {}] Starting optimized projectile processing", frame);
        
        // Get all projectiles as a vector for parallel processing
        let mut all_projectiles = {
            let mut guard = self.projectiles.write();
            std::mem::take(&mut *guard)
        };
        
        let total_projectiles = all_projectiles.len();
        trace!("[Frame {}] Processing {} projectiles", frame, total_projectiles);
        
        // Shared results that will be updated by parallel workers
        let hits = Arc::new(Mutex::new(Vec::new()));
        let wall_hits = Arc::new(Mutex::new(Vec::new()));
        let spatial_updates = Arc::new(Mutex::new(Vec::new()));
        
        // Process projectiles in parallel chunks
        let chunk_size = 50.max(total_projectiles / rayon::current_num_threads());
        
        let to_remove: Vec<usize> = all_projectiles
            .par_chunks_mut(chunk_size)
            .enumerate()
            .flat_map(|(chunk_idx, chunk)| {
                let mut chunk_to_remove = Vec::new();
                let chunk_start_idx = chunk_idx * chunk_size;
                
                for (local_idx, proj) in chunk.iter_mut().enumerate() {
                    let global_idx = chunk_start_idx + local_idx;
                    
                    // Update position
                    let old_x = proj.x;
                    let old_y = proj.y;
                    proj.x += proj.velocity_x * delta_time;
                    proj.y += proj.velocity_y * delta_time;
                    
                    // Collect spatial update (will be applied after parallel phase)
                    spatial_updates.lock().unwrap().push((proj.id, proj.x, proj.y));
                    
                    // Check bounds
                    if proj.x < WORLD_MIN_X || proj.x > WORLD_MAX_X || 
                    proj.y < WORLD_MIN_Y || proj.y > WORLD_MAX_Y {
                        chunk_to_remove.push(global_idx);
                        continue;
                    }
                    
                    // Check lifetime
                    if proj.should_remove() {
                        chunk_to_remove.push(global_idx);
                        continue;
                    }
                    
                    // Optimize wall collision by checking partition
                    let partition_idx = self.world_partition_manager.get_partition_index_for_point(proj.x, proj.y);
                    
                    if let Some(partition) = self.world_partition_manager.get_partition(partition_idx) {
                        let mut hit_wall = false;
                        
                        // Use ray casting for better collision detection
                        let ray_length = ((proj.x - old_x).powi(2) + (proj.y - old_y).powi(2)).sqrt();
                        let ray_steps = (ray_length / 5.0).ceil() as usize; // Check every 5 units
                        
                        'wall_check: for step in 0..=ray_steps {
                            let t = step as f32 / ray_steps.max(1) as f32;
                            let check_x = old_x + (proj.x - old_x) * t;
                            let check_y = old_y + (proj.y - old_y) * t;
                            
                            // Check walls in current position
                            for wall_entry in partition.all_walls_in_partition.iter() {
                                let wall = wall_entry.value();
                                
                                // Skip destroyed destructible walls
                                if wall.is_destructible && wall.current_health <= 0 {
                                    continue;
                                }
                                
                                // Projectile-wall collision
                                if check_x >= wall.x && check_x <= wall.x + wall.width &&
                                check_y >= wall.y && check_y <= wall.y + wall.height {
                                    
                                    // Set projectile position to collision point
                                    proj.x = check_x;
                                    proj.y = check_y;
                                    
                                    if wall.is_destructible {
                                        wall_hits.lock().unwrap().push((wall.id, proj.damage));
                                        
                                        // Generate impact event
                                        self.global_game_events.push(
                                            GameEvent::WallImpact {
                                                position: Vec2::new(check_x, check_y),
                                                wall_id: wall.id,
                                                damage: proj.damage,
                                            },
                                            EventPriority::Normal
                                        );
                                    }
                                    
                                    chunk_to_remove.push(global_idx);
                                    hit_wall = true;
                                    break 'wall_check;
                                }
                            }
                        }
                        
                        if !hit_wall {
                            // Check player collisions using spatial index
                            let nearby_players = self.spatial_index.query_nearby_players(
                                proj.x, 
                                proj.y, 
                                PLAYER_RADIUS + 20.0 // Small buffer for fast projectiles
                            );
                            
                            for target_id in nearby_players {
                                if target_id == proj.owner_id {
                                    continue; // Can't hit yourself
                                }
                                
                                if let Some(target_state) = self.player_manager.get_player_state(&target_id) {
                                    if !target_state.alive {
                                        continue;
                                    }
                                    
                                    // More accurate collision using ray casting
                                    let mut hit = false;
                                    for step in 0..=ray_steps {
                                        let t = step as f32 / ray_steps.max(1) as f32;
                                        let check_x = old_x + (proj.x - old_x) * t;
                                        let check_y = old_y + (proj.y - old_y) * t;
                                        
                                        let dx = target_state.x - check_x;
                                        let dy = target_state.y - check_y;
                                        let dist_sq = dx * dx + dy * dy;
                                        
                                        if dist_sq <= PLAYER_RADIUS * PLAYER_RADIUS {
                                            proj.x = check_x;
                                            proj.y = check_y;
                                            hit = true;
                                            break;
                                        }
                                    }
                                    
                                    if hit {
                                        hits.lock().unwrap().push((
                                            proj.owner_id.clone(),
                                            target_id.clone(),
                                            proj.damage,
                                            proj.weapon_type
                                        ));
                                        chunk_to_remove.push(global_idx);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                
                chunk_to_remove
            })
            .collect();
        
        // Apply spatial updates
        let spatial_updates_vec = spatial_updates.lock().unwrap().clone();
        self.spatial_index.batch_update_projectiles(&spatial_updates_vec);
        
        // Remove dead projectiles
        let to_remove_set: HashSet<_> = to_remove.into_iter().collect();
        let mut kept_projectiles = Vec::with_capacity(all_projectiles.len());
        let mut removed_ids = Vec::new();
        
        for (idx, proj) in all_projectiles.into_iter().enumerate() {
            if to_remove_set.contains(&idx) {
                removed_ids.push(proj.id);
            } else {
                kept_projectiles.push(proj);
            }
        }
        
        // Clean up spatial index for removed projectiles
        for proj_id in &removed_ids {
            self.spatial_index.remove_projectile(proj_id);
        }
        
        // Put remaining projectiles back
        *self.projectiles.write() = kept_projectiles;
        
        // Process wall damage
        let wall_hits_vec = wall_hits.lock().unwrap().clone();
        for (wall_id, damage) in &wall_hits_vec {
            let partition_idx = self.world_partition_manager
                .get_partitions_for_processing()
                .iter()
                .position(|p| p.all_walls_in_partition.contains_key(wall_id));
                
            if let Some(idx) = partition_idx {
                if let Some(partition) = self.world_partition_manager.get_partition(idx) {
                    if let Some((destroyed, pos)) = partition.damage_destructible_wall(*wall_id, *damage) {
                        if destroyed {
                            self.global_game_events.push(
                                GameEvent::WallDestroyed {
                                    wall_id: *wall_id,
                                    position: pos,
                                },
                                EventPriority::High
                            );
                            self.destroyed_wall_ids_this_tick.write().insert(*wall_id);
                            self.wall_respawn_manager.wall_destroyed(*wall_id);
                        }
                    }
                }
            }
        }
        
        let hits_vec = hits.lock().unwrap().clone();
        
        trace!(
            "[Frame {}] Projectile processing complete: {} processed, {} hits, {} wall hits, {} removed",
            frame, total_projectiles, hits_vec.len(), wall_hits_vec.len(), removed_ids.len()
        );
        
        ProjectileResults {
            total_processed: total_projectiles,
            hits: hits_vec,
            wall_hits: wall_hits_vec,
            to_remove: Vec::new(), // Already handled
        }
    }



    
    async fn apply_projectile_results(&self, results: ProjectileResults) {
        // Track if we need to rebuild spatial index
        let mut walls_destroyed = false;
        
        // Process hits - reuse existing game logic
        for (attacker_id, target_id, damage, weapon) in results.hits {
            if let Some(mut target_state_entry) = self.player_manager.get_player_state_mut(&target_id) {
                if target_state_entry.alive {
                    let died = target_state_entry.apply_damage(damage);
                    let target_pos = Vec2::new(target_state_entry.x, target_state_entry.y);
                    
                    self.global_game_events.push(GameEvent::PlayerDamaged {
                        target_id: target_id.clone(),
                        attacker_id: Some(attacker_id.clone()),
                        damage,
                        weapon,
                        position: target_pos,
                    }, EventPriority::Normal);
                    
                    if died {
                        // Store flag carry state before clearing it
                        let victim_was_carrying_flag_id = target_state_entry.is_carrying_flag_team_id;
                        let victim_username = target_state_entry.username.clone();
                        
                        // Clear flag carry state on the victim
                        if victim_was_carrying_flag_id != 0 {
                            target_state_entry.is_carrying_flag_team_id = 0;
                            target_state_entry.mark_field_changed(FIELD_FLAG);
                        }
                        
                        // Handle death (existing logic from run_physics_update)
                        if attacker_id != target_id {
                            // Get team information for friendly fire check
                            let attacker_team = self.player_manager.get_player_state(&attacker_id)
                                .map(|p| p.team_id)
                                .unwrap_or(0);
                            let victim_team = target_state_entry.team_id;
                            
                            if let Some(mut attacker_state_entry) = self.player_manager.get_player_state_mut(&attacker_id) {
                                attacker_state_entry.kills += 1;
                                
                                // Check for friendly fire
                                if attacker_team != 0 && victim_team != 0 && attacker_team == victim_team {
                                    // Friendly fire: double negative score
                                    attacker_state_entry.score -= 200;
                                    info!("Friendly fire penalty: {} killed teammate {}, -200 score", 
                                          attacker_state_entry.username, victim_username);
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
                                let attacker_team = self.player_manager.get_player_state(&attacker_id)
                                    .map(|p| p.team_id)
                                    .unwrap_or(0);
                                let victim_team = target_state_entry.team_id;
                                
                                // Award point to attacker's team if it's a valid team kill
                                if attacker_team != 0 && victim_team != 0 && attacker_team != victim_team {
                                    let mut match_info_write = self.match_info.write();
                                    let team_score = match_info_write.team_scores.entry(attacker_team).or_insert(0);
                                    *team_score += 1;
                                    info!("Team {} scored! New score: {} (kill by player on victim from team {})", 
                                          attacker_team, *team_score, victim_team);
                                }
                            }
                        }
                        
                        self.global_game_events.push(GameEvent::PlayerKilled {
                            victim_id: target_id.clone(),
                            killer_id: attacker_id.clone(),
                            weapon,
                            position: target_pos,
                        }, EventPriority::High);
                        
                        // Update kill feed
                        let killer_username = self.player_manager.get_player_state(&attacker_id)
                            .map_or_else(|| "World".to_string(), |p| p.username.clone());
                        
                        let mut kill_feed_guard = self.kill_feed.write();
                        kill_feed_guard.push_back(ServerKillFeedEntry {
                            killer_name: killer_username.clone(),
                            victim_name: victim_username.clone(),
                            weapon,
                            timestamp: self.frame_counter.load(AtomicOrdering::Relaxed),
                        });
                        if kill_feed_guard.len() > MAX_KILL_FEED_HISTORY {
                            kill_feed_guard.pop_front();
                        }
                        drop(kill_feed_guard);
                        
                        // Handle flag dropping if victim was carrying a flag
                        if victim_was_carrying_flag_id != 0 {
                            let mut match_info_guard = self.match_info.write();
                            
                            // Drop the flag
                            if let Some(flag_state) = match_info_guard.flag_states.get_mut(&victim_was_carrying_flag_id) {
                                flag_state.status = fb::FlagStatus::Dropped;
                                flag_state.position = target_pos;
                                flag_state.carrier_id = None;
                                flag_state.respawn_timer = 30.0;
                                
                                // Push flag dropped event after releasing match_info lock
                                drop(match_info_guard);
                                
                                self.global_game_events.push(GameEvent::FlagDropped {
                                    player_id: target_id.clone(), 
                                    flag_team_id: victim_was_carrying_flag_id, 
                                    position: target_pos
                                }, EventPriority::High);
                                
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
        for pickup in pickups_guard.iter_mut() {
            if !pickup.is_active {
                if let Some(timer) = &mut pickup.respawn_timer {
                    *timer -= delta_time;
                    if *timer <= 0.0 {
                        pickup.is_active = true;
                        pickup.respawn_timer = None;
                    }
                }
            }
        }
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
            partition_arc.all_walls_in_partition.iter().for_each(|wall_entry| {
                let wall = wall_entry.value();
                // Send ALL walls including destroyed ones - client needs to render them as rubble/obstacles
                all_walls.push(wall.clone());
            });
        }
        all_walls
    }


    pub async fn run_game_logic_update(&self, delta_time: f32) {
        // Process projectiles to be added to the main simulation
        while let Some(proj) = self.projectiles_to_add.pop() {
            self.projectiles.write().push(proj);
        }

        // Update match state (timer, transitions)
        {
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
                        if match_info_guard.game_mode == fb::GameModeType::TeamDeathmatch || match_info_guard.game_mode == fb::GameModeType::CaptureTheFlag {
                            let team1_score = match_info_guard.team_scores.get(&1).cloned().unwrap_or(0);
                            let team2_score = match_info_guard.team_scores.get(&2).cloned().unwrap_or(0);
                            
                            // Determine and announce the winner
                            if team1_score > team2_score {
                                info!("Team 1 wins with {} points vs Team 2's {} points!", team1_score, team2_score);
                            } else if team2_score > team1_score {
                                info!("Team 2 wins with {} points vs Team 1's {} points!", team2_score, team1_score);
                            } else if team1_score == team2_score && team1_score > 0 {
                                info!("Match ended in a draw! Both teams scored {} points.", team1_score);
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

        // Player pickup collection logic
        self.player_manager.for_each_player_mut(|player_id_arc_for_pickup, player_state_for_pickup| {
            if !player_state_for_pickup.alive { return; }

            let mut pickups_guard = self.pickups.write();
            for pickup_idx in 0..pickups_guard.len() {
                if pickups_guard[pickup_idx].is_active {
                    let pickup_ref = &pickups_guard[pickup_idx];
                    let dx = player_state_for_pickup.x - pickup_ref.x;
                    let dy = player_state_for_pickup.y - pickup_ref.y;

                    if (dx * dx + dy * dy) < (PICKUP_COLLECTION_RADIUS * PICKUP_COLLECTION_RADIUS) {
                        let mut collected = false;
                        let pickup_pos = Vec2::new(pickup_ref.x, pickup_ref.y);
                        let pickup_id_event = pickup_ref.id;
                        let pickup_type_event = pickup_ref.pickup_type.clone();
                        let mut_pickup = &mut pickups_guard[pickup_idx];

                        match &mut_pickup.pickup_type {
                            CorePickupType::Health => {
                                if player_state_for_pickup.health < player_state_for_pickup.max_health {
                                    player_state_for_pickup.health = (player_state_for_pickup.health + 50).min(player_state_for_pickup.max_health);
                                    player_state_for_pickup.mark_field_changed(FIELD_HEALTH_ALIVE);
                                    collected = true;
                                }
                            }
                            CorePickupType::Ammo => {
                                player_state_for_pickup.ammo = PlayerState::get_max_ammo_for_weapon(player_state_for_pickup.weapon);
                                player_state_for_pickup.mark_field_changed(FIELD_WEAPON_AMMO);
                                collected = true;
                            }
                            CorePickupType::WeaponCrate(weapon) => {
                                player_state_for_pickup.weapon = *weapon;
                                player_state_for_pickup.ammo = PlayerState::get_max_ammo_for_weapon(*weapon);
                                player_state_for_pickup.reload_progress = None;
                                player_state_for_pickup.mark_field_changed(FIELD_WEAPON_AMMO);
                                collected = true;
                            }
                            CorePickupType::SpeedBoost => {
                                player_state_for_pickup.speed_boost_remaining = 10.0;
                                player_state_for_pickup.mark_field_changed(FIELD_POWERUPS);
                                collected = true;
                            }
                            CorePickupType::DamageBoost => {
                                player_state_for_pickup.damage_boost_remaining = 10.0;
                                player_state_for_pickup.mark_field_changed(FIELD_POWERUPS);
                                collected = true;
                            }
                            CorePickupType::Shield => {
                                player_state_for_pickup.shield_max = 50;
                                player_state_for_pickup.shield_current = player_state_for_pickup.shield_max;
                                player_state_for_pickup.mark_field_changed(FIELD_SHIELD);
                                collected = true;
                            }
                        }

                        if collected {
                            mut_pickup.is_active = false;
                            mut_pickup.respawn_timer = Some(mut_pickup.get_respawn_duration());
                            self.global_game_events.push(GameEvent::PowerupCollected {
                                player_id: player_id_arc_for_pickup.clone(),
                                pickup_id: pickup_id_event,
                                pickup_type: pickup_type_event,
                                position: pickup_pos,
                            }, EventPriority::Normal);
                            break;
                        }
                    }
                }
            }
        });

        // CTF Logic
        let mut match_info_write_guard = self.match_info.write();
        if match_info_write_guard.game_mode == fb::GameModeType::CaptureTheFlag && match_info_write_guard.match_state == fb::MatchStateType::Active {
            for flag_state in match_info_write_guard.flag_states.values_mut() {
                if flag_state.status == fb::FlagStatus::Dropped && flag_state.respawn_timer > 0.0 {
                    flag_state.respawn_timer -= delta_time;
                    if flag_state.respawn_timer <= 0.0 {
                        flag_state.respawn_timer = 0.0;
                        flag_state.status = fb::FlagStatus::AtBase;
                        flag_state.position = Self::get_flag_base_position(flag_state.team_id);
                        flag_state.carrier_id = None;
                        self.global_game_events.push(GameEvent::FlagReturned {
                            player_id: Arc::new("server".to_string()),
                            flag_team_id: flag_state.team_id,
                            position: flag_state.position
                        }, EventPriority::High);
                        info!("Flag of team {} auto-returned to base.", flag_state.team_id);
                    }
                }
            }

            let mut player_snapshots: HashMap<PlayerID, PlayerState> = HashMap::new();
             self.player_manager.for_each_player(|id, state| {
                player_snapshots.insert(id.clone(), state.clone());
            });

            for (player_id_arc, player_state_snapshot) in &player_snapshots {
                if !player_state_snapshot.alive { continue; }

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
                            },
                            _ => false
                        };
                        
                        if can_interact {
                            let dx = player_state_snapshot.x - flag_state.position.x;
                            let dy = player_state_snapshot.y - flag_state.position.y;
                            if (dx * dx + dy * dy) < (PICKUP_COLLECTION_RADIUS * PICKUP_COLLECTION_RADIUS) {
                                if flag_state.team_id != player_state_snapshot.team_id {
                                    // Enemy picking up flag
                                    flag_state.status = fb::FlagStatus::Carried;
                                    flag_state.carrier_id = Some(player_id_arc.clone());
                                    if let Some(mut p_state_mut_entry) = self.player_manager.get_player_state_mut(player_id_arc) {
                                        let p_state_mut = &mut *p_state_mut_entry;
                                        p_state_mut.is_carrying_flag_team_id = flag_state.team_id;
                                        p_state_mut.mark_field_changed(FIELD_FLAG);
                                    }
                                    self.global_game_events.push(GameEvent::FlagGrabbed { player_id: player_id_arc.clone(), flag_team_id: flag_state.team_id, position: flag_state.position }, EventPriority::High);
                                    info!("Player {} grabbed flag of team {}", player_state_snapshot.username, flag_state.team_id);
                                    break;
                                } else if flag_state.status == fb::FlagStatus::Dropped && flag_state.team_id == player_state_snapshot.team_id {
                                    // Own team returning flag
                                    flag_state.status = fb::FlagStatus::AtBase;
                                    flag_state.position = Self::get_flag_base_position(flag_state.team_id);
                                    flag_state.carrier_id = None;
                                    flag_state.respawn_timer = 0.0;
                                    self.global_game_events.push(GameEvent::FlagReturned { player_id: player_id_arc.clone(), flag_team_id: flag_state.team_id, position: flag_state.position }, EventPriority::High);
                                    info!("Player {} returned own team {}'s flag.", player_state_snapshot.username, flag_state.team_id);
                                    break;
                                }
                            }
                        }
                    }
                }

                if player_state_snapshot.is_carrying_flag_team_id != 0 &&
                   player_state_snapshot.is_carrying_flag_team_id != player_state_snapshot.team_id {
                    let own_player_team_id = player_state_snapshot.team_id;

                    let own_flag_at_base = match_info_write_guard.flag_states.get(&own_player_team_id)
                        .map_or(false, |ofs| ofs.status == fb::FlagStatus::AtBase);

                    if own_flag_at_base {
                        let own_flag_base_pos = Self::get_flag_base_position(own_player_team_id);
                        let dx = player_state_snapshot.x - own_flag_base_pos.x;
                        let dy = player_state_snapshot.y - own_flag_base_pos.y;

                        if (dx*dx + dy*dy) < (PICKUP_COLLECTION_RADIUS * PICKUP_COLLECTION_RADIUS) {
                            let captured_flag_team_id = player_state_snapshot.is_carrying_flag_team_id;

                            if let Some(captured_flag) = match_info_write_guard.flag_states.get_mut(&captured_flag_team_id) {
                                captured_flag.status = fb::FlagStatus::AtBase;
                                captured_flag.position = Self::get_flag_base_position(captured_flag_team_id);
                                captured_flag.carrier_id = None;
                            }

                            if let Some(mut p_state_mut_entry) = self.player_manager.get_player_state_mut(player_id_arc) {
                                let p_state_mut = &mut *p_state_mut_entry;
                                p_state_mut.is_carrying_flag_team_id = 0;
                                p_state_mut.mark_field_changed(FIELD_FLAG);
                                p_state_mut.score += 100;
                                p_state_mut.mark_field_changed(FIELD_SCORE_STATS);
                            }

                            let team_score_mut_ref = match_info_write_guard.team_scores.entry(own_player_team_id).or_insert(0);
                            *team_score_mut_ref += 1;
                            let current_score = *team_score_mut_ref;

                            self.global_game_events.push(GameEvent::FlagCaptured {
                                capturer_id: player_id_arc.clone(),
                                captured_flag_team_id,
                                capturing_team_id: own_player_team_id,
                                position: own_flag_base_pos
                            }, EventPriority::High);
                            info!("Player {} captured team {}'s flag for team {}! (Score: {})", player_state_snapshot.username, captured_flag_team_id, own_player_team_id, current_score);

                            if current_score >= 3 {
                                 match_info_write_guard.match_state = fb::MatchStateType::Ended;
                                 info!("Team {} wins by capturing {} flags!", own_player_team_id, current_score);
                            }
                        }
                    }
                }
            }
        }
        drop(match_info_write_guard);

        // Melee Event Processing - Fix 1
        let mut melee_hit_events_to_process = Vec::new();
        let mut other_events_to_requeue = Vec::new();

        while let Some(event_popped) = self.global_game_events.pop() {
            if matches!(event_popped, GameEvent::MeleeHit { .. }) {
                melee_hit_events_to_process.push(event_popped);
            } else {
                other_events_to_requeue.push(event_popped);
            }
            if melee_hit_events_to_process.len() + other_events_to_requeue.len() > 200 {
                warn!("Event processing loop safety break triggered in run_game_logic_update.");
                break;
            }
        }

        // Process melee hits (extracted logic)
        self.process_melee_hits(melee_hit_events_to_process);

        // Re-queue other events *after* melee processing is done
        for event_to_requeue in other_events_to_requeue {
            self.global_game_events.push(event_to_requeue, EventPriority::Normal);
        }
        // End of Fix 1 for Melee

        self.manage_bot_population();
        // self.destroyed_wall_ids_this_tick.write().clear(); // Moved to process_game_tick
    }

    async fn process_client_broadcast(
        peer_id_str: &str, 
        client_info: &ClientInfo, 
        shared_data: &SharedBroadcastData,
        server: &Arc<MassiveGameServer>, // Correctly takes &Arc<MassiveGameServer>
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> { 
        let frame = server.frame_counter.load(AtomicOrdering::Relaxed);
        
        trace!("[Frame {}] Starting broadcast for client {}", frame, peer_id_str);
        
        let player_exists = {
            let player_id_arc = server.player_manager.id_pool.get_or_create(peer_id_str);
            server.player_manager.get_player_state(&player_id_arc).is_some()
        };
    
        if !player_exists {
            warn!("[Frame {}] Player {} not found, skipping broadcast for this client.", frame, peer_id_str);
            return Ok(()); 
        }
        
        let state_result = if client_info.needs_initial_state {
            trace!("[Frame {}] Building initial state for {}", frame, peer_id_str);
            server.build_initial_state_optimized(peer_id_str, shared_data).await
        } else {
            trace!("[Frame {}] Building delta state for {}", frame, peer_id_str);
            let client_state_snapshot = server.client_states_map
                .read() // Acquire read lock
                .get(peer_id_str)
                .map(|cs_state_ref| cs_state_ref.clone()) // Clone the ClientState from the &ClientState
                .unwrap_or_else(|| {
                    warn!("[Frame {}] ClientState not found for {} during delta build, using default. This might indicate a logic issue.", server.frame_counter.load(AtomicOrdering::Relaxed), peer_id_str);
                    ClientState::default() 
                });
            server.build_delta_state_optimized(peer_id_str, &client_state_snapshot, shared_data).await
        };
        
        let bytes_to_send = match state_result {
            Ok(b) => {
                trace!("[Frame {}] State built successfully for {} ({} bytes)", frame, peer_id_str, b.len());
                b
            }
            Err(_e) => { 
                error!("[Frame {}] Failed to build state for {}: {:?}", frame, peer_id_str, _e);
                return Err(format!("Failed to build state for client {}", peer_id_str).into());
            }
        };
        
        trace!("[Frame {}] Sending {} bytes to client {}", frame, bytes_to_send.len(), peer_id_str);
        
        const SEND_TIMEOUT_MS: u64 = 50; 
        match tokio::time::timeout(
            Duration::from_millis(SEND_TIMEOUT_MS),
            client_info.data_channel.send(&bytes_to_send)
        ).await {
            Ok(Ok(_)) => {
                trace!("[Frame {}] Data sent successfully to {}", frame, peer_id_str);
            }
            Ok(Err(e)) => {
                warn!("[Frame {}] Send error for client {}: {:?}", frame, peer_id_str, e);
            }
            Err(_) => { 
                warn!("[Frame {}] Send timeout for client {} after {}ms", frame, peer_id_str, SEND_TIMEOUT_MS);
            }
        }
        
        trace!("[Frame {}] Updating client state for {}", frame, peer_id_str);
        if client_info.needs_initial_state {
            server.update_client_state_after_initial(peer_id_str, shared_data);
        } else {
            // Fix: Properly handle the mutable client state update
            let player_id = server.player_manager.id_pool.get_or_create(peer_id_str);
            let mut client_states_guard = server.client_states_map.write();
            
            if let Some(client_state) = client_states_guard.get_mut(peer_id_str) {
                // Drop the guard before calling the method to avoid deadlock
                let mut client_state_clone = client_state.clone();
                drop(client_states_guard);
                
                server.update_client_state_after_delta(&mut client_state_clone, &player_id);
                
                // Re-acquire the lock and update
                server.client_states_map.write().insert(peer_id_str.to_string(), client_state_clone);
            }
        }
        
        if !client_info.needs_initial_state {
            // Get client state and immediately drop the lock guard
            let client_state_option = server.client_states_map
                .read()  // acquire lock
                .get(peer_id_str)
                .cloned();  // clone the value and drop the lock
            
            // Now we can safely await without holding the lock
            if let Some(client_state_for_chat) = client_state_option {
                server.send_chat_messages_optimized(
                    peer_id_str,
                    &client_info.data_channel,
                    &client_state_for_chat, 
                    &shared_data.chat_messages
                ).await;
            }
        }
        
        
        trace!("[Frame {}] Broadcast processing complete for client {}", frame, peer_id_str);
        Ok(())
    }

    fn get_server_timestamp(&self) -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    // Extracted melee processing logic
    fn process_melee_hits(&self, melee_hit_events: Vec<GameEvent>) {
        for event in melee_hit_events {
            if let GameEvent::MeleeHit { attacker_id, position: _attack_pos, .. } = event {
                let melee_range_sq = 50.0 * 50.0;
                let melee_arc_angle_rad = std::f32::consts::FRAC_PI_3;
                let melee_damage = 30;

                // Get attacker info
                let (attacker_pos_x, attacker_pos_y, attacker_rot, attacker_team_id, attacker_username) = {
                    if let Some(attacker_state_guard) = self.player_manager.get_player_state(&attacker_id) {
                        (
                            attacker_state_guard.x,
                            attacker_state_guard.y,
                            attacker_state_guard.rotation,
                            attacker_state_guard.team_id,
                            attacker_state_guard.username.clone()
                        )
                    } else {
                        continue; // Attacker not found
                    }
                };

                // Use spatial index for nearby players
                let melee_check_radius = 70.0;
                let nearby_player_ids = self.spatial_index.query_nearby_players(attacker_pos_x, attacker_pos_y, melee_check_radius);

                // Process each potential target
                for target_id_arc_nearby in nearby_player_ids {
                    if target_id_arc_nearby == attacker_id { continue; }

                    // Collect all the data we need from the target before applying damage
                    let target_hit_data = {
                        if let Some(mut target_state_entry) = self.player_manager.get_player_state_mut(&target_id_arc_nearby) {
                            let target_state = &mut *target_state_entry;

                            if !target_state.alive ||
                               (target_state.team_id != 0 && attacker_team_id != 0 && target_state.team_id == attacker_team_id) {
                                continue; // Skip dead or same-team targets
                            }

                            let dx = target_state.x - attacker_pos_x;
                            let dy = target_state.y - attacker_pos_y;
                            let dist_sq = dx*dx + dy*dy;

                            if dist_sq >= melee_range_sq {
                                continue; // Out of range
                            }

                            let angle_to_target = dy.atan2(dx);
                            let mut angle_diff = (angle_to_target - attacker_rot).rem_euclid(2.0 * std::f32::consts::PI);
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
                            let victim_was_carrying_flag_id = if died { target_state.is_carrying_flag_team_id } else { 0 };
                            
                            if died {
                                // Reset flag carry state on the victim
                                target_state.is_carrying_flag_team_id = 0;
                                target_state.mark_field_changed(FIELD_FLAG);
                            }

                            Some((died, target_position, target_username, victim_was_carrying_flag_id))
                        } else {
                            None
                        }
                    };

                    // Now process the hit results without holding any mutable borrows
                    if let Some((died, target_position, target_username, victim_was_carrying_flag_id)) = target_hit_data {
                        // Push damage event
                        self.global_game_events.push(GameEvent::PlayerDamaged {
                            target_id: target_id_arc_nearby.clone(),
                            attacker_id: Some(attacker_id.clone()),
                            damage: melee_damage,
                            weapon: ServerWeaponType::Melee,
                            position: target_position,
                        }, EventPriority::Normal);

                        if died {
                            // Update attacker stats
                            if attacker_id != target_id_arc_nearby {
                                // Get victim team for friendly fire check
                                let victim_team = self.player_manager.get_player_state(&target_id_arc_nearby)
                                    .map(|p| p.team_id)
                                    .unwrap_or(0);
                                
                                if let Some(mut attacker_mut_state_entry) = self.player_manager.get_player_state_mut(&attacker_id) {
                                    let attacker_mut_state = &mut *attacker_mut_state_entry;
                                    attacker_mut_state.kills += 1;
                                    
                                    // Check for friendly fire
                                    if attacker_team_id != 0 && victim_team != 0 && attacker_team_id == victim_team {
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
                            self.global_game_events.push(GameEvent::PlayerKilled {
                                victim_id: target_id_arc_nearby.clone(),
                                killer_id: attacker_id.clone(),
                                weapon: ServerWeaponType::Melee,
                                position: target_position,
                            }, EventPriority::High);

                            // Update kill feed
                            {
                                let mut kill_feed_guard = self.kill_feed.write();
                                kill_feed_guard.push_back(ServerKillFeedEntry {
                                    killer_name: attacker_username.clone(),
                                    victim_name: target_username,
                                    weapon: ServerWeaponType::Melee,
                                    timestamp: self.frame_counter.load(std::sync::atomic::Ordering::Relaxed),
                                });
                                if kill_feed_guard.len() > MAX_KILL_FEED_HISTORY { 
                                    kill_feed_guard.pop_front(); 
                                }
                            }

                            // Handle flag dropping if victim was carrying a flag
                            if victim_was_carrying_flag_id != 0 {
                                let mut match_info_guard = self.match_info.write();

                                // Award score to attacker's team if applicable
                                if let Some(attacker_state_for_score) = self.player_manager.get_player_state(&attacker_id) {
                                    if attacker_state_for_score.team_id != 0 && 
                                       attacker_state_for_score.team_id != victim_was_carrying_flag_id {
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
                                if let Some(flag_state) = match_info_guard.flag_states.get_mut(&victim_was_carrying_flag_id) {
                                    flag_state.status = fb::FlagStatus::Dropped;
                                    flag_state.position = target_position;
                                    flag_state.carrier_id = None;
                                    flag_state.respawn_timer = 30.0;
                                    
                                    // Push flag dropped event after releasing match_info lock
                                    drop(match_info_guard);
                                    
                                    self.global_game_events.push(GameEvent::FlagDropped {
                                        player_id: target_id_arc_nearby.clone(), 
                                        flag_team_id: victim_was_carrying_flag_id, 
                                        position: target_position
                                    }, EventPriority::High);
                                    
                                    info!("(Melee Kill) Flag of team {} dropped at ({:.1}, {:.1})", 
                                          victim_was_carrying_flag_id, target_position.x, target_position.y);
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
        match_info.flag_states.insert(1, ServerFlagState {
            team_id: 1,
            status: fb::FlagStatus::AtBase,
            position: team1_flag_pos,
            carrier_id: None,
            respawn_timer: 0.0,
        });
        let team2_flag_pos = Self::get_flag_base_position(2);
        match_info.flag_states.insert(2, ServerFlagState {
            team_id: 2,
            status: fb::FlagStatus::AtBase,
            position: team2_flag_pos,
            carrier_id: None,
            respawn_timer: 0.0,
        });
        info!("CTF Flags initialized. T1 at {:?}, T2 at {:?}", team1_flag_pos, team2_flag_pos);
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
        if match_info.match_state == fb::MatchStateType::Waiting && match_info.game_mode == fb::GameModeType::CaptureTheFlag {
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


    async fn prepare_shared_broadcast_data(&self) -> SharedBroadcastData {
        let current_timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        
        // Collect events efficiently
        let mut events = Vec::with_capacity(100);
        while let Some(event) = self.global_game_events.pop() {
            events.push(event);
            if events.len() >= 100 { break; }
        }
        
        // Snapshot destroyed walls
        let destroyed_wall_ids = self.destroyed_wall_ids_this_tick
            .read()
            .iter()
            .cloned()
            .collect();
        
        // Snapshot updated walls
        let updated_walls = self.updated_walls_this_tick
            .read()
            .clone();
        
        // Snapshot chat messages
        let chat_messages = self.chat_messages_queue
            .read()
            .await
            .iter()
            .cloned()
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
        let kill_feed_snapshot = self.kill_feed
            .read()
            .iter()
            .cloned()
            .collect();
        
        SharedBroadcastData {
            timestamp_ms: current_timestamp_ms,
            events,
            destroyed_wall_ids,
            updated_walls,
            chat_messages,
            match_info_snapshot,
            kill_feed_snapshot,
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

    
   


    #[allow(dead_code)] 
    async fn process_client_broadcast_static(
        peer_id_str: String,
        data_channel: Arc<crate::core::types::RTCDataChannel>,
        shared_data: &SharedBroadcastData,
        player_manager: &Arc<ImprovedPlayerManager>,
        player_aois: &PlayerAoIs, 
        client_states_map: &ClientStatesMap, 
        _world_partition_manager: &Arc<WorldPartitionManager>, 
        _pickups: &Arc<ParkingLotRwLock<Vec<Pickup>>>, 
        projectiles: &Arc<ParkingLotRwLock<Vec<Projectile>>>,
        kill_feed: &Arc<ParkingLotRwLock<VecDeque<ServerKillFeedEntry>>>,
        chat_messages_queue: &ChatMessagesQueue,
        frame_num: u64, 
    ) {
        let mut client_state_copy = client_states_map
            .read() 
            .get(&peer_id_str)
            .cloned() 
            .unwrap_or_else(|| ClientState::default());
        
        if !client_state_copy.known_walls_sent {
            warn!("[Frame {}] Client {} needs initial state in process_client_broadcast_static. This path might be deprecated or an error.", frame_num, peer_id_str);
            return;
        }
        
        let message_result = Self::build_delta_state_static(
            &peer_id_str,
            &client_state_copy,
            shared_data,
            player_manager,
            player_aois,
            _pickups, 
            projectiles,
            kill_feed,
        ).await;
        
        if let Ok(bytes) = message_result {
            if let Err(e) = data_channel.send(&bytes).await {
                handle_dc_send_error(&e.to_string(), &peer_id_str, "delta state (static path)");
            }
        }
        
        Self::send_chat_messages_static(
            &peer_id_str,
            &data_channel,
            &mut client_state_copy,
            &shared_data.chat_messages,
            chat_messages_queue,
        ).await;
        
        Self::update_client_state_after_delta_static(
            &peer_id_str,
            client_state_copy, 
            shared_data,
            client_states_map, 
            frame_num, 
        );
    }

    fn manage_bot_population(&self) { // Ensure this method is defined within the impl block
        let human_player_count = self.player_manager.player_count().saturating_sub(self.bot_players.len());
        let current_bot_count = self.bot_players.len();

        // Corrected line: Directly use the usize value from config
        let max_players_in_match = self.config.max_players_per_match;

        let desired_bot_count = if human_player_count >= max_players_in_match {
            0
        } else {
            (max_players_in_match - human_player_count).min(self.target_bot_count.load(std::sync::atomic::Ordering::Relaxed) as usize) // Also consider target_bot_count
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

fn spawn_additional_bots(&self, count_to_add: usize) {
    if count_to_add == 0 {
        return;
    }
    info!("[Bot Management] Attempting to spawn {} additional bots...", count_to_add);

    let team_spawn_areas = crate::world::map_generator::MapGenerator::get_team_spawn_areas();
    let mut rng = rand::thread_rng();
    let bot_names = ["Alpha", "Beta", "Gamma", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India", "Juliet", "Kilo", "Lima", "Mike", "November", "Oscar", "Papa", "Quebec", "Romeo", "Sierra", "Tango", "Uniform", "Victor", "Whiskey", "Xray", "Yankee", "Zulu"];


    for _i in 0..count_to_add { // _i as it's not directly used for bot naming index here
        let current_total_players = self.player_manager.player_count();
        if current_total_players >= self.config.max_players_per_match {
            info!("[Bot Management] Max player limit ({}) reached, stopping additional bot spawn. Current players: {}", self.config.max_players_per_match, current_total_players);
            break;
        }

        let bot_name_num = self.bot_name_counter.fetch_add(1, AtomicOrdering::SeqCst);
        let bot_base_name = bot_names.get(bot_name_num as usize % bot_names.len()).unwrap_or(&"Extra");
        let bot_name = format!("Bot {}{}", bot_base_name, if bot_name_num >= bot_names.len() as u64 { (bot_name_num / bot_names.len() as u64).to_string() } else { "".to_string() });

        let bot_player_id_str = format!("bot_{}", uuid::Uuid::new_v4());

        let mut team1_player_count = 0; // Count players (human + bot) on team 1
        let mut team2_player_count = 0; // Count players (human + bot) on team 2
        self.player_manager.for_each_player(|_id, p_state| {
            if p_state.team_id == 1 { team1_player_count +=1; }
            else if p_state.team_id == 2 { team2_player_count +=1; }
        });

        let team_id = if team1_player_count <= team2_player_count { 1 } else { 2 };

        // Get spawn points for the selected team
        let potential_spawns_for_team: Vec<Vec2> = team_spawn_areas.iter()
            .filter(|(_, sp_team_id)| *sp_team_id == team_id as u8)
            .map(|(pos, _)| *pos)
            .collect();

        let spawn_pos = if !potential_spawns_for_team.is_empty() {
            // Use team spawn point with some random offset
            let base_spawn = potential_spawns_for_team[rng.gen_range(0..potential_spawns_for_team.len())];
            let offset_radius = 50.0; // Small offset to prevent stacking
            let angle = rng.gen_range(0.0..2.0 * std::f32::consts::PI);
            let offset_x = offset_radius * angle.cos();
            let offset_y = offset_radius * angle.sin();
            Vec2::new(
                (base_spawn.x + offset_x).clamp(WORLD_MIN_X + PLAYER_RADIUS, WORLD_MAX_X - PLAYER_RADIUS),
                (base_spawn.y + offset_y).clamp(WORLD_MIN_Y + PLAYER_RADIUS, WORLD_MAX_Y - PLAYER_RADIUS)
            )
        } else {
            // Fallback: use respawn manager
            self.respawn_manager.get_respawn_position(self, &Arc::new(bot_player_id_str.clone()), Some(team_id as u8), &[])
        };


        if let Some(player_id_arc) = self.player_manager.add_player(bot_player_id_str.clone(), bot_name.clone(), spawn_pos.x, spawn_pos.y) {
            if let Some(mut p_state_entry) = self.player_manager.get_player_state_mut(&player_id_arc) {
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
            error!("[Bot Management] Failed to add bot {} to player manager.", bot_name);
        }
    }
}

fn remove_bots(&self, count: usize) {
    let mut removed_count = 0;
    let bot_keys_to_remove: Vec<PlayerID> = self.bot_players.iter()
                                               .map(|entry| entry.key().clone())
                                               .take(count)
                                               .collect();

    for bot_key in bot_keys_to_remove {
        if self.bot_players.remove(&bot_key).is_some() {
            self.player_manager.remove_player(bot_key.as_str());
            info!("[Bot Management] Removed bot {} to adjust match population.", bot_key);
            removed_count += 1;
            if removed_count >= count {
                break;
            }
        }
    }
}
    
    // Complete replacement for build_delta_state_optimized method:
    // In server/src/server/instance.rs
pub async fn build_delta_state_optimized(
    &self,
    peer_id_str: &str,
    client_state: &ClientState,
    shared_data: &SharedBroadcastData,
) -> Result<Bytes, Box<dyn std::error::Error + Send + Sync>> {
    use std::cell::RefCell;

    thread_local! {
        static BUILDER: RefCell<flatbuffers::FlatBufferBuilder<'static>> = 
            RefCell::new(flatbuffers::FlatBufferBuilder::with_capacity(16384));
    }

    BUILDER.with(|builder_cell| {
        let mut builder = builder_cell.borrow_mut();
        builder.reset();
        
        let build_start = Instant::now();
        let player_id = self.player_manager.id_pool.get_or_create(peer_id_str);
        
        trace!("[{}] DeltaBuilder: Started", peer_id_str);
        
        // Get player's current AoI
        let player_aoi = self.player_aois
            .get(peer_id_str)
            .map(|entry| entry.value().clone())
            .unwrap_or_else(|| PlayerAoI::new());
        
        // Build player deltas - fix the method call
        let mut players_fb_vec = Vec::new();
        let mut removed_player_ids_vec = Vec::new();
        
        // Add self player
        if let Some(self_state) = self.player_manager.get_player_state(&player_id) {
            players_fb_vec.push(create_fb_player_state_for_delta(&mut builder, &self_state, 0xFFFF));
        }
        
        // Add visible players
        for visible_player_id in &player_aoi.visible_players {
            if visible_player_id != &player_id {
                if let Some(player_state) = self.player_manager.get_player_state(visible_player_id) {
                    players_fb_vec.push(create_fb_player_state_for_delta(&mut builder, &player_state, 0xFFFF));
                }
            }
        }
        
        // Find removed players
        for known_player_id in &client_state.last_known_players {
            if !player_aoi.visible_players.contains(known_player_id) && known_player_id != &player_id {
                removed_player_ids_vec.push(builder.create_string(known_player_id.as_str()));
            }
        }
        
        let players_fb = builder.create_vector(&players_fb_vec);
        let removed_players_fb = builder.create_vector(&removed_player_ids_vec);
        
        // Build projectile deltas
        let mut new_projectiles_vec = Vec::new();
        let mut removed_projectile_ids_vec = Vec::new();
        
        let projectiles_guard = self.projectiles.read();
        for proj_id in &player_aoi.visible_projectiles {
            if !client_state.last_known_projectile_ids.contains(proj_id) {
                if let Some(proj) = projectiles_guard.iter().find(|p| p.id == *proj_id) {
                    let id_str = builder.create_string(&proj.id.to_string());
                    let owner_str = builder.create_string(proj.owner_id.as_str());
                    
                    let proj_fb = fb::ProjectileState::create(&mut builder, &fb::ProjectileStateArgs {
                        id: Some(id_str),
                        x: proj.x,
                        y: proj.y,
                        owner_id: Some(owner_str),
                        weapon_type: map_server_weapon_to_fb(proj.weapon_type),
                        velocity_x: proj.velocity_x,  // not vx
                        velocity_y: proj.velocity_y,  // not vy
                    });
                    new_projectiles_vec.push(proj_fb);
                }
            }
        }
        
        for known_proj_id in &client_state.last_known_projectile_ids {
            if !player_aoi.visible_projectiles.contains(known_proj_id) {
                let id_str = builder.create_string(&known_proj_id.to_string());
                removed_projectile_ids_vec.push(id_str);
            }
        }
        drop(projectiles_guard);
        
        let projectiles_fb = builder.create_vector(&new_projectiles_vec);
        let removed_projectiles_fb = builder.create_vector(&removed_projectile_ids_vec);
        
        // Build pickup deltas
        let mut pickups_delta_vec = Vec::new();
        let mut deactivated_pickup_ids_vec = Vec::new();
        
        let pickups_guard = self.pickups.read();
        for pickup_id in &player_aoi.visible_pickups {
            if let Some(pickup) = pickups_guard.iter().find(|p| p.id == *pickup_id) {
                let should_send = if let Some(last_known_state) = client_state.last_known_pickup_states.get(pickup_id) {
                    last_known_state.is_active != pickup.is_active
                } else {
                    true
                };
                
                if should_send {
                    let (pickup_type_fb, weapon_type_fb) = map_core_pickup_to_fb(&pickup.pickup_type);
                    let id_str = builder.create_string(&pickup.id.to_string());
                    
                    let pickup_fb = fb::Pickup::create(&mut builder, &fb::PickupArgs {
                        id: Some(id_str),
                        x: pickup.x,
                        y: pickup.y,
                        pickup_type: pickup_type_fb,
                        weapon_type: weapon_type_fb.unwrap_or(fb::WeaponType::Pistol),
                        is_active: pickup.is_active,
                    });
                    pickups_delta_vec.push(pickup_fb);
                }
            }
        }
        
        for (known_pickup_id, _) in &client_state.last_known_pickup_states {
            if !player_aoi.visible_pickups.contains(known_pickup_id) {
                let id_str = builder.create_string(&known_pickup_id.to_string());
                deactivated_pickup_ids_vec.push(id_str);
            }
        }
        drop(pickups_guard);
        
        let pickups_fb = builder.create_vector(&pickups_delta_vec);
        let deactivated_pickups_fb = builder.create_vector(&deactivated_pickup_ids_vec);
        
        // Build events
        let events_vec: Vec<_> = shared_data.events.iter().take(50).map(|event| {
            build_game_event_fb(&mut builder, event)
        }).collect();
        let game_events_fb = builder.create_vector(&events_vec);
        
        // Build kill feed
        let kill_feed_vec: Vec<_> = shared_data.kill_feed_snapshot.iter().map(|entry| {
            let killer_name_fb = builder.create_string(&entry.killer_name);
            let victim_name_fb = builder.create_string(&entry.victim_name);
            fb::KillFeedEntry::create(&mut builder, &fb::KillFeedEntryArgs {
                killer_name: Some(killer_name_fb),
                victim_name: Some(victim_name_fb),
                weapon: map_server_weapon_to_fb(entry.weapon),
                timestamp: entry.timestamp as f32,
                killer_position: None,
                victim_position: None,
                is_headshot: false,
            })
        }).collect();
        let kill_feed_fb = builder.create_vector(&kill_feed_vec);
        
        // Build match info if changed
        let match_info_fb = {
            let match_snapshot = &shared_data.match_info_snapshot;
            let team_scores_vec: Vec<_> = match_snapshot.team_scores.iter().map(|(team_id, score)| {
                fb::TeamScoreEntry::create(&mut builder, &fb::TeamScoreEntryArgs {
                    team_id: *team_id as i8,
                    score: *score,
                })
            }).collect();
            let team_scores_fb = builder.create_vector(&team_scores_vec);
            
            Some(fb::MatchInfo::create(&mut builder, &fb::MatchInfoArgs {
                time_remaining: match_snapshot.time_remaining,
                match_state: match_snapshot.match_state,
                winner_id: None,
                winner_name: None,
                game_mode: match_snapshot.game_mode,
                team_scores: Some(team_scores_fb),
            }))
        };
        
        // Build destroyed wall IDs
        let destroyed_walls_vec: Vec<_> = shared_data.destroyed_wall_ids.iter()
            .map(|id| builder.create_string(&id.to_string()))
            .collect();
        let destroyed_wall_ids_fb = if !destroyed_walls_vec.is_empty() {
            Some(builder.create_vector(&destroyed_walls_vec))
        } else {
            None
        };
        
        // Build updated walls (respawned walls)
        let mut updated_walls_vec = Vec::new();
        
        // Get updated walls from shared data (not from instance to avoid race condition)
        for (wall_id, wall_data) in shared_data.updated_walls.iter() {
            // Check if this wall is visible to the player
            if player_aoi.visible_walls.contains(wall_id) {
                info!("[{}] Sending updated wall {} to client (health: {}/{})", peer_id_str, wall_id, wall_data.current_health, wall_data.max_health);
                let id_fb = builder.create_string(&wall_data.id.to_string());
                let wall_fb = fb::Wall::create(&mut builder, &fb::WallArgs {
                    id: Some(id_fb),
                    x: wall_data.x,
                    y: wall_data.y,
                    width: wall_data.width,
                    height: wall_data.height,
                    is_destructible: wall_data.is_destructible,
                    current_health: wall_data.current_health,
                    max_health: wall_data.max_health,
                });
                updated_walls_vec.push(wall_fb);
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
            game_events: Some(game_events_fb),
            timestamp: shared_data.timestamp_ms,
            last_processed_input_sequence: 0, // Get from player state if needed
            changed_player_fields: None,
            kill_feed: Some(kill_feed_fb),
            match_info: match_info_fb,
            destroyed_wall_ids: destroyed_wall_ids_fb,
            flag_states: None,
            removed_player_ids: Some(removed_players_fb),
            updated_walls: updated_walls_fb,
        };
        
        let delta_state = fb::DeltaStateMessage::create(&mut builder, &delta_state_args);
        
        // Wrap in GameMessage
        let game_msg = fb::GameMessage::create(&mut builder, &fb::GameMessageArgs {
            msg_type: fb::MessageType::DeltaState,
            actual_message_type: fb::MessagePayload::DeltaStateMessage,
            actual_message: Some(delta_state.as_union_value()),
        });
        
        builder.finish(game_msg, None);
        let bytes = Bytes::from(builder.finished_data().to_vec());
        
        trace!("[{}] DeltaBuilder: Completed in {:?}", peer_id_str, build_start.elapsed());
        Ok(bytes)
    })
}

    // 1. Fix build_projectile_deltas_optimized - add the missing method
fn build_projectile_deltas_optimized<'a>(
    &self,
    builder: &mut flatbuffers::FlatBufferBuilder<'a>,
    aoi_data: &PlayerAoI,
    client_state: &ClientState,
) -> (
    flatbuffers::WIPOffset<flatbuffers::Vector<'a, flatbuffers::ForwardsUOffset<fb::ProjectileState<'a>>>>,
    flatbuffers::WIPOffset<flatbuffers::Vector<'a, flatbuffers::ForwardsUOffset<&'a str>>>
) {
    let mut new_projectiles_fb_vec = Vec::new();
    let mut removed_projectiles_ids_str_vec = Vec::new();
    
    let projectiles_read_guard = self.projectiles.read();
    let current_projectiles_in_aoi: HashSet<EntityId> = aoi_data.visible_projectiles.clone();
    
    // New projectiles
    for proj_id in current_projectiles_in_aoi.iter() {
        if !client_state.last_known_projectile_ids.contains(proj_id) {
            if let Some(proj) = projectiles_read_guard.iter().find(|p| p.id == *proj_id) {
                let id_fb = builder.create_string(&proj.id.to_string());
                let owner_id_fb = builder.create_string(proj.owner_id.as_str());
                new_projectiles_fb_vec.push(fb::ProjectileState::create(builder, &fb::ProjectileStateArgs{
                    id: Some(id_fb), 
                    x: proj.x, 
                    y: proj.y, 
                    owner_id: Some(owner_id_fb),
                    weapon_type: map_server_weapon_to_fb(proj.weapon_type),
                    velocity_x: proj.velocity_x, 
                    velocity_y: proj.velocity_y,
                }));
            }
        }
    }
    
    // Removed projectiles
    for known_proj_id in client_state.last_known_projectile_ids.iter() {
        if !current_projectiles_in_aoi.contains(known_proj_id) {
            removed_projectiles_ids_str_vec.push(known_proj_id.to_string());
        }
    }
    
    let projectiles_fb = builder.create_vector(&new_projectiles_fb_vec);
    let removed_projectiles_fb_offsets: Vec<_> = removed_projectiles_ids_str_vec.iter()
        .map(|s| builder.create_string(s))
        .collect();
    let removed_projectiles_fb = builder.create_vector(&removed_projectiles_fb_offsets);
    
    (projectiles_fb, removed_projectiles_fb)
}

// 2. Fix build_events_fb - add the missing method
fn build_events_fb<'a>(
    &self,
    builder: &mut flatbuffers::FlatBufferBuilder<'a>,
    events: &[GameEvent],
) -> flatbuffers::WIPOffset<flatbuffers::Vector<'a, flatbuffers::ForwardsUOffset<fb::GameEvent<'a>>>> {
    let game_events_fb_vec: Vec<_> = events.iter().take(50).map(|event| {
        let event_pos = event_position(event);
        let pos_fb_offset = fb::Vec2::create(builder, &fb::Vec2Args { x: event_pos.x, y: event_pos.y });
        let instigator_id_fb = event_instigator_id(event).map(|id| builder.create_string(id.as_str()));
        let target_id_fb = event_target_id(event).map(|id_str| builder.create_string(&id_str));
        let weapon_type_fb = event_weapon_type(event).map_or(fb::WeaponType::Pistol, map_server_weapon_to_fb);
        fb::GameEvent::create(builder, &fb::GameEventArgs{
            event_type: map_game_event_type_to_fb(event),
            position: Some(pos_fb_offset),
            instigator_id: instigator_id_fb,
            target_id: target_id_fb,
            weapon_type: weapon_type_fb,
            value: event_value(event).unwrap_or(0.0),
        })
    }).collect();
    
    builder.create_vector(&game_events_fb_vec)
}

    async fn build_delta_state_static(
        peer_id_str: &str,
        client_state: &ClientState,
        shared_data: &SharedBroadcastData,
        player_manager: &Arc<ImprovedPlayerManager>,
        player_aois: &PlayerAoIs,
        _pickups: &Arc<ParkingLotRwLock<Vec<Pickup>>>,
        _projectiles: &Arc<ParkingLotRwLock<Vec<Projectile>>>,
        _kill_feed: &Arc<ParkingLotRwLock<VecDeque<ServerKillFeedEntry>>>,
    ) -> Result<Bytes, ()> {
        thread_local! {
            static BUILDER: RefCell<flatbuffers::FlatBufferBuilder<'static>> = 
                RefCell::new(flatbuffers::FlatBufferBuilder::with_capacity(16384));
        }
        
        BUILDER.with(|builder_cell| {
            let mut builder = builder_cell.borrow_mut();
            builder.reset();
            
            let self_player_id = player_manager.id_pool.get_or_create(peer_id_str);
            let mut last_processed_input_for_client = 0;
            
            // Build player deltas
            let mut players_delta_fb_vec = Vec::new();
            let mut player_fields_mask_fb_vec = Vec::new();
            
            // Process self player
            if let Some(self_player_state_guard) = player_manager.get_player_state(&self_player_id) {
                let self_player_state = &*self_player_state_guard;
                last_processed_input_for_client = self_player_state.last_processed_input_sequence;
                if self_player_state.changed_fields > 0 {
                    players_delta_fb_vec.push(create_fb_player_state_for_delta(&mut builder, self_player_state, self_player_state.changed_fields));
                    player_fields_mask_fb_vec.push(self_player_state.changed_fields as u8);
                }
            }
            
            // Process visible players from AoI
            if let Some(aoi_entry) = player_aois.get(peer_id_str) {
                let p_aoi = aoi_entry.value();
                for visible_player_id in p_aoi.visible_players.iter() {
                    if visible_player_id == &self_player_id { continue; }
                    
                    if let Some(other_pstate_guard) = player_manager.get_player_state(visible_player_id) {
                        let other_pstate = &*other_pstate_guard;
                        if other_pstate.changed_fields > 0 ||
                           client_state.last_known_player_states.get(visible_player_id).map_or(true, |old_ps| *old_ps != *other_pstate) {
                            players_delta_fb_vec.push(create_fb_player_state_for_delta(&mut builder, other_pstate, other_pstate.changed_fields));
                            player_fields_mask_fb_vec.push(other_pstate.changed_fields as u8);
                        }
                    }
                }
            }
            
            let players_fb = builder.create_vector(&players_delta_fb_vec);
            let player_fields_mask_fb = builder.create_vector(&player_fields_mask_fb_vec);
            
            // Build empty vectors for simplified example
            let empty_projectiles: Vec<flatbuffers::WIPOffset<fb::ProjectileState>> = Vec::new();
            let projectiles_fb = builder.create_vector(&empty_projectiles);
            
            let empty_strings: Vec<flatbuffers::WIPOffset<&str>> = Vec::new();
            let removed_projectiles_fb = builder.create_vector(&empty_strings);
            
            let empty_pickups: Vec<flatbuffers::WIPOffset<fb::Pickup>> = Vec::new();
            let pickups_delta_fb = builder.create_vector(&empty_pickups);
            let deactivated_pickups_fb = builder.create_vector(&empty_strings);
            
            // Build events
            let game_events_fb_vec: Vec<_> = shared_data.events.iter().take(50).map(|event| {
                let event_pos = event_position(event);
                let pos_fb_offset = fb::Vec2::create(&mut builder, &fb::Vec2Args { x: event_pos.x, y: event_pos.y });
                let instigator_id_fb = event_instigator_id(event).map(|id| builder.create_string(id.as_str()));
                let target_id_fb = event_target_id(event).map(|id_str| builder.create_string(&id_str));
                let weapon_type_fb = event_weapon_type(event).map_or(fb::WeaponType::Pistol, map_server_weapon_to_fb);
                fb::GameEvent::create(&mut builder, &fb::GameEventArgs{
                    event_type: map_game_event_type_to_fb(event),
                    position: Some(pos_fb_offset),
                    instigator_id: instigator_id_fb,
                    target_id: target_id_fb,
                    weapon_type: weapon_type_fb,
                    value: event_value(event).unwrap_or(0.0),
                })
            }).collect();
            let game_events_fb = builder.create_vector(&game_events_fb_vec);
            
            // Build destroyed wall IDs
            let destroyed_walls_fb_vec: Vec<_> = shared_data.destroyed_wall_ids
                .iter()
                .take(20)
                .map(|wall_id| builder.create_string(&wall_id.to_string()))
                .collect();
            let destroyed_walls_fb = if !destroyed_walls_fb_vec.is_empty() {
                Some(builder.create_vector(&destroyed_walls_fb_vec))
            } else {
                None
            };
            
            let delta_state_args = fb::DeltaStateMessageArgs {
                players: Some(players_fb),
                projectiles: Some(projectiles_fb),
                removed_projectiles: Some(removed_projectiles_fb),
                pickups: Some(pickups_delta_fb),
                deactivated_pickup_ids: Some(deactivated_pickups_fb),
                game_events: Some(game_events_fb),
                timestamp: shared_data.timestamp_ms,
                last_processed_input_sequence: last_processed_input_for_client,
                changed_player_fields: Some(player_fields_mask_fb),
                kill_feed: None,
                match_info: None,
                destroyed_wall_ids: destroyed_walls_fb,
                flag_states: None,
                removed_player_ids: None,
                updated_walls: None,
            };
            
            let delta_state_msg = fb::DeltaStateMessage::create(&mut builder, &delta_state_args);
            let game_msg = fb::GameMessage::create(&mut builder, &fb::GameMessageArgs {
                msg_type: fb::MessageType::DeltaState,
                actual_message_type: fb::MessagePayload::DeltaStateMessage,
                actual_message: Some(delta_state_msg.as_union_value()),
            });
            
            builder.finish(game_msg, None);
            Ok(Bytes::from(builder.finished_data().to_vec()))
        })
    }
    
    // Fast AoI data retrieval with minimal locking
    fn get_player_aoi_data_fast(&self, player_id: &PlayerID) -> PlayerAoI {
        if let Some(aoi_entry) = self.player_aois.get(player_id.as_str()) {
            PlayerAoI {
                visible_players: aoi_entry.visible_players.clone(),
                visible_projectiles: aoi_entry.visible_projectiles.clone(),
                visible_pickups: aoi_entry.visible_pickups.clone(),
                visible_walls: aoi_entry.visible_walls.clone(),
                last_update: aoi_entry.last_update.clone(),//Instant::now(),
            }
        } else {
            Self::get_empty_player_aoi()
        }
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
    
    // Helper function for building wall deltas (if you have destructible walls)
        // Helper function for building wall deltas (if you have destructible walls)
    // LIFETIME FIX APPLIED HERE
    fn build_wall_deltas_optimized<'a>(
        &self,
        builder: &mut flatbuffers::FlatBufferBuilder<'a>,
        player_aoi: &PlayerAoI,
        last_known_walls: &HashSet<EntityId>,
    ) -> (
        Vec<flatbuffers::WIPOffset<fb::Wall<'a>>>,
        Vec<flatbuffers::WIPOffset<&'a str>>
    ) {
        let mut new_or_changed_walls = Vec::new();
        let mut destroyed_wall_ids = Vec::new();
        
        // Find new walls or walls with changed health
        for wall_id in &player_aoi.visible_walls {
            let should_send_wall = !last_known_walls.contains(wall_id);
            
            if should_send_wall {
                // Get wall from appropriate partition
                for partition in self.world_partition_manager.get_partitions_for_processing() {
                    if let Some(wall) = partition.get_wall(*wall_id) {
                        if wall.is_destructible && wall.current_health <= 0 {
                            // Wall is destroyed, add to destroyed list
                            let id_str = builder.create_string(&wall_id.to_string());
                            destroyed_wall_ids.push(id_str);
                        } else {
                            // Wall exists and is not destroyed
                            let id_str = builder.create_string(&wall.id.to_string());
                            let wall_fb = fb::Wall::create(builder, &fb::WallArgs {
                                id: Some(id_str),
                                x: wall.x,
                                y: wall.y,
                                width: wall.width,
                                height: wall.height,
                                is_destructible: wall.is_destructible,
                                current_health: wall.current_health,
                                max_health: wall.max_health,
                            });
                            new_or_changed_walls.push(wall_fb);
                        }
                        break;
                    }
                }
            }
        }
        
        // Find walls that are no longer visible
        for known_wall_id in last_known_walls {
            if !player_aoi.visible_walls.contains(known_wall_id) {
                let id_str = builder.create_string(&known_wall_id.to_string());
                destroyed_wall_ids.push(id_str);
            }
        }
        
        (new_or_changed_walls, destroyed_wall_ids)
    }
    
    // Optimized player delta building
    fn build_player_deltas_optimized<'a>(
        &self,
        builder: &mut flatbuffers::FlatBufferBuilder<'a>,
        self_player_id: &PlayerID,
        aoi_data: &PlayerAoI,
        client_state: &ClientState,
    ) -> flatbuffers::WIPOffset<flatbuffers::Vector<'a, flatbuffers::ForwardsUOffset<fb::PlayerState<'a>>>> {
        let mut players_fb_vec = Vec::new();
        
        // Process self first
        if let Some(self_state) = self.player_manager.get_player_state(self_player_id) {
            if self_state.changed_fields > 0 {
                players_fb_vec.push(create_fb_player_state_for_delta(builder, &self_state, self_state.changed_fields));
            }
        }
        
        // Process visible players
        let visible_states: Vec<_> = aoi_data.visible_players
            .iter()
            .filter_map(|id| self.player_manager.get_player_state(id).map(|s| (id, s)))
            .collect();
        
        for (_id, state) in visible_states {
            if state.changed_fields > 0 || 
               !client_state.last_known_player_states.contains_key(_id) {
                players_fb_vec.push(create_fb_player_state_for_delta(builder, &state, state.changed_fields));
            }
        }
        
        builder.create_vector(&players_fb_vec)
    }
    
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


    
    // Cache frequently used strings
    fn create_fb_player_state_cached<'a>(
        &self,
        builder: &mut flatbuffers::FlatBufferBuilder<'a>,
        state: &PlayerState,
    ) -> flatbuffers::WIPOffset<fb::PlayerState<'a>> {
        // Just call the regular function without caching to avoid borrow checker issues
        create_fb_player_state_for_delta(builder, state, state.changed_fields)
    }




    

    
    pub async fn broadcast_world_updates_optimized(self: Arc<Self>) {
        const BROADCAST_INTERVAL_FRAMES: u64 = 1; 
    
        let current_frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        let last_broadcast = self.last_broadcast_frame.load(AtomicOrdering::Relaxed);
    
        if current_frame < last_broadcast + BROADCAST_INTERVAL_FRAMES && current_frame != 0 {
            trace!("[Frame {}] Skipping broadcast (interval). Last broadcast: {}", current_frame, last_broadcast);
            return;
        }
        
        let connected_clients = self.data_channels_map.len();
        if connected_clients == 0 {
            if current_frame % 30 == 0 {  // Log every 30 frames
                // Debug: List all keys in the map to see if there's a mismatch
                info!("[Frame {}] No connected clients in data_channels_map. Checking map contents...", current_frame);
                info!("[Frame {}] Map ptr in broadcast: {:p}", current_frame, Arc::as_ptr(&self.data_channels_map));
                for entry in self.data_channels_map.iter() {
                    info!("[Frame {}] Found entry in map: key={}", current_frame, entry.key());
                }
                info!("[Frame {}] Total entries found: {}", current_frame, self.data_channels_map.len());
            }
            return;
        }
    
        info!("[Frame {}] Starting broadcast to {} clients. Last broadcast frame: {}", current_frame, connected_clients, last_broadcast);
        self.last_broadcast_frame.store(current_frame, AtomicOrdering::Relaxed);
    
        let shared_broadcast_data = self.prepare_shared_broadcast_data().await;
        trace!("[Frame {}] Prepared shared broadcast data. Events: {}, Destroyed Walls: {}, Chat: {}, KF: {}", 
            current_frame, shared_broadcast_data.events.len(), shared_broadcast_data.destroyed_wall_ids.len(),
            shared_broadcast_data.chat_messages.len(), shared_broadcast_data.kill_feed_snapshot.len());
    
        let client_entries: Vec<_> = self.data_channels_map.iter()
            .map(|entry| (entry.key().clone(), Arc::clone(entry.value())))
            .collect();
    
        for (peer_id_str, data_channel_arc) in client_entries {
            let needs_initial = !self.client_states_map
                .read() // Acquire read lock first
                .get(&peer_id_str)
                .map_or(false, |cs_state| cs_state.known_walls_sent); // cs_state is &ClientState
            
            let client_info = ClientInfo {
                data_channel: data_channel_arc.clone(), 
                needs_initial_state: needs_initial,
            };
            
            trace!("[Frame {}] Processing client: {}, Needs Initial: {}", current_frame, peer_id_str, client_info.needs_initial_state);

            // Pass &self (which is &Arc<MassiveGameServer>) to the static method
            if let Err(e) = Self::process_client_broadcast(&peer_id_str, &client_info, &shared_broadcast_data, &self).await {
                 error!("[Frame {}] Error processing broadcast for client {}: {:?}", current_frame, peer_id_str, e);
            }
        }
        debug!("[Frame {}] Broadcast processing loop complete.", current_frame);
    }

    

    async fn build_initial_state_optimized(
        &self,
        peer_id_str: &str,
        shared_data: &SharedBroadcastData, // Used for timestamp, match_info, kill_feed
    ) -> Result<Bytes, Box<dyn std::error::Error + Send + Sync>> {
        const MAX_INITIAL_PLAYERS: usize = 50;
        const MAX_INITIAL_WALLS: usize = 350; // Increased slightly, adjust as needed
        const MAX_INITIAL_PROJECTILES: usize = 500;
        const MAX_INITIAL_PICKUPS: usize = 50;
        const MAX_INITIAL_EVENTS: usize = 30;
        const MAX_INITIAL_KILL_FEED: usize = 10;
        const MAX_MESSAGE_SIZE_BYTES: usize = 160000; // Slightly less than 64KB

        thread_local! {
            static BUILDER: RefCell<flatbuffers::FlatBufferBuilder<'static>> =
                RefCell::new(flatbuffers::FlatBufferBuilder::with_capacity(32768)); // Increased capacity
        }

        BUILDER.with(|builder_cell| {
            let mut builder = builder_cell.borrow_mut();
            builder.reset();
            let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
            info!("[Frame {}] Client {}: Building InitialStateMessage.", frame, peer_id_str);

            let self_player_id_arc = self.player_manager.id_pool.get_or_create(peer_id_str);

            // 1. Walls: Get CURRENT wall states from partitions, not cached initial states
            // IMPORTANT: We need to get the CURRENT state of walls, not the cached initial state
            let mut active_walls_to_send = Vec::new();
            
            // Iterate through all partitions to get current wall states
            for partition in self.world_partition_manager.get_partitions_for_processing() {
                for wall_entry in partition.all_walls_in_partition.iter() {
                    let wall = wall_entry.value();
                    
                    // Only send non-destructible walls and active destructible walls
                    if !wall.is_destructible || (wall.is_destructible && wall.current_health > 0) {
                        active_walls_to_send.push(wall.clone());
                    } else {
                        debug!("[Frame {} Client {}] InitialState: Filtering out destroyed wall {} (health: {}/{})", 
                              frame, peer_id_str, wall.id, wall.current_health, wall.max_health);
                    }
                }
            }
            
            info!("[Frame {} Client {}] InitialState: Collected {} active walls (filtered from all partitions).", 
                  frame, peer_id_str, active_walls_to_send.len());

            let mut walls_fb_vec = Vec::with_capacity(active_walls_to_send.len().min(MAX_INITIAL_WALLS));
            for wall_data in active_walls_to_send.iter().take(MAX_INITIAL_WALLS) {
                let id_fb = fb_safe_str(&mut builder, &wall_data.id.to_string());
                walls_fb_vec.push(fb::Wall::create(&mut builder, &fb::WallArgs{
                    id: Some(id_fb), x: wall_data.x, y: wall_data.y, width: wall_data.width, height: wall_data.height,
                    is_destructible: wall_data.is_destructible,
                    current_health: wall_data.current_health,
                    max_health: wall_data.max_health,
                }));
            }
            let walls_fb = builder.create_vector(&walls_fb_vec);
            info!("[Frame {} Client {}] InitialState: Serialized {} walls.", frame, peer_id_str, walls_fb_vec.len());

            // 2. Player States (Self + AoI)
            let mut players_fb_vec = Vec::new();
            let mut player_aoi_data_for_initial_state = Self::get_empty_player_aoi(); // Default empty

            if let Some(self_pstate_guard) = self.player_manager.get_player_state(&self_player_id_arc) {
                players_fb_vec.push(create_fb_player_state_for_delta(&mut builder, &*self_pstate_guard, 0xFFFF));
                // Fetch AoI based on self's current position for other entities
                player_aoi_data_for_initial_state = self.get_player_aoi_data_fast(&self_player_id_arc);
            } else {
                warn!("[Frame {} Client {}] InitialState: Self player state not found!", frame, peer_id_str);
            }

            for visible_player_id in player_aoi_data_for_initial_state.visible_players.iter().take(MAX_INITIAL_PLAYERS.saturating_sub(players_fb_vec.len())) {
                if visible_player_id != &self_player_id_arc { // Already added self
                    if let Some(pstate_guard) = self.player_manager.get_player_state(visible_player_id) {
                        players_fb_vec.push(create_fb_player_state_for_delta(&mut builder, &*pstate_guard, 0xFFFF));
                    }
                }
            }
            let players_fb = builder.create_vector(&players_fb_vec);
            info!("[Frame {} Client {}] InitialState: Serialized {} player states.", frame, peer_id_str, players_fb_vec.len());

            // 3. Projectiles (from AoI)
            let mut projectiles_fb_vec = Vec::new();
            let projectiles_guard = self.projectiles.read();
            for proj_id in player_aoi_data_for_initial_state.visible_projectiles.iter().take(MAX_INITIAL_PROJECTILES) {
                if let Some(proj) = projectiles_guard.iter().find(|p| p.id == *proj_id) {
                    let id_fb = fb_safe_str(&mut builder, &proj.id.to_string());
                    let owner_id_fb = fb_safe_str(&mut builder, proj.owner_id.as_str());
                    projectiles_fb_vec.push(fb::ProjectileState::create(&mut builder, &fb::ProjectileStateArgs{
                        id: Some(id_fb), x: proj.x, y: proj.y, owner_id: Some(owner_id_fb),
                        weapon_type: map_server_weapon_to_fb(proj.weapon_type),
                        velocity_x: proj.velocity_x, velocity_y: proj.velocity_y,
                    }));
                }
            }
            let projectiles_fb = builder.create_vector(&projectiles_fb_vec);
            info!("[Frame {} Client {}] InitialState: Serialized {} projectiles.", frame, peer_id_str, projectiles_fb_vec.len());

            // 4. Pickups (Active ones from AoI)
            let mut pickups_fb_vec = Vec::new();
            let pickups_guard = self.pickups.read();
            for pickup_id in player_aoi_data_for_initial_state.visible_pickups.iter().take(MAX_INITIAL_PICKUPS) {
                if let Some(pickup) = pickups_guard.iter().find(|p| p.id == *pickup_id) {
                    if pickup.is_active { // Only send active pickups
                        let (fb_pickup_type, fb_weapon_type_opt) = map_core_pickup_to_fb(&pickup.pickup_type);
                        let id_fb = fb_safe_str(&mut builder, &pickup.id.to_string());
                        pickups_fb_vec.push(fb::Pickup::create(&mut builder, &fb::PickupArgs {
                            id: Some(id_fb), x: pickup.x, y: pickup.y, pickup_type: fb_pickup_type,
                            weapon_type: fb_weapon_type_opt.unwrap_or(fb::WeaponType::Pistol),
                            is_active: pickup.is_active,
                        }));
                    }
                }
            }
            let pickups_fb = builder.create_vector(&pickups_fb_vec);
            info!("[Frame {} Client {}] InitialState: Serialized {} active pickups.", frame, peer_id_str, pickups_fb_vec.len());

            // 5. Match Info (from shared_data snapshot)
            let match_snapshot = &shared_data.match_info_snapshot;
            let fb_team_scores_vec: Vec<_> = match_snapshot.team_scores.iter().map(|(team_id, score)| {
                fb::TeamScoreEntry::create(&mut builder, &fb::TeamScoreEntryArgs { team_id: *team_id as i8, score: *score })
            }).collect();
            let team_scores_fb = builder.create_vector(&fb_team_scores_vec);

            let match_info_fb = fb::MatchInfo::create(&mut builder, &fb::MatchInfoArgs {
                time_remaining: match_snapshot.time_remaining,
                match_state: match_snapshot.match_state,
                winner_id: None, // Typically not known at initial state
                winner_name: None,
                game_mode: match_snapshot.game_mode,
                team_scores: Some(team_scores_fb),
            });

            // 6. Flag States (from shared_data snapshot)
            let fb_flag_states_vec: Vec<_> = match_snapshot.flag_states.values().map(|fs| {
                let carrier_id_fb = fs.carrier_id.as_ref().map(|id| fb_safe_str(&mut builder, id.as_str()));
                let pos_fb = fb::Vec2::create(&mut builder, &fb::Vec2Args{ x: fs.position.x, y: fs.position.y });
                fb::FlagState::create(&mut builder, &fb::FlagStateArgs {
                    team_id: fs.team_id as i8, status: fs.status, position: Some(pos_fb),
                    carrier_id: carrier_id_fb, respawn_timer: fs.respawn_timer,
                })
            }).collect();
            let flag_states_fb = builder.create_vector(&fb_flag_states_vec);

            // 7. Map Name
            let map_name_fb = fb_safe_str(&mut builder, "Massive Arena"); // Or get from config/state

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
            };
            let game_msg = fb::GameMessage::create(&mut builder, &game_msg_args);
            builder.finish(game_msg, None);

            let finished_data = builder.finished_data();
            info!("[Frame {} Client {}] InitialStateMessage built. Size: {} bytes.", frame, peer_id_str, finished_data.len());

            if finished_data.len() > MAX_MESSAGE_SIZE_BYTES {
                return Err("Initial state too large".into());
            }
            
            Ok(Bytes::from(finished_data.to_vec()))
        })
    }

    async fn send_initial_state_to_client(&self, peer_id_str: &str, data_channel: &Arc<crate::core::types::RTCDataChannel>, client_state: &mut ClientState) {
        info!("[{}] Sending initial state to client", peer_id_str); // Add this

        let mut builder = flatbuffers::FlatBufferBuilder::with_capacity(16384);

        let self_player_id_arc = self.player_manager.id_pool.get_or_create(peer_id_str);

        let mut players_fb_vec = Vec::new();
        if let Some(aoi_entry) = self.player_aois.get(peer_id_str) {
            let p_aoi = aoi_entry.value();
            if let Some(self_pstate_guard) = self.player_manager.get_player_state(&self_player_id_arc) {
                 players_fb_vec.push(create_fb_player_state_for_delta(&mut builder, &*self_pstate_guard, 0xFFFF));
            }
            for visible_player_id in p_aoi.visible_players.iter() {
                if visible_player_id != &self_player_id_arc {
                    if let Some(pstate_guard) = self.player_manager.get_player_state(visible_player_id) {
                        players_fb_vec.push(create_fb_player_state_for_delta(&mut builder, &*pstate_guard, 0xFFFF));
                    }
                }
            }
        } else {
             if let Some(self_pstate_guard) = self.player_manager.get_player_state(&self_player_id_arc) {
                 players_fb_vec.push(create_fb_player_state_for_delta(&mut builder, &*self_pstate_guard, 0xFFFF));
            }
        }
        let players_fb = builder.create_vector(&players_fb_vec);

        let mut walls_fb_vec = Vec::new();
        let current_active_walls = self.collect_all_walls_current_state();
        for wall_data in current_active_walls {
            let id_fb = builder.create_string(&wall_data.id.to_string());
            walls_fb_vec.push(fb::Wall::create(&mut builder, &fb::WallArgs{
                id: Some(id_fb), x: wall_data.x, y: wall_data.y, width: wall_data.width, height: wall_data.height,
                is_destructible: wall_data.is_destructible, current_health: wall_data.current_health, max_health: wall_data.max_health,
            }));
        }
        let walls_fb = builder.create_vector(&walls_fb_vec);

        let mut projectiles_fb_vec = Vec::new();
        if let Some(aoi_entry) = self.player_aois.get(peer_id_str) {
            let p_aoi = aoi_entry.value();
            let projectiles_guard = self.projectiles.read();
            for proj_id in p_aoi.visible_projectiles.iter() {
                if let Some(proj) = projectiles_guard.iter().find(|p| p.id == *proj_id) {
                    let id_fb = builder.create_string(&proj.id.to_string());
                    let owner_id_fb = builder.create_string(proj.owner_id.as_str());
                    projectiles_fb_vec.push(fb::ProjectileState::create(&mut builder, &fb::ProjectileStateArgs{
                        id: Some(id_fb), x: proj.x, y: proj.y, owner_id: Some(owner_id_fb),
                        weapon_type: map_server_weapon_to_fb(proj.weapon_type),
                        velocity_x: proj.velocity_x, velocity_y: proj.velocity_y,
                    }));
                }
            }
        }
        let projectiles_fb = builder.create_vector(&projectiles_fb_vec);

        let mut pickups_fb_vec = Vec::new();
        if let Some(aoi_entry) = self.player_aois.get(peer_id_str) {
            let p_aoi = aoi_entry.value();
            let pickups_guard = self.pickups.read();
            for pickup_id in p_aoi.visible_pickups.iter() {
                if let Some(pickup) = pickups_guard.iter().find(|p| p.id == *pickup_id) {
                    if pickup.is_active {
                        let (fb_pickup_type, fb_weapon_type_opt) = map_core_pickup_to_fb(&pickup.pickup_type);
                        let id_fb = builder.create_string(&pickup.id.to_string());
                        pickups_fb_vec.push(fb::Pickup::create(&mut builder, &fb::PickupArgs {
                            id: Some(id_fb), x: pickup.x, y: pickup.y, pickup_type: fb_pickup_type,
                            weapon_type: fb_weapon_type_opt.unwrap_or(fb::WeaponType::Pistol),
                            is_active: pickup.is_active,
                        }));
                    }
                }
            }
        }
        let pickups_fb = builder.create_vector(&pickups_fb_vec);

        let player_id_fb_initial = builder.create_string(peer_id_str);
        let match_info_guard = self.match_info.read();
        let fb_team_scores_vec: Vec<_> = match_info_guard.team_scores.iter().map(|(team_id, score)| {
            fb::TeamScoreEntry::create(&mut builder, &fb::TeamScoreEntryArgs { team_id: *team_id as i8, score: *score })
        }).collect();
        let team_scores_fb = builder.create_vector(&fb_team_scores_vec);

        let fb_flag_states_vec: Vec<_> = match_info_guard.flag_states.values().map(|fs| {
            let carrier_id_fb = fs.carrier_id.as_ref().map(|id| builder.create_string(id.as_str()));
            let pos_fb = fb::Vec2::create(&mut builder, &fb::Vec2Args{ x: fs.position.x, y: fs.position.y });
            fb::FlagState::create(&mut builder, &fb::FlagStateArgs {
                team_id: fs.team_id as i8, status: fs.status, position: Some(pos_fb),
                carrier_id: carrier_id_fb, respawn_timer: fs.respawn_timer,
            })
        }).collect();
        let flag_states_fb = builder.create_vector(&fb_flag_states_vec);

        let match_info_fb = fb::MatchInfo::create(&mut builder, &fb::MatchInfoArgs {
            time_remaining: match_info_guard.time_remaining,
            match_state: match_info_guard.match_state,
            winner_id: None,
            winner_name: None,
            game_mode: match_info_guard.game_mode,
            team_scores: Some(team_scores_fb),
        });
        drop(match_info_guard);

        let map_name_fb = builder.create_string("Massive Arena 10v10");
        let timestamp_initial = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64;

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

        let game_msg_args = fb::GameMessageArgs {
            msg_type: fb::MessageType::InitialState,
            actual_message_type: fb::MessagePayload::InitialStateMessage,
            actual_message: Some(initial_state_msg.as_union_value()),
        };
        let game_msg = fb::GameMessage::create(&mut builder, &game_msg_args);
        builder.finish(game_msg, None);

        let data_bytes = Bytes::from(builder.finished_data().to_vec());
        let dc_clone = Arc::clone(data_channel);
        let peer_id_clone = peer_id_str.to_string();
        tokio::spawn(async move {
            if let Err(e) = dc_clone.send(&data_bytes).await {
                handle_dc_send_error(&e.to_string(), &peer_id_clone, "initial state");
            }
        });

        client_state.known_walls_sent = true;
        client_state.last_known_player_states.clear();
        // This logic was slightly off, it should be based on what was actually sent (AoI based)
        // For initial state, we sent players in AoI (plus self).
        if let Some(aoi_entry) = self.player_aois.get(peer_id_str) {
            let p_aoi = aoi_entry.value();
            if let Some(self_pstate_guard) = self.player_manager.get_player_state(&self_player_id_arc) {
                 client_state.last_known_player_states.insert(self_player_id_arc.clone(), (*self_pstate_guard).clone());
            }
            for visible_player_id in p_aoi.visible_players.iter() {
                if let Some(pstate_guard) = self.player_manager.get_player_state(visible_player_id) {
                    client_state.last_known_player_states.insert(visible_player_id.clone(), (*pstate_guard).clone());
                }
            }
        } else {
             if let Some(self_pstate_guard) = self.player_manager.get_player_state(&self_player_id_arc) {
                 client_state.last_known_player_states.insert(self_player_id_arc.clone(), (*self_pstate_guard).clone());
            }
        }


        client_state.last_known_projectile_ids.clear();
        if let Some(aoi_entry) = self.player_aois.get(peer_id_str) {
            aoi_entry.value().visible_projectiles.iter().for_each(|id| { client_state.last_known_projectile_ids.insert(*id); });
        }

        client_state.last_known_pickup_states.clear();
        if let Some(aoi_entry) = self.player_aois.get(peer_id_str) {
            let p_aoi = aoi_entry.value();
            let pickups_guard = self.pickups.read();
            for pickup_id in p_aoi.visible_pickups.iter() {
                if let Some(pickup) = pickups_guard.iter().find(|p| p.id == *pickup_id) {
                    client_state.last_known_pickup_states.insert(*pickup_id, PickupState { is_active: pickup.is_active });
                }
            }
        }
        let current_match_info_for_state = self.match_info.read();
        client_state.last_known_match_state = Some(current_match_info_for_state.match_state);
        client_state.last_known_match_time_remaining = Some(current_match_info_for_state.time_remaining);
        client_state.last_known_team_scores = current_match_info_for_state.team_scores.clone(); // Add this
        client_state.known_destroyed_wall_ids.clear(); // Should be empty initially for client
        info!("[{}] Initial state sent successfully", peer_id_str); // Add this

    }

    async fn send_delta_state_to_client(
        &self,
        peer_id_str: &str,
        data_channel: &Arc<crate::core::types::RTCDataChannel>,
        client_state: &mut ClientState,
        events_to_broadcast: &[GameEvent],
        destroyed_wall_ids_snapshot: &[EntityId],
        current_timestamp_ms: u64
    ) {
        let mut builder = flatbuffers::FlatBufferBuilder::with_capacity(8192);

        let self_player_id_arc = self.player_manager.id_pool.get_or_create(peer_id_str);

        // Player deltas
        let mut players_delta_fb_vec = Vec::new();
        let mut player_fields_mask_fb_vec = Vec::new();
        let mut last_processed_input_for_client = 0;

        // Process self player state first
        if let Some(self_player_state_guard) = self.player_manager.get_player_state(&self_player_id_arc) {
            let self_player_state = &*self_player_state_guard;
            last_processed_input_for_client = self_player_state.last_processed_input_sequence;

            if self_player_state.changed_fields > 0 || client_state.last_known_player_states.get(&self_player_id_arc).map_or(true, |old| old.changed_fields == 0xFFFF) {
                players_delta_fb_vec.push(create_fb_player_state_for_delta(&mut builder, self_player_state, self_player_state.changed_fields));
                player_fields_mask_fb_vec.push(self_player_state.changed_fields as u8);
                client_state.last_known_player_states.insert(self_player_id_arc.clone(), self_player_state.clone());
            }
        }

        // Process other players in AoI
        if let Some(aoi_entry) = self.player_aois.get(peer_id_str) {
            let p_aoi = aoi_entry.value();
            for visible_player_id in p_aoi.visible_players.iter() {
                if visible_player_id == &self_player_id_arc { continue; }

                if let Some(other_pstate_guard) = self.player_manager.get_player_state(visible_player_id) {
                    let other_pstate = &*other_pstate_guard;
                    if other_pstate.changed_fields > 0 ||
                       client_state.last_known_player_states.get(visible_player_id).map_or(true, |old_ps| *old_ps != *other_pstate) {
                        players_delta_fb_vec.push(create_fb_player_state_for_delta(&mut builder, other_pstate, other_pstate.changed_fields));
                        player_fields_mask_fb_vec.push(other_pstate.changed_fields as u8);
                        client_state.last_known_player_states.insert(visible_player_id.clone(), other_pstate.clone());
                    }
                }
            }
            client_state.last_known_player_states.retain(|id, _| id == &self_player_id_arc || p_aoi.visible_players.contains(id));
        } else {
            client_state.last_known_player_states.retain(|id, _| id == &self_player_id_arc);
        }
        let players_fb = builder.create_vector(&players_delta_fb_vec);
        let player_fields_mask_fb = builder.create_vector(&player_fields_mask_fb_vec);

        // Projectile deltas
        let mut new_projectiles_fb_vec = Vec::new();
        let mut removed_projectiles_ids_str_vec = Vec::new();
        if let Some(aoi_entry) = self.player_aois.get(peer_id_str) {
            let p_aoi = aoi_entry.value();
            let projectiles_read_guard = self.projectiles.read();
            let current_projectiles_in_aoi: HashSet<EntityId> = p_aoi.visible_projectiles.clone();

            for proj_id in current_projectiles_in_aoi.iter() {
                if !client_state.last_known_projectile_ids.contains(proj_id) {
                    if let Some(proj) = projectiles_read_guard.iter().find(|p| p.id == *proj_id) {
                        let id_fb = builder.create_string(&proj.id.to_string());
                        let owner_id_fb = builder.create_string(proj.owner_id.as_str());
                        new_projectiles_fb_vec.push(fb::ProjectileState::create(&mut builder, &fb::ProjectileStateArgs{
                            id: Some(id_fb), x: proj.x, y: proj.y, owner_id: Some(owner_id_fb),
                            weapon_type: map_server_weapon_to_fb(proj.weapon_type),
                            velocity_x: proj.velocity_x, velocity_y: proj.velocity_y,
                        }));
                    }
                }
            }
            for known_proj_id in client_state.last_known_projectile_ids.iter() {
                if !current_projectiles_in_aoi.contains(known_proj_id) {
                    removed_projectiles_ids_str_vec.push(known_proj_id.to_string());
                }
            }
            client_state.last_known_projectile_ids = current_projectiles_in_aoi;
        } else {
            for known_proj_id in client_state.last_known_projectile_ids.iter() {
                removed_projectiles_ids_str_vec.push(known_proj_id.to_string());
            }
            client_state.last_known_projectile_ids.clear();
        }
        let projectiles_delta_fb = builder.create_vector(&new_projectiles_fb_vec);
        let removed_projectiles_fb_offsets: Vec<_> = removed_projectiles_ids_str_vec.iter().map(|s| builder.create_string(s)).collect();
        let removed_projectiles_fb = builder.create_vector(&removed_projectiles_fb_offsets);

        // Pickup deltas
        let mut changed_pickups_fb_vec = Vec::new();
        let mut deactivated_pickup_ids_str_vec = Vec::new();
        if let Some(aoi_entry) = self.player_aois.get(peer_id_str) {
            let p_aoi = aoi_entry.value();
            let pickups_read_guard = self.pickups.read();
            let mut current_pickups_in_aoi_states = HashMap::new();

            for pickup_id_in_aoi in p_aoi.visible_pickups.iter() {
                if let Some(pickup) = pickups_read_guard.iter().find(|p| p.id == *pickup_id_in_aoi) {
                    current_pickups_in_aoi_states.insert(*pickup_id_in_aoi, PickupState { is_active: pickup.is_active });
                    let last_known_active_opt = client_state.last_known_pickup_states.get(pickup_id_in_aoi);
                    if last_known_active_opt.map_or(true, |last_state| last_state.is_active != pickup.is_active) {
                        let id_fb = builder.create_string(&pickup.id.to_string());
                        let (pt, wt_opt) = map_core_pickup_to_fb(&pickup.pickup_type);
                        changed_pickups_fb_vec.push(fb::Pickup::create(&mut builder, &fb::PickupArgs{
                            id: Some(id_fb), x: pickup.x, y: pickup.y, pickup_type: pt,
                            weapon_type: wt_opt.unwrap_or(fb::WeaponType::Pistol),
                            is_active: pickup.is_active,
                        }));
                    }
                }
            }
            for (known_pickup_id, was_active_ref) in client_state.last_known_pickup_states.iter() {
                // TYPE FIX: The closure should take &PickupState and return pickup_state.is_active
                let is_still_in_aoi_and_active = current_pickups_in_aoi_states.get(known_pickup_id)
                    .map_or(false, |pickup_state_ref| pickup_state_ref.is_active); // Corrected: was |&now_active_bool| now_active_bool
                if was_active_ref.is_active && !is_still_in_aoi_and_active {

                    deactivated_pickup_ids_str_vec.push(known_pickup_id.to_string());
                }
            }
            client_state.last_known_pickup_states = current_pickups_in_aoi_states;
        } else {
            for (known_pickup_id, was_active) in client_state.last_known_pickup_states.iter() {
                if was_active.is_active { deactivated_pickup_ids_str_vec.push(known_pickup_id.to_string()); }
            }
            client_state.last_known_pickup_states.clear();
        }
        let pickups_delta_fb = builder.create_vector(&changed_pickups_fb_vec);
        let deactivated_pickups_fb_offsets: Vec<_> = deactivated_pickup_ids_str_vec.iter().map(|s| builder.create_string(s)).collect();
        let deactivated_pickups_fb = builder.create_vector(&deactivated_pickups_fb_offsets);

        // Game Events
        let game_events_fb_vec: Vec<_> = events_to_broadcast.iter().map(|event| {
            let event_pos = event_position(event);
            let pos_fb_offset = fb::Vec2::create(&mut builder, &fb::Vec2Args { x: event_pos.x, y: event_pos.y });
            let instigator_id_fb = event_instigator_id(event).map(|id| builder.create_string(id.as_str()));
            let target_id_fb = event_target_id(event).map(|id_str| builder.create_string(&id_str));
            let weapon_type_fb = event_weapon_type(event).map_or(fb::WeaponType::Pistol, map_server_weapon_to_fb);
            fb::GameEvent::create(&mut builder, &fb::GameEventArgs{
                event_type: map_game_event_type_to_fb(event),
                position: Some(pos_fb_offset),
                instigator_id: instigator_id_fb,
                target_id: target_id_fb,
                weapon_type: weapon_type_fb,
                value: event_value(event).unwrap_or(0.0),
            })
        }).collect();
        let game_events_fb = builder.create_vector(&game_events_fb_vec);

        // Match Info
        let current_match_info_guard = self.match_info.read();
        let team_scores_changed = client_state.last_known_team_scores != current_match_info_guard.team_scores;
        let match_info_fb_offset = if client_state.last_known_match_state != Some(current_match_info_guard.match_state) ||
                                        client_state.last_known_match_time_remaining.map_or(true, |t| (t - current_match_info_guard.time_remaining).abs() > 0.5) ||
                                        team_scores_changed {
            client_state.last_known_match_state = Some(current_match_info_guard.match_state);
            client_state.last_known_match_time_remaining = Some(current_match_info_guard.time_remaining);
            client_state.last_known_team_scores = current_match_info_guard.team_scores.clone();

            let fb_team_scores_vec_delta: Vec<_> = current_match_info_guard.team_scores.iter().map(|(team_id_ref, score_ref)| {
                fb::TeamScoreEntry::create(&mut builder, &fb::TeamScoreEntryArgs { team_id: *team_id_ref as i8, score: *score_ref })
            }).collect();
            let team_scores_fb_delta = builder.create_vector(&fb_team_scores_vec_delta);

            let mut winner_id_fb = None;
            let mut winner_name_fb = None;
            if current_match_info_guard.match_state == fb::MatchStateType::Ended {
                if current_match_info_guard.game_mode == fb::GameModeType::TeamDeathmatch || current_match_info_guard.game_mode == fb::GameModeType::CaptureTheFlag {
                    let t1_score = current_match_info_guard.team_scores.get(&1).cloned().unwrap_or(0);
                    let t2_score = current_match_info_guard.team_scores.get(&2).cloned().unwrap_or(0);
                    if t1_score > t2_score {
                        winner_id_fb = Some(builder.create_string("1"));
                        winner_name_fb = Some(builder.create_string("Red Team"));
                    } else if t2_score > t1_score {
                        winner_id_fb = Some(builder.create_string("2"));
                        winner_name_fb = Some(builder.create_string("Blue Team"));
                    } else if t1_score == t2_score && t1_score > 0 {
                        // Only a draw if both teams have equal non-zero scores
                        winner_name_fb = Some(builder.create_string("Draw"));
                    }
                    // If 0-0, leave winner_name_fb as None (no winner)
                }
            }
            Some(fb::MatchInfo::create(&mut builder, &fb::MatchInfoArgs{
                time_remaining: current_match_info_guard.time_remaining,
                match_state: current_match_info_guard.match_state,
                winner_id: winner_id_fb,
                winner_name: winner_name_fb,
                game_mode: current_match_info_guard.game_mode,
                team_scores: Some(team_scores_fb_delta),
            }))
        } else { None };

        let flag_states_delta_fb_vec: Vec<_> = if match_info_fb_offset.is_some() && current_match_info_guard.game_mode == fb::GameModeType::CaptureTheFlag {
            current_match_info_guard.flag_states.values().map(|fs| {
                let carrier_id_fb = fs.carrier_id.as_ref().map(|id| builder.create_string(id.as_str()));
                let pos_fb = fb::Vec2::create(&mut builder, &fb::Vec2Args{ x: fs.position.x, y: fs.position.y });
                fb::FlagState::create(&mut builder, &fb::FlagStateArgs {
                    team_id: fs.team_id as i8, status: fs.status, position: Some(pos_fb),
                    carrier_id: carrier_id_fb, respawn_timer: fs.respawn_timer,
                })
            }).collect()
        } else { Vec::new() };
        let flag_states_delta_fb = if !flag_states_delta_fb_vec.is_empty() { Some(builder.create_vector(&flag_states_delta_fb_vec)) } else { None };
        drop(current_match_info_guard);

        // Kill Feed
        let kill_feed_guard = self.kill_feed.read();
        let new_kill_feed_entries_to_send = kill_feed_guard.iter()
            .skip(client_state.last_kill_feed_count_sent)
            .map(|kf_entry| {
                let killer_fb = builder.create_string(&kf_entry.killer_name);
                let victim_fb = builder.create_string(&kf_entry.victim_name);
                fb::KillFeedEntry::create(&mut builder, &fb::KillFeedEntryArgs{
                    killer_name: Some(killer_fb), victim_name: Some(victim_fb),
                    weapon: map_server_weapon_to_fb(kf_entry.weapon), timestamp: kf_entry.timestamp as f32,
                    killer_position: None, victim_position: None, is_headshot: false,
                })
            }).collect::<Vec<_>>();
        client_state.last_kill_feed_count_sent = kill_feed_guard.len();
        drop(kill_feed_guard);
        let kill_feed_fb = if !new_kill_feed_entries_to_send.is_empty() { Some(builder.create_vector(&new_kill_feed_entries_to_send)) } else { None };

        // Destroyed Walls
        let mut new_destroyed_wall_ids_for_client_update_set = Vec::new();
        let new_destroyed_wall_ids_fb_vec: Vec<_> = destroyed_wall_ids_snapshot.iter()
            .filter(|wall_id_ref| !client_state.known_destroyed_wall_ids.contains(*wall_id_ref))
            .map(|&wall_id_val| {
                new_destroyed_wall_ids_for_client_update_set.push(wall_id_val);
                builder.create_string(&wall_id_val.to_string())
            })
            .collect();
        for wall_id_to_add in new_destroyed_wall_ids_for_client_update_set {
            client_state.known_destroyed_wall_ids.insert(wall_id_to_add);
        }
        let destroyed_walls_fb = if !new_destroyed_wall_ids_fb_vec.is_empty() { Some(builder.create_vector(&new_destroyed_wall_ids_fb_vec)) } else { None };

        // Updated Walls (Respawned walls and walls newly in AoI)
        let mut updated_walls_fb_vec = Vec::new();
        if let Some(aoi_entry) = self.player_aois.get(peer_id_str) {
            let p_aoi = aoi_entry.value();
            
            // First, check walls from updated_walls_this_tick
            let updated_walls_read_guard = self.updated_walls_this_tick.read();
            for (wall_id, wall_data) in updated_walls_read_guard.iter() {
                if p_aoi.visible_walls.contains(wall_id) {
                    let id_fb = builder.create_string(&wall_data.id.to_string());
                    updated_walls_fb_vec.push(fb::Wall::create(&mut builder, &fb::WallArgs{
                        id: Some(id_fb),
                        x: wall_data.x, y: wall_data.y,
                        width: wall_data.width, height: wall_data.height,
                        is_destructible: wall_data.is_destructible,
                        current_health: wall_data.current_health,
                        max_health: wall_data.max_health,
                    }));
                    client_state.known_destroyed_wall_ids.remove(wall_id);
                    // Update client's known wall state
                    client_state.last_known_wall_states.insert(*wall_id, (wall_data.current_health, wall_data.max_health));
                }
            }
            drop(updated_walls_read_guard);
            
            // Second, check for walls that are newly visible or have changed state
            for visible_wall_id in &p_aoi.visible_walls {
                // Skip if already added from updated_walls_this_tick
                if updated_walls_fb_vec.iter().any(|_| false) { // This check is placeholder, would need wall ID in fb::Wall
                    continue;
                }
                
                // Get current wall state from partitions
                let mut wall_found = false;
                for partition in self.world_partition_manager.get_partitions_for_processing() {
                    if let Some(wall) = partition.get_wall(*visible_wall_id) {
                        wall_found = true;
                        
                        // Check if client knows about this wall's current state
                        let should_send = if let Some(&(known_health, known_max_health)) = client_state.last_known_wall_states.get(visible_wall_id) {
                            // Send if health changed or wall was destroyed/respawned
                            known_health != wall.current_health || known_max_health != wall.max_health
                        } else {
                            // Client doesn't know about this wall yet
                            true
                        };
                        
                        if should_send {
                            let id_fb = builder.create_string(&wall.id.to_string());
                            updated_walls_fb_vec.push(fb::Wall::create(&mut builder, &fb::WallArgs{
                                id: Some(id_fb),
                                x: wall.x, y: wall.y,
                                width: wall.width, height: wall.height,
                                is_destructible: wall.is_destructible,
                                current_health: wall.current_health,
                                max_health: wall.max_health,
                            }));
                            
                            // Update client's known state
                            client_state.last_known_wall_states.insert(*visible_wall_id, (wall.current_health, wall.max_health));
                            
                            // Remove from destroyed list if it was there
                            if wall.current_health > 0 {
                                client_state.known_destroyed_wall_ids.remove(visible_wall_id);
                            }
                        }
                        break;
                    }
                }
                
                // If wall not found in partitions, it might have been removed
                if !wall_found {
                    client_state.last_known_wall_states.remove(visible_wall_id);
                }
            }
            
            // Clean up walls that are no longer visible
            client_state.last_known_wall_states.retain(|wall_id, _| p_aoi.visible_walls.contains(wall_id));
        }
        
        let updated_walls_fb = if !updated_walls_fb_vec.is_empty() {
            Some(builder.create_vector(&updated_walls_fb_vec))
        } else {
            None
        };


        // Removed Player IDs
        let mut removed_player_ids_fb_offsets_vec = Vec::new();
        let mut player_ids_to_remove_from_client_state_tracking = HashSet::new();

        for known_player_id_arc in client_state.last_known_player_states.keys() {
            if !self.player_manager.get_player_state(known_player_id_arc).is_some() {
                removed_player_ids_fb_offsets_vec.push(builder.create_string(known_player_id_arc.as_str()));
                player_ids_to_remove_from_client_state_tracking.insert(known_player_id_arc.clone());
            }
        }
        for id_to_remove in player_ids_to_remove_from_client_state_tracking {
            client_state.last_known_player_states.remove(&id_to_remove);
        }
        let removed_players_fb = if !removed_player_ids_fb_offsets_vec.is_empty() {
            Some(builder.create_vector(&removed_player_ids_fb_offsets_vec))
        } else {
            None
        };

        let delta_state_args = fb::DeltaStateMessageArgs {
            players: Some(players_fb),
            projectiles: Some(projectiles_delta_fb),
            removed_projectiles: Some(removed_projectiles_fb),
            pickups: Some(pickups_delta_fb),
            deactivated_pickup_ids: Some(deactivated_pickups_fb),
            game_events: Some(game_events_fb),
            timestamp: current_timestamp_ms,
            last_processed_input_sequence: last_processed_input_for_client,
            changed_player_fields: Some(player_fields_mask_fb),
            kill_feed: kill_feed_fb,
            match_info: match_info_fb_offset,
            destroyed_wall_ids: destroyed_walls_fb,
            flag_states: flag_states_delta_fb,
            removed_player_ids: removed_players_fb,
            updated_walls: updated_walls_fb,
        };
        let delta_state_msg = fb::DeltaStateMessage::create(&mut builder, &delta_state_args);

        let game_msg_args = fb::GameMessageArgs {
            msg_type: fb::MessageType::DeltaState,
            actual_message_type: fb::MessagePayload::DeltaStateMessage,
            actual_message: Some(delta_state_msg.as_union_value()),
        };
        let game_msg = fb::GameMessage::create(&mut builder, &game_msg_args);
        builder.finish(game_msg, None);

        let data_bytes = Bytes::from(builder.finished_data().to_vec());
        let dc_clone = Arc::clone(data_channel);
        let peer_id_clone = peer_id_str.to_string();
        tokio::spawn(async move {
            if let Err(e) = dc_clone.send(&data_bytes).await {
                handle_dc_send_error(&e.to_string(), &peer_id_clone, "delta state");
            }
        });
    }

    async fn send_pending_chat_messages(
        &self,
        peer_id_str: &str,
        data_channel: &Arc<crate::core::types::RTCDataChannel>,
        client_state: &mut ClientState
    ) {
        let last_seq_sent = client_state.last_chat_message_seq_sent;
        let mut max_seq_in_batch = last_seq_sent;

        let chat_messages_to_send: Vec<ChatMessage> = {
            let chat_guard = self.chat_messages_queue.read().await;
            chat_guard.iter()
                .filter(|msg| msg.seq > last_seq_sent)
                .cloned()
                .collect()
        };

        if !chat_messages_to_send.is_empty() {
            for chat_entry in chat_messages_to_send.iter() {
                let mut chat_builder = flatbuffers::FlatBufferBuilder::with_capacity(256);

                let player_id_fb = chat_builder.create_string(chat_entry.player_id.as_str());
                let username_fb = chat_builder.create_string(&chat_entry.username);
                let message_fb = chat_builder.create_string(&chat_entry.message);

                let chat_payload_offset = fb::ChatMessage::create(&mut chat_builder, &fb::ChatMessageArgs {
                    seq: chat_entry.seq,
                    player_id: Some(player_id_fb),
                    username: Some(username_fb),
                    message: Some(message_fb),
                    timestamp: chat_entry.timestamp,
                });

                let game_message_offset = fb::GameMessage::create(&mut chat_builder, &fb::GameMessageArgs {
                    msg_type: fb::MessageType::Chat,
                    actual_message_type: fb::MessagePayload::ChatMessage,
                    actual_message: Some(chat_payload_offset.as_union_value()),
                });

                chat_builder.finish(game_message_offset, None);
                let chat_msg_bytes = Bytes::from(chat_builder.finished_data().to_vec());

                let dc_for_chat = Arc::clone(data_channel);
                let peer_id_for_log = peer_id_str.to_string();
                let current_seq = chat_entry.seq;

                tokio::spawn(async move {
                    if let Err(e) = dc_for_chat.send(&chat_msg_bytes).await {
                        handle_dc_send_error(&e.to_string(), &peer_id_for_log, &format!("chat message seq {}", current_seq));
                    }
                });

                if chat_entry.seq > max_seq_in_batch {
                    max_seq_in_batch = chat_entry.seq;
                }
            }
            client_state.last_chat_message_seq_sent = max_seq_in_batch;
        }
    }


    pub async fn process_game_tick(self: Arc<Self>, dt: f32) -> Result<(), ServerError> {
        let tick_started = Instant::now();
        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
    
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
                }).await;
                if result.is_err() {
                    if frame % 60 == 0 { 
                        warn!("[Frame {}] Task '{}' timed out after {}ms", frame, task_name, NET_IO_TIMEOUT_MS);
                    }
                }
                trace!("[Frame {}] Finished task: {}", frame, task_name);
            }
        });
        
        if frame % AI_UPDATE_STRIDE == 0 {
            set.spawn({
                let server_clone = Arc::clone(&self);
                async move {
                    let task_name = "ai_update";
                    trace!("[Frame {}] Starting task: {}", frame, task_name);
                    let result = timeout(Duration::from_millis(AI_TIMEOUT_MS), async {
                        server_clone.run_ai_update().await;
                    }).await;
                    if result.is_err() {
                        if frame % 60 == 0 { 
                            warn!("[Frame {}] Task '{}' timed out after {}ms", frame, task_name, AI_TIMEOUT_MS);
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
        trace!("[Frame {}] Stage 1 (Input/AI) took: {:?}", frame, stage1_elapsed);
    
        // Stage 2: Physics & Game Logic (Sequential, mutation-heavy)
        let stage2_start = Instant::now();

        let physics_start = Instant::now();
        self.run_physics_update(dt).await;
        let physics_elapsed = physics_start.elapsed();
        trace!("[Frame {}] Physics update took: {:?}", frame, physics_elapsed);
    
        let game_logic_start = Instant::now();
        self.run_game_logic_update(dt).await;
        let game_logic_elapsed = game_logic_start.elapsed();
        trace!("[Frame {}] Game logic update took: {:?}", frame, game_logic_elapsed);
        
        let stage2_elapsed = stage2_start.elapsed();
        if stage2_elapsed > Duration::from_millis(SLOW_TICK_LOG_MS) && frame % 60 == 0 {
             warn!(
                ?frame,
                ms = stage2_elapsed.as_micros() as f64 / 1000.0,
                physics_ms = physics_elapsed.as_micros() as f64 / 1000.0,
                game_logic_ms = game_logic_elapsed.as_micros() as f64 / 1000.0,
                "Stage 2 (Physics/Logic) exceeded soft budget {}ms", SLOW_TICK_LOG_MS
            );
        }
    
        // Stage 3: State Sync & Broadcast
        let stage3_start = Instant::now();

        let sync_start = Instant::now();
        self.synchronize_state().await; 
        let sync_elapsed = sync_start.elapsed();
        trace!("[Frame {}] State synchronization took: {:?}", frame, sync_elapsed);
    
        let broadcast_start_time = Instant::now(); 
        let broadcast_elapsed_duration; 
        let broadcast_timed_out_flag; 
        {
            let server_for_broadcast_call = Arc::clone(&self); 
            let broadcast_future = server_for_broadcast_call.broadcast_world_updates_optimized(); 
            
            let timed_broadcast_future = tokio::time::timeout(
                Duration::from_millis(FAN_OUT_TIMEOUT_MS),
                broadcast_future
            );
            
            let b_start_inner = Instant::now();
            broadcast_timed_out_flag = timed_broadcast_future.await.is_err();
            broadcast_elapsed_duration = b_start_inner.elapsed();
        } 

        trace!("[Frame {}] Broadcast took: {:?} (timed_out: {})", frame, broadcast_elapsed_duration, broadcast_timed_out_flag);
    
        if broadcast_timed_out_flag {
             if frame % 60 == 0 { 
                error!("[Frame {}] Broadcast stage timed out after {}ms (actual: {:?})", frame, FAN_OUT_TIMEOUT_MS, broadcast_elapsed_duration);
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

    fn fb_safe_str(s: &str) -> String {
        s.chars()
            .filter(|&c| c != '\0' && c.is_ascii())
            .take(255) // Limit length
            .collect()
    }
    

    async fn send_chat_messages_optimized(
        &self,
        peer_id_str: &str,
        data_channel: &Arc<crate::core::types::RTCDataChannel>,
        client_state: &ClientState,
        chat_messages: &[ChatMessage],
    ) {
        let mut client_state_copy = client_state.clone();
        Self::send_chat_messages_static(
            peer_id_str,
            data_channel,
            &mut client_state_copy,
            chat_messages,
            &self.chat_messages_queue,
        ).await;
        // Update the client state
        self.client_states_map.write().insert(peer_id_str.to_string(), client_state_copy);
    }
    
}


fn event_position(event: &GameEvent) -> Vec2 {
    match event {
        GameEvent::PlayerDamaged { position, .. } => *position,
        GameEvent::PlayerKilled { position, .. } => *position,
        GameEvent::ProjectileHitWall { position, .. } => *position,
        GameEvent::PowerupCollected { position, .. } => *position,
        GameEvent::WeaponFired { position, .. } => *position,
        GameEvent::WallDestroyed { position, .. } => *position,
        GameEvent::WallImpact { position, .. } => *position,
        GameEvent::MeleeHit { position, .. } => *position,
        GameEvent::Footstep { position, .. } => *position,
        GameEvent::FlagGrabbed { position, .. } => *position,
        GameEvent::FlagDropped { position, .. } => *position,
        GameEvent::FlagReturned { position, .. } => *position,
        GameEvent::FlagCaptured { position, .. } => *position,
        _ => Vec2::zero(),
    }
}
fn event_instigator_id(event: &GameEvent) -> Option<PlayerID> {
    match event {
        GameEvent::PlayerDamaged { attacker_id, .. } => attacker_id.clone(),
        GameEvent::PlayerKilled { killer_id, .. } => Some(killer_id.clone()),
        GameEvent::WeaponFired { player_id, ..} => Some(player_id.clone()),
        GameEvent::PowerupCollected { player_id, .. } => Some(player_id.clone()),
        GameEvent::FlagGrabbed { player_id, .. } => Some(player_id.clone()),
        GameEvent::FlagCaptured { capturer_id, .. } => Some(capturer_id.clone()),
        _ => None,
    }
}
fn event_target_id(event: &GameEvent) -> Option<String> {
    match event {
        GameEvent::PlayerDamaged { target_id, .. } => Some(target_id.to_string()),
        GameEvent::PlayerKilled { victim_id, .. } => Some(victim_id.to_string()),
        GameEvent::ProjectileHitWall { wall_id, .. } => Some(wall_id.to_string()),
        GameEvent::PowerupCollected { pickup_id, ..} => Some(pickup_id.to_string()),
        GameEvent::WallDestroyed { wall_id, .. } => Some(wall_id.to_string()),
        GameEvent::WallImpact { wall_id, .. } => Some(wall_id.to_string()),
        GameEvent::MeleeHit { target_id, .. } => target_id.as_ref().map(|id| id.to_string()),
        GameEvent::FlagDropped { flag_team_id, .. } => Some(flag_team_id.to_string()),
        GameEvent::FlagReturned { flag_team_id, .. } => Some(flag_team_id.to_string()),
        _ => None,
    }
}
fn event_weapon_type(event: &GameEvent) -> Option<ServerWeaponType> {
    match event {
        GameEvent::PlayerDamaged { weapon, .. } => Some(*weapon),
        GameEvent::PlayerKilled { weapon, .. } => Some(*weapon),
        GameEvent::WeaponFired { weapon, .. } => Some(*weapon),
        _ => None,
    }
}
fn event_value(event: &GameEvent) -> Option<f32> {
    match event {
        GameEvent::PlayerDamaged { damage, .. } => Some(*damage as f32),
        GameEvent::WallImpact { damage, .. } => Some(*damage as f32),
        _ => None,
    }
}
fn map_game_event_type_to_fb(event: &GameEvent) -> fb::GameEventType {
    match event {
         GameEvent::PlayerDamaged { .. } => fb::GameEventType::PlayerDamageEffect,
         GameEvent::PlayerKilled { .. } => fb::GameEventType::PlayerDamageEffect,
         GameEvent::ProjectileHitWall { .. } => fb::GameEventType::WallImpact,
         GameEvent::PowerupCollected { .. } => fb::GameEventType::PowerupActivated,
         GameEvent::WeaponFired { .. } => fb::GameEventType::WeaponFire,
         GameEvent::WallDestroyed { .. } => fb::GameEventType::WallDestroyed,
         GameEvent::WallImpact { .. } => fb::GameEventType::WallImpact,
         GameEvent::FlagGrabbed { .. } => fb::GameEventType::FlagGrabbed,
         GameEvent::FlagDropped { .. } => fb::GameEventType::FlagDropped,
         GameEvent::FlagReturned { .. } => fb::GameEventType::FlagReturned,
         GameEvent::FlagCaptured { .. } => fb::GameEventType::FlagCaptured,
         GameEvent::PlayerJoined { .. } | GameEvent::PlayerLeft { .. } => fb::GameEventType::BulletImpact, // Placeholder, consider specific events
         GameEvent::MeleeHit { .. } => fb::GameEventType::PlayerDamageEffect, // Could be a specific MeleeImpact event type
         GameEvent::Footstep { .. } => fb::GameEventType::BulletImpact,  // Placeholder, consider specific events
    }
}
