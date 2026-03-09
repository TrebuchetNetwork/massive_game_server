use super::persistence::{init_redis_cache, load_persistent_store, spawn_persist_auth_store};
use super::phone_utils::{
    active_phone_lookup_key, configure_gdpr_hash_salt, constant_time_eq_str, generate_otp_code,
    hash_phone_for_anonymization, mask_phone_number, normalize_phone_number, phone_last4, unix_now,
};
use super::progression::{progression_reward_from_match, to_profile_view};
use super::rate_limiting::configure_token_validation_rate_limit;
use super::types::{
    AuthError, AuthInner, AuthProfileView, AuthService, OtpChallenge, RequestCodeResult,
    SessionRecord, UserRecord, VerifyCodeResult,
};
use super::{DEFAULT_ACCOUNT_DELETION_GRACE_PERIOD_HOURS, DEFAULT_MAX_VERIFY_ATTEMPTS};
use super::{
    DEFAULT_OTP_TTL_SECONDS, DEFAULT_REDIS_STORE_KEY, DEFAULT_RESEND_INTERVAL_SECONDS,
    DEFAULT_SESSION_TTL_SECONDS, DEFAULT_TOKEN_VALIDATION_RATE_LIMIT_BURST,
    DEFAULT_TOKEN_VALIDATION_RATE_LIMIT_PER_SEC, DELETED_PHONE_HASH_PREFIX, MAX_LEADERBOARD_LIMIT,
};
use crate::core::types::PlayerState;
use crate::operational::config::env_registry::{load_app_env_config, AuthEnv};
use crate::operational::monitoring::metrics;
use dashmap::DashMap;
use parking_lot::RwLock;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};
use uuid::Uuid;

fn default_auth_env() -> AuthEnv {
    AuthEnv {
        store_path: "data/auth_store.json".to_owned(),
        otp_ttl_seconds: DEFAULT_OTP_TTL_SECONDS,
        session_ttl_seconds: DEFAULT_SESSION_TTL_SECONDS,
        resend_interval_seconds: DEFAULT_RESEND_INTERVAL_SECONDS,
        max_verify_attempts: DEFAULT_MAX_VERIFY_ATTEMPTS,
        token_validation_rate_limit_per_sec: DEFAULT_TOKEN_VALIDATION_RATE_LIMIT_PER_SEC,
        token_validation_rate_limit_burst: DEFAULT_TOKEN_VALIDATION_RATE_LIMIT_BURST,
        sms_command: None,
        sms_dev_mode: false,
        use_auth_cookies: false,
        deletion_grace_period_hours: DEFAULT_ACCOUNT_DELETION_GRACE_PERIOD_HOURS,
        redis_url: None,
        redis_store_key: DEFAULT_REDIS_STORE_KEY.to_owned(),
        gdpr_hash_salt: None,
    }
}

impl AuthService {
    pub fn new_from_env() -> Self {
        match load_app_env_config() {
            Ok(app_env) => Self::new_from_env_config_with_cookie_security(
                &app_env.auth,
                app_env.ws_security.behind_tls_proxy,
            ),
            Err(err) => {
                warn!(
                    "Falling back to default auth env config due to invalid environment: {}",
                    err
                );
                let fallback = default_auth_env();
                Self::new_from_env_config(&fallback)
            }
        }
    }

    pub fn new_from_env_config(env: &AuthEnv) -> Self {
        Self::new_from_env_config_with_cookie_security(env, false)
    }

    pub fn new_from_env_config_with_cookie_security(
        env: &AuthEnv,
        auth_cookie_secure: bool,
    ) -> Self {
        let store_path = PathBuf::from(env.store_path.as_str());
        let otp_ttl_seconds = env.otp_ttl_seconds.max(60);
        let allow_short_session_ttl_for_tests = std::env::var("MGS_TEST_ALLOW_SHORT_SESSION_TTL")
            .ok()
            .map(|raw| {
                let normalized = raw.trim().to_ascii_lowercase();
                matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
            })
            .unwrap_or(false);
        let session_ttl_seconds = if allow_short_session_ttl_for_tests {
            env.session_ttl_seconds.max(1)
        } else {
            env.session_ttl_seconds.max(300)
        };
        let resend_interval_seconds = env.resend_interval_seconds.max(5);
        let max_verify_attempts = env.max_verify_attempts.max(1);
        let sms_command = env
            .sms_command
            .as_ref()
            .map(|raw| raw.trim().to_owned())
            .filter(|raw| !raw.is_empty());
        let sms_dev_mode = env.sms_dev_mode;
        let use_auth_cookies = env.use_auth_cookies;
        let deletion_grace_period_hours = env.deletion_grace_period_hours.max(1);
        configure_token_validation_rate_limit(
            env.token_validation_rate_limit_per_sec,
            env.token_validation_rate_limit_burst,
        );
        configure_gdpr_hash_salt(env.gdpr_hash_salt.as_deref());
        let redis_cache =
            init_redis_cache(env.redis_url.as_deref(), Some(env.redis_store_key.as_str()));

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
            if auth_cookie_secure {
                info!(
                    "Cookie-based auth enabled: verify-code will set HttpOnly Secure session cookie."
                );
            } else {
                warn!(
                    "Cookie-based auth enabled without TLS proxy: session cookie will omit Secure. Use this only on localhost or non-production HTTP environments."
                );
            }
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
                auth_cookie_secure,
            }),
        }
    }

    pub(super) fn request_phone_code(
        &self,
        phone_number_raw: &str,
    ) -> Result<RequestCodeResult, AuthError> {
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

    pub(super) fn verify_phone_code(
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
        let deleted_phone_lookup_key = format!(
            "{}{}",
            DELETED_PHONE_HASH_PREFIX,
            hash_phone_for_anonymization(&phone_number)
        );
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
        } else if persistent_guard
            .phone_to_user_id
            .contains_key(&deleted_phone_lookup_key)
        {
            metrics::record_auth_attempt("verify_code", "account_deleted");
            return Err(AuthError::AccountDeleted);
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
                total_flag_captures: 0,
                top_streak: 0,
                kills_per_weapon: [0; 5],
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
            if user.deleted {
                metrics::record_auth_attempt("verify_code", "account_deleted");
                return Err(AuthError::AccountDeleted);
            }
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
            token: Some(session_token),
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

    /// Returns true when auth cookies should carry the Secure attribute.
    pub fn auth_cookie_secure(&self) -> bool {
        self.inner.auth_cookie_secure
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

    pub fn weapon_kills_by_user_id(&self, user_id: &str) -> Option<[u64; 5]> {
        let persistent_guard = self.inner.persistent_store.read();
        persistent_guard
            .users
            .get(user_id)
            .map(|user| user.kills_per_weapon)
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
            user.total_flag_captures = user
                .total_flag_captures
                .saturating_add(player_state.flag_captures.max(0) as u64);
            user.top_streak = user.top_streak.max(player_state.peak_streak as u64);
            for (idx, kills) in player_state.kills_per_weapon.iter().enumerate() {
                if let Some(total_slot) = user.kills_per_weapon.get_mut(idx) {
                    let session_baseline = player_state.career_kills_per_weapon[idx];
                    *total_slot = (*total_slot)
                        .max(session_baseline)
                        .saturating_add((*kills).max(0) as u64);
                }
            }
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

        let bounded_limit = limit.clamp(1, MAX_LEADERBOARD_LIMIT);
        if profiles.len() > bounded_limit {
            profiles.truncate(bounded_limit);
        }
        profiles
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
