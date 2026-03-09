use crate::core::constants::DEFAULT_LAG_COMPENSATION_MS;
use anyhow::{anyhow, Result};

#[derive(Debug, Clone)]
pub struct AppEnvConfig {
    pub map_path: Option<String>,
    pub instance: InstanceEnv,
    pub feature_flags: FeatureFlagsEnv,
    pub diagnostics: DiagnosticsEnv,
    pub live_replay_enabled: bool,
    pub quic_primary_only: bool,
    pub cdn_origin: Option<String>,
    pub arena_worker: ArenaWorkerEnv,
    pub auth: AuthEnv,
    pub admin_auth: AdminAuthEnv,
    pub backup: BackupEnv,
    pub signaling: SignalingEnv,
    pub network_bind: NetworkBindEnv,
    pub shutdown_drain_timeout_secs: u64,
    pub allowed_cors_origins: Vec<String>,
    pub ws_security: WsSecurityEnv,
}

#[derive(Debug, Clone)]
pub struct FeatureFlagsEnv {
    pub store_path: String,
    pub bootstrap_flags: Option<String>,
    pub redis_url: Option<String>,
    pub redis_store_key: String,
}

#[derive(Debug, Clone)]
pub struct InstanceEnv {
    pub map_path: Option<String>,
    pub match_type: Option<String>,
    pub match_duration_override_secs: Option<f32>,
    pub force_10v10_map: bool,
    pub map_target_players: Option<usize>,
    pub map_seed: Option<u64>,
    pub map_template: Option<String>,
    pub target_bot_count: Option<u64>,
    pub human_priority_enabled: bool,
    pub reserved_human_slots: usize,
    pub spectator_slot_cap: usize,
    pub lag_compensation_ms: u64,
    pub live_replay_enabled: bool,
    pub live_replay_capacity: usize,
    pub live_replay_player_cap: usize,
    pub live_replay_dispute_persist_enabled: bool,
    pub live_replay_dispute_store_path: String,
    pub live_replay_dispute_redis_url: Option<String>,
    pub live_replay_dispute_redis_key: String,
    pub live_replay_dispute_signing_key: Option<String>,
    pub live_replay_dispute_audit_capacity: usize,
    pub live_replay_match_persist_enabled: bool,
    pub live_replay_match_store_dir: String,
    pub live_replay_match_redis_url: Option<String>,
    pub live_replay_match_redis_key: String,
    pub live_replay_match_retention: usize,
    pub direct_packet_queue_cap: usize,
    pub navmesh_enabled: bool,
    pub navmesh_rebuild_interval_frames: u64,
    pub navmesh_cell_wall_limit: usize,
    pub progressive_destructible_enabled: bool,
    pub commander_mode_enabled: bool,
    pub single_machine_opt: bool,
    pub single_machine_mode: bool,
    pub join_tail_policy_enabled: bool,
    pub join_packet_batching_enabled: bool,
    pub join_soa_snapshot_enabled: bool,
    pub join_soa_adaptive_fallback_enabled: bool,
    pub join_entity_soa_snapshot_enabled: bool,
    pub join_initial_state_chunking_enabled: bool,
    pub join_authoritative_aoi_snapshot_enabled: bool,
    pub dynamic_mode_transitions_enabled: bool,
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
    pub redis_url: Option<String>,
    pub redis_store_key: String,
    pub auth_store_path: String,
    pub feature_flags_store_path: String,
    pub arena_store_path: String,
    pub live_replay_dispute_store_path: String,
    pub extra_paths: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SignalingEnv {
    pub chat_cooldown_ms: u64,
    pub chat_burst_capacity: u64,
    pub chat_burst_window_ms: u64,
    pub disable_stun: bool,
    pub stun_urls: Vec<String>,
    pub turn_urls: Vec<String>,
    pub turn_credential_type: Option<String>,
    pub turn_username: Option<String>,
    pub turn_credential: Option<String>,
    pub extra_ice_servers: Option<String>,
    pub sdp_concurrency: usize,
    pub webrtc_nat_1to1_ips: Vec<String>,
    pub webrtc_nat_1to1_candidate_type: Option<String>,
    pub webrtc_udp_port_min: Option<u16>,
    pub webrtc_udp_port_max: Option<u16>,
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
    let match_type = get_optional_trimmed("MGS_MATCH_TYPE");
    let match_duration_override_secs =
        parse_optional_f32("MGS_MATCH_DURATION_OVERRIDE_SECS", &mut errors)
            .map(|value| value.clamp(10.0, 3600.0));
    let force_10v10_map = parse_bool_with_default("MGS_FORCE_10V10_MAP", false, &mut errors);
    let map_target_players =
        parse_optional_usize("MGS_MAP_TARGET_PLAYERS", &mut errors).filter(|value| *value > 0);
    let map_seed = parse_optional_u64("MGS_MAP_SEED", &mut errors);
    let map_template = get_optional_trimmed("MGS_MAP_TEMPLATE");
    let feature_flag_store_path = get_optional_trimmed("MGS_FEATURE_FLAG_STORE_PATH")
        .unwrap_or_else(|| "data/feature_flags.json".to_owned());
    let feature_flag_bootstrap_flags = get_optional_trimmed("MGS_FEATURE_FLAGS");
    let feature_flag_redis_url = get_optional_trimmed("MGS_FEATURE_FLAGS_REDIS_URL")
        .or_else(|| get_optional_trimmed("MGS_REDIS_URL"));
    let feature_flag_redis_store_key = get_optional_trimmed("MGS_REDIS_FEATURE_FLAGS_KEY")
        .unwrap_or_else(|| "mgs:feature_flags:store".to_owned());
    let target_bot_count = parse_optional_u64("MGS_TARGET_BOT_COUNT", &mut errors);
    let human_priority_enabled =
        parse_bool_with_default("MGS_HUMAN_PRIORITY_ENABLED", true, &mut errors);
    let reserved_human_slots = parse_usize_with_default("MGS_RESERVED_HUMAN_SLOTS", 2, &mut errors);
    let spectator_slot_cap =
        parse_usize_with_default("MGS_SPECTATOR_CAP", 20, &mut errors).clamp(0, 256);
    let lag_compensation_ms = parse_u64_with_default(
        "MGS_LAG_COMPENSATION_MS",
        DEFAULT_LAG_COMPENSATION_MS,
        &mut errors,
    )
    .min(250);

    let diagnostics_enabled =
        parse_bool_with_default("MGS_DIAGNOSTICS_ENABLED", false, &mut errors);
    let frame_watchdog_check_ms =
        parse_u64_with_default("MGS_FRAME_WATCHDOG_CHECK_MS", 200, &mut errors).max(50);
    let frame_watchdog_stale_ms =
        parse_u64_with_default("MGS_FRAME_WATCHDOG_STALE_MS", 200, &mut errors).max(100);

    let live_replay_enabled =
        parse_bool_with_default("MGS_LIVE_REPLAY_ENABLED", false, &mut errors);
    let live_replay_capacity =
        parse_usize_with_default("MGS_LIVE_REPLAY_CAPACITY", 3600, &mut errors).clamp(120, 100_000);
    let live_replay_player_cap =
        parse_usize_with_default("MGS_LIVE_REPLAY_PLAYER_CAP", 64, &mut errors).clamp(8, 512);
    let live_replay_dispute_persist_enabled = parse_bool_with_default(
        "MGS_LIVE_REPLAY_DISPUTE_PERSIST",
        live_replay_enabled,
        &mut errors,
    );
    let live_replay_dispute_store_path = get_optional_trimmed("MGS_LIVE_REPLAY_DISPUTE_STORE_PATH")
        .unwrap_or_else(|| "data/live_replay/disputes.jsonl".to_owned());
    let live_replay_dispute_redis_url = get_optional_trimmed("MGS_LIVE_REPLAY_DISPUTE_REDIS_URL")
        .or_else(|| get_optional_trimmed("MGS_REDIS_URL"));
    let live_replay_dispute_redis_key = get_optional_trimmed("MGS_REDIS_LIVE_REPLAY_DISPUTE_KEY")
        .unwrap_or_else(|| "mgs:live_replay:disputes".to_owned());
    let live_replay_dispute_signing_key =
        get_optional_trimmed("MGS_LIVE_REPLAY_DISPUTE_SIGNING_KEY");
    let live_replay_dispute_audit_capacity =
        parse_usize_with_default("MGS_LIVE_REPLAY_DISPUTE_AUDIT_CAPACITY", 512, &mut errors)
            .clamp(16, 4096);
    let live_replay_match_persist_enabled = parse_bool_with_default(
        "MGS_LIVE_REPLAY_MATCH_PERSIST",
        live_replay_enabled,
        &mut errors,
    );
    let live_replay_match_store_dir = get_optional_trimmed("MGS_LIVE_REPLAY_MATCH_STORE_DIR")
        .unwrap_or_else(|| "data/live_replay/matches".to_owned());
    let live_replay_match_redis_url = get_optional_trimmed("MGS_LIVE_REPLAY_MATCH_REDIS_URL")
        .or_else(|| get_optional_trimmed("MGS_REDIS_URL"));
    let live_replay_match_redis_key = get_optional_trimmed("MGS_REDIS_LIVE_REPLAY_MATCH_KEY")
        .unwrap_or_else(|| "mgs:live_replay:matches".to_owned());
    let live_replay_match_retention =
        parse_usize_with_default("MGS_LIVE_REPLAY_MATCH_RETENTION", 100, &mut errors)
            .clamp(1, 2_000);
    let direct_packet_queue_cap =
        parse_usize_with_default("MGS_DIRECT_PACKET_QUEUE_CAP", 64, &mut errors).clamp(8, 512);
    let navmesh_enabled = parse_bool_with_default("MGS_NAVMESH_ENABLED", false, &mut errors);
    let navmesh_rebuild_interval_frames =
        parse_u64_with_default("MGS_NAVMESH_REBUILD_INTERVAL_FRAMES", 180, &mut errors).max(1);
    let navmesh_cell_wall_limit =
        parse_usize_with_default("MGS_NAVMESH_CELL_WALL_LIMIT", 16, &mut errors).clamp(0, 2048);
    let progressive_destructible_enabled =
        !parse_bool_with_default("MGS_DISABLE_PROGRESSIVE_DESTRUCTIBLE", false, &mut errors);
    let commander_mode_enabled =
        !parse_bool_with_default("MGS_DISABLE_COMMANDER_MODE", false, &mut errors);
    let single_machine_opt = parse_bool_with_default("MGS_SINGLE_MACHINE_OPT", false, &mut errors);
    let single_machine_mode =
        parse_bool_with_default("MGS_SINGLE_MACHINE_MODE", false, &mut errors);
    let join_tail_policy_enabled =
        !parse_bool_with_default("MGS_JOIN_DISABLE_TAIL_POLICY", false, &mut errors);
    let join_packet_batching_enabled =
        !parse_bool_with_default("MGS_JOIN_DISABLE_PACKET_BATCHING", false, &mut errors);
    let join_soa_snapshot_enabled =
        !parse_bool_with_default("MGS_JOIN_DISABLE_SOA_SNAPSHOT", false, &mut errors);
    let join_soa_adaptive_fallback_enabled =
        !parse_bool_with_default("MGS_JOIN_DISABLE_SOA_ADAPTIVE_FALLBACK", false, &mut errors);
    let join_entity_soa_snapshot_enabled =
        !parse_bool_with_default("MGS_JOIN_DISABLE_ENTITY_SOA_SNAPSHOT", false, &mut errors);
    let join_initial_state_chunking_enabled =
        !parse_bool_with_default("MGS_JOIN_DISABLE_INITIAL_CHUNKING", false, &mut errors);
    let join_authoritative_aoi_snapshot_enabled = !parse_bool_with_default(
        "MGS_JOIN_DISABLE_AUTHORITATIVE_AOI_SNAPSHOT",
        false,
        &mut errors,
    ) || parse_bool_with_default(
        "MGS_JOIN_ENABLE_AUTHORITATIVE_AOI_SNAPSHOT",
        false,
        &mut errors,
    );
    let dynamic_mode_transitions_enabled =
        parse_bool_with_default("MGS_DYNAMIC_MODE_TRANSITIONS", false, &mut errors);
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
    let allow_short_session_ttl_for_tests = std::env::var("MGS_TEST_ALLOW_SHORT_SESSION_TTL")
        .ok()
        .map(|raw| {
            let normalized = raw.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false);
    let auth_session_ttl_seconds =
        parse_u64_with_default("MGS_AUTH_SESSION_TTL_SECONDS", 60 * 60 * 24, &mut errors);
    let auth_session_ttl_seconds = if allow_short_session_ttl_for_tests {
        auth_session_ttl_seconds.max(1)
    } else {
        auth_session_ttl_seconds.max(300)
    };
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
    let backup_redis_url = get_optional_trimmed("MGS_BACKUP_REDIS_URL")
        .or_else(|| get_optional_trimmed("MGS_REDIS_URL"));
    let backup_redis_store_key = get_optional_trimmed("MGS_REDIS_BACKUP_KEY")
        .unwrap_or_else(|| "mgs:backup:latest".to_owned());
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
    let signaling_chat_cooldown_ms =
        parse_u64_with_default("MGS_CHAT_COOLDOWN_MS", 450, &mut errors).clamp(0, 5_000);
    let signaling_chat_burst_capacity =
        parse_u64_with_default("MGS_CHAT_BURST_CAPACITY", 5, &mut errors).clamp(0, 100);
    let signaling_chat_burst_window_ms =
        parse_u64_with_default("MGS_CHAT_BURST_WINDOW_MS", 5_000, &mut errors).clamp(500, 60_000);
    let signaling_disable_stun = parse_bool_with_default("MGS_DISABLE_STUN", false, &mut errors);
    let signaling_stun_urls = parse_list("MGS_STUN_URLS");
    let signaling_turn_urls = parse_list("MGS_TURN_URLS");
    let signaling_turn_credential_type =
        get_optional_trimmed("MGS_TURN_CREDENTIAL_TYPE").map(|raw| raw.to_ascii_lowercase());
    let signaling_turn_username = get_optional_trimmed("MGS_TURN_USERNAME");
    let signaling_turn_credential = get_optional_trimmed("MGS_TURN_CREDENTIAL");
    let signaling_extra_ice_servers = get_optional_trimmed("MGS_ICE_SERVERS");
    let signaling_sdp_concurrency =
        parse_usize_with_default("MGS_SIGNALING_SDP_CONCURRENCY", 64, &mut errors);
    let signaling_webrtc_nat_1to1_ips = parse_list("MGS_WEBRTC_NAT_1TO1_IPS");
    let signaling_webrtc_nat_1to1_candidate_type =
        get_optional_trimmed("MGS_WEBRTC_NAT_1TO1_CANDIDATE_TYPE")
            .map(|raw| raw.to_ascii_lowercase());
    let signaling_webrtc_udp_port_min = parse_optional_u16("MGS_WEBRTC_UDP_PORT_MIN", &mut errors);
    let signaling_webrtc_udp_port_max = parse_optional_u16("MGS_WEBRTC_UDP_PORT_MAX", &mut errors);
    if signaling_webrtc_udp_port_min.is_some() ^ signaling_webrtc_udp_port_max.is_some() {
        errors.push(
            "MGS_WEBRTC_UDP_PORT_MIN and MGS_WEBRTC_UDP_PORT_MAX must be set together".to_owned(),
        );
    }
    if let (Some(min_port), Some(max_port)) =
        (signaling_webrtc_udp_port_min, signaling_webrtc_udp_port_max)
    {
        if min_port > max_port {
            errors.push(format!(
                "MGS_WEBRTC_UDP_PORT_MIN ({}) must be <= MGS_WEBRTC_UDP_PORT_MAX ({})",
                min_port, max_port
            ));
        }
    }
    if let Some(raw_candidate_type) = signaling_webrtc_nat_1to1_candidate_type.as_deref() {
        if raw_candidate_type != "host" && raw_candidate_type != "srflx" {
            errors.push(format!(
                "MGS_WEBRTC_NAT_1TO1_CANDIDATE_TYPE must be 'host' or 'srflx' (got '{}')",
                raw_candidate_type
            ));
        }
    }

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
        map_path: map_path.clone(),
        instance: InstanceEnv {
            map_path,
            match_type,
            match_duration_override_secs,
            force_10v10_map,
            map_target_players,
            map_seed,
            map_template,
            target_bot_count,
            human_priority_enabled,
            reserved_human_slots,
            spectator_slot_cap,
            lag_compensation_ms,
            live_replay_enabled,
            live_replay_capacity,
            live_replay_player_cap,
            live_replay_dispute_persist_enabled,
            live_replay_dispute_store_path,
            live_replay_dispute_redis_url,
            live_replay_dispute_redis_key,
            live_replay_dispute_signing_key,
            live_replay_dispute_audit_capacity,
            live_replay_match_persist_enabled,
            live_replay_match_store_dir,
            live_replay_match_redis_url,
            live_replay_match_redis_key,
            live_replay_match_retention,
            direct_packet_queue_cap,
            navmesh_enabled,
            navmesh_rebuild_interval_frames,
            navmesh_cell_wall_limit,
            progressive_destructible_enabled,
            commander_mode_enabled,
            single_machine_opt,
            single_machine_mode,
            join_tail_policy_enabled,
            join_packet_batching_enabled,
            join_soa_snapshot_enabled,
            join_soa_adaptive_fallback_enabled,
            join_entity_soa_snapshot_enabled,
            join_initial_state_chunking_enabled,
            join_authoritative_aoi_snapshot_enabled,
            dynamic_mode_transitions_enabled,
        },
        feature_flags: FeatureFlagsEnv {
            store_path: feature_flag_store_path,
            bootstrap_flags: feature_flag_bootstrap_flags,
            redis_url: feature_flag_redis_url,
            redis_store_key: feature_flag_redis_store_key,
        },
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
            redis_url: backup_redis_url,
            redis_store_key: backup_redis_store_key,
            auth_store_path: backup_auth_store_path,
            feature_flags_store_path: backup_feature_flags_store_path,
            arena_store_path: backup_arena_store_path,
            live_replay_dispute_store_path: backup_live_replay_dispute_store_path,
            extra_paths: backup_extra_paths,
        },
        signaling: SignalingEnv {
            chat_cooldown_ms: signaling_chat_cooldown_ms,
            chat_burst_capacity: signaling_chat_burst_capacity,
            chat_burst_window_ms: signaling_chat_burst_window_ms,
            disable_stun: signaling_disable_stun,
            stun_urls: signaling_stun_urls,
            turn_urls: signaling_turn_urls,
            turn_credential_type: signaling_turn_credential_type,
            turn_username: signaling_turn_username,
            turn_credential: signaling_turn_credential,
            extra_ice_servers: signaling_extra_ice_servers,
            sdp_concurrency: signaling_sdp_concurrency,
            webrtc_nat_1to1_ips: signaling_webrtc_nat_1to1_ips,
            webrtc_nat_1to1_candidate_type: signaling_webrtc_nat_1to1_candidate_type,
            webrtc_udp_port_min: signaling_webrtc_udp_port_min,
            webrtc_udp_port_max: signaling_webrtc_udp_port_max,
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
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.parse::<u32>() {
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

fn parse_optional_usize(var_name: &str, errors: &mut Vec<String>) -> Option<usize> {
    let raw = std::env::var(var_name).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.parse::<usize>() {
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
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.parse::<u64>() {
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

fn parse_optional_f32(var_name: &str, errors: &mut Vec<String>) -> Option<f32> {
    let raw = std::env::var(var_name).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.parse::<f32>() {
        Ok(value) if value.is_finite() => Some(value),
        Ok(_) | Err(_) => {
            errors.push(format!(
                "{} has invalid finite float value '{}'",
                var_name, raw
            ));
            None
        }
    }
}

fn parse_optional_u16(var_name: &str, errors: &mut Vec<String>) -> Option<u16> {
    let raw = std::env::var(var_name).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.parse::<u16>() {
        Ok(value) => Some(value),
        Err(_) => {
            errors.push(format!("{} has invalid port value '{}'", var_name, raw));
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
        let message = format!("{}", result.expect_err("expected error"));
        assert!(message.contains("MGS_PORT"));
    }

    #[test]
    fn load_env_config_treats_empty_optional_numeric_values_as_unset() {
        let result = temp_env::with_vars(
            [
                ("MGS_MATCH_DURATION_OVERRIDE_SECS", Some("")),
                ("MGS_TARGET_BOT_COUNT", Some("")),
                ("MGS_WEBRTC_UDP_PORT_MIN", Some("")),
            ],
            load_app_env_config,
        )
        .expect("empty optional values should be ignored");

        assert_eq!(result.instance.match_duration_override_secs, None);
        assert_eq!(result.instance.target_bot_count, None);
        assert_eq!(result.signaling.webrtc_udp_port_min, None);
    }

    #[test]
    fn load_env_config_reads_chat_burst_settings() {
        let result = temp_env::with_vars(
            [
                ("MGS_CHAT_BURST_CAPACITY", Some("7")),
                ("MGS_CHAT_BURST_WINDOW_MS", Some("4200")),
            ],
            load_app_env_config,
        )
        .expect("chat burst settings should parse");

        assert_eq!(result.signaling.chat_burst_capacity, 7);
        assert_eq!(result.signaling.chat_burst_window_ms, 4_200);
    }

    #[test]
    fn load_env_config_reads_feature_flag_redis_settings() {
        let result = temp_env::with_vars(
            [
                ("MGS_REDIS_URL", Some("redis://127.0.0.1:6379/")),
                ("MGS_REDIS_FEATURE_FLAGS_KEY", Some("mgs:test:flags")),
            ],
            load_app_env_config,
        )
        .expect("feature flag redis settings should parse");

        assert_eq!(
            result.feature_flags.redis_url.as_deref(),
            Some("redis://127.0.0.1:6379/")
        );
        assert_eq!(result.feature_flags.redis_store_key, "mgs:test:flags");
    }

    #[test]
    fn load_env_config_reads_live_replay_dispute_redis_settings() {
        let result = temp_env::with_vars(
            [
                ("MGS_REDIS_URL", Some("redis://127.0.0.1:6379/")),
                (
                    "MGS_REDIS_LIVE_REPLAY_DISPUTE_KEY",
                    Some("mgs:test:live_replay:disputes"),
                ),
            ],
            load_app_env_config,
        )
        .expect("live replay dispute redis settings should parse");

        assert_eq!(
            result.instance.live_replay_dispute_redis_url.as_deref(),
            Some("redis://127.0.0.1:6379/")
        );
        assert_eq!(
            result.instance.live_replay_dispute_redis_key,
            "mgs:test:live_replay:disputes"
        );
    }

    #[test]
    fn load_env_config_reads_live_replay_match_redis_settings() {
        let result = temp_env::with_vars(
            [
                ("MGS_REDIS_URL", Some("redis://127.0.0.1:6379/")),
                (
                    "MGS_REDIS_LIVE_REPLAY_MATCH_KEY",
                    Some("mgs:test:live_replay:matches"),
                ),
            ],
            load_app_env_config,
        )
        .expect("live replay match redis settings should parse");

        assert_eq!(
            result.instance.live_replay_match_redis_url.as_deref(),
            Some("redis://127.0.0.1:6379/")
        );
        assert_eq!(
            result.instance.live_replay_match_redis_key,
            "mgs:test:live_replay:matches"
        );
    }

    #[test]
    fn load_env_config_reads_backup_redis_settings() {
        let result = temp_env::with_vars(
            [
                ("MGS_REDIS_URL", Some("redis://127.0.0.1:6379/")),
                ("MGS_REDIS_BACKUP_KEY", Some("mgs:test:backup:latest")),
            ],
            load_app_env_config,
        )
        .expect("backup redis settings should parse");

        assert_eq!(
            result.backup.redis_url.as_deref(),
            Some("redis://127.0.0.1:6379/")
        );
        assert_eq!(result.backup.redis_store_key, "mgs:test:backup:latest");
    }
}
