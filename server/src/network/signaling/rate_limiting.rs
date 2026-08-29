use crate::core::constants::*;
use crate::network::rate_limiter::TokenBucket;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::{
    net::IpAddr,
    sync::{Arc, Mutex as StdMutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{mpsc, OwnedSemaphorePermit, Semaphore};
use tracing::{info, warn};
use warp::ws::Message;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

use super::sanitization::signaling_protocol_version;

#[derive(Serialize, Deserialize, Debug)]
pub(super) struct SignalingMessageJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) protocol_version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) sdp: Option<RTCSessionDescription>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) ice: Option<RTCIceCandidateInitSerde>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub(super) struct RTCIceCandidateInitSerde {
    pub(super) candidate: String,
    #[serde(rename = "sdpMid")]
    pub(super) sdp_mid: Option<String>,
    #[serde(rename = "sdpMLineIndex")]
    pub(super) sdp_m_line_index: Option<u16>,
    #[serde(rename = "usernameFragment")]
    pub(super) username_fragment: Option<String>,
}

pub(super) const DEFAULT_JOIN_RATE_LIMIT_PER_SEC: u32 = 30;
pub(super) const DEFAULT_JOIN_RATE_LIMIT_BURST: u32 = 50;
pub(super) const DEFAULT_IP_RATE_LIMIT_PER_SEC: u32 = 20;
pub(super) const DEFAULT_IP_RATE_LIMIT_BURST: u32 = 40;
pub(super) const DEFAULT_ICE_CANDIDATE_RATE_LIMIT_PER_SEC: u32 = 80;
pub(super) const DEFAULT_ICE_CANDIDATE_RATE_LIMIT_BURST: u32 = 160;
pub(super) const DEFAULT_SDP_ADMISSION_CONCURRENCY: usize = 64;
const MAX_SDP_ADMISSION_CONCURRENCY: usize = 512;
pub(super) const JOIN_RATE_LIMIT_THROTTLED_MESSAGE: &str =
    "Server busy handling joins, retry shortly.";
pub(super) const MAX_SIGNALING_TEXT_BYTES: usize = 128 * 1024;
pub(super) const MAX_SIGNALING_SDP_BYTES: usize = 120 * 1024;
pub(super) const MAX_SIGNALING_ICE_CANDIDATE_BYTES: usize = 4 * 1024;
pub(super) const MAX_SIGNALING_ICE_SDP_MID_BYTES: usize = 256;
pub(super) const MAX_SIGNALING_ICE_USERNAME_FRAGMENT_BYTES: usize = 256;
pub(super) const SIGNALING_OUTBOX_CAPACITY: usize = 1000;
pub(super) const DEFAULT_WS_KEEPALIVE_INTERVAL_SECS: u32 = 30;
pub(super) const MIN_WS_KEEPALIVE_INTERVAL_SECS: u32 = 5;
pub(super) const MAX_WS_KEEPALIVE_INTERVAL_SECS: u32 = 300;
pub(super) const DISCONNECTED_CLEANUP_GRACE_SECS: u64 = 10;
/// Maximum allowed size for incoming FlatBuffer messages on data channels.
/// Messages exceeding this are dropped to prevent OOM from oversized payloads.
pub(super) const MAX_DATACHANNEL_MESSAGE_BYTES: usize = 1024 * 1024; // 1 MB

