// massive_game_server/server/src/network/signaling.rs
use crate::core::config::ServerConfig;
use crate::core::types::{
    EntityId, PlayerAoIs, PlayerID, PlayerInputData, RTCDataChannel as CoreRTCDataChannel,
    FIELD_FLAG, FIELD_SCORE_STATS,
};

use crate::core::constants::*;
use crate::core::types::PlayerState;
use crate::entities::player::ImprovedPlayerManager;
use crate::flatbuffers_generated::game_protocol as fb;
use crate::network::connection_manager::{
    shared_connection_manager, ConnectionInfo, TransportKind,
};
use crate::operational::auth::AuthService;
use crate::operational::monitoring::metrics;
use crate::server::instance::MassiveGameServer; // Added for server access for initial spawn
use crate::world::partition::WorldPartitionManager;
use parking_lot::RwLock as ParkingLotRwLock;

use bytes::Bytes;
use dashmap::DashMap;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    net::IpAddr,
    sync::{Arc, Mutex as StdMutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{mpsc, Mutex as AsyncMutex, OwnedSemaphorePermit, RwLock, Semaphore};
use tracing::{debug, error, info, warn};
use warp::ws::{Message, WebSocket};
use webrtc::{
    api::{media_engine::MediaEngine, APIBuilder, API},
    data_channel::{data_channel_message::DataChannelMessage, RTCDataChannel},
    ice_transport::{
        ice_candidate::{RTCIceCandidate, RTCIceCandidateInit},
        ice_server::RTCIceServer,
    },
    peer_connection::{
        configuration::RTCConfiguration, peer_connection_state::RTCPeerConnectionState,
        sdp::session_description::RTCSessionDescription,
    },
};
// Removed: use rand::Rng; // Not directly used here after spawn logic change

// Type Aliases
pub type SignalingPeers = Arc<DashMap<String, mpsc::Sender<Result<Message, warp::Error>>>>;
pub type PlayerManagerRef = Arc<ImprovedPlayerManager>;
pub type DataChannelsMap = Arc<DashMap<String, Arc<CoreRTCDataChannel>>>;
pub type WorldPartitionManagerRef = Arc<WorldPartitionManager>;
pub type ServerInstanceRef = Arc<MassiveGameServer>; // Type alias for server instance

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub seq: u64,
    pub player_id: PlayerID,
    pub username: String,
    pub message: String,
    pub timestamp: u64,
}
pub type ChatMessagesQueue = Arc<RwLock<VecDeque<ChatMessage>>>;
static NEXT_CHAT_MESSAGE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static SHARED_WEBRTC_API: OnceLock<Result<Arc<API>, String>> = OnceLock::new();
static WEBRTC_PEER_STATES: OnceLock<DashMap<String, &'static str>> = OnceLock::new();
const MAX_CHAT_MESSAGE_CHARS: usize = 160;
const MAX_CHAT_USERNAME_CHARS: usize = 32;
const CHAT_HISTORY_LIMIT: usize = 50;
const WEBRTC_STATE_LABELS: [&str; 7] = [
    "new",
    "connecting",
    "connected",
    "disconnected",
    "failed",
    "closed",
    "other",
];

pub fn next_chat_message_seq() -> u64 {
    NEXT_CHAT_MESSAGE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn shared_webrtc_api() -> Result<Arc<API>, String> {
    match SHARED_WEBRTC_API.get_or_init(|| {
        let mut media_engine = MediaEngine::default();
        media_engine
            .register_default_codecs()
            .map_err(|e| format!("register_default_codecs failed: {e}"))?;
        Ok(Arc::new(
            APIBuilder::new().with_media_engine(media_engine).build(),
        ))
    }) {
        Ok(api) => Ok(Arc::clone(api)),
        Err(err) => Err(err.clone()),
    }
}

fn shared_webrtc_peer_states() -> &'static DashMap<String, &'static str> {
    WEBRTC_PEER_STATES.get_or_init(DashMap::new)
}

fn webrtc_state_label(state: RTCPeerConnectionState) -> &'static str {
    match state {
        RTCPeerConnectionState::New => "new",
        RTCPeerConnectionState::Connecting => "connecting",
        RTCPeerConnectionState::Connected => "connected",
        RTCPeerConnectionState::Disconnected => "disconnected",
        RTCPeerConnectionState::Failed => "failed",
        RTCPeerConnectionState::Closed => "closed",
        _ => "other",
    }
}

fn publish_webrtc_peer_state_gauges(states: &DashMap<String, &'static str>) {
    let mut counts = [0usize; WEBRTC_STATE_LABELS.len()];
    states.iter().for_each(|entry| {
        if let Some(index) = WEBRTC_STATE_LABELS
            .iter()
            .position(|label| label == entry.value())
        {
            counts[index] = counts[index].saturating_add(1);
        }
    });
    for (index, label) in WEBRTC_STATE_LABELS.iter().enumerate() {
        metrics::set_webrtc_peers_in_state(label, counts[index]);
    }
}

fn record_webrtc_peer_state(peer_id: &str, state: RTCPeerConnectionState) {
    let label = webrtc_state_label(state);
    metrics::record_webrtc_peer_state_transition(label);
    let states = shared_webrtc_peer_states();
    states.insert(peer_id.to_owned(), label);
    publish_webrtc_peer_state_gauges(states);
}

fn remove_webrtc_peer_state(peer_id: &str) {
    let states = shared_webrtc_peer_states();
    states.remove(peer_id);
    publish_webrtc_peer_state_gauges(states);
}

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
    pub last_known_players: HashSet<Arc<String>>,
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

#[derive(Serialize, Deserialize, Debug)]
struct SignalingMessageJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    sdp: Option<RTCSessionDescription>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ice: Option<RTCIceCandidateInitSerde>,
}

fn env_bool(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|raw| {
            let normalized = raw.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "yes" || normalized == "on"
        })
        .unwrap_or(false)
}

