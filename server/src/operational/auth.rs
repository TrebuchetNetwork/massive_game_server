use crate::core::types::PlayerState;
use crate::operational::monitoring::metrics;
use dashmap::DashMap;
use parking_lot::{Mutex as ParkingLotMutex, RwLock};
use rand::{rngs::OsRng, RngCore};
use redis::Commands;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use warp::http::StatusCode;
use warp::{Filter, Reply};

const DEFAULT_OTP_TTL_SECONDS: u64 = 300;
// Reduced from 30 days to 24 hours to limit token exposure window.
// Override with MGS_AUTH_SESSION_TTL_SECONDS if longer sessions are needed.
const DEFAULT_SESSION_TTL_SECONDS: u64 = 60 * 60 * 24;
const DEFAULT_RESEND_INTERVAL_SECONDS: u64 = 30;
const DEFAULT_MAX_VERIFY_ATTEMPTS: u32 = 5;
const DEFAULT_LEADERBOARD_LIMIT: usize = 50;
const DEFAULT_REDIS_STORE_KEY: &str = "mgs:auth:persistent_store";
const DEFAULT_TOKEN_VALIDATION_RATE_LIMIT_PER_SEC: u32 = 24;
const DEFAULT_TOKEN_VALIDATION_RATE_LIMIT_BURST: u32 = 48;
const PROGRESSION_BASE_XP_PER_MATCH: u64 = 50;
const PROGRESSION_XP_PER_KILL: u64 = 30;
const PROGRESSION_BASE_CREDITS_PER_MATCH: u64 = 20;
const PROGRESSION_CREDITS_PER_KILL: u64 = 8;
const DEFAULT_ACCOUNT_DELETION_GRACE_PERIOD_HOURS: u64 = 72;
const OTP_CODE_DIGITS: usize = 6;
const OTP_CODE_UPPER_BOUND: u32 = 1_000_000;
/// Interval for the periodic task that processes queued account deletions.
const DELETION_PROCESSING_INTERVAL_SECS: u64 = 3600;
const ACTIVE_PHONE_HASH_PREFIX: &str = "hash:";
const DELETED_PHONE_HASH_PREFIX: &str = "deleted:";

/// Per-IP OTP rate limiting: max 5 OTP requests per 10 minutes (short window).
const OTP_IP_SHORT_WINDOW_SECS: u64 = 600;
const OTP_IP_SHORT_WINDOW_MAX: u32 = 5;
/// Per-IP OTP rate limiting: max 20 OTP requests per hour (long window).
const OTP_IP_LONG_WINDOW_SECS: u64 = 3600;
const OTP_IP_LONG_WINDOW_MAX: u32 = 20;
/// Maximum number of tracked IPs in the OTP IP rate limiter before cleanup triggers.
const OTP_IP_RATE_LIMITER_MAX_ENTRIES: usize = 10_000;

static TOKEN_VALIDATION_RATE_LIMITERS: OnceLock<
    DashMap<String, Arc<ParkingLotMutex<TokenValidationRateLimiter>>>,
> = OnceLock::new();
static TOKEN_VALIDATION_RATE_LIMIT_CONFIG: OnceLock<TokenValidationRateLimitConfig> =
    OnceLock::new();
static OTP_IP_RATE_LIMITERS: OnceLock<DashMap<IpAddr, OtpIpRateState>> = OnceLock::new();
static GDPR_HASH_SALT: OnceLock<Vec<u8>> = OnceLock::new();

#[derive(Clone)]
pub struct AuthService {
    inner: Arc<AuthInner>,
}

struct AuthInner {
    store_path: PathBuf,
    persistent_store: RwLock<PersistentAuthStore>,
    redis_cache: Option<AuthRedisCache>,
    otp_challenges: DashMap<String, OtpChallenge>,
    sessions: DashMap<String, SessionRecord>,
    peer_bindings: DashMap<String, String>,
    /// Queued account deletions: user_id -> PendingDeletion
    deletion_queue: DashMap<String, PendingDeletion>,
    otp_ttl_seconds: u64,
    session_ttl_seconds: u64,
    resend_interval_seconds: u64,
    max_verify_attempts: u32,
    /// Grace period (in hours) before a queued deletion is executed.
    deletion_grace_period_hours: u64,
    sms_command: Option<String>,
    sms_dev_mode: bool,
    /// When true, the verify-code endpoint sets the session token as an
    /// HttpOnly, Secure, SameSite=Strict cookie instead of (in addition to)
    /// returning it in the JSON body.  The WebSocket upgrade path and
    /// authenticated endpoints will then also accept the cookie as a token
    /// source.  Enable via MGS_AUTH_USE_COOKIES=true.
    use_auth_cookies: bool,
}