pub(super) fn try_queue_signaling_message(
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
pub(super) struct InputRateLimitConfig {
    pub(super) per_sec: u32,
    pub(super) burst: u32,
}

#[derive(Clone, Copy, Debug)]
struct IpRateLimitConfig {
    per_sec: u32,
    burst: u32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct IceCandidateRateLimitConfig {
    pub(super) per_sec: u32,
    pub(super) burst: u32,
}

#[derive(Debug)]
pub(super) struct JoinRateLimiter {
    bucket: TokenBucket,
    last_seen_at: Instant,
}

impl JoinRateLimiter {
    pub(super) fn new(refill_per_sec: u32, capacity: u32) -> Self {
        Self {
            bucket: TokenBucket::new(refill_per_sec, capacity),
            last_seen_at: Instant::now(),
        }
    }

    pub(super) fn try_acquire(&mut self) -> bool {
        self.last_seen_at = Instant::now();
        self.bucket.try_acquire()
    }
}

#[derive(Debug)]
pub(super) struct InputRateLimiter {
    bucket: TokenBucket,
    last_drop_log_at: Instant,
}

impl InputRateLimiter {
    pub(super) fn new(refill_per_sec: u32, capacity: u32) -> Self {
        Self {
            bucket: TokenBucket::new(refill_per_sec, capacity),
            last_drop_log_at: Instant::now()
                .checked_sub(Duration::from_secs(
                    INPUT_RATE_LIMIT_THROTTLE_LOG_INTERVAL_SECS,
                ))
                .unwrap_or_else(Instant::now),
        }
    }

    pub(super) fn try_acquire(&mut self) -> bool {
        self.bucket.try_acquire()
    }

    pub(super) fn should_log_throttle(&mut self) -> bool {
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

pub(super) fn env_u32(name: &str, default_value: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or(default_value)
}

pub(super) fn normalize_ws_keepalive_interval_secs(interval_secs: u32) -> Option<u32> {
    if interval_secs == 0 {
        None
    } else {
        Some(interval_secs.clamp(
            MIN_WS_KEEPALIVE_INTERVAL_SECS,
            MAX_WS_KEEPALIVE_INTERVAL_SECS,
        ))
    }
}

pub(super) fn ws_keepalive_interval() -> Option<Duration> {
    static WS_KEEPALIVE_INTERVAL: OnceLock<Option<Duration>> = OnceLock::new();
    *WS_KEEPALIVE_INTERVAL.get_or_init(|| {
        let raw = env_u32(
            "MGS_WS_KEEPALIVE_INTERVAL_SECS",
            DEFAULT_WS_KEEPALIVE_INTERVAL_SECS,
        );
        let normalized = normalize_ws_keepalive_interval_secs(raw);
        match normalized {
            Some(secs) if secs != raw => {
                warn!(
                    "WebSocket keepalive interval clamped from {}s to {}s.",
                    raw, secs
                );
                Some(Duration::from_secs(secs as u64))
            }
            Some(secs) => {
                info!("WebSocket keepalive pings enabled every {}s.", secs);
                Some(Duration::from_secs(secs as u64))
            }
            None => {
                info!("WebSocket keepalive pings disabled (MGS_WS_KEEPALIVE_INTERVAL_SECS=0).");
                None
            }
        }
    })
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

pub(crate) const DEFAULT_MAX_WS_CONNECTIONS_PER_IP: u32 = 12;

/// Holds a source IP's slot in the per-IP concurrent-connection cap for as
/// long as the connection is alive; releases it on drop so every exit path
/// out of `handle_signaling_connection` (including early returns) frees the
/// slot without needing a matching manual release call. Acquired pre-upgrade
/// in the `/ws` route (`check_ws_ip_connection_cap`) so over-cap clients are
/// rejected at the HTTP handshake instead of after a successful 101 upgrade.
#[derive(Debug)]
pub struct IpConnectionGuard(Option<IpAddr>);

impl Drop for IpConnectionGuard {
    fn drop(&mut self) {
        if let Some(ip) = self.0 {
            release_ip_connection_slot(&ip);
        }
    }
}

fn max_ws_connections_per_ip() -> u32 {
    static MAX_PER_IP: OnceLock<u32> = OnceLock::new();
    *MAX_PER_IP.get_or_init(|| {
        let value = env_u32(
            "MGS_MAX_WS_CONNECTIONS_PER_IP",
            DEFAULT_MAX_WS_CONNECTIONS_PER_IP,
        );
        if value == 0 {
            info!("Per-IP concurrent WebSocket connection cap disabled (MGS_MAX_WS_CONNECTIONS_PER_IP=0).");
        } else {
            info!(
                "Per-IP concurrent WebSocket connection cap: {} connections.",
                value
            );
        }
        value
    })
}

fn ip_connection_counts() -> &'static DashMap<IpAddr, u32> {
    static IP_CONNECTION_COUNTS: OnceLock<DashMap<IpAddr, u32>> = OnceLock::new();
    IP_CONNECTION_COUNTS.get_or_init(DashMap::new)
}

/// Reserves one of this IP's connection slots. Returns `None` if the cap for
/// that IP is already reached — the caller should reject the connection.
/// The global connection semaphore (`build_connection_cap_filter`) already
/// bounds total connections; this bounds how much of that pool one IP can
/// claim, so one source can't exhaust the whole server before the join-rate
/// limiter would otherwise catch it.
pub(crate) fn try_acquire_ip_connection_slot(client_ip: &IpAddr) -> Option<IpConnectionGuard> {
    let max = max_ws_connections_per_ip();
    if max == 0 {
        return Some(IpConnectionGuard(None));
    }
    let counts = ip_connection_counts();
    let mut entry = counts.entry(*client_ip).or_insert(0);
    if *entry >= max {
        return None;
    }
    *entry += 1;
    Some(IpConnectionGuard(Some(*client_ip)))
}

fn release_ip_connection_slot(client_ip: &IpAddr) {
    let counts = ip_connection_counts();
    if let Some(mut entry) = counts.get_mut(client_ip) {
        *entry = entry.saturating_sub(1);
        let now_empty = *entry == 0;
        drop(entry);
        if now_empty {
            // Avoid unbounded growth from IPs that connect once and never return.
            counts.remove_if(client_ip, |_, count| *count == 0);
        }
    }
}

pub(super) fn input_rate_limit_config() -> Option<InputRateLimitConfig> {
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

pub(super) fn ice_candidate_rate_limit_config() -> Option<IceCandidateRateLimitConfig> {
    static ICE_CANDIDATE_RATE_LIMIT_CONFIG: OnceLock<Option<IceCandidateRateLimitConfig>> =
        OnceLock::new();
    ICE_CANDIDATE_RATE_LIMIT_CONFIG
        .get_or_init(|| {
            let per_sec = env_u32(
                "MGS_SIGNALING_ICE_RATE_LIMIT_PER_SEC",
                DEFAULT_ICE_CANDIDATE_RATE_LIMIT_PER_SEC,
            );
            if per_sec == 0 {
                info!(
                    "ICE candidate rate limiter disabled (MGS_SIGNALING_ICE_RATE_LIMIT_PER_SEC=0)."
                );
                return None;
            }
            let burst = env_u32(
                "MGS_SIGNALING_ICE_RATE_LIMIT_BURST",
                DEFAULT_ICE_CANDIDATE_RATE_LIMIT_BURST,
            )
            .max(per_sec);
            info!(
                "ICE candidate rate limiter enabled: {} candidates/sec with burst {}.",
                per_sec, burst
            );
            Some(IceCandidateRateLimitConfig { per_sec, burst })
        })
        .as_ref()
        .copied()
}

fn sdp_admission_semaphore() -> Option<&'static Arc<Semaphore>> {
    static SDP_ADMISSION_SEMAPHORE: OnceLock<Option<Arc<Semaphore>>> = OnceLock::new();
    SDP_ADMISSION_SEMAPHORE
        .get_or_init(|| {
            let limit = super::signaling_env_config().sdp_concurrency;
            if limit == 0 {
                info!("SDP admission gate disabled (MGS_SIGNALING_SDP_CONCURRENCY=0).");
                None
            } else {
                let limit = limit.clamp(1, MAX_SDP_ADMISSION_CONCURRENCY);
                info!(
                    "SDP admission gate enabled with max {} concurrent offers.",
                    limit
                );
                Some(Arc::new(Semaphore::new(limit)))
            }
        })
        .as_ref()
}

pub(super) async fn acquire_sdp_admission_permit(
    peer_id: &str,
    sender: &mpsc::Sender<Result<Message, warp::Error>>,
) -> Result<Option<OwnedSemaphorePermit>, ()> {
    let Some(semaphore) = sdp_admission_semaphore() else {
        return Ok(None);
    };
    match semaphore.clone().try_acquire_owned() {
        Ok(permit) => Ok(Some(permit)),
        Err(_) => {
            let queue_hint = semaphore.available_permits().saturating_add(1).max(1);
            let queue_notice = serde_json::json!({
                "event": "sdp_offer_queue",
                "queue_position_hint": queue_hint,
                "server_protocol_version": signaling_protocol_version(),
            })
            .to_string();
            let _ = try_queue_signaling_message(
                sender,
                Ok(Message::text(queue_notice)),
                peer_id,
                "sdp_offer_queue",
            );
            match semaphore.clone().acquire_owned().await {
                Ok(permit) => Ok(Some(permit)),
                Err(err) => {
                    warn!(
                        "[{}]: Failed to acquire SDP admission permit because semaphore is closed: {}",
                        peer_id, err
                    );
                    let rejection_notice = serde_json::json!({
                        "event": "sdp_offer_rejected",
                        "reason": "server_busy",
                        "server_protocol_version": signaling_protocol_version(),
                    })
                    .to_string();
                    let _ = try_queue_signaling_message(
                        sender,
                        Ok(Message::text(rejection_notice)),
                        peer_id,
                        "sdp_offer_rejected",
                    );
                    Err(())
                }
            }
        }
    }
}

pub(super) fn try_acquire_join_rate_limit_token() -> bool {
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
/// Minimum map size before periodic cleanup sweep is considered.
const IP_RATE_LIMITER_CLEANUP_MIN_ENTRIES: usize = 512;
/// Minimum interval between periodic cleanup sweeps.
const IP_RATE_LIMITER_CLEANUP_INTERVAL_SECS: u64 = 30;

fn cleanup_idle_ip_rate_limiters(limiters: &DashMap<IpAddr, JoinRateLimiter>) {
    let now = Instant::now();
    let idle_threshold = Duration::from_secs(IP_RATE_LIMITER_IDLE_SECS);
    limiters.retain(|_ip, limiter| {
        now.saturating_duration_since(limiter.last_seen_at) < idle_threshold
    });
}

fn maybe_cleanup_ip_rate_limiters(limiters: &DashMap<IpAddr, JoinRateLimiter>) {
    if limiters.len() < IP_RATE_LIMITER_CLEANUP_MIN_ENTRIES {
        return;
    }

    static LAST_IP_LIMITER_CLEANUP_UNIX_SECS: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(0);

    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs();
    let previous = LAST_IP_LIMITER_CLEANUP_UNIX_SECS.load(std::sync::atomic::Ordering::Relaxed);
    if now_secs.saturating_sub(previous) < IP_RATE_LIMITER_CLEANUP_INTERVAL_SECS {
        return;
    }

    if LAST_IP_LIMITER_CLEANUP_UNIX_SECS
        .compare_exchange(
            previous,
            now_secs,
            std::sync::atomic::Ordering::Relaxed,
            std::sync::atomic::Ordering::Relaxed,
        )
        .is_ok()
    {
        cleanup_idle_ip_rate_limiters(limiters);
    }
}

pub(super) fn try_acquire_ip_rate_limit_token(client_ip: &IpAddr) -> bool {
    let Some(cfg) = ip_rate_limit_config() else {
        return true;
    };

    static IP_RATE_LIMITERS: OnceLock<DashMap<IpAddr, JoinRateLimiter>> = OnceLock::new();
    let limiters = IP_RATE_LIMITERS.get_or_init(DashMap::new);

    // Enforce hard-cap cleanup and periodic sweeps before the map reaches the cap.
    if limiters.len() > IP_RATE_LIMITER_MAX_ENTRIES {
        cleanup_idle_ip_rate_limiters(limiters);
    } else {
        maybe_cleanup_ip_rate_limiters(limiters);
    }

    let mut limiter = limiters
        .entry(*client_ip)
        .or_insert_with(|| JoinRateLimiter::new(cfg.per_sec, cfg.burst));
    limiter.try_acquire()
}

pub(super) fn validate_signaling_payload(
    payload: &SignalingMessageJson,
) -> Result<(), &'static str> {
    match payload.protocol_version {
        Some(version) if version == signaling_protocol_version() => {}
        Some(_) => return Err("protocol_version mismatch"),
        None => return Err("missing protocol_version"),
    }

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

pub(super) fn signaling_error_json(code: &str, detail: impl Into<String>) -> String {
    serde_json::json!({
        "error": code,
        "detail": detail.into(),
        "server_protocol_version": signaling_protocol_version(),
    })
    .to_string()
}