fn parse_csv(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn is_bidi_or_directional_control(ch: char) -> bool {
    matches!(
        ch,
        '\u{200E}'
            | '\u{200F}'
            | '\u{202A}'
            | '\u{202B}'
            | '\u{202C}'
            | '\u{202D}'
            | '\u{202E}'
            | '\u{2066}'
            | '\u{2067}'
            | '\u{2068}'
            | '\u{2069}'
    )
}

fn sanitize_text_field(raw: &str, max_chars: usize, username_mode: bool) -> Option<String> {
    if max_chars == 0 {
        return None;
    }

    let mut cleaned = String::with_capacity(raw.len().min(max_chars));
    let mut count = 0usize;
    let mut last_was_space = true;

    for ch in raw.chars() {
        if (ch.is_control() && !ch.is_whitespace()) || is_bidi_or_directional_control(ch) {
            continue;
        }
        let normalized = if ch.is_whitespace() { ' ' } else { ch };
        if matches!(
            normalized,
            '<' | '>' | '`' | '&' | '"' | '\'' | '\\' | '/' | '{' | '}'
        ) {
            continue;
        }

        if username_mode
            && !(normalized.is_alphanumeric()
                || normalized == '_'
                || normalized == '-'
                || normalized == '.'
                || normalized == ' ')
        {
            continue;
        }

        if normalized == ' ' {
            if last_was_space {
                continue;
            }
            last_was_space = true;
        } else {
            last_was_space = false;
        }

        cleaned.push(normalized);
        count += 1;
        if count >= max_chars {
            break;
        }
    }

    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn sanitize_chat_field(raw: &str, max_chars: usize) -> Option<String> {
    sanitize_text_field(raw, max_chars, false)
}

fn sanitize_username_field(raw: &str, max_chars: usize) -> Option<String> {
    sanitize_text_field(raw, max_chars, true)
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn build_welcome_message_bytes(player_id: &str, server_tick_rate: u16) -> Bytes {
    let mut builder_welcome = flatbuffers::FlatBufferBuilder::with_capacity(256);
    let player_id_fb_welcome = builder_welcome.create_string(player_id);
    let welcome_text_fb = builder_welcome.create_string("Welcome to MassiveGameServer!");
    let welcome_msg_args = fb::WelcomeMessageArgs {
        player_id: Some(player_id_fb_welcome),
        message: Some(welcome_text_fb),
        server_tick_rate,
    };
    let welcome_msg = fb::WelcomeMessage::create(&mut builder_welcome, &welcome_msg_args);
    let game_msg_welcome_args = fb::GameMessageArgs {
        msg_type: fb::MessageType::Welcome,
        actual_message_type: fb::MessagePayload::WelcomeMessage,
        actual_message: Some(welcome_msg.as_union_value()),
        protocol_version: GAME_PROTOCOL_VERSION,
    };
    let game_msg_welcome = fb::GameMessage::create(&mut builder_welcome, &game_msg_welcome_args);
    builder_welcome.finish(game_msg_welcome, None);
    let (buffer, root_index) = builder_welcome.collapse();
    Bytes::from(buffer).slice(root_index..)
}

fn parse_ice_servers_env(raw: &str) -> Vec<RTCIceServer> {
    raw.split(';')
        .filter_map(|entry| {
            let mut parts = entry.split('|').map(|segment| segment.trim());
            let urls_raw = parts.next().unwrap_or_default();
            let urls = parse_csv(urls_raw);
            if urls.is_empty() {
                return None;
            }

            let username = parts.next().unwrap_or_default().to_owned();
            let credential = parts.next().unwrap_or_default().to_owned();

            let mut server = RTCIceServer {
                urls,
                ..Default::default()
            };
            if !username.is_empty() {
                server.username = username;
            }
            if !credential.is_empty() {
                server.credential = credential;
            }
            Some(server)
        })
        .collect()
}

fn build_ice_servers() -> Vec<RTCIceServer> {
    let disable_stun = env_bool("MGS_DISABLE_STUN");
    let mut ice_servers: Vec<RTCIceServer> = Vec::new();

    if !disable_stun {
        let stun_urls = std::env::var("MGS_STUN_URLS")
            .ok()
            .map(|raw| parse_csv(&raw))
            .filter(|urls| !urls.is_empty())
            .unwrap_or_else(|| vec!["stun:stun.l.google.com:19302".to_owned()]);
        ice_servers.push(RTCIceServer {
            urls: stun_urls,
            ..Default::default()
        });
    }

    let turn_urls = std::env::var("MGS_TURN_URLS")
        .ok()
        .map(|raw| parse_csv(&raw))
        .unwrap_or_default();
    if !turn_urls.is_empty() {
        let mut turn_server = RTCIceServer {
            urls: turn_urls,
            ..Default::default()
        };
        if let Ok(username) = std::env::var("MGS_TURN_USERNAME") {
            if !username.trim().is_empty() {
                turn_server.username = username.trim().to_owned();
            }
        }
        if let Ok(credential) = std::env::var("MGS_TURN_CREDENTIAL") {
            if !credential.trim().is_empty() {
                turn_server.credential = credential.trim().to_owned();
            }
        }
        ice_servers.push(turn_server);
    }

    if let Ok(extra_ice_servers) = std::env::var("MGS_ICE_SERVERS") {
        ice_servers.extend(parse_ice_servers_env(&extra_ice_servers));
    }

    ice_servers
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct RTCIceCandidateInitSerde {
    candidate: String,
    #[serde(rename = "sdpMid")]
    sdp_mid: Option<String>,
    #[serde(rename = "sdpMLineIndex")]
    sdp_m_line_index: Option<u16>,
    #[serde(rename = "usernameFragment")]
    username_fragment: Option<String>,
}

const DEFAULT_JOIN_RATE_LIMIT_PER_SEC: u32 = 30;
const DEFAULT_JOIN_RATE_LIMIT_BURST: u32 = 50;
const DEFAULT_IP_RATE_LIMIT_PER_SEC: u32 = 20;
const DEFAULT_IP_RATE_LIMIT_BURST: u32 = 40;
const DEFAULT_SDP_ADMISSION_CONCURRENCY: usize = 4;
const JOIN_RATE_LIMIT_THROTTLED_MESSAGE: &str = "Server busy handling joins, retry shortly.";
const MAX_SIGNALING_TEXT_BYTES: usize = 128 * 1024;
const MAX_SIGNALING_SDP_BYTES: usize = 120 * 1024;
const MAX_SIGNALING_ICE_CANDIDATE_BYTES: usize = 4 * 1024;
const MAX_SIGNALING_ICE_SDP_MID_BYTES: usize = 256;
const MAX_SIGNALING_ICE_USERNAME_FRAGMENT_BYTES: usize = 256;
const SIGNALING_OUTBOX_CAPACITY: usize = 1000;

fn try_queue_signaling_message(
    sender: &mpsc::Sender<Result<Message, warp::Error>>,
    message: Result<Message, warp::Error>,
    peer_id: &str,
    label: &str,
) -> bool {
    match sender.try_send(message) {
        Ok(()) => true,
        Err(mpsc::error::TrySendError::Full(_)) => {
            warn!(
                "[{}]: Signaling outbox full while sending {} (capacity={}). Dropping message.",
                peer_id, label, SIGNALING_OUTBOX_CAPACITY
            );
            false
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            warn!(
                "[{}]: Signaling outbox closed while sending {}.",
                peer_id, label
            );
            false
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct InputRateLimitConfig {
    per_sec: u32,
    burst: u32,
}

#[derive(Clone, Copy, Debug)]
struct IpRateLimitConfig {
    per_sec: u32,
    burst: u32,
}

#[derive(Debug)]
struct JoinRateLimiter {
    refill_per_sec: f64,
    capacity: f64,
    available_tokens: f64,
    last_refill_at: Instant,
}

impl JoinRateLimiter {
    fn new(refill_per_sec: u32, capacity: u32) -> Self {
        let refill = refill_per_sec.max(1) as f64;
        let cap = capacity.max(1) as f64;
        Self {
            refill_per_sec: refill,
            capacity: cap,
            available_tokens: cap,
            last_refill_at: Instant::now(),
        }
    }

    fn try_acquire(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now
            .checked_duration_since(self.last_refill_at)
            .unwrap_or(Duration::from_millis(0))
            .as_secs_f64();
        if elapsed > 0.0 {
            self.available_tokens =
                (self.available_tokens + (elapsed * self.refill_per_sec)).min(self.capacity);
            self.last_refill_at = now;
        }

        if self.available_tokens >= 1.0 {
            self.available_tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[derive(Debug)]
struct InputRateLimiter {
    refill_per_sec: f64,
    capacity: f64,
    available_tokens: f64,
    last_refill_at: Instant,
    last_drop_log_at: Instant,
}

impl InputRateLimiter {
    fn new(refill_per_sec: u32, capacity: u32) -> Self {
        let refill = refill_per_sec.max(1) as f64;
        let cap = capacity.max(1) as f64;
        Self {
            refill_per_sec: refill,
            capacity: cap,
            available_tokens: cap,
            last_refill_at: Instant::now(),
            last_drop_log_at: Instant::now()
                .checked_sub(Duration::from_secs(
                    INPUT_RATE_LIMIT_THROTTLE_LOG_INTERVAL_SECS,
                ))
                .unwrap_or_else(Instant::now),
        }
    }

    fn try_acquire(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now
            .checked_duration_since(self.last_refill_at)
            .unwrap_or(Duration::from_millis(0))
            .as_secs_f64();
        if elapsed > 0.0 {
            self.available_tokens =
                (self.available_tokens + (elapsed * self.refill_per_sec)).min(self.capacity);
            self.last_refill_at = now;
        }

        if self.available_tokens >= 1.0 {
            self.available_tokens -= 1.0;
            true
        } else {
            false
        }
    }

    fn should_log_throttle(&mut self) -> bool {
        let now = Instant::now();
        if now
            .checked_duration_since(self.last_drop_log_at)
            .unwrap_or(Duration::from_millis(0))
            >= Duration::from_secs(INPUT_RATE_LIMIT_THROTTLE_LOG_INTERVAL_SECS)
        {
            self.last_drop_log_at = now;
            true
        } else {
            false
        }
    }
}

fn env_u32(name: &str, default_value: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or(default_value)
}

fn join_rate_limiter() -> Option<&'static StdMutex<JoinRateLimiter>> {
    static JOIN_RATE_LIMITER: OnceLock<Option<StdMutex<JoinRateLimiter>>> = OnceLock::new();
    JOIN_RATE_LIMITER
        .get_or_init(|| {
            let per_sec = env_u32(
                "MGS_JOIN_RATE_LIMIT_PER_SEC",
                DEFAULT_JOIN_RATE_LIMIT_PER_SEC,
            );
            if per_sec == 0 {
                info!("Join rate limiter disabled (MGS_JOIN_RATE_LIMIT_PER_SEC=0).");
                return None;
            }

            let burst =
                env_u32("MGS_JOIN_RATE_LIMIT_BURST", DEFAULT_JOIN_RATE_LIMIT_BURST).max(per_sec);
            info!(
                "Join rate limiter enabled: {} joins/sec with burst {}.",
                per_sec, burst
            );
            Some(StdMutex::new(JoinRateLimiter::new(per_sec, burst)))
        })
        .as_ref()
}

fn ip_rate_limit_config() -> Option<IpRateLimitConfig> {
    static IP_RATE_LIMIT_CONFIG: OnceLock<Option<IpRateLimitConfig>> = OnceLock::new();
    IP_RATE_LIMIT_CONFIG
        .get_or_init(|| {
            let per_sec = env_u32("MGS_IP_RATE_LIMIT_PER_SEC", DEFAULT_IP_RATE_LIMIT_PER_SEC);
            if per_sec == 0 {
                info!("IP rate limiter disabled (MGS_IP_RATE_LIMIT_PER_SEC=0).");
                return None;
            }

            let burst =
                env_u32("MGS_IP_RATE_LIMIT_BURST", DEFAULT_IP_RATE_LIMIT_BURST).max(per_sec);
            info!(
                "IP rate limiter enabled: {} connects/sec per source IP with burst {}.",
                per_sec, burst
            );
            Some(IpRateLimitConfig { per_sec, burst })
        })
        .as_ref()
        .copied()
}

fn input_rate_limit_config() -> Option<InputRateLimitConfig> {
    static INPUT_RATE_LIMIT_CONFIG: OnceLock<Option<InputRateLimitConfig>> = OnceLock::new();
    INPUT_RATE_LIMIT_CONFIG
        .get_or_init(|| {
            let per_sec = env_u32(
                "MGS_INPUT_RATE_LIMIT_PER_SEC",
                DEFAULT_INPUT_RATE_LIMIT_PER_SEC,
            );
            if per_sec == 0 {
                info!("Input rate limiter disabled (MGS_INPUT_RATE_LIMIT_PER_SEC=0).");
                return None;
            }

            let burst =
                env_u32("MGS_INPUT_RATE_LIMIT_BURST", DEFAULT_INPUT_RATE_LIMIT_BURST).max(per_sec);
            info!(
                "Input rate limiter enabled: {} inputs/sec with burst {}.",
                per_sec, burst
            );
            Some(InputRateLimitConfig { per_sec, burst })
        })
        .as_ref()
        .copied()
}

fn sdp_admission_semaphore() -> Option<&'static Arc<Semaphore>> {
    static SDP_ADMISSION_SEMAPHORE: OnceLock<Option<Arc<Semaphore>>> = OnceLock::new();
    SDP_ADMISSION_SEMAPHORE
        .get_or_init(|| {
            let limit = std::env::var("MGS_SIGNALING_SDP_CONCURRENCY")
                .ok()
                .and_then(|raw| raw.parse::<usize>().ok())
                .unwrap_or(DEFAULT_SDP_ADMISSION_CONCURRENCY);
            if limit == 0 {
                info!("SDP admission gate disabled (MGS_SIGNALING_SDP_CONCURRENCY=0).");
                None
            } else {
                info!(
                    "SDP admission gate enabled with max {} concurrent offers.",
                    limit
                );
                Some(Arc::new(Semaphore::new(limit)))
            }
        })
        .as_ref()
}

async fn acquire_sdp_admission_permit(
    peer_id: &str,
    sender: &mpsc::Sender<Result<Message, warp::Error>>,
) -> Option<OwnedSemaphorePermit> {
    let semaphore = sdp_admission_semaphore()?;
    match semaphore.clone().try_acquire_owned() {
        Ok(permit) => Some(permit),
        Err(_) => {
            let queue_hint = semaphore.available_permits().saturating_add(1).max(1);
            let queue_notice = serde_json::json!({
                "event": "sdp_offer_queue",
                "queue_position_hint": queue_hint,
            })
            .to_string();
            let _ = try_queue_signaling_message(
                sender,
                Ok(Message::text(queue_notice)),
                peer_id,
                "sdp_offer_queue",
            );
            match semaphore.clone().acquire_owned().await {
                Ok(permit) => Some(permit),
                Err(err) => {
                    warn!(
                        "[{}]: Failed to acquire SDP admission permit because semaphore is closed: {}",
                        peer_id, err
                    );
                    None
                }
            }
        }
    }
}

fn try_acquire_join_rate_limit_token() -> bool {
    let Some(rate_limiter) = join_rate_limiter() else {
        return true;
    };

    let mut limiter_guard = match rate_limiter.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            warn!("Join rate limiter mutex poisoned; continuing with recovered state.");
            poisoned.into_inner()
        }
    };
    limiter_guard.try_acquire()
}

/// Maximum number of tracked IPs in the signaling IP rate limiter map before cleanup triggers.
const IP_RATE_LIMITER_MAX_ENTRIES: usize = 10_000;
/// Entries idle for longer than this are eligible for eviction during cleanup.
const IP_RATE_LIMITER_IDLE_SECS: u64 = 300;

fn try_acquire_ip_rate_limit_token(client_ip: &IpAddr) -> bool {
    let Some(cfg) = ip_rate_limit_config() else {
        return true;
    };

    static IP_RATE_LIMITERS: OnceLock<DashMap<IpAddr, JoinRateLimiter>> = OnceLock::new();
    let limiters = IP_RATE_LIMITERS.get_or_init(DashMap::new);

    // Periodic cleanup: evict entries that have been idle for over 5 minutes.
    if limiters.len() > IP_RATE_LIMITER_MAX_ENTRIES {
        let now = Instant::now();
        let idle_threshold = Duration::from_secs(IP_RATE_LIMITER_IDLE_SECS);
        limiters.retain(|_ip, limiter| {
            now.saturating_duration_since(limiter.last_refill_at) < idle_threshold
        });
    }

    let mut limiter = limiters
        .entry(*client_ip)
        .or_insert_with(|| JoinRateLimiter::new(cfg.per_sec, cfg.burst));
    limiter.try_acquire()
}

fn validate_signaling_payload(payload: &SignalingMessageJson) -> Result<(), &'static str> {
    if payload.sdp.is_none() && payload.ice.is_none() {
        return Err("empty signaling payload");
    }

    if let Some(sdp) = payload.sdp.as_ref() {
        if sdp.sdp.len() > MAX_SIGNALING_SDP_BYTES {
            return Err("SDP payload too large");
        }
    }

    if let Some(ice) = payload.ice.as_ref() {
        if ice.candidate.len() > MAX_SIGNALING_ICE_CANDIDATE_BYTES {
            return Err("ICE candidate payload too large");
        }
        if ice
            .sdp_mid
            .as_ref()
            .is_some_and(|value| value.len() > MAX_SIGNALING_ICE_SDP_MID_BYTES)
        {
            return Err("ICE sdpMid payload too large");
        }
        if ice
            .username_fragment
            .as_ref()
            .is_some_and(|value| value.len() > MAX_SIGNALING_ICE_USERNAME_FRAGMENT_BYTES)
        {
            return Err("ICE usernameFragment payload too large");
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_signaling_connection(
    ws: WebSocket,
    peer_id_str: String,
    signaling_peers: SignalingPeers,
    player_manager: PlayerManagerRef,
    _world_partition_manager: WorldPartitionManagerRef, // Marked as unused if not directly used in this function
    data_channels_map: DataChannelsMap,
    client_states_map: ClientStatesMap,
    chat_messages_queue: ChatMessagesQueue,
    config: Arc<ServerConfig>,
    player_aois: PlayerAoIs,
    server_instance: ServerInstanceRef, // Added server instance for initial spawn
    auth_service: AuthService,
    auth_user_id: Option<String>,
    requested_team_id: Option<u8>,
    remote_ip: Option<IpAddr>,
    is_mobile: bool,
) {
    shared_connection_manager().upsert(ConnectionInfo::new(
        peer_id_str.clone(),
        TransportKind::WebRtc,
    ));

    if !try_acquire_join_rate_limit_token() {
        warn!(
            "[{}]: Join attempt throttled by join rate limiter.",
            peer_id_str
        );
        let (mut throttled_ws_tx, _) = ws.split();
        let throttled_payload = serde_json::json!({
            "error": "join_rate_limited",
            "detail": JOIN_RATE_LIMIT_THROTTLED_MESSAGE,
        })
        .to_string();
        let _ = throttled_ws_tx.send(Message::text(throttled_payload)).await;
        let _ = throttled_ws_tx.send(Message::close()).await;
        let _ = shared_connection_manager().remove(&peer_id_str);
        return;
    }
    if let Some(client_ip) = remote_ip {
        if !try_acquire_ip_rate_limit_token(&client_ip) {
            warn!(
                "[{}]: Join attempt throttled by IP rate limiter (ip={}).",
                peer_id_str, client_ip
            );
            let (mut throttled_ws_tx, _) = ws.split();
            let throttled_payload = serde_json::json!({
                "error": "ip_rate_limited",
                "detail": "Too many signaling connections from this IP. Retry shortly.",
            })
            .to_string();
            let _ = throttled_ws_tx.send(Message::text(throttled_payload)).await;
            let _ = throttled_ws_tx.send(Message::close()).await;
            let _ = shared_connection_manager().remove(&peer_id_str);
            return;
        }
    }

    info!("[{}]: New WebSocket connection for signaling.", peer_id_str);
    server_instance.note_join_enqueued(&peer_id_str);

    let (mut ws_tx, mut ws_rx) = ws.split();
    let (client_signaling_tx, mut client_signaling_rx) = mpsc::channel(SIGNALING_OUTBOX_CAPACITY);

    signaling_peers.insert(peer_id_str.clone(), client_signaling_tx.clone());
    metrics::set_ws_connections_active(signaling_peers.len());

    let peer_id_fwd = peer_id_str.clone();
    tokio::spawn(async move {
        while let Some(message_result) = client_signaling_rx.recv().await {
            match message_result {
                Ok(msg) => {
                    shared_connection_manager().touch(peer_id_fwd.as_str());
                    metrics::record_network_bytes("egress_ws", msg.as_bytes().len());
                    if ws_tx.send(msg).await.is_err() {
                        warn!(
                            "[{}]: WebSocket send error, terminating forwarder.",
                            peer_id_fwd
                        );
                        break;
                    }
                }
                Err(e) => {
                    error!(
                        "[{}]: Error in message to send via WebSocket: {:?}",
                        peer_id_fwd, e
                    );
                    break;
                }
            }
        }
        info!("[{}]: Signaling forwarder task ended.", peer_id_fwd);
    });

    let api = match shared_webrtc_api() {
        Ok(api) => api,
        Err(e) => {
            error!(
                "[{}]: Failed to initialize shared WebRTC API: {}",
                peer_id_str, e
            );
            cleanup_connection(
                &peer_id_str,
                &signaling_peers,
                &player_manager,
                &data_channels_map,
                &client_states_map,
                &player_aois,
                &auth_service,
            );
            return;
        }
    };
    let ice_servers = build_ice_servers();
    info!(
        "[{}]: Using {} ICE server(s) for WebRTC negotiation.",
        peer_id_str,
        ice_servers.len()
    );
    let rtc_config = RTCConfiguration {
        ice_servers,
        ..Default::default()
    };

    let peer_connection = match api.new_peer_connection(rtc_config).await {
        Ok(pc) => Arc::new(pc),
        Err(e) => {
            error!("[{}]: Failed to create PeerConnection: {}", peer_id_str, e);
            cleanup_connection(
                &peer_id_str,
                &signaling_peers,
                &player_manager,
                &data_channels_map,
                &client_states_map,
                &player_aois,
                &auth_service,
            );
            return;
        }
    };

    let pc_for_ice = Arc::clone(&peer_connection);
    let ice_sender_clone = client_signaling_tx.clone();
    let peer_id_for_ice = peer_id_str.clone();
    pc_for_ice.on_ice_candidate(Box::new(move |candidate: Option<RTCIceCandidate>| {
        let ice_sender = ice_sender_clone.clone();
        let pid_ice = peer_id_for_ice.clone();
        Box::pin(async move {
            if let Some(c) = candidate {
                match c.to_json() {
                    Ok(ice_init_struct) => {
                        let ice_serde = RTCIceCandidateInitSerde {
                            candidate: ice_init_struct.candidate,
                            sdp_mid: ice_init_struct.sdp_mid,
                            sdp_m_line_index: ice_init_struct.sdp_mline_index,
                            username_fragment: ice_init_struct.username_fragment,
                        };
                        let sig_msg = SignalingMessageJson {
                            sdp: None,
                            ice: Some(ice_serde),
                        };
                        match serde_json::to_string(&sig_msg) {
                            Ok(json_msg) => {
                                if !try_queue_signaling_message(
                                    &ice_sender,
                                    Ok(Message::text(json_msg)),
                                    &pid_ice,
                                    "ice_candidate",
                                ) {
                                    warn!(
                                        "[{}]: Failed to send ICE candidate via channel.",
                                        pid_ice
                                    );
                                }
                            }
                            Err(e) => {
                                error!("[{}]: Error serializing ICE candidate: {}", pid_ice, e)
                            }
                        }
                    }
                    Err(e) => error!(
                        "[{}]: Error converting ICE candidate to JSON: {}",
                        pid_ice, e
                    ),
                }
            }
        })
    }));

    let pc_for_state_change = Arc::clone(&peer_connection);
    let peer_id_for_state_change = peer_id_str.clone();
    let sp_clone_sc = signaling_peers.clone();
    let pm_clone_sc = player_manager.clone();
    let dc_map_clone_sc = data_channels_map.clone();
    let cs_map_clone_sc = client_states_map.clone();
    let pa_map_clone_sc = player_aois.clone();
    let auth_service_clone_sc = auth_service.clone();

    pc_for_state_change.on_peer_connection_state_change(Box::new(
        move |s: RTCPeerConnectionState| {
            let current_peer_id = peer_id_for_state_change.clone();
            record_webrtc_peer_state(&current_peer_id, s);
            info!(
                "[{}]: Peer Connection State changed: {}",
                current_peer_id, s
            );
            if matches!(
                s,
                RTCPeerConnectionState::Failed
                    | RTCPeerConnectionState::Closed
                    | RTCPeerConnectionState::Disconnected
            ) {
                info!(
                    "[{}]: Peer disconnected/closed. Initiating cleanup.",
                    current_peer_id
                );
                cleanup_connection(
                    &current_peer_id,
                    &sp_clone_sc,
                    &pm_clone_sc,
                    &dc_map_clone_sc,
                    &cs_map_clone_sc,
                    &pa_map_clone_sc,
                    &auth_service_clone_sc,
                );
            }
            Box::pin(async {})
        },
    ));

    let pc_for_datachannel_event = Arc::clone(&peer_connection);
    let peer_id_for_dc_event = peer_id_str.clone();
    let player_manager_for_dc_event = player_manager.clone();
    let data_channels_map_for_dc_event = data_channels_map.clone();
    let client_states_map_for_dc_event = client_states_map.clone();
    let chat_messages_queue_for_dc_event = chat_messages_queue.clone();
    let config_for_dc_event = config.clone();
    let server_instance_for_dc_event = server_instance.clone(); // Clone server instance for DC event
    let auth_service_for_dc_event = auth_service.clone();
    let auth_user_id_for_dc_event = auth_user_id.clone();

    pc_for_datachannel_event.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
        let dc_label_owned = dc.label().to_owned();
        let current_peer_id_on_dc = peer_id_for_dc_event.clone();
        info!(
            "[{}]: DataChannel '{}' received from client.",
            current_peer_id_on_dc, dc_label_owned
        );

        let dc_on_open_arc = Arc::clone(&dc);
        let dc_for_closure = Arc::clone(&dc);
        let peer_id_on_open = current_peer_id_on_dc.clone();
        let player_manager_on_open = player_manager_for_dc_event.clone();
        let data_channels_map_on_open = data_channels_map_for_dc_event.clone();
        let client_states_map_on_open = client_states_map_for_dc_event.clone();
        let config_on_open = config_for_dc_event.clone();
        let dc_label_for_on_open = dc_label_owned.clone();
        let server_instance_on_open = server_instance_for_dc_event.clone(); // Clone server instance for on_open
        let auth_service_on_open = auth_service_for_dc_event.clone();
        let auth_user_id_on_open = auth_user_id_for_dc_event.clone();
        let requested_team_on_open = requested_team_id;

        dc_on_open_arc.on_open(Box::new(move || {
            let current_peer_id_on_open_cb = peer_id_on_open.clone();
            let current_dc_label_on_open_cb = dc_label_for_on_open.clone();
            info!(
                "[{}]: DataChannel '{}' OPENED (on_open callback).",
                current_peer_id_on_open_cb, current_dc_label_on_open_cb
            );
            let dc_for_async_block = Arc::clone(&dc_for_closure);
            server_instance_on_open.note_join_channel_open(&current_peer_id_on_open_cb);

            let core_dc = Arc::new(crate::core::types::RTCDataChannel::new(Arc::clone(
                &dc_for_async_block,
            )));
            data_channels_map_on_open.insert(current_peer_id_on_open_cb.clone(), core_dc.clone());
            info!(
                "[{}]: Added data channel to map. Map size: {}, Map ptr: {:p}",
                current_peer_id_on_open_cb,
                data_channels_map_on_open.len(),
                Arc::as_ptr(&data_channels_map_on_open)
            );

            let initial_client_state = ClientState {
                known_walls_sent: false,
                last_update_sent_time: Instant::now(),
                is_mobile,
                mobile_delta_skip_modulus: if is_mobile { crate::core::constants::MOBILE_DELTA_SKIP_MODULUS } else { 1 },
                ..Default::default()
            };
            client_states_map_on_open
                .write()
                .insert(current_peer_id_on_open_cb.clone(), initial_client_state);
            info!(
                "[{}]: Added client state. Client states map size: {}",
                current_peer_id_on_open_cb,
                client_states_map_on_open.read().len()
            );

            let mut username = format!(
                "Player_{}",
                &current_peer_id_on_open_cb[..4.min(current_peer_id_on_open_cb.len())]
            );

            if let Some(bound_user_id) = auth_user_id_on_open.as_deref() {
                if let Some(profile) = auth_service_on_open.profile_by_user_id(bound_user_id) {
                    username = sanitize_username_field(&profile.display_name, MAX_CHAT_USERNAME_CHARS)
                        .unwrap_or_else(|| username.clone());
                    auth_service_on_open.bind_peer_to_user(&current_peer_id_on_open_cb, bound_user_id);
                    info!(
                        "[{}]: Bound authenticated user '{}' to peer.",
                        current_peer_id_on_open_cb, bound_user_id
                    );
                }
            }

            let requested_spectator = requested_team_on_open == Some(0);
            if requested_spectator && !server_instance_on_open.can_accept_spectator_join() {
                warn!(
                    "[{}]: spectator join rejected due to spectator cap. Closing data channel.",
                    current_peer_id_on_open_cb
                );
                let dc_reject = Arc::clone(&dc_for_async_block);
                return Box::pin(async move {
                    let _ = dc_reject.close().await;
                });
            }
            let team_to_assign = if requested_spectator {
                0
            } else {
                requested_team_on_open
                    .filter(|team| *team == 1 || *team == 2)
                    .unwrap_or_else(|| player_manager_on_open.assign_team_to_new_player())
            };
            if !requested_spectator
                && !server_instance_on_open.ensure_human_join_capacity_for_team(
                    &current_peer_id_on_open_cb,
                    Some(team_to_assign),
                )
            {
                warn!(
                    "[{}]: server is full and no bot slot could be reclaimed for human priority join.",
                    current_peer_id_on_open_cb
                );
            }

            // Fix 2.2: Use RespawnManager for initial spawn
            let player_id_arc_for_spawn = player_manager_on_open
                .id_pool
                .get_or_create(&current_peer_id_on_open_cb);
            let initial_spawn_pos = if requested_spectator {
                crate::core::types::Vec2::new(0.0, 0.0)
            } else {
                server_instance_on_open
                    .respawn_manager
                    .get_respawn_position(
                        &server_instance_on_open, // Pass the server instance
                        &player_id_arc_for_spawn,
                        Some(team_to_assign),
                        &[], // No specific enemy positions for initial spawn balancing here
                    )
            };

            info!(
                "[{}] Player spawned at ({}, {})",
                current_peer_id_on_open_cb, initial_spawn_pos.x, initial_spawn_pos.y
            );

            let new_player_id_arc_for_team = player_manager_on_open
                .add_player(
                    current_peer_id_on_open_cb.clone(),
                    username.clone(),
                    initial_spawn_pos.x, // Use determined spawn position
                    initial_spawn_pos.y, // Use determined spawn position
                )
                .unwrap_or_else(|| {
                    warn!(
                        "[{}]: add_player returned None, attempting to get existing PlayerID Arc.",
                        current_peer_id_on_open_cb
                    );
                    player_manager_on_open
                        .id_pool
                        .get_or_create(&current_peer_id_on_open_cb)
                });

            if let Some(mut p_state_entry) =
                player_manager_on_open.get_player_state_mut(&new_player_id_arc_for_team)
            {
                let p_state: &mut PlayerState = &mut p_state_entry;
                p_state.team_id = team_to_assign;
                p_state.is_spectator = requested_spectator;
                if requested_spectator {
                    p_state.health = p_state.max_health;
                    p_state.respawn_timer = None;
                    p_state.reload_progress = None;
                }
                p_state.mark_field_changed(FIELD_SCORE_STATS | FIELD_FLAG);
                info!(
                    "[{}] assigned to team {}. Player state marked as changed.",
                    current_peer_id_on_open_cb, team_to_assign
                );
            }

            if let Some(player_state) =
                player_manager_on_open.get_player_state(&new_player_id_arc_for_team)
            {
                // add_player already updated spatial index; only AoI needs explicit initialization.
                server_instance_on_open.update_player_aoi(
                    &new_player_id_arc_for_team,
                    player_state.x,
                    player_state.y,
                );

                info!(
                    "[{}] Player AoI initialized at position ({}, {})",
                    current_peer_id_on_open_cb, player_state.x, player_state.y
                );
            }

            // Send welcome immediately; initial world snapshot is sent by broadcast pipeline.
            let config_for_welcome = config_on_open.clone();
            let server_for_join_packets = server_instance_on_open.clone();
            let core_dc_for_join_packets = core_dc.clone();

            Box::pin(async move {
                let welcome_bytes = build_welcome_message_bytes(
                    &current_peer_id_on_open_cb,
                    config_for_welcome.tick_rate as u16,
                );
                let match_info_bytes = server_for_join_packets.build_match_info_only_bytes();
                let outbound_packets = [welcome_bytes, match_info_bytes];
                let sent_packets = server_for_join_packets
                    .send_packet_batch_optimized(&core_dc_for_join_packets, &outbound_packets, 100)
                    .await;

                if sent_packets < outbound_packets.len() {
                    warn!(
                        "[{}]: Initial join packet batch partial send ({}/{} packets).",
                        current_peer_id_on_open_cb,
                        sent_packets,
                        outbound_packets.len()
                    );
                    // Fallback: re-attempt welcome using the same batched transport path.
                    let fallback_sent = server_for_join_packets
                        .send_packet_batch_optimized(
                            &core_dc_for_join_packets,
                            &outbound_packets[..1],
                            100,
                        )
                        .await;
                    if fallback_sent == 0 {
                        handle_dc_send_error(
                            "send timeout/failure",
                            &current_peer_id_on_open_cb,
                            "welcome message fallback",
                        );
                    }
                    server_for_join_packets
                        .send_match_info_only(
                            &current_peer_id_on_open_cb,
                            &core_dc_for_join_packets,
                        )
                        .await;
                } else {
                    info!(
                        "[{}]: Sent welcome + match-info batch. Initial state will be sent by broadcast pipeline.",
                        current_peer_id_on_open_cb
                    );
                }
            })
        }));

        let dc_on_message_arc = Arc::clone(&dc);
        let peer_id_on_message = current_peer_id_on_dc.clone();
        let player_manager_on_message = player_manager_for_dc_event.clone();
        let chat_q_on_message = chat_messages_queue_for_dc_event.clone();
        let input_rate_limiter = input_rate_limit_config()
            .map(|cfg| Arc::new(AsyncMutex::new(InputRateLimiter::new(cfg.per_sec, cfg.burst))));

        dc_on_message_arc.on_message(Box::new(move |msg: DataChannelMessage| {
            let pid_msg_inner_str = peer_id_on_message.clone();
            let players_map_on_msg = player_manager_on_message.clone();
            let chat_q_on_msg = chat_q_on_message.clone();
            let input_rate_limiter_on_msg = input_rate_limiter.clone();

            Box::pin(async move {
                metrics::record_network_bytes("ingress_data_channel", msg.data.len());
                if let Ok(game_msg_root) = fb::root_as_game_message(&msg.data) {
                    let protocol_version = game_msg_root.protocol_version();
                    if protocol_version != GAME_PROTOCOL_VERSION {
                        warn!(
                            "[{}]: Dropping message with protocol_version={} (server expects {}).",
                            pid_msg_inner_str, protocol_version, GAME_PROTOCOL_VERSION
                        );
                        return;
                    }

                    match game_msg_root.msg_type() {
                        fb::MessageType::Input => {
                            if let Some(rate_limiter) = input_rate_limiter_on_msg.as_ref() {
                                let mut limiter_guard = rate_limiter.lock().await;
                                if !limiter_guard.try_acquire() {
                                    if limiter_guard.should_log_throttle() {
                                        warn!(
                                            "[{}]: Dropping input due to per-connection input rate limit.",
                                            pid_msg_inner_str
                                        );
                                    }
                                    return;
                                }
                            }

                            if game_msg_root.actual_message_type()
                                == fb::MessagePayload::PlayerInput
                            {
                                if let Some(input_fb) =
                                    game_msg_root.actual_message_as_player_input()
                                {
                                    let p_input_data = PlayerInputData {
                                        timestamp: input_fb.timestamp(),
                                        sequence: input_fb.sequence(),
                                        move_forward: input_fb.move_forward(),
                                        move_backward: input_fb.move_backward(),
                                        move_left: input_fb.move_left(),
                                        move_right: input_fb.move_right(),
                                        shooting: input_fb.shooting(),
                                        reload: input_fb.reload(),
                                        rotation: input_fb.rotation(),
                                        melee_attack: input_fb.melee_attack(),
                                        change_weapon_slot: input_fb.change_weapon_slot() as u8,
                                        use_ability_slot: input_fb.use_ability_slot() as u8,
                                        ping_x: input_fb.ping_x(),
                                        ping_y: input_fb.ping_y(),
                                    };

                                    let player_id_arc: PlayerID = players_map_on_msg
                                        .id_pool
                                        .get_or_create(&pid_msg_inner_str);
                                    if let Some(mut player_entry) =
                                        players_map_on_msg.get_player_state_mut(&player_id_arc)
                                    {
                                        let seq = p_input_data.sequence;
                                        if player_entry.queue_input(p_input_data) {
                                            debug!(
                                                "[{}]: Accepted player input (seq: {})",
                                                pid_msg_inner_str, seq
                                            );
                                        } else {
                                            debug!(
                                                "[{}]: Rejected player input (seq: {}, last_accepted: {}) – replay or sequence gap",
                                                pid_msg_inner_str, seq, player_entry.last_queued_input_sequence
                                            );
                                        }
                                    } else {
                                        warn!(
                                            "[{}]: Player state not found for input processing.",
                                            pid_msg_inner_str
                                        );
                                    }
                                }
                            }
                        }
                        fb::MessageType::Chat => {
                            if game_msg_root.actual_message_type()
                                == fb::MessagePayload::ChatMessage
                            {
                                if let Some(chat_fb) =
                                    game_msg_root.actual_message_as_chat_message()
                                {
                                    if let Some(message_text_fb) = chat_fb.message() {
                                        let player_id_from_connection = pid_msg_inner_str.clone();
                                        let player_id_arc_for_chat = players_map_on_msg
                                            .id_pool
                                            .get_or_create(&player_id_from_connection);
                                        let Some(sanitized_message) = sanitize_chat_field(
                                            message_text_fb,
                                            MAX_CHAT_MESSAGE_CHARS,
                                        ) else {
                                            warn!(
                                                "[{}]: Dropping empty/invalid chat payload.",
                                                pid_msg_inner_str
                                            );
                                            return;
                                        };
                                        let authoritative_username = players_map_on_msg
                                            .get_player_state(&player_id_arc_for_chat)
                                            .map(|state| state.username.clone());
                                        let sanitized_username = sanitize_username_field(
                                            authoritative_username
                                                .as_deref()
                                                .unwrap_or("Player"),
                                            MAX_CHAT_USERNAME_CHARS,
                                        )
                                        .unwrap_or_else(|| "Player".to_owned());
                                        let current_seq = next_chat_message_seq();
                                        let chat_entry = ChatMessage {
                                            seq: current_seq,
                                            player_id: player_id_arc_for_chat,
                                            username: sanitized_username,
                                            message: sanitized_message,
                                            timestamp: chat_fb.timestamp().max(now_millis()),
                                        };
                                        info!(
                                            "[CHAT] {} ({}): {}",
                                            chat_entry.username,
                                            *chat_entry.player_id,
                                            chat_entry.message
                                        );
                                        let mut chat_q_guard = chat_q_on_msg.write().await;
                                        chat_q_guard.push_back(chat_entry);
                                        if chat_q_guard.len() > CHAT_HISTORY_LIMIT {
                                            chat_q_guard.pop_front();
                                        }
                                    }
                                }
                            }
                        }
                        _ => warn!(
                            "[{}]: Received unhandled FB message type: {:?}",
                            pid_msg_inner_str,
                            game_msg_root.msg_type()
                        ),
                    }
                } else {
                    error!(
                        "[{}]: Failed to parse FlatBuffer message from client.",
                        pid_msg_inner_str
                    );
                }
            })
        }));

        let dc_on_close_arc = Arc::clone(&dc);
        let peer_id_on_close = current_peer_id_on_dc.clone();
        let dc_label_for_on_close = dc_label_owned.clone();

        dc_on_close_arc.on_close(Box::new(move || {
            info!(
                "[{}]: DataChannel '{}' CLOSED.",
                peer_id_on_close, dc_label_for_on_close
            );
            Box::pin(async {})
        }));

        let dc_on_error_arc = Arc::clone(&dc);
        let peer_id_on_error = current_peer_id_on_dc.clone();
        let dc_label_for_on_error = dc_label_owned.clone();

        dc_on_error_arc.on_error(Box::new(move |err| {
            error!(
                "[{}]: DataChannel '{}' ERROR: {}",
                peer_id_on_error, dc_label_for_on_error, err
            );
            Box::pin(async {})
        }));

        Box::pin(async move {})
    }));

    let pc_signal_receiver = Arc::clone(&peer_connection);
    let ws_signal_sender_clone = client_signaling_tx.clone();
    let current_peer_id_ws = peer_id_str.clone();

    while let Some(result) = ws_rx.next().await {
        shared_connection_manager().touch(&current_peer_id_ws);
        match result {
            Ok(msg) => {
                if msg.is_text() {
                    metrics::record_network_bytes("ingress_ws", msg.as_bytes().len());
                    if msg.as_bytes().len() > MAX_SIGNALING_TEXT_BYTES {
                        warn!(
                            "[{}]: Signaling message exceeds {} bytes; closing connection.",
                            current_peer_id_ws, MAX_SIGNALING_TEXT_BYTES
                        );
                        break;
                    }
                    if let Ok(text_content) = msg.to_str() {
                        match serde_json::from_str::<SignalingMessageJson>(text_content) {
                            Ok(sig_data) => {
                                if let Err(reason) = validate_signaling_payload(&sig_data) {
                                    warn!(
                                        "[{}]: Invalid signaling payload: {}. Closing connection.",
                                        current_peer_id_ws, reason
                                    );
                                    break;
                                }
                                if let Some(sdp) = sig_data.sdp {
                                    let _sdp_permit = acquire_sdp_admission_permit(
                                        &current_peer_id_ws,
                                        &ws_signal_sender_clone,
                                    )
                                    .await;
                                    if let Err(e) =
                                        pc_signal_receiver.set_remote_description(sdp.clone()).await
                                    {
                                        error!(
                                            "[{}]: Error setting remote description: {}",
                                            current_peer_id_ws, e
                                        );
                                        continue;
                                    }
                                    if pc_signal_receiver.remote_description().await.is_some_and(|rd| rd.sdp_type == webrtc::peer_connection::sdp::sdp_type::RTCSdpType::Offer) {
                                        match pc_signal_receiver.create_answer(None).await {
                                            Ok(answer) => {
                                                if pc_signal_receiver.set_local_description(answer.clone()).await.is_ok() {
                                                    let resp_msg = SignalingMessageJson { sdp: Some(answer), ice: None };
                                                    if let Ok(json_resp) = serde_json::to_string(&resp_msg) {
                                                        if !try_queue_signaling_message(
                                                            &ws_signal_sender_clone,
                                                            Ok(Message::text(json_resp)),
                                                            &current_peer_id_ws,
                                                            "sdp_answer",
                                                        ) {
                                                            warn!("[{}]: Failed to send SDP answer via channel.", current_peer_id_ws);
                                                        }
                                                    } else {
                                                        error!("[{}]: Error serializing SDP answer.", current_peer_id_ws);
                                                    }
                                                } else {
                                                    error!("[{}]: Error setting local description for answer.", current_peer_id_ws);
                                                }
                                            }
                                            Err(e) => error!("[{}]: Error creating SDP answer: {}", current_peer_id_ws, e),
                                        }
                                    }
                                } else if let Some(ice) = sig_data.ice {
                                    let ice_init = RTCIceCandidateInit {
                                        candidate: ice.candidate,
                                        sdp_mid: ice.sdp_mid,
                                        sdp_mline_index: ice.sdp_m_line_index,
                                        username_fragment: ice.username_fragment,
                                    };
                                    if let Err(e) =
                                        pc_signal_receiver.add_ice_candidate(ice_init).await
                                    {
                                        warn!(
                                            "[{}]: Error adding ICE candidate: {}",
                                            current_peer_id_ws, e
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                error!(
                                    "[{}]: Failed to parse signaling message: {} (len={}).",
                                    current_peer_id_ws,
                                    e,
                                    text_content.len()
                                );
                            }
                        }
                    }
                } else if msg.is_close() {
                    info!("[{}]: WebSocket closed by client.", current_peer_id_ws);
                    break;
                }
            }
            Err(e) => {
                warn!("[{}]: WebSocket error: {}", current_peer_id_ws, e);
                break;
            }
        }
    }

    info!(
        "[{}]: WebSocket connection handler for signaling ending.",
        peer_id_str
    );
    cleanup_connection(
        &peer_id_str,
        &signaling_peers,
        &player_manager,
        &data_channels_map,
        &client_states_map,
        &player_aois,
        &auth_service,
    );
    if let Err(e) = peer_connection.close().await {
        error!("[{}]: Error closing PeerConnection: {}", peer_id_str, e);
    }
}

pub fn cleanup_connection(
    peer_id_str: &str,
    signaling_peers: &SignalingPeers,
    player_manager: &PlayerManagerRef, // This is Arc<ImprovedPlayerManager>
    data_channels_map: &DataChannelsMap,
    client_states_map: &ClientStatesMap,
    player_aois: &PlayerAoIs,
    auth_service: &AuthService,
) {
    info!("[{}]: Cleaning up resources.", peer_id_str);
    remove_webrtc_peer_state(peer_id_str);
    let _ = shared_connection_manager().remove(peer_id_str);
    // Remove signaling sender first; duplicate cleanups are expected under concurrent callbacks.
    let removed_signaling_entry = signaling_peers.remove(peer_id_str).is_some();
    metrics::set_ws_connections_active(signaling_peers.len());
    // Maintain a single lock acquisition order for lifecycle paths:
    // client state first, then player manager operations.
    client_states_map.write().remove(peer_id_str);
    data_channels_map.remove(peer_id_str);
    player_aois.remove(peer_id_str);

    let player_state_snapshot = {
        let player_id_lookup: PlayerID = Arc::new(peer_id_str.to_owned());
        player_manager
            .get_player_state(&player_id_lookup)
            .map(|entry| entry.clone())
    };

    if removed_signaling_entry {
        if let Some(player_state) = player_state_snapshot.as_ref() {
            auth_service.record_disconnect_score_for_peer(peer_id_str, player_state);
        } else {
            auth_service.clear_peer_binding(peer_id_str);
        }
    } else {
        debug!(
            "[{}]: Signaling sender already removed; continuing idempotent cleanup.",
            peer_id_str
        );
        if player_state_snapshot.is_none() {
            auth_service.clear_peer_binding(peer_id_str);
        }
    }

    if player_state_snapshot.is_some() {
        player_manager.remove_player(peer_id_str);
    }
    info!("[{}]: Player AoI data removed.", peer_id_str);
}

pub fn handle_dc_send_error(error_string: &str, peer_id_str: &str, message_type: &str) {
    let is_stream_closed_error = error_string.contains("stream closed")
        || error_string.contains("Stream closed")
        || error_string.contains("connection reset")
        || error_string.contains("Channel closed");

    if !is_stream_closed_error {
        error!(
            "[{}]: Error sending {} on data channel: {}",
            peer_id_str, message_type, error_string
        );
    }
}
