use anyhow::{anyhow, Result};

#[derive(Debug, Clone)]
pub struct AppEnvConfig {
    pub map_path: Option<String>,
    pub diagnostics: DiagnosticsEnv,
    pub live_replay_enabled: bool,
    pub quic_primary_only: bool,
    pub cdn_origin: Option<String>,
    pub arena_worker: ArenaWorkerEnv,
    pub auth: AuthEnv,
    pub admin_auth: AdminAuthEnv,
    pub backup: BackupEnv,
    pub network_bind: NetworkBindEnv,
    pub shutdown_drain_timeout_secs: u64,
    pub allowed_cors_origins: Vec<String>,
    pub ws_security: WsSecurityEnv,
}

#[derive(Debug, Clone)]
pub struct DiagnosticsEnv {
    pub enabled: bool,
    pub frame_watchdog_check_ms: u64,
    pub frame_watchdog_stale_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ArenaWorkerEnv {
    pub enabled: bool,
    pub interval_ms: u64,
    pub max_ticks: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct AuthEnv {
    pub store_path: String,
    pub otp_ttl_seconds: u64,
    pub session_ttl_seconds: u64,
    pub resend_interval_seconds: u64,
    pub max_verify_attempts: u32,
    pub token_validation_rate_limit_per_sec: u32,
    pub token_validation_rate_limit_burst: u32,
    pub sms_command: Option<String>,
    pub sms_dev_mode: bool,
    pub use_auth_cookies: bool,
    pub deletion_grace_period_hours: u64,
    pub redis_url: Option<String>,
    pub redis_store_key: String,
    pub gdpr_hash_salt: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AdminAuthEnv {
    pub bearer_token: Option<String>,
    pub ip_allowlist: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BackupEnv {
    pub enabled: bool,
    pub interval_seconds: u64,
    pub output_dir: String,
    pub retention_count: usize,
    pub auth_store_path: String,
    pub feature_flags_store_path: String,
    pub arena_store_path: String,
    pub live_replay_dispute_store_path: String,
    pub extra_paths: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct NetworkBindEnv {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone)]
pub struct WsSecurityEnv {
    pub behind_tls_proxy: bool,
    pub dev_mode: bool,
    pub require_auth_env: bool,
    pub allow_insecure_ws_proxy_proto: bool,
    pub allowed_origins: Vec<String>,
    pub trusted_proxy_cidrs: Vec<String>,
    pub max_concurrent_connections: Option<u64>,
}

pub fn load_app_env_config() -> Result<AppEnvConfig> {
    let mut errors = Vec::new();

    let map_path = get_optional_trimmed("MGS_MAP_PATH");

    let diagnostics_enabled =
        parse_bool_with_default("MGS_DIAGNOSTICS_ENABLED", false, &mut errors);
    let frame_watchdog_check_ms =
        parse_u64_with_default("MGS_FRAME_WATCHDOG_CHECK_MS", 200, &mut errors).max(50);
    let frame_watchdog_stale_ms =
        parse_u64_with_default("MGS_FRAME_WATCHDOG_STALE_MS", 200, &mut errors).max(100);

    let live_replay_enabled =
        parse_bool_with_default("MGS_LIVE_REPLAY_ENABLED", false, &mut errors);
    let quic_primary = parse_bool_with_default("MGS_QUIC_PRIMARY", false, &mut errors);
    let quic_primary_only_flag =
        parse_bool_with_default("MGS_QUIC_PRIMARY_ONLY", false, &mut errors);
    let quic_primary_only = quic_primary && quic_primary_only_flag;

    let cdn_origin = get_optional_trimmed("MGS_CDN_ORIGIN");

    let arena_worker_enabled =
        parse_bool_with_default("MGS_ARENA_WORKER_ENABLED", false, &mut errors);
    let arena_worker_interval_ms =
        parse_u64_with_default("MGS_ARENA_WORKER_INTERVAL_MS", 1000, &mut errors);
    if arena_worker_enabled && arena_worker_interval_ms < 100 {
        errors.push(format!(
            "MGS_ARENA_WORKER_INTERVAL_MS must be >= 100 when worker is enabled (got {})",
            arena_worker_interval_ms
        ));
    }
    let arena_worker_max_ticks = parse_optional_u32("MGS_ARENA_WORKER_MAX_TICKS", &mut errors);
    if let Some(max_ticks) = arena_worker_max_ticks {
        if max_ticks == 0 {
            errors.push("MGS_ARENA_WORKER_MAX_TICKS must be > 0 when set".to_owned());
        }
    }
    let auth_store_path = get_optional_trimmed("MGS_AUTH_STORE_PATH")
        .unwrap_or_else(|| "data/auth_store.json".to_owned());
    let auth_otp_ttl_seconds =
        parse_u64_with_default("MGS_AUTH_OTP_TTL_SECONDS", 300, &mut errors).max(60);
    let auth_session_ttl_seconds =
        parse_u64_with_default("MGS_AUTH_SESSION_TTL_SECONDS", 60 * 60 * 24, &mut errors).max(300);
    let auth_resend_interval_seconds =
        parse_u64_with_default("MGS_AUTH_RESEND_INTERVAL_SECONDS", 30, &mut errors).max(5);
    let auth_max_verify_attempts =
        parse_u32_with_default("MGS_AUTH_MAX_VERIFY_ATTEMPTS", 5, &mut errors).max(1);
    let auth_token_validation_rate_limit_per_sec =
        parse_u32_with_default("MGS_AUTH_TOKEN_RATE_LIMIT_PER_SEC", 24, &mut errors).max(1);
    let auth_token_validation_rate_limit_burst =
        parse_u32_with_default("MGS_AUTH_TOKEN_RATE_LIMIT_BURST", 48, &mut errors).max(1);
    let auth_sms_command = get_optional_trimmed("MGS_SMS_COMMAND");
    let auth_sms_dev_mode = parse_bool_with_default("MGS_SMS_DEV_MODE", false, &mut errors);
    let auth_use_auth_cookies = parse_bool_with_default("MGS_AUTH_USE_COOKIES", false, &mut errors);
    let auth_deletion_grace_period_hours =
        parse_u64_with_default("MGS_ACCOUNT_DELETION_GRACE_PERIOD_HOURS", 72, &mut errors).max(1);
    let auth_redis_url = get_optional_trimmed("MGS_REDIS_URL");
    let auth_redis_store_key = get_optional_trimmed("MGS_REDIS_AUTH_STORE_KEY")
        .unwrap_or_else(|| "mgs:auth:persistent_store".to_owned());
    let auth_gdpr_hash_salt = get_optional_trimmed("MGS_GDPR_HASH_SALT");

    let admin_bearer_token = get_optional_trimmed("MGS_ADMIN_BEARER_TOKEN")
        .or_else(|| get_optional_trimmed("MGS_ADMIN_TOKEN"));
    let mut admin_ip_allowlist = parse_list("MGS_ADMIN_IP_ALLOWLIST");
    admin_ip_allowlist.extend(parse_list("MGS_ADMIN_ALLOWED_IPS"));
    let backup_enabled = parse_bool_with_default("MGS_BACKUP_ENABLED", false, &mut errors);
    let backup_interval_seconds =
        parse_u64_with_default("MGS_BACKUP_INTERVAL_SECONDS", 3600, &mut errors).max(1);
    let backup_output_dir =
        get_optional_trimmed("MGS_BACKUP_DIR").unwrap_or_else(|| "data/backups".to_owned());
    let backup_retention_count =
        parse_usize_with_default("MGS_BACKUP_RETENTION_COUNT", 48, &mut errors).max(1);
    let backup_auth_store_path = get_optional_trimmed("MGS_AUTH_STORE_PATH")
        .unwrap_or_else(|| "data/auth_store.json".to_owned());
    let backup_feature_flags_store_path = get_optional_trimmed("MGS_FEATURE_FLAGS_STORE_PATH")
        .unwrap_or_else(|| "data/feature_flags.json".to_owned());
    let backup_arena_store_path = get_optional_trimmed("MGS_ARENA_STORE_PATH")
        .unwrap_or_else(|| "data/arena_store.json".to_owned());
    let backup_live_replay_dispute_store_path =
        get_optional_trimmed("MGS_LIVE_REPLAY_DISPUTE_STORE_PATH")
            .unwrap_or_else(|| "data/live_replay_disputes.jsonl".to_owned());
    let backup_extra_paths = parse_list("MGS_BACKUP_EXTRA_PATHS");

    let host = get_optional_trimmed("MGS_HOST").unwrap_or_else(|| "0.0.0.0".to_owned());
    let port = parse_u16_with_default("MGS_PORT", 8080, &mut errors);
    let shutdown_drain_timeout_secs =
        parse_u64_with_default("MGS_SHUTDOWN_DRAIN_TIMEOUT_SECONDS", 20, &mut errors).max(1);

    let allowed_cors_origins = parse_list("MGS_ALLOWED_ORIGINS");

    let ws_security = WsSecurityEnv {
        behind_tls_proxy: parse_bool_with_default("MGS_BEHIND_TLS_PROXY", false, &mut errors),
        dev_mode: parse_bool_with_default("MGS_DEV_MODE", false, &mut errors),
        require_auth_env: parse_bool_with_default("MGS_REQUIRE_AUTH", false, &mut errors),
        allow_insecure_ws_proxy_proto: parse_bool_with_default(
            "MGS_ALLOW_INSECURE_WS_PROXY_PROTO",
            false,
            &mut errors,
        ),
        allowed_origins: parse_list("MGS_ALLOWED_ORIGINS"),
        trusted_proxy_cidrs: parse_list("MGS_TRUSTED_PROXY_CIDRS"),
        max_concurrent_connections: parse_optional_u64(
            "MGS_MAX_CONCURRENT_CONNECTIONS",
            &mut errors,
        ),
    };
    if let Some(max_connections) = ws_security.max_concurrent_connections {
        if max_connections == 0 {
            errors.push("MGS_MAX_CONCURRENT_CONNECTIONS must be > 0 when set".to_owned());
        }
    }

    if !errors.is_empty() {
        return Err(anyhow!(errors.join("; ")));
    }

    Ok(AppEnvConfig {
        map_path,
        diagnostics: DiagnosticsEnv {
            enabled: diagnostics_enabled,
            frame_watchdog_check_ms,
            frame_watchdog_stale_ms,
        },
        live_replay_enabled,
        quic_primary_only,
        cdn_origin,
        arena_worker: ArenaWorkerEnv {
            enabled: arena_worker_enabled,
            interval_ms: arena_worker_interval_ms.max(100),
            max_ticks: arena_worker_max_ticks.filter(|value| *value > 0),
        },
        auth: AuthEnv {
            store_path: auth_store_path,
            otp_ttl_seconds: auth_otp_ttl_seconds,
            session_ttl_seconds: auth_session_ttl_seconds,
            resend_interval_seconds: auth_resend_interval_seconds,
            max_verify_attempts: auth_max_verify_attempts,
            token_validation_rate_limit_per_sec: auth_token_validation_rate_limit_per_sec,
            token_validation_rate_limit_burst: auth_token_validation_rate_limit_burst,
            sms_command: auth_sms_command,
            sms_dev_mode: auth_sms_dev_mode,
            use_auth_cookies: auth_use_auth_cookies,
            deletion_grace_period_hours: auth_deletion_grace_period_hours,
            redis_url: auth_redis_url,
            redis_store_key: auth_redis_store_key,
            gdpr_hash_salt: auth_gdpr_hash_salt,
        },
        admin_auth: AdminAuthEnv {
            bearer_token: admin_bearer_token,
            ip_allowlist: admin_ip_allowlist,
        },
        backup: BackupEnv {
            enabled: backup_enabled,
            interval_seconds: backup_interval_seconds,
            output_dir: backup_output_dir,
            retention_count: backup_retention_count,
            auth_store_path: backup_auth_store_path,
            feature_flags_store_path: backup_feature_flags_store_path,
            arena_store_path: backup_arena_store_path,
            live_replay_dispute_store_path: backup_live_replay_dispute_store_path,
            extra_paths: backup_extra_paths,
        },
        network_bind: NetworkBindEnv { host, port },
        shutdown_drain_timeout_secs,
        allowed_cors_origins,
        ws_security,
    })
}

fn get_optional_trimmed(var_name: &str) -> Option<String> {
    std::env::var(var_name)
        .ok()
        .map(|raw| raw.trim().to_owned())
        .filter(|raw| !raw.is_empty())
}

fn parse_list(var_name: &str) -> Vec<String> {
    std::env::var(var_name)
        .ok()
        .into_iter()
        .flat_map(|raw| {
            raw.split(',')
                .map(str::trim)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|entry| !entry.is_empty())
        .collect()
}

fn parse_bool_with_default(var_name: &str, default_value: bool, errors: &mut Vec<String>) -> bool {
    let Some(raw) = std::env::var(var_name).ok() else {
        return default_value;
    };
    match parse_boolish(&raw) {
        Some(value) => value,
        None => {
            errors.push(format!(
                "{} has invalid boolean value '{}'; expected one of: 1/0, true/false, yes/no, on/off",
                var_name, raw
            ));
            default_value
        }
    }
}

fn parse_boolish(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_u64_with_default(var_name: &str, default_value: u64, errors: &mut Vec<String>) -> u64 {
    let Some(raw) = std::env::var(var_name).ok() else {
        return default_value;
    };
    match raw.trim().parse::<u64>() {
        Ok(value) => value,
        Err(_) => {
            errors.push(format!(
                "{} has invalid unsigned integer value '{}'",
                var_name, raw
            ));
            default_value
        }
    }
}

fn parse_u16_with_default(var_name: &str, default_value: u16, errors: &mut Vec<String>) -> u16 {
    let Some(raw) = std::env::var(var_name).ok() else {
        return default_value;
    };
    match raw.trim().parse::<u16>() {
        Ok(value) => value,
        Err(_) => {
            errors.push(format!("{} has invalid port value '{}'", var_name, raw));
            default_value
        }
    }
}

fn parse_u32_with_default(var_name: &str, default_value: u32, errors: &mut Vec<String>) -> u32 {
    let Some(raw) = std::env::var(var_name).ok() else {
        return default_value;
    };
    match raw.trim().parse::<u32>() {
        Ok(value) => value,
        Err(_) => {
            errors.push(format!(
                "{} has invalid unsigned integer value '{}'",
                var_name, raw
            ));
            default_value
        }
    }
}

fn parse_usize_with_default(
    var_name: &str,
    default_value: usize,
    errors: &mut Vec<String>,
) -> usize {
    let Some(raw) = std::env::var(var_name).ok() else {
        return default_value;
    };
    match raw.trim().parse::<usize>() {
        Ok(value) => value,
        Err(_) => {
            errors.push(format!(
                "{} has invalid unsigned integer value '{}'",
                var_name, raw
            ));
            default_value
        }
    }
}

fn parse_optional_u32(var_name: &str, errors: &mut Vec<String>) -> Option<u32> {
    let raw = std::env::var(var_name).ok()?;
    match raw.trim().parse::<u32>() {
        Ok(value) => Some(value),
        Err(_) => {
            errors.push(format!(
                "{} has invalid unsigned integer value '{}'",
                var_name, raw
            ));
            None
        }
    }
}

fn parse_optional_u64(var_name: &str, errors: &mut Vec<String>) -> Option<u64> {
    let raw = std::env::var(var_name).ok()?;
    match raw.trim().parse::<u64>() {
        Ok(value) => Some(value),
        Err(_) => {
            errors.push(format!(
                "{} has invalid unsigned integer value '{}'",
                var_name, raw
            ));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_env_config_reports_invalid_values() {
        let key = "MGS_PORT";
        let result = temp_env::with_var(key, Some("not-a-port"), load_app_env_config);
        assert!(result.is_err());
        let message = format!("{}", result.err().expect("expected error"));
        assert!(message.contains("MGS_PORT"));
    }
}
