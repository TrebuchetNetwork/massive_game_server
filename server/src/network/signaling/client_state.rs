use crate::core::types::{EntityId, PlayerID, PlayerState};
use crate::flatbuffers_generated::game_protocol as fb;
use bytes::Bytes;
use parking_lot::RwLock as ParkingLotRwLock;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::Instant,
};

#[derive(Clone, Debug)]
pub struct ClientState {
    pub known_walls_sent: bool,
    pub pending_initial_state_bytes: Option<Bytes>,
    pub pending_initial_state_chunks: VecDeque<Bytes>,
    pub last_update_sent_time: Instant,
    pub last_known_player_states: HashMap<PlayerID, PlayerState>,
    pub last_known_projectile_ids: HashSet<EntityId>,
    pub last_known_pickup_states: HashMap<EntityId, PickupState>,
    pub last_known_match_state: Option<fb::MatchStateType>,
    pub last_known_match_time_remaining: Option<f32>,
    pub last_known_team_scores: HashMap<u8, i32>,
    pub known_destroyed_wall_ids: HashSet<EntityId>,
    pub last_kill_feed_count_sent: usize,
    pub last_chat_message_seq_sent: u64,
    pub last_broadcast_frame: u64,
    pub last_known_players: HashSet<PlayerID>,
    pub last_known_wall_ids: Option<HashSet<EntityId>>,
    pub last_known_wall_states: HashMap<EntityId, (i32, i32)>, // wall_id -> (current_health, max_health)
    pub match_info_pending: bool,
    pub is_mobile: bool,
    /// Mobile clients get updates at a lower frequency (every N frames)
    pub mobile_delta_skip_modulus: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PickupState {
    pub is_active: bool,
}

impl Default for ClientState {
    fn default() -> Self {
        ClientState {
            known_walls_sent: false,
            pending_initial_state_bytes: None,
            pending_initial_state_chunks: VecDeque::new(),
            last_update_sent_time: Instant::now(),
            last_known_player_states: HashMap::new(),
            last_known_projectile_ids: HashSet::new(),
            last_known_pickup_states: HashMap::new(),
            last_known_match_state: None,
            last_known_match_time_remaining: None,
            last_known_team_scores: HashMap::new(),
            known_destroyed_wall_ids: HashSet::new(),
            last_kill_feed_count_sent: 0,
            last_chat_message_seq_sent: 0,
            last_broadcast_frame: 0,
            last_known_players: HashSet::new(),
            last_known_wall_ids: None,
            last_known_wall_states: HashMap::new(),
            match_info_pending: true,
            is_mobile: false,
            mobile_delta_skip_modulus: 1,
        }
    }
}

//pub type ClientStatesMap = Arc<DashMap<String, ClientState>>;
pub type ClientStatesMap = Arc<ParkingLotRwLock<HashMap<String, ClientState>>>;
