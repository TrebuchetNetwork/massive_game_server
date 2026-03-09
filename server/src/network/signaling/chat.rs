use crate::core::types::PlayerID;
use dashmap::DashMap;
use std::sync::Arc;
use std::{
    collections::VecDeque,
    sync::{atomic::AtomicU64, OnceLock},
};
use tokio::sync::RwLock;

#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub seq: u64,
    pub player_id: PlayerID,
    pub username: String,
    pub message: String,
    pub timestamp: u64,
}

/// Maximum number of chat messages retained in the bounded queue.
pub const MAX_CHAT_QUEUE_SIZE: usize = 1000;

/// A chat message queue that enforces a maximum size. When the queue is full,
/// the oldest messages are dropped to make room. This prevents unbounded memory
/// growth regardless of message ingestion rate.
#[derive(Debug, Clone)]
pub struct BoundedChatQueue {
    inner: VecDeque<ChatMessage>,
    max_size: usize,
}

impl BoundedChatQueue {
    pub fn new(max_size: usize) -> Self {
        Self {
            inner: VecDeque::with_capacity(max_size.min(1024)),
            max_size,
        }
    }

    /// Push a message, dropping the oldest if the queue is at capacity.
    pub fn push_back(&mut self, msg: ChatMessage) {
        if self.inner.len() >= self.max_size {
            self.inner.pop_front();
        }
        self.inner.push_back(msg);
    }

    pub fn pop_front(&mut self) -> Option<ChatMessage> {
        self.inner.pop_front()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn iter(&self) -> std::collections::vec_deque::Iter<'_, ChatMessage> {
        self.inner.iter()
    }
}

pub type ChatMessagesQueue = Arc<RwLock<BoundedChatQueue>>;

static NEXT_CHAT_MESSAGE_SEQ: AtomicU64 = AtomicU64::new(1);

pub(super) const MAX_CHAT_MESSAGE_CHARS: usize = 160;
pub(super) const MAX_CHAT_USERNAME_CHARS: usize = 32;
const MIN_CHAT_COOLDOWN_MS: u64 = 0;
const MAX_CHAT_COOLDOWN_MS: u64 = 5_000;
const MAX_CHAT_BURST_CAPACITY: u64 = 100;
const MIN_CHAT_BURST_WINDOW_MS: u64 = 500;
const MAX_CHAT_BURST_WINDOW_MS: u64 = 60_000;
const CHAT_COOLDOWN_CLEANUP_INTERVAL_MS: u64 = 10 * 60 * 1000;
const CHAT_COOLDOWN_ENTRY_TTL_MS: u64 = 20 * 60 * 1000;

static LAST_CHAT_COOLDOWN_CLEANUP_MS: AtomicU64 = AtomicU64::new(0);

pub fn next_chat_message_seq() -> u64 {
    NEXT_CHAT_MESSAGE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ChatRateLimitState {
    last_sent_ms: u64,
    burst_tokens: f64,
    burst_last_refill_ms: u64,
}

fn chat_cooldown_ms() -> u64 {
    static CHAT_COOLDOWN_MS: OnceLock<u64> = OnceLock::new();
    *CHAT_COOLDOWN_MS.get_or_init(|| {
        super::signaling_env_config()
            .chat_cooldown_ms
            .clamp(MIN_CHAT_COOLDOWN_MS, MAX_CHAT_COOLDOWN_MS)
    })
}

fn chat_burst_capacity() -> u64 {
    static CHAT_BURST_CAPACITY: OnceLock<u64> = OnceLock::new();
    *CHAT_BURST_CAPACITY.get_or_init(|| {
        super::signaling_env_config()
            .chat_burst_capacity
            .clamp(0, MAX_CHAT_BURST_CAPACITY)
    })
}

fn chat_burst_window_ms() -> u64 {
    static CHAT_BURST_WINDOW_MS: OnceLock<u64> = OnceLock::new();
    *CHAT_BURST_WINDOW_MS.get_or_init(|| {
        super::signaling_env_config()
            .chat_burst_window_ms
            .clamp(MIN_CHAT_BURST_WINDOW_MS, MAX_CHAT_BURST_WINDOW_MS)
    })
}

fn shared_chat_rate_limits() -> &'static DashMap<String, ChatRateLimitState> {
    static CHAT_RATE_LIMITS_BY_PEER: OnceLock<DashMap<String, ChatRateLimitState>> =
        OnceLock::new();
    CHAT_RATE_LIMITS_BY_PEER.get_or_init(DashMap::new)
}