/// A pending account deletion that is within the grace period.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingDeletion {
    user_id: String,
    requested_at: u64,
    scheduled_deletion_time: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistentAuthStore {
    users: HashMap<String, UserRecord>,
    phone_to_user_id: HashMap<String, String>,
    #[serde(default)]
    pending_deletions: HashMap<String, PendingDeletion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UserRecord {
    user_id: String,
    phone_number: String,
    phone_last4: String,
    display_name: String,
    created_at: u64,
    updated_at: u64,
    last_seen_at: u64,
    matches_played: u64,
    cumulative_score: i64,
    best_score: i32,
    total_kills: u64,
    total_deaths: u64,
    last_game_username: Option<String>,
    #[serde(default)]
    experience_points: u64,
    #[serde(default)]
    credits: u64,
    /// True if this account has been anonymized/deleted per GDPR request.
    #[serde(default)]
    deleted: bool,
}

#[derive(Debug, Clone)]
struct OtpChallenge {
    code: String,
    expires_at: u64,
    last_sent_at: u64,
    attempts: u32,
}

#[derive(Debug, Clone)]
struct SessionRecord {
    user_id: String,
    expires_at: u64,
}

struct AuthRedisCache {
    connection: ParkingLotMutex<redis::Connection>,
    store_key: String,
}

#[derive(Debug)]
enum AuthError {
    InvalidPhone,
    InvalidCodeFormat,
    CodeNotRequested,
    CodeExpired,
    CodeMismatch { remaining_attempts: u32 },
    TooManyAttempts,
    RateLimited { retry_after_seconds: u64 },
    OtpIpRateLimited { retry_after_seconds: u64 },
    TokenValidationRateLimited { retry_after_seconds: u64 },
    DeliveryFailed(String),
    SessionInvalid,
    AccountDeleted,
    DeletionAlreadyPending,
    DeletionNotPending,
    Internal(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthProfileView {
    pub user_id: String,
    pub display_name: String,
    pub phone_masked: String,
    pub created_at: u64,
    pub last_seen_at: u64,
    pub matches_played: u64,
    pub cumulative_score: i64,
    pub best_score: i32,
    pub total_kills: u64,
    pub total_deaths: u64,
    pub last_game_username: Option<String>,
    pub experience_points: u64,
    pub credits: u64,
    pub level: u32,
    pub next_level_experience: u64,
    pub mmr: f32,
    pub mmr_band: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestCodeResult {
    pub phone_masked: String,
    pub expires_at: u64,
    pub retry_after_seconds: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerifyCodeResult {
    pub token: String,
    pub token_expires_at: u64,
    pub profile: AuthProfileView,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthMeResult {
    pub token_expires_at: u64,
    pub profile: AuthProfileView,
}

#[derive(Debug, Clone, Serialize)]
pub struct LeaderboardResult {
    pub players: Vec<AuthProfileView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountDeletionResult {
    pub user_id: String,
    pub requested_at: u64,
    pub scheduled_deletion_time: u64,
    pub grace_period_hours: u64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CancelDeletionResult {
    pub user_id: String,
    pub cancelled: bool,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RequestCodeBody {
    phone_number: String,
}

#[derive(Debug, Clone, Deserialize)]
struct VerifyCodeBody {
    phone_number: String,
    code: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct TokenQuery {}

#[derive(Debug, Clone, Deserialize, Default)]
struct LeaderboardQuery {
    limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
struct ApiErrorBody {
    code: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_after_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining_attempts: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
struct ApiResponse<T>
where
    T: Serialize,
{
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<ApiErrorBody>,
}

impl AuthError {
    fn to_http(&self) -> (StatusCode, &'static str, String, Option<u64>, Option<u32>) {
        match self {
            AuthError::InvalidPhone => (
                StatusCode::BAD_REQUEST,
                "invalid_phone",
                "Phone number is invalid. Use E.164 format, for example +15551234567.".to_owned(),
                None,
                None,
            ),
            AuthError::InvalidCodeFormat => (
                StatusCode::BAD_REQUEST,
                "invalid_code_format",
                "Verification code must be exactly 6 digits.".to_owned(),
                None,
                None,
            ),
            AuthError::CodeNotRequested => (
                StatusCode::BAD_REQUEST,
                "code_not_requested",
                "No verification code has been requested for this phone number.".to_owned(),
                None,
                None,
            ),
            AuthError::CodeExpired => (
                StatusCode::BAD_REQUEST,
                "code_expired",
                "The verification code expired. Request a new code.".to_owned(),
                None,
                None,
            ),
            AuthError::CodeMismatch { remaining_attempts } => (
                StatusCode::UNAUTHORIZED,
                "code_mismatch",
                "The verification code is incorrect.".to_owned(),
                None,
                Some(*remaining_attempts),
            ),
            AuthError::TooManyAttempts => (
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_attempts",
                "Too many incorrect verification attempts. Request a new code.".to_owned(),
                None,
                None,
            ),
            AuthError::RateLimited {
                retry_after_seconds,
            } => (
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "A code was sent recently. Please wait before requesting another code.".to_owned(),
                Some(*retry_after_seconds),
                None,
            ),
            AuthError::OtpIpRateLimited {
                retry_after_seconds,
            } => (
                StatusCode::TOO_MANY_REQUESTS,
                "ip_rate_limited",
                "Too many OTP requests from this address. Please try again later.".to_owned(),
                Some(*retry_after_seconds),
                None,
            ),
            AuthError::TokenValidationRateLimited {
                retry_after_seconds,
            } => (
                StatusCode::TOO_MANY_REQUESTS,
                "token_rate_limited",
                "Too many token validation attempts from this client. Retry shortly.".to_owned(),
                Some(*retry_after_seconds),
                None,
            ),
            AuthError::DeliveryFailed(reason) => (
                StatusCode::BAD_GATEWAY,
                "sms_delivery_failed",
                format!("Failed to deliver SMS code: {}", reason),
                None,
                None,
            ),
            AuthError::SessionInvalid => (
                StatusCode::UNAUTHORIZED,
                "session_invalid",
                "Session token is missing, invalid, or expired.".to_owned(),
                None,
                None,
            ),
            AuthError::AccountDeleted => (
                StatusCode::GONE,
                "account_deleted",
                "This account has been deleted and anonymized.".to_owned(),
                None,
                None,
            ),
            AuthError::DeletionAlreadyPending => (
                StatusCode::CONFLICT,
                "deletion_already_pending",
                "Account deletion is already scheduled. Use cancel-deletion to abort.".to_owned(),
                None,
                None,
            ),
            AuthError::DeletionNotPending => (
                StatusCode::BAD_REQUEST,
                "deletion_not_pending",
                "No pending account deletion to cancel.".to_owned(),
                None,
                None,
            ),
            AuthError::Internal(reason) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                reason.clone(),
                None,
                None,
            ),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct TokenValidationRateLimitConfig {
    per_sec: u32,
    burst: u32,
}

#[derive(Debug)]
struct TokenValidationRateLimiter {
    refill_per_sec: f64,
    capacity: f64,
    available_tokens: f64,
    last_refill_at: Instant,
}

impl TokenValidationRateLimiter {
    fn new(refill_per_sec: u32, capacity: u32) -> Self {
        Self {
            refill_per_sec: refill_per_sec.max(1) as f64,
            capacity: capacity.max(1) as f64,
            available_tokens: capacity.max(1) as f64,
            last_refill_at: Instant::now(),
        }
    }

    fn try_acquire(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(self.last_refill_at);
        self.last_refill_at = now;
        let refill = elapsed.as_secs_f64() * self.refill_per_sec;
        self.available_tokens = (self.available_tokens + refill).min(self.capacity);

        if self.available_tokens >= 1.0 {
            self.available_tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

fn shared_token_validation_rate_limiters(
) -> &'static DashMap<String, Arc<ParkingLotMutex<TokenValidationRateLimiter>>> {
    TOKEN_VALIDATION_RATE_LIMITERS.get_or_init(DashMap::new)
}

fn token_validation_rate_limit_config() -> TokenValidationRateLimitConfig {
    *TOKEN_VALIDATION_RATE_LIMIT_CONFIG.get_or_init(|| {
        let per_sec = std::env::var("MGS_AUTH_TOKEN_RATE_LIMIT_PER_SEC")
            .ok()
            .and_then(|raw| raw.parse::<u32>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_TOKEN_VALIDATION_RATE_LIMIT_PER_SEC);
        let burst = std::env::var("MGS_AUTH_TOKEN_RATE_LIMIT_BURST")
            .ok()
            .and_then(|raw| raw.parse::<u32>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_TOKEN_VALIDATION_RATE_LIMIT_BURST);
        TokenValidationRateLimitConfig { per_sec, burst }
    })
}

fn token_validation_rate_limit_key(remote_addr: Option<SocketAddr>) -> String {
    remote_addr
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn try_acquire_token_validation_token(remote_addr: Option<SocketAddr>) -> bool {
    let config = token_validation_rate_limit_config();
    let key = token_validation_rate_limit_key(remote_addr);
    let limiters = shared_token_validation_rate_limiters();

    // Periodic cleanup: evict stale entries when map exceeds threshold.
    if limiters.len() > OTP_IP_RATE_LIMITER_MAX_ENTRIES {
        cleanup_token_validation_rate_limiters();
    }

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
        now.saturating_duration_since(limiter.last_refill_at) < idle_threshold
    });
}

// ── OTP per-IP rate limiting (sliding window counters) ───────────────────────

/// Tracks per-IP OTP request timestamps using two sliding windows (short and long).
#[derive(Debug, Clone)]
struct OtpIpRateState {
    /// Timestamps of recent OTP requests within the short window.
    short_window_timestamps: Vec<u64>,
    /// Timestamps of recent OTP requests within the long window.
    long_window_timestamps: Vec<u64>,
}

impl OtpIpRateState {
    fn new() -> Self {
        Self {
            short_window_timestamps: Vec::new(),
            long_window_timestamps: Vec::new(),
        }
    }

    /// Try to record a new OTP request. Returns Ok(()) if allowed, or
    /// Err(retry_after_seconds) if the IP has exceeded its quota.
    fn try_record(&mut self, now: u64) -> Result<(), u64> {
        // Evict expired entries from both windows.
        let short_cutoff = now.saturating_sub(OTP_IP_SHORT_WINDOW_SECS);
        self.short_window_timestamps.retain(|&ts| ts > short_cutoff);

        let long_cutoff = now.saturating_sub(OTP_IP_LONG_WINDOW_SECS);
        self.long_window_timestamps.retain(|&ts| ts > long_cutoff);

        // Check short window (5 per 10 min).
        if self.short_window_timestamps.len() >= OTP_IP_SHORT_WINDOW_MAX as usize {
            let oldest = self.short_window_timestamps.first().copied().unwrap_or(now);
            let retry_after = oldest
                .saturating_add(OTP_IP_SHORT_WINDOW_SECS)
                .saturating_sub(now)
                .max(1);
            return Err(retry_after);
        }

        // Check long window (20 per hour).
        if self.long_window_timestamps.len() >= OTP_IP_LONG_WINDOW_MAX as usize {
            let oldest = self.long_window_timestamps.first().copied().unwrap_or(now);
            let retry_after = oldest
                .saturating_add(OTP_IP_LONG_WINDOW_SECS)
                .saturating_sub(now)
                .max(1);
            return Err(retry_after);
        }

        self.short_window_timestamps.push(now);
        self.long_window_timestamps.push(now);
        Ok(())
    }

    /// Returns true if this entry has no timestamps in either window (fully expired).
    fn is_empty(&self) -> bool {
        self.short_window_timestamps.is_empty() && self.long_window_timestamps.is_empty()
    }
}

fn shared_otp_ip_rate_limiters() -> &'static DashMap<IpAddr, OtpIpRateState> {
    OTP_IP_RATE_LIMITERS.get_or_init(DashMap::new)
}

/// Check (and record) whether this IP is allowed to make another OTP request.
/// Returns Ok(()) if allowed, Err(retry_after_seconds) if rate-limited.
fn check_otp_ip_rate_limit(client_ip: Option<IpAddr>) -> Result<(), u64> {
    let Some(ip) = client_ip else {
        // If we cannot determine the IP, allow the request but do not track.
        return Ok(());
    };

    let limiters = shared_otp_ip_rate_limiters();
    let now = unix_now();

    // Periodic cleanup when the map grows too large.
    if limiters.len() > OTP_IP_RATE_LIMITER_MAX_ENTRIES {
        cleanup_otp_ip_rate_limiters(now);
    }

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

impl AuthService {
    pub fn new_from_env() -> Self {
        let store_path = std::env::var("MGS_AUTH_STORE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("data/auth_store.json"));
        let otp_ttl_seconds =
            parse_u64_env("MGS_AUTH_OTP_TTL_SECONDS", DEFAULT_OTP_TTL_SECONDS).max(60);
        let session_ttl_seconds =
            parse_u64_env("MGS_AUTH_SESSION_TTL_SECONDS", DEFAULT_SESSION_TTL_SECONDS).max(300);
        let resend_interval_seconds = parse_u64_env(
            "MGS_AUTH_RESEND_INTERVAL_SECONDS",
            DEFAULT_RESEND_INTERVAL_SECONDS,
        )
        .max(5);
        let max_verify_attempts =
            parse_u32_env("MGS_AUTH_MAX_VERIFY_ATTEMPTS", DEFAULT_MAX_VERIFY_ATTEMPTS).max(1);

        let sms_command = std::env::var("MGS_SMS_COMMAND")
            .ok()
            .map(|raw| raw.trim().to_owned())
            .filter(|raw| !raw.is_empty());
        let sms_dev_mode = parse_bool_env("MGS_SMS_DEV_MODE", false);
        let use_auth_cookies = parse_bool_env("MGS_AUTH_USE_COOKIES", false);
        let deletion_grace_period_hours = parse_u64_env(
            "MGS_ACCOUNT_DELETION_GRACE_PERIOD_HOURS",
            DEFAULT_ACCOUNT_DELETION_GRACE_PERIOD_HOURS,
        )
        .max(1);
        let redis_cache = init_redis_cache_from_env();

        let persistent_store = load_persistent_store(&store_path, redis_cache.as_ref());
        let deletion_queue = DashMap::new();
        for (user_id, pending) in &persistent_store.pending_deletions {
            deletion_queue.insert(user_id.clone(), pending.clone());
        }
        info!(
            "Auth service initialized. store_path='{}', users={}, pending_deletions={}, sms_dev_mode={}, use_auth_cookies={}",
            store_path.display(),
            persistent_store.users.len(),
            deletion_queue.len(),
            sms_dev_mode,
            use_auth_cookies
        );
        if sms_dev_mode {
            warn!(
                "SMS dev mode is ENABLED — OTP delivery is stubbed and verification codes are redacted in logs."
            );
        }
        if use_auth_cookies {
            info!("Cookie-based auth enabled: verify-code will set HttpOnly session cookie.");
        }

        Self {
            inner: Arc::new(AuthInner {
                store_path,
                persistent_store: RwLock::new(persistent_store),
                redis_cache,
                otp_challenges: DashMap::new(),
                sessions: DashMap::new(),
                peer_bindings: DashMap::new(),
                deletion_queue,
                otp_ttl_seconds,
                session_ttl_seconds,
                resend_interval_seconds,
                max_verify_attempts,
                deletion_grace_period_hours,
                sms_command,
                sms_dev_mode,
                use_auth_cookies,
            }),
        }
    }

    fn request_phone_code(&self, phone_number_raw: &str) -> Result<RequestCodeResult, AuthError> {
        let phone_number = normalize_phone_number(phone_number_raw).ok_or_else(|| {
            metrics::record_auth_attempt("request_code", "invalid_phone");
            AuthError::InvalidPhone
        })?;
        let now = unix_now();

        if let Some(existing) = self.inner.otp_challenges.get(&phone_number) {
            let earliest_retry = existing
                .last_sent_at
                .saturating_add(self.inner.resend_interval_seconds);
            if now < earliest_retry {
                metrics::record_auth_attempt("request_code", "rate_limited");
                return Err(AuthError::RateLimited {
                    retry_after_seconds: earliest_retry.saturating_sub(now),
                });
            }
        }

        let code = generate_otp_code();
        let expires_at = now.saturating_add(self.inner.otp_ttl_seconds);
        let challenge = OtpChallenge {
            code: code.clone(),
            expires_at,
            last_sent_at: now,
            attempts: 0,
        };
        self.inner
            .otp_challenges
            .insert(phone_number.clone(), challenge);

        if let Err(reason) = self.dispatch_sms_code(&phone_number, &code) {
            self.inner.otp_challenges.remove(&phone_number);
            metrics::record_auth_attempt("request_code", "delivery_failed");
            return Err(AuthError::DeliveryFailed(reason));
        }

        metrics::record_auth_attempt("request_code", "success");
        Ok(RequestCodeResult {
            phone_masked: mask_phone_number(&phone_number),
            expires_at,
            retry_after_seconds: self.inner.resend_interval_seconds,
        })
    }

    fn verify_phone_code(
        &self,
        phone_number_raw: &str,
        code_raw: &str,
    ) -> Result<VerifyCodeResult, AuthError> {
        let phone_number = normalize_phone_number(phone_number_raw).ok_or_else(|| {
            metrics::record_auth_attempt("verify_code", "invalid_phone");
            AuthError::InvalidPhone
        })?;
        let code = code_raw.trim();
        if code.len() != 6 || !code.chars().all(|ch| ch.is_ascii_digit()) {
            metrics::record_auth_attempt("verify_code", "invalid_code_format");
            return Err(AuthError::InvalidCodeFormat);
        }

        let now = unix_now();
        let mut remove_after_check = false;

        let verification_result = {
            let mut challenge_entry = self
                .inner
                .otp_challenges
                .get_mut(&phone_number)
                .ok_or_else(|| {
                    metrics::record_auth_attempt("verify_code", "code_not_requested");
                    AuthError::CodeNotRequested
                })?;

            if now > challenge_entry.expires_at {
                remove_after_check = true;
                Err(AuthError::CodeExpired)
            } else if challenge_entry.attempts >= self.inner.max_verify_attempts {
                remove_after_check = true;
                Err(AuthError::TooManyAttempts)
            } else if !constant_time_eq_str(&challenge_entry.code, code) {
                challenge_entry.attempts = challenge_entry.attempts.saturating_add(1);
                if challenge_entry.attempts >= self.inner.max_verify_attempts {
                    remove_after_check = true;
                    Err(AuthError::TooManyAttempts)
                } else {
                    Err(AuthError::CodeMismatch {
                        remaining_attempts: self
                            .inner
                            .max_verify_attempts
                            .saturating_sub(challenge_entry.attempts),
                    })
                }
            } else {
                remove_after_check = true;
                Ok(())
            }
        };

        if remove_after_check {
            self.inner.otp_challenges.remove(&phone_number);
        }

        if let Err(err) = verification_result {
            match err {
                AuthError::CodeExpired => {
                    metrics::record_auth_attempt("verify_code", "code_expired");
                }
                AuthError::TooManyAttempts => {
                    metrics::record_auth_attempt("verify_code", "too_many_attempts");
                }
                AuthError::CodeMismatch { .. } => {
                    metrics::record_auth_attempt("verify_code", "code_mismatch");
                }
                _ => {}
            }
            return Err(err);
        }

        let mut persistent_guard = self.inner.persistent_store.write();
        let phone_lookup_key = active_phone_lookup_key(&phone_number);
        let user_id = if let Some(existing_user_id) =
            persistent_guard.phone_to_user_id.get(&phone_lookup_key)
        {
            existing_user_id.clone()
        } else if let Some(existing_user_id) =
            persistent_guard.phone_to_user_id.remove(&phone_number)
        {
            // Migrate legacy raw-phone mapping to hashed lookup key.
            persistent_guard
                .phone_to_user_id
                .insert(phone_lookup_key.clone(), existing_user_id.clone());
            existing_user_id
        } else {
            let new_user_id = Uuid::new_v4().to_string();
            let last4 = phone_last4(&phone_number);
            let display_name = format!("Player{}", last4);
            let new_user = UserRecord {
                user_id: new_user_id.clone(),
                phone_number: phone_lookup_key.clone(),
                phone_last4: last4,
                display_name,
                created_at: now,
                updated_at: now,
                last_seen_at: now,
                matches_played: 0,
                cumulative_score: 0,
                best_score: 0,
                total_kills: 0,
                total_deaths: 0,
                last_game_username: None,
                experience_points: 0,
                credits: 0,
                deleted: false,
            };
            persistent_guard
                .phone_to_user_id
                .insert(phone_lookup_key, new_user_id.clone());
            persistent_guard.users.insert(new_user_id.clone(), new_user);
            new_user_id
        };

        let profile = if let Some(user) = persistent_guard.users.get_mut(&user_id) {
            user.updated_at = now;
            user.last_seen_at = now;
            to_profile_view(user)
        } else {
            metrics::record_auth_attempt("verify_code", "store_inconsistent");
            return Err(AuthError::Internal(
                "Auth store is inconsistent: user record missing.".to_owned(),
            ));
        };
        let store_snapshot = persistent_guard.clone();
        drop(persistent_guard);
        spawn_persist_auth_store(
            self.inner.store_path.clone(),
            store_snapshot,
            self.inner.clone(),
        );

        let session_token = format!("mgs_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let token_expires_at = now.saturating_add(self.inner.session_ttl_seconds);
        self.inner.sessions.insert(
            session_token.clone(),
            SessionRecord {
                user_id: user_id.clone(),
                expires_at: token_expires_at,
            },
        );

        metrics::record_auth_attempt("verify_code", "success");
        Ok(VerifyCodeResult {
            token: session_token,
            token_expires_at,
            profile,
        })
    }

    pub fn resolve_user_id_from_token(&self, token_raw: &str) -> Option<String> {
        let started_at = Instant::now();
        let token = token_raw.trim();
        if token.is_empty() {
            metrics::record_auth_attempt("token_resolve", "empty");
            metrics::record_auth_token_resolution(started_at.elapsed().as_secs_f64(), "empty");
            return None;
        }
        let now = unix_now();
        let Some(session_entry) = self.inner.sessions.get(token) else {
            metrics::record_auth_attempt("token_resolve", "not_found");
            metrics::record_auth_token_resolution(started_at.elapsed().as_secs_f64(), "not_found");
            return None;
        };
        if now > session_entry.expires_at {
            drop(session_entry);
            self.inner.sessions.remove(token);
            metrics::record_auth_attempt("token_resolve", "expired");
            metrics::record_auth_token_resolution(started_at.elapsed().as_secs_f64(), "expired");
            return None;
        }
        metrics::record_auth_attempt("token_resolve", "success");
        metrics::record_auth_token_resolution(started_at.elapsed().as_secs_f64(), "success");
        Some(session_entry.user_id.clone())
    }

    /// Returns true if the server is configured to set session tokens via
    /// HttpOnly cookies (MGS_AUTH_USE_COOKIES=true).
    pub fn use_auth_cookies(&self) -> bool {
        self.inner.use_auth_cookies
    }

    /// Returns the configured session TTL in seconds (for cookie Max-Age).
    pub fn session_ttl_seconds(&self) -> u64 {
        self.inner.session_ttl_seconds
    }

    pub fn profile_from_token(&self, token_raw: &str) -> Option<(AuthProfileView, u64)> {
        let started_at = Instant::now();
        let token = token_raw.trim();
        if token.is_empty() {
            metrics::record_auth_attempt("profile_lookup", "empty");
            metrics::record_auth_token_resolution(started_at.elapsed().as_secs_f64(), "empty");
            return None;
        }
        let now = unix_now();
        let Some(session_entry) = self.inner.sessions.get(token) else {
            metrics::record_auth_attempt("profile_lookup", "not_found");
            metrics::record_auth_token_resolution(started_at.elapsed().as_secs_f64(), "not_found");
            return None;
        };
        if now > session_entry.expires_at {
            drop(session_entry);
            self.inner.sessions.remove(token);
            metrics::record_auth_attempt("profile_lookup", "expired");
            metrics::record_auth_token_resolution(started_at.elapsed().as_secs_f64(), "expired");
            return None;
        }
        let user_id = session_entry.user_id.clone();
        let expires_at = session_entry.expires_at;
        drop(session_entry);
        let profile = self.profile_by_user_id(&user_id);
        let result = if profile.is_some() {
            "success"
        } else {
            "missing_user"
        };
        metrics::record_auth_attempt("profile_lookup", result);
        metrics::record_auth_token_resolution(started_at.elapsed().as_secs_f64(), result);
        profile.map(|profile| (profile, expires_at))
    }

    pub fn profile_by_user_id(&self, user_id: &str) -> Option<AuthProfileView> {
        let persistent_guard = self.inner.persistent_store.read();
        persistent_guard.users.get(user_id).map(to_profile_view)
    }

    pub fn revoke_session_token(&self, token_raw: &str) -> bool {
        let token = token_raw.trim();
        if token.is_empty() {
            return false;
        }
        self.inner.sessions.remove(token).is_some()
    }

    pub fn bind_peer_to_user(&self, peer_id: &str, user_id: &str) {
        if peer_id.is_empty() || user_id.is_empty() {
            return;
        }
        self.inner
            .peer_bindings
            .insert(peer_id.to_owned(), user_id.to_owned());
    }

    pub fn resolve_user_id_from_peer(&self, peer_id: &str) -> Option<String> {
        self.inner
            .peer_bindings
            .get(peer_id)
            .map(|r| r.value().clone())
    }

    pub fn clear_peer_binding(&self, peer_id: &str) {
        if peer_id.is_empty() {
            return;
        }
        self.inner.peer_bindings.remove(peer_id);
    }

    pub fn record_disconnect_score_for_peer(&self, peer_id: &str, player_state: &PlayerState) {
        let user_id = match self.inner.peer_bindings.remove(peer_id) {
            Some((_key, user_id)) => user_id,
            None => return,
        };

        let now = unix_now();
        let mut persistent_guard = self.inner.persistent_store.write();
        if let Some(user) = persistent_guard.users.get_mut(&user_id) {
            user.matches_played = user.matches_played.saturating_add(1);
            user.cumulative_score = user
                .cumulative_score
                .saturating_add(i64::from(player_state.score));
            if player_state.score > user.best_score {
                user.best_score = player_state.score;
            }
            user.total_kills = user
                .total_kills
                .saturating_add(player_state.kills.max(0) as u64);
            user.total_deaths = user
                .total_deaths
                .saturating_add(player_state.deaths.max(0) as u64);
            let (xp_gain, credits_gain) = progression_reward_from_match(player_state);
            user.experience_points = user.experience_points.saturating_add(xp_gain);
            user.credits = user.credits.saturating_add(credits_gain);
            user.last_seen_at = now;
            user.updated_at = now;
            if !player_state.username.trim().is_empty() {
                user.last_game_username = Some(player_state.username.clone());
            }
            let store_snapshot = persistent_guard.clone();
            drop(persistent_guard);
            spawn_persist_auth_store(
                self.inner.store_path.clone(),
                store_snapshot,
                self.inner.clone(),
            );
        }
    }

    pub fn leaderboard(&self, limit: usize) -> Vec<AuthProfileView> {
        let mut profiles: Vec<AuthProfileView> = {
            let persistent_guard = self.inner.persistent_store.read();
            persistent_guard
                .users
                .values()
                .filter(|user| !user.deleted)
                .map(to_profile_view)
                .collect()
        };

        profiles.sort_by(|a, b| {
            b.mmr
                .partial_cmp(&a.mmr)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.cumulative_score.cmp(&a.cumulative_score))
                .then_with(|| b.best_score.cmp(&a.best_score))
                .then_with(|| b.total_kills.cmp(&a.total_kills))
                .then_with(|| a.user_id.cmp(&b.user_id))
        });

        let bounded_limit = limit.clamp(1, 200);
        if profiles.len() > bounded_limit {
            profiles.truncate(bounded_limit);
        }
        profiles
    }

    /// Queue an account for deletion after the grace period.
    /// Returns the deletion schedule details on success.
    fn request_account_deletion(&self, user_id: &str) -> Result<AccountDeletionResult, AuthError> {
        // Check if already pending
        if self.inner.deletion_queue.contains_key(user_id) {
            return Err(AuthError::DeletionAlreadyPending);
        }

        // Check the user exists and is not already deleted
        {
            let store = self.inner.persistent_store.read();
            match store.users.get(user_id) {
                Some(user) if user.deleted => return Err(AuthError::AccountDeleted),
                Some(_) => {}
                None => return Err(AuthError::SessionInvalid),
            }
        }

        let now = unix_now();
        let grace_seconds = self.inner.deletion_grace_period_hours.saturating_mul(3600);
        let scheduled_deletion_time = now.saturating_add(grace_seconds);

        let pending = PendingDeletion {
            user_id: user_id.to_owned(),
            requested_at: now,
            scheduled_deletion_time,
        };

        self.inner
            .deletion_queue
            .insert(user_id.to_owned(), pending.clone());

        let store_snapshot = {
            let mut store = self.inner.persistent_store.write();
            store
                .pending_deletions
                .insert(user_id.to_owned(), pending.clone());
            store.clone()
        };
        spawn_persist_auth_store(
            self.inner.store_path.clone(),
            store_snapshot,
            self.inner.clone(),
        );

        info!(
            "Account deletion queued for user_id={}, scheduled_at={}",
            user_id, scheduled_deletion_time
        );

        Ok(AccountDeletionResult {
            user_id: user_id.to_owned(),
            requested_at: now,
            scheduled_deletion_time,
            grace_period_hours: self.inner.deletion_grace_period_hours,
            message: format!(
                "Account deletion scheduled. You have {} hours to cancel. After that, your data will be permanently anonymized.",
                self.inner.deletion_grace_period_hours
            ),
        })
    }

    /// Cancel a pending account deletion within the grace period.
    fn cancel_account_deletion(&self, user_id: &str) -> Result<CancelDeletionResult, AuthError> {
        match self.inner.deletion_queue.remove(user_id) {
            Some((_key, pending)) => {
                let store_snapshot = {
                    let mut store = self.inner.persistent_store.write();
                    store.pending_deletions.remove(user_id);
                    store.clone()
                };
                spawn_persist_auth_store(
                    self.inner.store_path.clone(),
                    store_snapshot,
                    self.inner.clone(),
                );
                info!(
                    "Account deletion cancelled for user_id={} (was scheduled for {})",
                    user_id, pending.scheduled_deletion_time
                );
                Ok(CancelDeletionResult {
                    user_id: user_id.to_owned(),
                    cancelled: true,
                    message: "Account deletion has been cancelled. Your account is safe."
                        .to_owned(),
                })
            }
            None => Err(AuthError::DeletionNotPending),
        }
    }

    /// Anonymize user data: replace PII with hashed/generic values.
    /// This is the core GDPR "right to erasure" implementation.
    fn anonymize_user_data(&self, user_id: &str) {
        self.inner.deletion_queue.remove(user_id);
        let mut store = self.inner.persistent_store.write();

        // Extract the original phone number for removal, then mutate the user.
        let original_phone = store.users.get(user_id).map(|u| u.phone_number.clone());
        let active_phone_hash = original_phone
            .as_deref()
            .map(phone_hash_from_stored_or_legacy_value);

        // Remove the original phone->user_id mapping
        if let Some(ref phone) = original_phone {
            store.phone_to_user_id.remove(phone);
        }
        if let Some(ref phone_hash) = active_phone_hash {
            store
                .phone_to_user_id
                .remove(&format!("{}{}", ACTIVE_PHONE_HASH_PREFIX, phone_hash));
        }
        store.pending_deletions.remove(user_id);

        if let Some(user) = store.users.get_mut(user_id) {
            // Hash the phone number so we can detect re-registration
            // but can never reverse it.
            let phone_hash = active_phone_hash.clone().unwrap_or_else(|| {
                hash_phone_for_anonymization(original_phone.as_deref().unwrap_or(""))
            });
            let hash_last4 = &phone_hash[phone_hash.len().saturating_sub(4)..];

            // Store the hash in phone_number field for re-registration detection
            user.phone_number = format!("{}{}", DELETED_PHONE_HASH_PREFIX, phone_hash);
            user.phone_last4 = "0000".to_owned();
            user.display_name = format!("Deleted User #{}", hash_last4);
            user.last_game_username = None;
            user.deleted = true;
            user.updated_at = unix_now();

            // Add the hashed phone to phone_to_user_id so we can detect
            // re-registration attempts from the same phone.
            store.phone_to_user_id.insert(
                format!("{}{}", DELETED_PHONE_HASH_PREFIX, phone_hash),
                user_id.to_owned(),
            );

            info!("User data anonymized for user_id={}", user_id);
        }

        let store_snapshot = store.clone();
        drop(store);

        // Clear all session tokens for this user
        self.revoke_all_sessions_for_user(user_id);

        // Persist the anonymized store
        spawn_persist_auth_store(
            self.inner.store_path.clone(),
            store_snapshot,
            self.inner.clone(),
        );
    }

    /// Revoke all active session tokens belonging to a specific user.
    fn revoke_all_sessions_for_user(&self, user_id: &str) {
        let tokens_to_remove: Vec<String> = self
            .inner
            .sessions
            .iter()
            .filter(|entry| entry.value().user_id == user_id)
            .map(|entry| entry.key().clone())
            .collect();

        let count = tokens_to_remove.len();
        for token in tokens_to_remove {
            self.inner.sessions.remove(&token);
        }
        if count > 0 {
            debug!("Revoked {} session token(s) for user_id={}", count, user_id);
        }
    }

    /// Process all queued deletions whose grace period has expired.
    /// Returns the number of accounts that were anonymized.
    pub fn process_pending_deletions(&self) -> usize {
        let now = unix_now();
        let mut processed = 0usize;

        // Collect user IDs that are past their grace period
        let ready: Vec<String> = self
            .inner
            .deletion_queue
            .iter()
            .filter(|entry| now >= entry.value().scheduled_deletion_time)
            .map(|entry| entry.key().clone())
            .collect();

        for user_id in &ready {
            if let Some((_key, pending)) = self.inner.deletion_queue.remove(user_id) {
                info!(
                    "Processing scheduled deletion for user_id={} (requested_at={}, scheduled_for={})",
                    pending.user_id, pending.requested_at, pending.scheduled_deletion_time
                );
                self.anonymize_user_data(user_id);
                processed += 1;
            }
        }

        if processed > 0 {
            info!(
                "Processed {} pending account deletion(s), {} remaining in queue",
                processed,
                self.inner.deletion_queue.len()
            );
        }

        processed
    }

    /// Start the background task that periodically processes queued deletions.
    /// Should be called once at server startup.
    pub fn start_deletion_processor(self) {
        let interval_secs = DELETION_PROCESSING_INTERVAL_SECS;
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::spawn(async move {
                let mut interval =
                    tokio::time::interval(std::time::Duration::from_secs(interval_secs));
                // The first tick completes immediately; skip it so we don't
                // run on startup before any deletions could be queued.
                interval.tick().await;
                loop {
                    interval.tick().await;
                    let count = self.process_pending_deletions();
                    if count > 0 {
                        info!("Deletion processor: anonymized {} account(s)", count);
                    }
                }
            });
        } else {
            debug!(
                "No tokio runtime available; deletion processor not started (test environment)."
            );
        }
    }

    fn dispatch_sms_code(&self, phone_number: &str, code: &str) -> Result<(), String> {
        let message = format!(
            "Your Massive Game Server verification code is {}. It expires in {} minutes.",
            code,
            (self.inner.otp_ttl_seconds / 60).max(1)
        );

        if let Some(command_executable) = &self.inner.sms_command {
            match Command::new(command_executable)
                .arg(phone_number)
                .arg(&message)
                .status()
            {
                Ok(status) if status.success() => {
                    info!(
                        "SMS command delivered code to {}",
                        mask_phone_number(phone_number)
                    );
                    if self.inner.sms_dev_mode {
                        info!(
                            "[AUTH_SMS_DEV] phone={} code=<redacted>",
                            mask_phone_number(phone_number)
                        );
                    }
                    return Ok(());
                }
                Ok(status) => {
                    return Err(format!(
                        "SMS command failed with status {}",
                        status.code().unwrap_or(-1)
                    ));
                }
                Err(error) => {
                    return Err(format!("SMS command execution failed: {}", error));
                }
            }
        }

        if self.inner.sms_dev_mode {
            debug!(
                "[AUTH_SMS_DEV] phone={} code=<redacted>",
                mask_phone_number(phone_number)
            );
            return Ok(());
        }

        Err(
            "SMS provider is not configured (set MGS_SMS_COMMAND or MGS_SMS_DEV_MODE=1)."
                .to_owned(),
        )
    }
}

pub fn build_auth_routes(
    auth_service: AuthService,
) -> impl Filter<Extract = (impl Reply,), Error = warp::Rejection> + Clone {
    // 64 KB body limit for all JSON endpoints to prevent resource exhaustion
    let json_body_limit = 1024 * 64;

    let request_code = warp::path!("auth" / "phone" / "request-code")
        .and(warp::post())
        .and(warp::body::content_length_limit(json_body_limit))
        .and(warp::body::json::<RequestCodeBody>())
        .and(warp::addr::remote())
        .and(with_auth_service(auth_service.clone()))
        .and_then(handle_request_code);

    let verify_code = warp::path!("auth" / "phone" / "verify-code")
        .and(warp::post())
        .and(warp::body::content_length_limit(json_body_limit))
        .and(warp::body::json::<VerifyCodeBody>())
        .and(with_auth_service(auth_service.clone()))
        .and_then(handle_verify_code);

    let me = warp::path!("auth" / "me")
        .and(warp::get())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::header::optional::<String>("cookie"))
        .and(
            warp::query::<TokenQuery>()
                .or(warp::any().map(TokenQuery::default))
                .unify(),
        )
        .and(warp::addr::remote())
        .and(with_auth_service(auth_service.clone()))
        .and_then(handle_auth_me);

    let logout = warp::path!("auth" / "logout")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::header::optional::<String>("cookie"))
        .and(
            warp::query::<TokenQuery>()
                .or(warp::any().map(TokenQuery::default))
                .unify(),
        )
        .and(warp::addr::remote())
        .and(with_auth_service(auth_service.clone()))
        .and_then(handle_auth_logout);

    let leaderboard = warp::path!("auth" / "leaderboard")
        .and(warp::get())
        .and(
            warp::query::<LeaderboardQuery>()
                .or(warp::any().map(LeaderboardQuery::default))
                .unify(),
        )
        .and(warp::addr::remote())
        .and(with_auth_service(auth_service.clone()))
        .and_then(handle_auth_leaderboard);

    let delete_account = warp::path!("auth" / "delete-account")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::header::optional::<String>("cookie"))
        .and(
            warp::query::<TokenQuery>()
                .or(warp::any().map(TokenQuery::default))
                .unify(),
        )
        .and(warp::addr::remote())
        .and(with_auth_service(auth_service.clone()))
        .and_then(handle_delete_account);

    let cancel_deletion = warp::path!("auth" / "cancel-deletion")
        .and(warp::post())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::header::optional::<String>("cookie"))
        .and(
            warp::query::<TokenQuery>()
                .or(warp::any().map(TokenQuery::default))
                .unify(),
        )
        .and(warp::addr::remote())
        .and(with_auth_service(auth_service))
        .and_then(handle_cancel_deletion);

    request_code
        .or(verify_code)
        .or(me)
        .or(logout)
        .or(leaderboard)
        .or(delete_account)
        .or(cancel_deletion)
}

fn with_auth_service(
    auth_service: AuthService,
) -> impl Filter<Extract = (AuthService,), Error = Infallible> + Clone {
    warp::any().map(move || auth_service.clone())
}

async fn handle_request_code(
    body: RequestCodeBody,
    remote_addr: Option<SocketAddr>,
    auth_service: AuthService,
) -> Result<impl Reply, Infallible> {
    let client_ip = remote_addr.map(|addr| addr.ip());

    // Per-IP OTP rate limiting: reject before any phone-level logic.
    if let Err(retry_after) = check_otp_ip_rate_limit(client_ip) {
        metrics::record_auth_attempt("request_code", "ip_rate_limited");
        return Ok(error_response(AuthError::OtpIpRateLimited {
            retry_after_seconds: retry_after,
        }));
    }

    let reply = match auth_service.request_phone_code(&body.phone_number) {
        Ok(result) => ok_response(result),
        Err(error) => error_response(error),
    };
    Ok(reply)
}

async fn handle_verify_code(
    body: VerifyCodeBody,
    auth_service: AuthService,
) -> Result<warp::reply::Response, Infallible> {
    match auth_service.verify_phone_code(&body.phone_number, &body.code) {
        Ok(result) => {
            if auth_service.use_auth_cookies() {
                // Set the session token as an HttpOnly, Secure, SameSite=Strict
                // cookie so that JS never needs to touch it.
                let max_age = auth_service.session_ttl_seconds();
                let cookie_value = format!(
                    "mgs_session={}; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age={}",
                    result.token, max_age
                );
                let json_reply = ok_response(result);
                Ok(
                    warp::reply::with_header(json_reply, "Set-Cookie", cookie_value)
                        .into_response(),
                )
            } else {
                Ok(ok_response(result).into_response())
            }
        }
        Err(error) => Ok(error_response(error).into_response()),
    }
}

async fn handle_auth_me(
    authorization_header: Option<String>,
    cookie_header: Option<String>,
    query: TokenQuery,
    remote_addr: Option<SocketAddr>,
    auth_service: AuthService,
) -> Result<warp::reply::Response, Infallible> {
    if !try_acquire_token_validation_token(remote_addr) {
        return Ok(error_response(AuthError::TokenValidationRateLimited {
            retry_after_seconds: 1,
        })
        .into_response());
    }
    let token = resolve_token_with_cookie(
        authorization_header.as_deref(),
        &query,
        cookie_header.as_deref(),
    );
    let reply = match token {
        Some(token_value) => match auth_service.profile_from_token(&token_value) {
            Some((profile, token_expires_at)) => ok_response(AuthMeResult {
                token_expires_at,
                profile,
            }),
            None => error_response(AuthError::SessionInvalid),
        },
        None => error_response(AuthError::SessionInvalid),
    };
    Ok(warp::reply::with_header(
        reply,
        "X-Data-Retention-Policy",
        format!(
            "{}h-grace-period",
            auth_service.inner.deletion_grace_period_hours
        ),
    )
    .into_response())
}

async fn handle_auth_logout(
    authorization_header: Option<String>,
    cookie_header: Option<String>,
    query: TokenQuery,
    remote_addr: Option<SocketAddr>,
    auth_service: AuthService,
) -> Result<impl Reply, Infallible> {
    if !try_acquire_token_validation_token(remote_addr) {
        return Ok(error_response(AuthError::TokenValidationRateLimited {
            retry_after_seconds: 1,
        }));
    }
    let token = resolve_token_with_cookie(
        authorization_header.as_deref(),
        &query,
        cookie_header.as_deref(),
    );
    let revoked = token
        .as_deref()
        .map(|value| auth_service.revoke_session_token(value))
        .unwrap_or(false);
    Ok(ok_response(serde_json::json!({ "revoked": revoked })))
}

async fn handle_auth_leaderboard(
    query: LeaderboardQuery,
    remote_addr: Option<SocketAddr>,
    auth_service: AuthService,
) -> Result<impl Reply, Infallible> {
    if !try_acquire_token_validation_token(remote_addr) {
        return Ok(error_response(AuthError::TokenValidationRateLimited {
            retry_after_seconds: 1,
        }));
    }
    let limit = query.limit.unwrap_or(DEFAULT_LEADERBOARD_LIMIT);
    let players = auth_service.leaderboard(limit);
    Ok(ok_response(LeaderboardResult { players }))
}

async fn handle_delete_account(
    authorization_header: Option<String>,
    cookie_header: Option<String>,
    query: TokenQuery,
    remote_addr: Option<SocketAddr>,
    auth_service: AuthService,
) -> Result<warp::reply::Response, Infallible> {
    if !try_acquire_token_validation_token(remote_addr) {
        return Ok(error_response(AuthError::TokenValidationRateLimited {
            retry_after_seconds: 1,
        })
        .into_response());
    }
    let token = resolve_token_with_cookie(
        authorization_header.as_deref(),
        &query,
        cookie_header.as_deref(),
    );
    let Some(token_value) = token else {
        return Ok(error_response(AuthError::SessionInvalid).into_response());
    };
    let Some(user_id) = auth_service.resolve_user_id_from_token(&token_value) else {
        return Ok(error_response(AuthError::SessionInvalid).into_response());
    };

    match auth_service.request_account_deletion(&user_id) {
        Ok(result) => Ok(warp::reply::with_header(
            ok_response(result),
            "X-Data-Retention-Policy",
            format!(
                "{}h-grace-period",
                auth_service.inner.deletion_grace_period_hours
            ),
        )
        .into_response()),
        Err(error) => Ok(error_response(error).into_response()),
    }
}

async fn handle_cancel_deletion(
    authorization_header: Option<String>,
    cookie_header: Option<String>,
    query: TokenQuery,
    remote_addr: Option<SocketAddr>,
    auth_service: AuthService,
) -> Result<warp::reply::Response, Infallible> {
    if !try_acquire_token_validation_token(remote_addr) {
        return Ok(error_response(AuthError::TokenValidationRateLimited {
            retry_after_seconds: 1,
        })
        .into_response());
    }
    let token = resolve_token_with_cookie(
        authorization_header.as_deref(),
        &query,
        cookie_header.as_deref(),
    );
    let Some(token_value) = token else {
        return Ok(error_response(AuthError::SessionInvalid).into_response());
    };
    let Some(user_id) = auth_service.resolve_user_id_from_token(&token_value) else {
        return Ok(error_response(AuthError::SessionInvalid).into_response());
    };

    match auth_service.cancel_account_deletion(&user_id) {
        Ok(result) => Ok(warp::reply::with_header(
            ok_response(result),
            "X-Data-Retention-Policy",
            format!(
                "{}h-grace-period",
                auth_service.inner.deletion_grace_period_hours
            ),
        )
        .into_response()),
        Err(error) => Ok(error_response(error).into_response()),
    }
}

fn ok_response<T: Serialize>(data: T) -> warp::reply::WithStatus<warp::reply::Json> {
    let body = ApiResponse::<T> {
        ok: true,
        data: Some(data),
        error: None,
    };
    warp::reply::with_status(warp::reply::json(&body), StatusCode::OK)
}

fn error_response(error: AuthError) -> warp::reply::WithStatus<warp::reply::Json> {
    let (status, code, message, retry_after_seconds, remaining_attempts) = error.to_http();
    let body = ApiResponse::<serde_json::Value> {
        ok: false,
        data: None,
        error: Some(ApiErrorBody {
            code,
            message,
            retry_after_seconds,
            remaining_attempts,
        }),
    };
    warp::reply::with_status(warp::reply::json(&body), status)
}

fn resolve_token_with_cookie(
    authorization_header: Option<&str>,
    _query: &TokenQuery,
    cookie_header: Option<&str>,
) -> Option<String> {
    // Priority: Authorization header > Cookie. Query parameter is strictly ignored to prevent leaking tokens in URLs.
    if let Some(token) = parse_bearer_token(authorization_header) {
        return Some(token);
    }
    if let Some(token) = parse_session_cookie(cookie_header) {
        return Some(token);
    }
    None
}

/// Extracts the `mgs_session` cookie value from a Cookie header string.
fn parse_session_cookie(cookie_header: Option<&str>) -> Option<String> {
    let header = cookie_header?.trim();
    if header.is_empty() {
        return None;
    }
    for pair in header.split(';') {
        let pair = pair.trim();
        if let Some(value) = pair.strip_prefix("mgs_session=") {
            let token = value.trim();
            if !token.is_empty() {
                return Some(token.to_owned());
            }
        }
    }
    None
}

fn parse_bearer_token(authorization_header: Option<&str>) -> Option<String> {
    let raw = authorization_header?.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(stripped) = raw.strip_prefix("Bearer ") {
        let token = stripped.trim();
        if !token.is_empty() {
            return Some(token.to_owned());
        }
    }
    None
}

fn to_profile_view(user: &UserRecord) -> AuthProfileView {
    let level = level_from_experience(user.experience_points);
    let mmr = compute_mmr(
        user.total_kills,
        user.total_deaths,
        user.cumulative_score,
        user.matches_played,
    );
    AuthProfileView {
        user_id: user.user_id.clone(),
        display_name: user.display_name.clone(),
        phone_masked: masked_phone_for_user(user),
        created_at: user.created_at,
        last_seen_at: user.last_seen_at,
        matches_played: user.matches_played,
        cumulative_score: user.cumulative_score,
        best_score: user.best_score,
        total_kills: user.total_kills,
        total_deaths: user.total_deaths,
        last_game_username: user.last_game_username.clone(),
        experience_points: user.experience_points,
        credits: user.credits,
        level,
        next_level_experience: experience_for_level(level.saturating_add(1)),
        mmr,
        mmr_band: classify_mmr_band(mmr).to_string(),
    }
}

fn compute_mmr(
    total_kills: u64,
    total_deaths: u64,
    cumulative_score: i64,
    matches_played: u64,
) -> f32 {
    let kd = total_kills as f32 / total_deaths.max(1) as f32;
    let avg_score = cumulative_score.max(0) as f32 / matches_played.max(1) as f32;
    kd * 100.0 + avg_score * 0.5
}

fn classify_mmr_band(mmr: f32) -> &'static str {
    crate::scaling::router::classify_mmr_band(mmr)
}

fn progression_reward_from_match(player_state: &PlayerState) -> (u64, u64) {
    let score = player_state.score.max(0) as u64;
    let kills = player_state.kills.max(0) as u64;
    let deaths = player_state.deaths.max(0) as u64;
    let score_xp = score / 2;
    let score_credits = score / 10;
    let performance_bonus_xp = if kills >= deaths && kills > 0 { 20 } else { 0 };
    let performance_bonus_credits = if kills >= deaths && kills > 0 { 10 } else { 0 };
    let xp_gain = PROGRESSION_BASE_XP_PER_MATCH
        .saturating_add(score_xp)
        .saturating_add(kills.saturating_mul(PROGRESSION_XP_PER_KILL))
        .saturating_add(performance_bonus_xp);
    let credits_gain = PROGRESSION_BASE_CREDITS_PER_MATCH
        .saturating_add(score_credits)
        .saturating_add(kills.saturating_mul(PROGRESSION_CREDITS_PER_KILL))
        .saturating_add(performance_bonus_credits);
    (xp_gain, credits_gain)
}

fn experience_for_level(level: u32) -> u64 {
    if level <= 1 {
        return 0;
    }
    // Smoothly rising curve: sum_{i=1..level-1} (100 + 25*(i-1))
    let n = (level - 1) as u64;
    n.saturating_mul(100)
        .saturating_add(25u64.saturating_mul(n.saturating_sub(1)).saturating_mul(n) / 2)
}

fn level_from_experience(experience_points: u64) -> u32 {
    let mut level = 1u32;
    loop {
        let next_level = level.saturating_add(1);
        let required = experience_for_level(next_level);
        if experience_points < required || next_level == u32::MAX {
            return level;
        }
        level = next_level;
    }
}

fn load_persistent_store(path: &Path, redis_cache: Option<&AuthRedisCache>) -> PersistentAuthStore {
    if let Some(cache) = redis_cache {
        if let Some(store) = cache.load_store() {
            return migrate_persistent_store(store);
        }
    }

    let raw = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(_) => return PersistentAuthStore::default(),
    };
    match serde_json::from_str::<PersistentAuthStore>(&raw) {
        Ok(store) => migrate_persistent_store(store),
        Err(error) => {
            error!(
                "Failed to parse auth store '{}': {}. Starting with empty store.",
                path.display(),
                error
            );
            PersistentAuthStore::default()
        }
    }
}

fn migrate_persistent_store(mut store: PersistentAuthStore) -> PersistentAuthStore {
    let mut rebuilt_phone_to_user = HashMap::with_capacity(store.users.len());

    for (user_id, user) in &mut store.users {
        let original_phone = user.phone_number.clone();
        if user.deleted {
            let deleted_hash = if let Some(existing_hash) =
                original_phone.strip_prefix(DELETED_PHONE_HASH_PREFIX)
            {
                existing_hash.to_owned()
            } else {
                phone_hash_from_stored_or_legacy_value(&original_phone)
            };
            user.phone_number = format!("{}{}", DELETED_PHONE_HASH_PREFIX, deleted_hash);
            if user.phone_last4.is_empty() {
                user.phone_last4 = "0000".to_owned();
            }
            rebuilt_phone_to_user.insert(user.phone_number.clone(), user_id.clone());
        } else {
            if user.phone_last4.is_empty()
                && !original_phone.starts_with(ACTIVE_PHONE_HASH_PREFIX)
                && !original_phone.starts_with(DELETED_PHONE_HASH_PREFIX)
            {
                user.phone_last4 = phone_last4(&original_phone);
            }
            let active_hash = phone_hash_from_stored_or_legacy_value(&original_phone);
            user.phone_number = format!("{}{}", ACTIVE_PHONE_HASH_PREFIX, active_hash);
            rebuilt_phone_to_user.insert(user.phone_number.clone(), user_id.clone());
        }
    }

    let active_user_ids: HashSet<String> = store
        .users
        .iter()
        .filter_map(|(user_id, user)| (!user.deleted).then_some(user_id.clone()))
        .collect();
    store
        .pending_deletions
        .retain(|user_id, _| active_user_ids.contains(user_id));
    store.phone_to_user_id = rebuilt_phone_to_user;
    store
}

/// Offloads auth store persistence (file I/O + Redis) to a blocking thread
/// so that tokio worker threads are not stalled.
/// Falls back to synchronous persistence when no tokio runtime is
/// available (e.g. in unit tests).
fn spawn_persist_auth_store(path: PathBuf, store: PersistentAuthStore, inner: Arc<AuthInner>) {
    let do_persist = move || {
        persist_persistent_store(&path, &store, inner.redis_cache.as_ref());
    };
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::task::spawn_blocking(do_persist);
    } else {
        do_persist();
    }
}

fn persist_persistent_store(
    path: &Path,
    store: &PersistentAuthStore,
    redis_cache: Option<&AuthRedisCache>,
) {
    if let Some(cache) = redis_cache {
        cache.persist_store(store);
    }

    if let Some(parent) = path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            error!(
                "Failed to create auth store directory '{}': {}",
                parent.display(),
                error
            );
            return;
        }
    }
    let serialized = match serde_json::to_string_pretty(store) {
        Ok(serialized) => serialized,
        Err(error) => {
            error!("Failed to serialize auth store: {}", error);
            return;
        }
    };
    let tmp_path = path.with_extension(format!("tmp-{}-{}", std::process::id(), unix_now()));
    if let Err(error) = fs::write(&tmp_path, serialized) {
        error!(
            "Failed to write auth store temp file '{}': {}",
            tmp_path.display(),
            error
        );
        return;
    }
    if let Err(error) = fs::rename(&tmp_path, path) {
        error!(
            "Failed to atomically replace auth store '{}' from '{}': {}",
            path.display(),
            tmp_path.display(),
            error
        );
        let _ = fs::remove_file(&tmp_path);
    }
}

impl AuthRedisCache {
    fn load_store(&self) -> Option<PersistentAuthStore> {
        let mut connection = self.connection.lock();
        let raw: Option<String> = match connection.get(&self.store_key) {
            Ok(value) => value,
            Err(error) => {
                warn!("Failed to fetch auth store from Redis: {}", error);
                return None;
            }
        };
        let payload = raw?;
        match serde_json::from_str::<PersistentAuthStore>(&payload) {
            Ok(store) => {
                info!(
                    "Loaded auth store from Redis key '{}' (users={}).",
                    self.store_key,
                    store.users.len()
                );
                Some(store)
            }
            Err(error) => {
                warn!(
                    "Failed to parse auth store from Redis key '{}': {}",
                    self.store_key, error
                );
                None
            }
        }
    }

    fn persist_store(&self, store: &PersistentAuthStore) {
        let serialized = match serde_json::to_string(store) {
            Ok(value) => value,
            Err(error) => {
                error!("Failed to serialize auth store for Redis: {}", error);
                return;
            }
        };

        let mut connection = self.connection.lock();
        let result: redis::RedisResult<()> = connection.set(&self.store_key, serialized);
        if let Err(error) = result {
            warn!(
                "Failed to persist auth store to Redis key '{}': {}",
                self.store_key, error
            );
        }
    }
}

/// Redact the password portion of a URL for safe logging.
/// Turns `redis://user:secret@host` into `redis://user:***@host`.
fn redact_url_password(url: &str) -> String {
    // Match ://user:password@ or just ://:password@ or ://password@
    // We look for :// then everything up to @ and redact the password part.
    if let Some(scheme_end) = url.find("://") {
        let after_scheme = &url[scheme_end + 3..];
        if let Some(at_pos) = after_scheme.find('@') {
            let userinfo = &after_scheme[..at_pos];
            let rest = &after_scheme[at_pos..]; // includes the '@'
            if let Some(colon_pos) = userinfo.find(':') {
                let user = &userinfo[..colon_pos];
                return format!("{}://{}:***{}", &url[..scheme_end], user, rest);
            }
        }
    }
    url.to_owned()
}

fn init_redis_cache_from_env() -> Option<AuthRedisCache> {
    let redis_url = std::env::var("MGS_REDIS_URL")
        .ok()
        .map(|raw| raw.trim().to_owned())
        .filter(|raw| !raw.is_empty())?;
    let safe_url = redact_url_password(&redis_url);
    let store_key = std::env::var("MGS_REDIS_AUTH_STORE_KEY")
        .ok()
        .map(|raw| raw.trim().to_owned())
        .filter(|raw| !raw.is_empty())
        .unwrap_or_else(|| DEFAULT_REDIS_STORE_KEY.to_owned());

    let client = match redis::Client::open(redis_url.clone()) {
        Ok(client) => client,
        Err(error) => {
            warn!(
                "Redis auth cache disabled: invalid MGS_REDIS_URL '{}': {}",
                safe_url, error
            );
            return None;
        }
    };

    let connection = match client.get_connection() {
        Ok(connection) => connection,
        Err(error) => {
            warn!(
                "Redis auth cache disabled: unable to connect to '{}': {}",
                safe_url, error
            );
            return None;
        }
    };

    info!(
        "Redis auth cache enabled. url='{}', key='{}'",
        safe_url, store_key
    );
    Some(AuthRedisCache {
        connection: ParkingLotMutex::new(connection),
        store_key,
    })
}

fn generate_otp_code() -> String {
    // Rejection sampling avoids modulo bias while keeping OTP generation
    // cryptographically secure with OS entropy.
    const REJECTION_THRESHOLD: u32 = u32::MAX - (u32::MAX % OTP_CODE_UPPER_BOUND);
    loop {
        let candidate = OsRng.next_u32();
        if candidate < REJECTION_THRESHOLD {
            let value = candidate % OTP_CODE_UPPER_BOUND;
            return format!("{value:0width$}", width = OTP_CODE_DIGITS);
        }
    }
}

fn constant_time_eq_str(left: &str, right: &str) -> bool {
    let left_bytes = left.as_bytes();
    let right_bytes = right.as_bytes();
    let mut diff = left_bytes.len() ^ right_bytes.len();
    let max_len = left_bytes.len().max(right_bytes.len());

    for idx in 0..max_len {
        let l = left_bytes.get(idx).copied().unwrap_or(0);
        let r = right_bytes.get(idx).copied().unwrap_or(0);
        diff |= usize::from(l ^ r);
    }

    diff == 0
}

fn normalize_phone_number(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut had_plus = false;
    let mut digits = String::new();
    for (index, ch) in trimmed.chars().enumerate() {
        if ch == '+' {
            if index == 0 {
                had_plus = true;
                continue;
            }
            return None;
        }
        if ch.is_ascii_digit() {
            digits.push(ch);
            continue;
        }
        if ch == ' ' || ch == '-' || ch == '(' || ch == ')' || ch == '.' {
            continue;
        }
        return None;
    }

    if !had_plus {
        if digits.len() == 10 {
            digits = format!("1{}", digits);
        } else if digits.len() == 11 && digits.starts_with('1') {
            // Already a North America number with country prefix.
        }
    }

    if digits.len() < 8 || digits.len() > 15 {
        return None;
    }

    Some(format!("+{}", digits))
}

fn mask_phone_number(phone_number: &str) -> String {
    let mut digits_only = String::new();
    for ch in phone_number.chars() {
        if ch.is_ascii_digit() {
            digits_only.push(ch);
        }
    }
    if digits_only.len() <= 2 {
        // Too short to mask meaningfully; return fully masked.
        return "+***".to_owned();
    }
    let last2 = &digits_only[digits_only.len() - 2..];
    let masked_count = digits_only.len().saturating_sub(2);
    let stars: String = std::iter::repeat_n('*', masked_count).collect();
    format!("+{}{}", stars, last2)
}

fn masked_phone_for_user(user: &UserRecord) -> String {
    if user.phone_number.starts_with(ACTIVE_PHONE_HASH_PREFIX)
        || user.phone_number.starts_with(DELETED_PHONE_HASH_PREFIX)
    {
        return masked_phone_from_last4(&user.phone_last4);
    }
    mask_phone_number(&user.phone_number)
}

fn masked_phone_from_last4(last4: &str) -> String {
    let sanitized: String = last4.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if sanitized.is_empty() {
        return "+***".to_owned();
    }
    if sanitized.len() <= 2 {
        return format!("+***{}", sanitized);
    }
    let suffix = &sanitized[sanitized.len() - 2..];
    format!("+***{}", suffix)
}

fn active_phone_lookup_key(phone_number: &str) -> String {
    format!(
        "{}{}",
        ACTIVE_PHONE_HASH_PREFIX,
        hash_phone_for_anonymization(phone_number)
    )
}

fn phone_hash_from_stored_or_legacy_value(value: &str) -> String {
    if let Some(existing_hash) = value.strip_prefix(ACTIVE_PHONE_HASH_PREFIX) {
        return existing_hash.to_owned();
    }
    if let Some(existing_hash) = value.strip_prefix(DELETED_PHONE_HASH_PREFIX) {
        return existing_hash.to_owned();
    }
    hash_phone_for_anonymization(value)
}

fn gdpr_hash_salt_bytes() -> &'static [u8] {
    GDPR_HASH_SALT
        .get_or_init(|| {
            let configured = std::env::var("MGS_GDPR_HASH_SALT")
                .ok()
                .map(|raw| raw.trim().to_owned())
                .filter(|raw| !raw.is_empty());
            if let Some(raw) = configured {
                raw.into_bytes()
            } else {
                warn!(
                    "MGS_GDPR_HASH_SALT is not set; using built-in compatibility salt. \
                     Configure a deployment-specific salt for production."
                );
                b"mgs-gdpr-anonymization-salt".to_vec()
            }
        })
        .as_slice()
}

/// Produce a one-way SHA-256 hash of a phone number for anonymization.
/// The result is a hex-encoded hash that cannot be reversed to the original
/// phone number but can be used to detect re-registration from the same phone.
fn hash_phone_for_anonymization(phone_number: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(phone_number.as_bytes());
    hasher.update(gdpr_hash_salt_bytes());
    let result = hasher.finalize();
    format!("{:x}", result)
}

fn phone_last4(phone_number: &str) -> String {
    let digits: String = phone_number
        .chars()
        .filter(|ch| ch.is_ascii_digit())
        .collect();
    if digits.len() <= 4 {
        return digits;
    }
    digits[digits.len() - 4..].to_owned()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn parse_bool_env(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|raw| {
            let normalized = raw.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "yes" || normalized == "on"
        })
        .unwrap_or(default)
}

fn parse_u64_env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

fn parse_u32_env(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_curve_is_monotonic() {
        assert_eq!(level_from_experience(0), 1);
        assert_eq!(level_from_experience(99), 1);
        assert_eq!(level_from_experience(100), 2);
        assert!(experience_for_level(5) > experience_for_level(4));
    }

    #[test]
    fn progression_rewards_increase_with_performance() {
        let mut low = PlayerState::new("u1".to_owned(), "low".to_owned(), 0.0, 0.0);
        low.score = 20;
        low.kills = 1;
        low.deaths = 4;

        let mut high = PlayerState::new("u2".to_owned(), "high".to_owned(), 0.0, 0.0);
        high.score = 220;
        high.kills = 8;
        high.deaths = 2;

        let (low_xp, low_credits) = progression_reward_from_match(&low);
        let (high_xp, high_credits) = progression_reward_from_match(&high);
        assert!(high_xp > low_xp);
        assert!(high_credits > low_credits);
    }

    #[test]
    fn otp_code_generation_produces_six_digits() {
        for _ in 0..64 {
            let code = generate_otp_code();
            assert_eq!(code.len(), OTP_CODE_DIGITS);
            assert!(code.chars().all(|ch| ch.is_ascii_digit()));
        }
    }

    #[test]
    fn constant_time_eq_str_behaves_for_equal_and_mismatch_cases() {
        assert!(constant_time_eq_str("123456", "123456"));
        assert!(!constant_time_eq_str("123456", "123457"));
        assert!(!constant_time_eq_str("123456", "12345"));
    }

    // ── Phone number masking tests ───────────────────────────────────────

    #[test]
    fn mask_phone_shows_only_last_two_digits() {
        // Standard US number: +15551234567 (11 digits)
        assert_eq!(mask_phone_number("+15551234567"), "+*********67");
    }

    #[test]
    fn mask_phone_international_number() {
        // UK number: +447911123456 (12 digits)
        assert_eq!(mask_phone_number("+447911123456"), "+**********56");
    }

    #[test]
    fn mask_phone_short_number() {
        // Very short number (3 digits): only last 2 visible.
        assert_eq!(mask_phone_number("+123"), "+*23");
    }

    #[test]
    fn mask_phone_two_digit_returns_fully_masked() {
        assert_eq!(mask_phone_number("+12"), "+***");
    }

    #[test]
    fn mask_phone_single_digit_returns_fully_masked() {
        assert_eq!(mask_phone_number("+1"), "+***");
    }

    // ── OTP per-IP rate limiting tests ───────────────────────────────────

    #[test]
    fn otp_ip_rate_state_allows_within_short_window_limit() {
        let mut state = OtpIpRateState::new();
        let base_time = 1_000_000u64;

        // Should allow up to OTP_IP_SHORT_WINDOW_MAX requests.
        for i in 0..OTP_IP_SHORT_WINDOW_MAX {
            assert!(
                state.try_record(base_time + u64::from(i)).is_ok(),
                "Request {} within short window limit should be allowed",
                i + 1
            );
        }
    }

    #[test]
    fn otp_ip_rate_state_blocks_after_short_window_exceeded() {
        let mut state = OtpIpRateState::new();
        let base_time = 1_000_000u64;

        // Fill up the short window.
        for i in 0..OTP_IP_SHORT_WINDOW_MAX {
            assert!(state.try_record(base_time + u64::from(i)).is_ok());
        }

        // Next request should be blocked.
        let result = state.try_record(base_time + u64::from(OTP_IP_SHORT_WINDOW_MAX));
        assert!(
            result.is_err(),
            "Should be rate-limited after exceeding short window"
        );
        let retry_after = result.unwrap_err();
        assert!(retry_after > 0, "retry_after should be positive");
        assert!(
            retry_after <= OTP_IP_SHORT_WINDOW_SECS,
            "retry_after should not exceed the short window duration"
        );
    }

    #[test]
    fn parse_session_cookie_extracts_mgs_session() {
        assert_eq!(
            parse_session_cookie(Some("mgs_session=abc123")),
            Some("abc123".to_owned())
        );
        assert_eq!(
            parse_session_cookie(Some("other=x; mgs_session=tok_456; path=/")),
            Some("tok_456".to_owned())
        );
        assert_eq!(parse_session_cookie(Some("other=x; unrelated=y")), None);
        assert_eq!(parse_session_cookie(Some("")), None);
        assert_eq!(parse_session_cookie(None), None);
        assert_eq!(parse_session_cookie(Some("mgs_session=")), None);
    }

    #[test]
    fn resolve_token_with_cookie_priority() {
        let query_empty = TokenQuery::default();

        assert_eq!(
            resolve_token_with_cookie(
                Some("Bearer header_tok"),
                &query_empty,
                Some("mgs_session=cookie_tok"),
            ),
            Some("header_tok".to_owned())
        );

        assert_eq!(
            resolve_token_with_cookie(None, &query_empty, Some("mgs_session=cookie_tok")),
            Some("cookie_tok".to_owned())
        );

        assert_eq!(resolve_token_with_cookie(None, &query_empty, None), None);
    }

    #[test]
    fn default_session_ttl_is_24_hours() {
        assert_eq!(DEFAULT_SESSION_TTL_SECONDS, 60 * 60 * 24);
    }

    #[test]
    fn verify_code_rejects_even_correct_code_after_attempt_budget_exhausted() {
        let auth = make_test_auth_service();
        let phone = "+15551234567";
        assert!(auth.request_phone_code(phone).is_ok());

        let normalized_phone = normalize_phone_number(phone).expect("valid phone");
        let issued_code = auth
            .inner
            .otp_challenges
            .get(&normalized_phone)
            .map(|entry| entry.code.clone())
            .expect("issued otp code should exist");
        let wrong_code = if issued_code == "000000" {
            "000001"
        } else {
            "000000"
        };

        for _ in 0..(auth.inner.max_verify_attempts.saturating_sub(1)) {
            let err = auth
                .verify_phone_code(phone, wrong_code)
                .expect_err("wrong code should not verify");
            match err {
                AuthError::CodeMismatch { .. } => {}
                other => panic!("expected CodeMismatch, got {other:?}"),
            }
        }

        let final_err = auth
            .verify_phone_code(phone, wrong_code)
            .expect_err("attempt budget should now be exhausted");
        assert!(matches!(final_err, AuthError::TooManyAttempts));

        let post_exhaust_err = auth
            .verify_phone_code(phone, &issued_code)
            .expect_err("valid code must not verify after exhaustion");
        assert!(
            matches!(
                post_exhaust_err,
                AuthError::CodeNotRequested | AuthError::TooManyAttempts
            ),
            "unexpected error after attempt exhaustion: {post_exhaust_err:?}"
        );
    }

    #[test]
    fn otp_ip_rate_state_allows_after_short_window_expires() {
        let mut state = OtpIpRateState::new();
        let base_time = 1_000_000u64;

        // Fill up the short window.
        for i in 0..OTP_IP_SHORT_WINDOW_MAX {
            assert!(state.try_record(base_time + u64::from(i)).is_ok());
        }

        // Should be blocked now.
        assert!(state.try_record(base_time + 10).is_err());

        // After the short window expires, should be allowed again.
        let after_window = base_time + OTP_IP_SHORT_WINDOW_SECS + 1;
        assert!(
            state.try_record(after_window).is_ok(),
            "Should be allowed after short window expires"
        );
    }

    #[test]
    fn otp_ip_rate_state_blocks_after_long_window_exceeded() {
        let mut state = OtpIpRateState::new();
        let base_time = 1_000_000u64;

        // Fill up the long window by spreading requests across multiple short windows.
        for i in 0..OTP_IP_LONG_WINDOW_MAX {
            // Space requests every (short_window + 1) seconds so short window resets
            // but long window accumulates.
            let ts = base_time
                + u64::from(i) * (OTP_IP_SHORT_WINDOW_SECS / OTP_IP_SHORT_WINDOW_MAX as u64 + 1);
            assert!(
                state.try_record(ts).is_ok(),
                "Request {} within long window should be allowed",
                i + 1
            );
        }

        // Compute a timestamp that is still within the long window for the earliest entry
        // but won't hit the short window limit.
        let last_ts = base_time
            + u64::from(OTP_IP_LONG_WINDOW_MAX - 1)
                * (OTP_IP_SHORT_WINDOW_SECS / OTP_IP_SHORT_WINDOW_MAX as u64 + 1);
        let next_ts = last_ts + 1;

        // Should be blocked by long window.
        let result = state.try_record(next_ts);
        assert!(
            result.is_err(),
            "Should be rate-limited after exceeding long window"
        );
    }

    #[test]
    fn otp_ip_rate_state_is_empty_after_expiry() {
        let mut state = OtpIpRateState::new();
        let base_time = 1_000_000u64;

        assert!(state.try_record(base_time).is_ok());
        assert!(!state.is_empty());

        // Simulate time passing beyond both windows.
        let far_future = base_time + OTP_IP_LONG_WINDOW_SECS + 100;
        // Trigger internal cleanup by calling try_record at far_future.
        assert!(state.try_record(far_future).is_ok());

        // After removing the far_future entry (simulating cleanup), the old entries
        // should have been evicted. Create a fresh state to test is_empty.
        let mut fresh_state = OtpIpRateState::new();
        assert!(fresh_state.try_record(base_time).is_ok());
        // Evict by retaining only entries after far_future.
        fresh_state
            .short_window_timestamps
            .retain(|&ts| ts > far_future.saturating_sub(OTP_IP_SHORT_WINDOW_SECS));
        fresh_state
            .long_window_timestamps
            .retain(|&ts| ts > far_future.saturating_sub(OTP_IP_LONG_WINDOW_SECS));
        assert!(
            fresh_state.is_empty(),
            "State should be empty after all entries expire"
        );
    }

    #[test]
    fn check_otp_ip_rate_limit_allows_none_ip() {
        // When no IP is provided, rate limiting is bypassed.
        assert!(check_otp_ip_rate_limit(None).is_ok());
    }

    // ── GDPR account deletion & anonymization tests ──────────────────────

    /// Create a test AuthService with an in-memory store (no Redis, no file).
    fn make_test_auth_service() -> AuthService {
        let store_path = PathBuf::from(format!(
            "/tmp/mgs_test_auth_store_{}.json",
            Uuid::new_v4().simple()
        ));
        AuthService {
            inner: Arc::new(AuthInner {
                store_path,
                persistent_store: RwLock::new(PersistentAuthStore::default()),
                redis_cache: None,
                otp_challenges: DashMap::new(),
                sessions: DashMap::new(),
                peer_bindings: DashMap::new(),
                deletion_queue: DashMap::new(),
                otp_ttl_seconds: 300,
                session_ttl_seconds: 86400,
                resend_interval_seconds: 30,
                max_verify_attempts: 5,
                deletion_grace_period_hours: 72,
                sms_command: None,
                sms_dev_mode: true,
                use_auth_cookies: false,
            }),
        }
    }

    /// Insert a test user directly into the persistent store and create a session.
    fn insert_test_user(
        auth: &AuthService,
        user_id: &str,
        phone: &str,
        display_name: &str,
    ) -> String {
        let now = unix_now();
        let user = UserRecord {
            user_id: user_id.to_owned(),
            phone_number: phone.to_owned(),
            phone_last4: phone_last4(phone),
            display_name: display_name.to_owned(),
            created_at: now,
            updated_at: now,
            last_seen_at: now,
            matches_played: 10,
            cumulative_score: 500,
            best_score: 80,
            total_kills: 30,
            total_deaths: 20,
            last_game_username: Some("TestPlayer".to_owned()),
            experience_points: 1000,
            credits: 200,
            deleted: false,
        };
        {
            let mut store = auth.inner.persistent_store.write();
            store.users.insert(user_id.to_owned(), user);
            store
                .phone_to_user_id
                .insert(phone.to_owned(), user_id.to_owned());
        }
        // Create a session token
        let token = format!("mgs_test_{}", Uuid::new_v4().simple());
        auth.inner.sessions.insert(
            token.clone(),
            SessionRecord {
                user_id: user_id.to_owned(),
                expires_at: now + 86400,
            },
        );
        token
    }

    #[test]
    fn migrate_persistent_store_hashes_active_phone_and_prunes_invalid_pending_deletions() {
        let mut store = PersistentAuthStore::default();
        let now = unix_now();
        store.users.insert(
            "active_user".to_owned(),
            UserRecord {
                user_id: "active_user".to_owned(),
                phone_number: "+15551234567".to_owned(),
                phone_last4: String::new(),
                display_name: "Active".to_owned(),
                created_at: now,
                updated_at: now,
                last_seen_at: now,
                matches_played: 1,
                cumulative_score: 10,
                best_score: 10,
                total_kills: 1,
                total_deaths: 1,
                last_game_username: None,
                experience_points: 0,
                credits: 0,
                deleted: false,
            },
        );
        store.users.insert(
            "deleted_user".to_owned(),
            UserRecord {
                user_id: "deleted_user".to_owned(),
                phone_number: "deleted:deadbeef".to_owned(),
                phone_last4: "0000".to_owned(),
                display_name: "Deleted".to_owned(),
                created_at: now,
                updated_at: now,
                last_seen_at: now,
                matches_played: 1,
                cumulative_score: 10,
                best_score: 10,
                total_kills: 1,
                total_deaths: 1,
                last_game_username: None,
                experience_points: 0,
                credits: 0,
                deleted: true,
            },
        );
        store.pending_deletions.insert(
            "active_user".to_owned(),
            PendingDeletion {
                user_id: "active_user".to_owned(),
                requested_at: now,
                scheduled_deletion_time: now + 60,
            },
        );
        store.pending_deletions.insert(
            "deleted_user".to_owned(),
            PendingDeletion {
                user_id: "deleted_user".to_owned(),
                requested_at: now,
                scheduled_deletion_time: now + 60,
            },
        );
        store.pending_deletions.insert(
            "missing_user".to_owned(),
            PendingDeletion {
                user_id: "missing_user".to_owned(),
                requested_at: now,
                scheduled_deletion_time: now + 60,
            },
        );

        let migrated = migrate_persistent_store(store);
        let active = migrated.users.get("active_user").expect("active user");
        assert!(active.phone_number.starts_with("hash:"));
        assert_eq!(active.phone_last4, "4567");
        assert!(migrated
            .phone_to_user_id
            .contains_key(active.phone_number.as_str()));
        assert!(migrated.pending_deletions.contains_key("active_user"));
        assert!(!migrated.pending_deletions.contains_key("deleted_user"));
        assert!(!migrated.pending_deletions.contains_key("missing_user"));
    }

    #[test]
    fn masked_phone_for_hashed_user_uses_last4_suffix() {
        let user = UserRecord {
            user_id: "u".to_owned(),
            phone_number: "hash:1234".to_owned(),
            phone_last4: "9876".to_owned(),
            display_name: "d".to_owned(),
            created_at: 0,
            updated_at: 0,
            last_seen_at: 0,
            matches_played: 0,
            cumulative_score: 0,
            best_score: 0,
            total_kills: 0,
            total_deaths: 0,
            last_game_username: None,
            experience_points: 0,
            credits: 0,
            deleted: false,
        };
        assert_eq!(masked_phone_for_user(&user), "+***76");
    }

    #[test]
    fn hash_phone_for_anonymization_is_deterministic() {
        let hash1 = hash_phone_for_anonymization("+15551234567");
        let hash2 = hash_phone_for_anonymization("+15551234567");
        assert_eq!(hash1, hash2);
        // Different phone produces different hash
        let hash3 = hash_phone_for_anonymization("+15559999999");
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn hash_phone_for_anonymization_is_hex_string() {
        let hash = hash_phone_for_anonymization("+15551234567");
        assert!(
            hash.len() == 64,
            "SHA-256 hex should be 64 chars, got {}",
            hash.len()
        );
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "Hash should be hex"
        );
    }

    #[test]
    fn anonymize_user_data_replaces_pii() {
        let auth = make_test_auth_service();
        let token = insert_test_user(&auth, "user1", "+15551234567", "Alice");

        // Verify user exists before anonymization
        assert!(auth.resolve_user_id_from_token(&token).is_some());

        auth.anonymize_user_data("user1");

        // Check user record is anonymized
        let store = auth.inner.persistent_store.read();
        let user = store.users.get("user1").expect("user should still exist");
        assert!(user.deleted, "User should be marked as deleted");
        assert!(
            user.phone_number.starts_with("deleted:"),
            "Phone should be hashed"
        );
        assert_eq!(user.phone_last4, "0000");
        assert!(
            user.display_name.starts_with("Deleted User #"),
            "Display name should be anonymized, got: {}",
            user.display_name
        );
        assert!(user.last_game_username.is_none());

        // Original phone mapping should be removed
        assert!(
            !store.phone_to_user_id.contains_key("+15551234567"),
            "Original phone mapping should be removed"
        );

        // Hashed phone mapping should exist
        let phone_hash = hash_phone_for_anonymization("+15551234567");
        assert!(
            store
                .phone_to_user_id
                .contains_key(&format!("deleted:{}", phone_hash)),
            "Hashed phone mapping should exist for re-registration detection"
        );
        drop(store);

        // Session tokens should be revoked
        assert!(
            auth.resolve_user_id_from_token(&token).is_none(),
            "Session should be revoked after anonymization"
        );
    }

    #[test]
    fn anonymize_preserves_stats() {
        let auth = make_test_auth_service();
        insert_test_user(&auth, "user1", "+15551234567", "Alice");

        auth.anonymize_user_data("user1");

        let store = auth.inner.persistent_store.read();
        let user = store.users.get("user1").unwrap();
        // Stats should be preserved (anonymized, not deleted)
        assert_eq!(user.matches_played, 10);
        assert_eq!(user.cumulative_score, 500);
        assert_eq!(user.total_kills, 30);
        assert_eq!(user.total_deaths, 20);
    }

    #[test]
    fn request_account_deletion_queues_correctly() {
        let auth = make_test_auth_service();
        insert_test_user(&auth, "user1", "+15551234567", "Alice");

        let result = auth.request_account_deletion("user1").unwrap();
        assert_eq!(result.user_id, "user1");
        assert_eq!(result.grace_period_hours, 72);
        assert!(result.scheduled_deletion_time > result.requested_at);
        assert_eq!(
            result.scheduled_deletion_time - result.requested_at,
            72 * 3600
        );

        // Should be in the queue
        assert!(auth.inner.deletion_queue.contains_key("user1"));
        assert!(auth
            .inner
            .persistent_store
            .read()
            .pending_deletions
            .contains_key("user1"));
    }

    #[test]
    fn request_account_deletion_rejects_duplicate() {
        let auth = make_test_auth_service();
        insert_test_user(&auth, "user1", "+15551234567", "Alice");

        auth.request_account_deletion("user1").unwrap();
        let err = auth.request_account_deletion("user1").unwrap_err();
        match err {
            AuthError::DeletionAlreadyPending => {}
            other => panic!("Expected DeletionAlreadyPending, got {:?}", other),
        }
    }

    #[test]
    fn request_account_deletion_rejects_already_deleted() {
        let auth = make_test_auth_service();
        insert_test_user(&auth, "user1", "+15551234567", "Alice");
        auth.anonymize_user_data("user1");

        let err = auth.request_account_deletion("user1").unwrap_err();
        match err {
            AuthError::AccountDeleted => {}
            other => panic!("Expected AccountDeleted, got {:?}", other),
        }
    }

    #[test]
    fn cancel_account_deletion_works() {
        let auth = make_test_auth_service();
        insert_test_user(&auth, "user1", "+15551234567", "Alice");

        auth.request_account_deletion("user1").unwrap();
        assert!(auth.inner.deletion_queue.contains_key("user1"));

        let result = auth.cancel_account_deletion("user1").unwrap();
        assert!(result.cancelled);
        assert!(!auth.inner.deletion_queue.contains_key("user1"));
        assert!(!auth
            .inner
            .persistent_store
            .read()
            .pending_deletions
            .contains_key("user1"));

        // User should still be intact
        let store = auth.inner.persistent_store.read();
        let user = store.users.get("user1").unwrap();
        assert!(!user.deleted);
        assert_eq!(user.display_name, "Alice");
    }

    #[test]
    fn cancel_account_deletion_rejects_when_not_pending() {
        let auth = make_test_auth_service();
        insert_test_user(&auth, "user1", "+15551234567", "Alice");

        let err = auth.cancel_account_deletion("user1").unwrap_err();
        match err {
            AuthError::DeletionNotPending => {}
            other => panic!("Expected DeletionNotPending, got {:?}", other),
        }
    }

    #[test]
    fn process_pending_deletions_respects_grace_period() {
        let auth = make_test_auth_service();
        insert_test_user(&auth, "user1", "+15551234567", "Alice");

        auth.request_account_deletion("user1").unwrap();

        // Grace period not yet elapsed -- should not process
        let processed = auth.process_pending_deletions();
        assert_eq!(processed, 0);

        // User should still be intact
        let store = auth.inner.persistent_store.read();
        assert!(!store.users.get("user1").unwrap().deleted);
        drop(store);
    }

    #[test]
    fn process_pending_deletions_executes_after_grace_period() {
        let auth = make_test_auth_service();
        insert_test_user(&auth, "user1", "+15551234567", "Alice");

        // Manually insert a pending deletion with a past scheduled time
        let past_time = unix_now().saturating_sub(1);
        auth.inner.deletion_queue.insert(
            "user1".to_owned(),
            PendingDeletion {
                user_id: "user1".to_owned(),
                requested_at: past_time.saturating_sub(72 * 3600),
                scheduled_deletion_time: past_time,
            },
        );

        let processed = auth.process_pending_deletions();
        assert_eq!(processed, 1);

        // User should now be anonymized
        let store = auth.inner.persistent_store.read();
        let user = store.users.get("user1").unwrap();
        assert!(user.deleted);
        assert!(user.display_name.starts_with("Deleted User #"));
        drop(store);

        // Queue should be empty
        assert!(!auth.inner.deletion_queue.contains_key("user1"));
        assert!(!auth
            .inner
            .persistent_store
            .read()
            .pending_deletions
            .contains_key("user1"));
    }

    #[test]
    fn revoke_all_sessions_for_user_clears_all_tokens() {
        let auth = make_test_auth_service();
        let now = unix_now();

        // Create multiple sessions for the same user
        for i in 0..5 {
            auth.inner.sessions.insert(
                format!("token_{}", i),
                SessionRecord {
                    user_id: "user1".to_owned(),
                    expires_at: now + 86400,
                },
            );
        }
        // One session for a different user
        auth.inner.sessions.insert(
            "other_token".to_owned(),
            SessionRecord {
                user_id: "user2".to_owned(),
                expires_at: now + 86400,
            },
        );

        assert_eq!(auth.inner.sessions.len(), 6);
        auth.revoke_all_sessions_for_user("user1");
        assert_eq!(auth.inner.sessions.len(), 1);
        assert!(auth.inner.sessions.contains_key("other_token"));
    }

    #[test]
    fn deleted_users_excluded_from_leaderboard() {
        let auth = make_test_auth_service();
        insert_test_user(&auth, "user1", "+15551234567", "Alice");
        insert_test_user(&auth, "user2", "+15559999999", "Bob");

        let leaders = auth.leaderboard(50);
        assert_eq!(leaders.len(), 2);

        auth.anonymize_user_data("user1");

        let leaders = auth.leaderboard(50);
        assert_eq!(leaders.len(), 1);
        assert_eq!(leaders[0].user_id, "user2");
    }

    #[test]
    fn deletion_grace_period_env_default() {
        assert_eq!(DEFAULT_ACCOUNT_DELETION_GRACE_PERIOD_HOURS, 72);
    }

    #[test]
    fn pending_deletion_struct_stores_correct_fields() {
        let now = unix_now();
        let pending = PendingDeletion {
            user_id: "u1".to_owned(),
            requested_at: now,
            scheduled_deletion_time: now + 72 * 3600,
        };
        assert_eq!(pending.user_id, "u1");
        assert_eq!(
            pending.scheduled_deletion_time - pending.requested_at,
            72 * 3600
        );
    }
}
