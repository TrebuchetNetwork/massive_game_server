mod gdpr;
mod persistence;
mod phone_utils;
mod progression;
mod rate_limiting;
mod routes;
mod service;
mod types;

// ── Re-exports: public API must remain identical ──────────────────────────────

pub use routes::build_auth_routes;
pub use types::{
    AccountDeletionResult, AuthMeResult, AuthProfileView, AuthService, CancelDeletionResult,
    LeaderboardResult, RequestCodeResult, VerifyCodeResult,
};

// ── Constants (used by multiple submodules) ───────────────────────────────────

use dashmap::DashMap;
use parking_lot::Mutex as ParkingLotMutex;
use std::net::IpAddr;
use std::sync::{Arc, OnceLock};

use types::{OtpIpRateState, TokenValidationRateLimitConfig, TokenValidationRateLimiter};

const DEFAULT_OTP_TTL_SECONDS: u64 = 300;
// Reduced from 30 days to 24 hours to limit token exposure window.
// Override with MGS_AUTH_SESSION_TTL_SECONDS if longer sessions are needed.
const DEFAULT_SESSION_TTL_SECONDS: u64 = 60 * 60 * 24;
const DEFAULT_RESEND_INTERVAL_SECONDS: u64 = 30;
const DEFAULT_MAX_VERIFY_ATTEMPTS: u32 = 5;
const DEFAULT_LEADERBOARD_LIMIT: usize = 50;
const MAX_LEADERBOARD_LIMIT: usize = 100;
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
/// High-watermark for token validation limiter map; forces immediate cleanup
/// even if the periodic interval has not elapsed.
const TOKEN_VALIDATION_RATE_LIMITER_MAX_ENTRIES: usize = 10_000;
const TOKEN_VALIDATION_CLEANUP_INTERVAL_SECS: u64 = 30;
const OTP_IP_CLEANUP_MIN_ENTRIES: usize = 256;
const OTP_IP_CLEANUP_INTERVAL_SECS: u64 = 30;

static TOKEN_VALIDATION_RATE_LIMITERS: OnceLock<
    DashMap<String, Arc<ParkingLotMutex<TokenValidationRateLimiter>>>,
> = OnceLock::new();
static TOKEN_VALIDATION_RATE_LIMIT_CONFIG: OnceLock<TokenValidationRateLimitConfig> =
    OnceLock::new();
