use super::types::{OtpIpRateState, TokenValidationRateLimitConfig, TokenValidationRateLimiter};
use super::{
    DEFAULT_TOKEN_VALIDATION_RATE_LIMIT_BURST, DEFAULT_TOKEN_VALIDATION_RATE_LIMIT_PER_SEC,
};
use super::{
    OTP_IP_CLEANUP_INTERVAL_SECS, OTP_IP_CLEANUP_MIN_ENTRIES, OTP_IP_LONG_WINDOW_SECS,
    OTP_IP_RATE_LIMITER_MAX_ENTRIES, OTP_IP_SHORT_WINDOW_SECS,
    TOKEN_VALIDATION_CLEANUP_INTERVAL_SECS, TOKEN_VALIDATION_RATE_LIMITER_MAX_ENTRIES,
};
use super::{
    OTP_IP_RATE_LIMITERS, TOKEN_VALIDATION_RATE_LIMITERS, TOKEN_VALIDATION_RATE_LIMIT_CONFIG,
};
use dashmap::DashMap;
use parking_lot::Mutex as ParkingLotMutex;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use super::phone_utils::unix_now;

// ── Token validation rate limiting ────────────────────────────────────────────

pub(super) fn shared_token_validation_rate_limiters(
) -> &'static DashMap<String, Arc<ParkingLotMutex<TokenValidationRateLimiter>>> {
    TOKEN_VALIDATION_RATE_LIMITERS.get_or_init(DashMap::new)
}

pub(super) fn token_validation_rate_limit_config() -> TokenValidationRateLimitConfig {
    *TOKEN_VALIDATION_RATE_LIMIT_CONFIG.get_or_init(|| TokenValidationRateLimitConfig {
        per_sec: DEFAULT_TOKEN_VALIDATION_RATE_LIMIT_PER_SEC,
        burst: DEFAULT_TOKEN_VALIDATION_RATE_LIMIT_BURST,
    })
}

pub(super) fn configure_token_validation_rate_limit(per_sec: u32, burst: u32) {
    let _ = TOKEN_VALIDATION_RATE_LIMIT_CONFIG.set(TokenValidationRateLimitConfig {
        per_sec: per_sec.max(1),
        burst: burst.max(1),
    });
}

fn token_validation_rate_limit_key(remote_addr: Option<SocketAddr>) -> String {
    remote_addr
        .map(|addr| addr.ip().to_string())
        .unwrap_or_default()
}

pub(super) fn try_acquire_token_validation_token(remote_addr: Option<SocketAddr>) -> bool {
    if remote_addr.is_none() {
        return true;
    }
    let config = token_validation_rate_limit_config();
    let key = token_validation_rate_limit_key(remote_addr);
    let limiters = shared_token_validation_rate_limiters();

    maybe_cleanup_token_validation_rate_limiters(limiters);

    let limiter_arc = limiters
        .entry(key)
        .or_insert_with(|| {
            Arc::new(ParkingLotMutex::new(TokenValidationRateLimiter::new(
                config.per_sec,
                config.burst,
            )))
        })
        .clone();
    let mut limiter = limiter_arc.lock();
    limiter.try_acquire()
}

/// Remove token validation rate limiter entries that have been idle for over 5 minutes.
fn cleanup_token_validation_rate_limiters() {
    let limiters = shared_token_validation_rate_limiters();
    let now = Instant::now();
    let idle_threshold = std::time::Duration::from_secs(300);
    limiters.retain(|_key, limiter_arc| {
        let limiter = limiter_arc.lock();
        now.saturating_duration_since(limiter.last_seen_at) < idle_threshold
    });
}

fn maybe_cleanup_token_validation_rate_limiters(
    limiters: &DashMap<String, Arc<ParkingLotMutex<TokenValidationRateLimiter>>>,
) {
    static LAST_CLEANUP_UNIX_SECS: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(0);
    let len = limiters.len();
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(std::time::Duration::from_secs(0))
        .as_secs();
    let previous = LAST_CLEANUP_UNIX_SECS.load(std::sync::atomic::Ordering::Relaxed);
    let interval_elapsed = previous == 0
        || now_secs.saturating_sub(previous) >= TOKEN_VALIDATION_CLEANUP_INTERVAL_SECS;
    let above_high_watermark = len > TOKEN_VALIDATION_RATE_LIMITER_MAX_ENTRIES;
    if !interval_elapsed && !above_high_watermark {
        return;
    }
    LAST_CLEANUP_UNIX_SECS.store(now_secs, std::sync::atomic::Ordering::Relaxed);
    cleanup_token_validation_rate_limiters();
}

// ── OTP per-IP rate limiting (sliding window counters) ───────────────────────

fn shared_otp_ip_rate_limiters() -> &'static DashMap<IpAddr, OtpIpRateState> {
    OTP_IP_RATE_LIMITERS.get_or_init(DashMap::new)
}

/// Check (and record) whether this IP is allowed to make another OTP request.
/// Returns Ok(()) if allowed, Err(retry_after_seconds) if rate-limited.
pub(super) fn check_otp_ip_rate_limit(client_ip: Option<IpAddr>) -> Result<(), u64> {
    if std::env::var("MGS_TEST_DISABLE_OTP_IP_RATE_LIMIT")
        .ok()
        .map(|raw| {
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
    {
        return Ok(());
    }
    let Some(ip) = client_ip else {
        // If we cannot determine the IP, allow the request but do not track.
        return Ok(());
    };

    let limiters = shared_otp_ip_rate_limiters();
    let now = unix_now();

    maybe_cleanup_otp_ip_rate_limiters(limiters, now);

    let mut entry = limiters.entry(ip).or_insert_with(OtpIpRateState::new);
    entry.try_record(now)
}

/// Evict OTP IP rate state entries that have fully expired.
fn cleanup_otp_ip_rate_limiters(now: u64) {
    let limiters = shared_otp_ip_rate_limiters();
    let long_cutoff = now.saturating_sub(OTP_IP_LONG_WINDOW_SECS);
    limiters.retain(|_ip, state| {
        state
            .short_window_timestamps
            .retain(|&ts| ts > now.saturating_sub(OTP_IP_SHORT_WINDOW_SECS));
        state.long_window_timestamps.retain(|&ts| ts > long_cutoff);
        !state.is_empty()
    });
}

fn maybe_cleanup_otp_ip_rate_limiters(limiters: &DashMap<IpAddr, OtpIpRateState>, now: u64) {
    if limiters.len() < OTP_IP_CLEANUP_MIN_ENTRIES {
        return;
    }
    static LAST_OTP_CLEANUP_UNIX_SECS: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(0);
    let previous = LAST_OTP_CLEANUP_UNIX_SECS.load(std::sync::atomic::Ordering::Relaxed);
    if previous != 0
        && now.saturating_sub(previous) < OTP_IP_CLEANUP_INTERVAL_SECS
        && limiters.len() <= OTP_IP_RATE_LIMITER_MAX_ENTRIES
    {
        return;
    }
    LAST_OTP_CLEANUP_UNIX_SECS.store(now, std::sync::atomic::Ordering::Relaxed);
    cleanup_otp_ip_rate_limiters(now);
}