pub(super) fn try_consume_chat_rate_limit_with_map(
    peer_id: &str,
    now_timestamp_ms: u64,
    cooldown_ms: u64,
    burst_capacity: u64,
    burst_window_ms: u64,
    rate_limits: &DashMap<String, ChatRateLimitState>,
) -> bool {
    if cooldown_ms == 0 && (burst_capacity == 0 || burst_window_ms == 0) {
        return true;
    }
    maybe_cleanup_chat_rate_limits(now_timestamp_ms, rate_limits);
    match rate_limits.entry(peer_id.to_owned()) {
        dashmap::mapref::entry::Entry::Occupied(mut occupied) => {
            let mut state = *occupied.get();
            if cooldown_ms > 0 && now_timestamp_ms.saturating_sub(state.last_sent_ms) < cooldown_ms
            {
                return false;
            }

            if burst_capacity > 0 && burst_window_ms > 0 {
                let refill_rate_per_ms = burst_capacity as f64 / burst_window_ms as f64;
                let elapsed_ms = now_timestamp_ms.saturating_sub(state.burst_last_refill_ms);
                state.burst_tokens = (state.burst_tokens + elapsed_ms as f64 * refill_rate_per_ms)
                    .min(burst_capacity as f64);
                state.burst_last_refill_ms = now_timestamp_ms;
                if state.burst_tokens < 1.0 {
                    return false;
                }
                state.burst_tokens -= 1.0;
            }

            state.last_sent_ms = now_timestamp_ms;
            *occupied.get_mut() = state;
            true
        }
        dashmap::mapref::entry::Entry::Vacant(vacant) => {
            let mut state = ChatRateLimitState {
                last_sent_ms: now_timestamp_ms,
                burst_tokens: burst_capacity as f64,
                burst_last_refill_ms: now_timestamp_ms,
            };
            if burst_capacity > 0 && burst_window_ms > 0 {
                state.burst_tokens = (state.burst_tokens - 1.0).max(0.0);
            }
            vacant.insert(state);
            true
        }
    }
}

fn maybe_cleanup_chat_rate_limits(
    now_timestamp_ms: u64,
    rate_limits: &DashMap<String, ChatRateLimitState>,
) {
    let previous = LAST_CHAT_COOLDOWN_CLEANUP_MS.load(std::sync::atomic::Ordering::Relaxed);
    if now_timestamp_ms.saturating_sub(previous) < CHAT_COOLDOWN_CLEANUP_INTERVAL_MS {
        return;
    }

    if LAST_CHAT_COOLDOWN_CLEANUP_MS
        .compare_exchange(
            previous,
            now_timestamp_ms,
            std::sync::atomic::Ordering::Relaxed,
            std::sync::atomic::Ordering::Relaxed,
        )
        .is_ok()
    {
        rate_limits.retain(|_peer_id, state| {
            now_timestamp_ms.saturating_sub(state.last_sent_ms) <= CHAT_COOLDOWN_ENTRY_TTL_MS
        });
    }
}

pub(super) fn try_consume_chat_rate_limit(peer_id: &str, now_timestamp_ms: u64) -> bool {
    try_consume_chat_rate_limit_with_map(
        peer_id,
        now_timestamp_ms,
        chat_cooldown_ms(),
        chat_burst_capacity(),
        chat_burst_window_ms(),
        shared_chat_rate_limits(),
    )
}

pub(super) fn clear_chat_rate_limit(peer_id: &str) {
    shared_chat_rate_limits().remove(peer_id);
}