static OTP_IP_RATE_LIMITERS: OnceLock<DashMap<IpAddr, OtpIpRateState>> = OnceLock::new();
static GDPR_HASH_SALT: OnceLock<Vec<u8>> = OnceLock::new();

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::persistence::migrate_persistent_store;
    use super::phone_utils::*;
    use super::progression::*;
    use super::rate_limiting::*;
    use super::routes::*;
    use super::types::*;
    use super::*;
    use crate::core::types::PlayerState;
    use dashmap::DashMap;
    use parking_lot::RwLock;
    use std::path::PathBuf;
    use std::sync::Arc;
    use uuid::Uuid;

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

    #[test]
    fn token_validation_rate_limit_allows_none_remote_addr() {
        for _ in 0..256 {
            assert!(
                try_acquire_token_validation_token(None),
                "missing remote address should bypass token validation limiter"
            );
        }
    }

    // ── GDPR account deletion & anonymization tests ──────────────────────

    /// Create a test AuthService with an in-memory store (no Redis, no file).
    fn make_test_auth_service_with_cookie_mode_and_security(
        use_auth_cookies: bool,
        auth_cookie_secure: bool,
    ) -> AuthService {
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
                use_auth_cookies,
                auth_cookie_secure,
            }),
        }
    }

    fn make_test_auth_service_with_cookie_mode(use_auth_cookies: bool) -> AuthService {
        make_test_auth_service_with_cookie_mode_and_security(use_auth_cookies, false)
    }

    fn make_test_auth_service() -> AuthService {
        make_test_auth_service_with_cookie_mode(false)
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
            total_flag_captures: 4,
            top_streak: 6,
            kills_per_weapon: [6, 5, 12, 4, 3],
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
                total_flag_captures: 0,
                top_streak: 1,
                kills_per_weapon: [1, 0, 0, 0, 0],
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
                total_flag_captures: 0,
                top_streak: 0,
                kills_per_weapon: [0; 5],
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
            total_flag_captures: 0,
            top_streak: 0,
            kills_per_weapon: [0; 5],
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
    fn verify_phone_code_rejects_re_registration_for_deleted_phone() {
        let auth = make_test_auth_service();
        let phone = "+15551234567";
        insert_test_user(&auth, "user1", phone, "Alice");
        auth.anonymize_user_data("user1");

        auth.inner.otp_challenges.insert(
            phone.to_owned(),
            OtpChallenge {
                code: "123456".to_owned(),
                expires_at: unix_now() + 300,
                last_sent_at: unix_now(),
                attempts: 0,
            },
        );

        let err = auth.verify_phone_code(phone, "123456").unwrap_err();
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

    #[tokio::test(flavor = "current_thread")]
    async fn verify_code_cookie_mode_sets_cookie_and_omits_json_token() {
        let auth = make_test_auth_service_with_cookie_mode(true);
        let phone = "+15551230111";
        assert!(auth.request_phone_code(phone).is_ok());

        let normalized_phone = normalize_phone_number(phone).expect("valid phone");
        let issued_code = auth
            .inner
            .otp_challenges
            .get(&normalized_phone)
            .map(|entry| entry.code.clone())
            .expect("issued otp code should exist");

        let response = handle_verify_code(
            VerifyCodeBody {
                phone_number: phone.to_owned(),
                code: issued_code,
            },
            auth.clone(),
        )
        .await
        .expect("verify handler should not fail");

        let cookie_header = response
            .headers()
            .get("Set-Cookie")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        assert!(
            cookie_header.contains("mgs_session="),
            "expected Set-Cookie to include mgs_session"
        );
        assert!(
            cookie_header.contains("HttpOnly"),
            "expected cookie to be HttpOnly"
        );
        assert!(
            !cookie_header.contains("Secure"),
            "plain-http cookie mode should omit Secure unless TLS proxy mode is enabled"
        );

        let body_bytes = warp::hyper::body::to_bytes(response.into_body())
            .await
            .expect("response body bytes");
        let payload: serde_json::Value = serde_json::from_slice(&body_bytes).expect("json payload");
        assert_eq!(payload.get("ok").and_then(|v| v.as_bool()), Some(true));
        let data = payload.get("data").expect("data payload");
        assert!(
            data.get("token").is_none(),
            "cookie mode must not expose token in JSON payload"
        );
        assert!(
            data.get("token_expires_at").is_some(),
            "token expiry should still be returned"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn verify_code_cookie_mode_sets_secure_cookie_when_enabled() {
        let auth = make_test_auth_service_with_cookie_mode_and_security(true, true);
        let phone = "+15551230112";
        assert!(auth.request_phone_code(phone).is_ok());

        let normalized_phone = normalize_phone_number(phone).expect("valid phone");
        let issued_code = auth
            .inner
            .otp_challenges
            .get(&normalized_phone)
            .map(|entry| entry.code.clone())
            .expect("issued otp code should exist");

        let response = handle_verify_code(
            VerifyCodeBody {
                phone_number: phone.to_owned(),
                code: issued_code,
            },
            auth,
        )
        .await
        .expect("verify handler should not fail");

        let cookie_header = response
            .headers()
            .get("Set-Cookie")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        assert!(
            cookie_header.contains("Secure"),
            "TLS proxy cookie mode should emit Secure cookies"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn logout_cookie_mode_clears_cookie() {
        let auth = make_test_auth_service_with_cookie_mode(true);
        let phone = "+15551230113";
        assert!(auth.request_phone_code(phone).is_ok());

        let normalized_phone = normalize_phone_number(phone).expect("valid phone");
        let issued_code = auth
            .inner
            .otp_challenges
            .get(&normalized_phone)
            .map(|entry| entry.code.clone())
            .expect("issued otp code should exist");

        let verify_response = handle_verify_code(
            VerifyCodeBody {
                phone_number: phone.to_owned(),
                code: issued_code,
            },
            auth.clone(),
        )
        .await
        .expect("verify handler should not fail");

        let cookie_pair = verify_response
            .headers()
            .get("Set-Cookie")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(str::to_owned)
            .expect("session cookie pair");

        let logout_response =
            handle_auth_logout(None, Some(cookie_pair), TokenQuery::default(), None, auth)
                .await
                .expect("logout handler should not fail");

        let cleared_cookie = logout_response
            .headers()
            .get("Set-Cookie")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        assert!(
            cleared_cookie.contains("mgs_session=") && cleared_cookie.contains("Max-Age=0"),
            "logout should expire the session cookie, got {cleared_cookie}"
        );
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
