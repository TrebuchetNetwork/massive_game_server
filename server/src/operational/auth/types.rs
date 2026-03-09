use crate::network::rate_limiter::TokenBucket;
use parking_lot::{Mutex as ParkingLotMutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;
use warp::http::StatusCode;

use dashmap::DashMap;

// ── Internal struct definitions ───────────────────────────────────────────────

#[derive(Clone)]
pub struct AuthService {
    pub(super) inner: std::sync::Arc<AuthInner>,
}

pub(super) struct AuthInner {
    pub(super) store_path: PathBuf,
    pub(super) persistent_store: RwLock<PersistentAuthStore>,
    pub(super) redis_cache: Option<AuthRedisCache>,
    pub(super) otp_challenges: DashMap<String, OtpChallenge>,
    pub(super) sessions: DashMap<String, SessionRecord>,
    pub(super) peer_bindings: DashMap<String, String>,
    /// Queued account deletions: user_id -> PendingDeletion
    pub(super) deletion_queue: DashMap<String, PendingDeletion>,
    pub(super) otp_ttl_seconds: u64,
    pub(super) session_ttl_seconds: u64,
    pub(super) resend_interval_seconds: u64,
    pub(super) max_verify_attempts: u32,
    /// Grace period (in hours) before a queued deletion is executed.
    pub(super) deletion_grace_period_hours: u64,
    pub(super) sms_command: Option<String>,
    pub(super) sms_dev_mode: bool,
    /// When true, the verify-code endpoint sets the session token as an
    /// HttpOnly, SameSite=Strict cookie instead of (in addition to) returning
    /// it in the JSON body. The WebSocket upgrade path and authenticated
    /// endpoints will then also accept the cookie as a token source. Enable via
    /// MGS_AUTH_USE_COOKIES=true.
    pub(super) use_auth_cookies: bool,
    /// When true, emitted auth cookies carry the Secure attribute.
    pub(super) auth_cookie_secure: bool,
}

/// A pending account deletion that is within the grace period.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PendingDeletion {
    pub(super) user_id: String,
    pub(super) requested_at: u64,
    pub(super) scheduled_deletion_time: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct PersistentAuthStore {
    pub(super) users: HashMap<String, UserRecord>,
    pub(super) phone_to_user_id: HashMap<String, String>,
    #[serde(default)]
    pub(super) pending_deletions: HashMap<String, PendingDeletion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct UserRecord {
    pub(super) user_id: String,
    pub(super) phone_number: String,
    pub(super) phone_last4: String,
    pub(super) display_name: String,
    pub(super) created_at: u64,
    pub(super) updated_at: u64,
    pub(super) last_seen_at: u64,
    pub(super) matches_played: u64,
    pub(super) cumulative_score: i64,
    pub(super) best_score: i32,
    pub(super) total_kills: u64,
    pub(super) total_deaths: u64,
    #[serde(default)]
    pub(super) total_flag_captures: u64,
    #[serde(default)]
    pub(super) top_streak: u64,
    #[serde(default)]
    pub(super) kills_per_weapon: [u64; 5],
    pub(super) last_game_username: Option<String>,
    #[serde(default)]
    pub(super) experience_points: u64,
    #[serde(default)]
    pub(super) credits: u64,
    /// True if this account has been anonymized/deleted per GDPR request.
    #[serde(default)]
    pub(super) deleted: bool,
}

#[derive(Debug, Clone)]
pub(super) struct OtpChallenge {
    pub(super) code: String,
    pub(super) expires_at: u64,
    pub(super) last_sent_at: u64,
    pub(super) attempts: u32,
}

#[derive(Debug, Clone)]
pub(super) struct SessionRecord {
    pub(super) user_id: String,
    pub(super) expires_at: u64,
}

pub(super) struct AuthRedisCache {
    pub(super) client: redis::Client,
    pub(super) connection: ParkingLotMutex<Option<redis::Connection>>,
    pub(super) store_key: String,
}

#[derive(Debug)]
pub(super) enum AuthError {
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

// ── Public view / response types ──────────────────────────────────────────────

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
    pub total_flag_captures: u64,
    pub top_streak: u64,
    pub favorite_weapon: String,
    pub lifetime_kd: f32,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
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

// ── Request body types (used by routes) ───────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RequestCodeBody {
    pub(super) phone_number: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct VerifyCodeBody {
    pub(super) phone_number: String,
    pub(super) code: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(super) struct TokenQuery {}

#[derive(Debug, Clone, Deserialize, Default)]
pub(super) struct LeaderboardQuery {
    pub(super) limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ApiErrorBody {
    pub(super) code: &'static str,
    pub(super) message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) retry_after_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) remaining_attempts: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ApiResponse<T>
where
    T: Serialize,
{
    pub(super) ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) error: Option<ApiErrorBody>,
}

// ── AuthError -> HTTP mapping ─────────────────────────────────────────────────

impl AuthError {
    pub(super) fn to_http(&self) -> (StatusCode, &'static str, String, Option<u64>, Option<u32>) {
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

// ── Rate limiting types ───────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub(super) struct TokenValidationRateLimitConfig {
    pub(super) per_sec: u32,
    pub(super) burst: u32,
}

#[derive(Debug)]
pub(super) struct TokenValidationRateLimiter {
    bucket: TokenBucket,
    pub(super) last_seen_at: Instant,
}

impl TokenValidationRateLimiter {
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

// ── OTP IP rate state ─────────────────────────────────────────────────────────

/// Tracks per-IP OTP request timestamps using two sliding windows (short and long).
#[derive(Debug, Clone)]
pub(super) struct OtpIpRateState {
    /// Timestamps of recent OTP requests within the short window.
    pub(super) short_window_timestamps: Vec<u64>,
    /// Timestamps of recent OTP requests within the long window.
    pub(super) long_window_timestamps: Vec<u64>,
}

impl OtpIpRateState {
    pub(super) fn new() -> Self {
        Self {
            short_window_timestamps: Vec::new(),
            long_window_timestamps: Vec::new(),
        }
    }

    /// Try to record a new OTP request. Returns Ok(()) if allowed, or
    /// Err(retry_after_seconds) if the IP has exceeded its quota.
    pub(super) fn try_record(&mut self, now: u64) -> Result<(), u64> {
        use super::{
            OTP_IP_LONG_WINDOW_MAX, OTP_IP_LONG_WINDOW_SECS, OTP_IP_SHORT_WINDOW_MAX,
            OTP_IP_SHORT_WINDOW_SECS,
        };

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
    pub(super) fn is_empty(&self) -> bool {
        self.short_window_timestamps.is_empty() && self.long_window_timestamps.is_empty()
    }
}
