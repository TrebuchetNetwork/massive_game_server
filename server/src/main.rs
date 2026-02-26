// massive_game_server/server/src/main.rs
use dashmap::DashMap;
use ipnet::IpNet;
use massive_game_server_core::concurrent::thread_pools::ThreadPoolSystem;
use massive_game_server_core::core::config::ServerConfig;
use massive_game_server_core::core::types::{PlayerAoI, PlayerInputData};
use massive_game_server_core::network::quic::{
    connected_quic_peer_count, start_quic_runtime_from_env_with_handler, QuicRequestHandler,
};
use massive_game_server_core::network::connection_manager::shared_connection_manager;
use massive_game_server_core::network::signaling::{
    cleanup_connection,
    handle_signaling_connection,
    ChatMessagesQueue,
    ClientStatesMap,
    DataChannelsMap,
    PlayerManagerRef,
    ServerInstanceRef, // Added ServerInstanceRef
    SignalingPeers,
    WorldPartitionManagerRef,
};
use massive_game_server_core::operational::arena::{build_arena_routes, ArenaService};
use massive_game_server_core::operational::auth::{build_auth_routes, AuthService};
use massive_game_server_core::operational::backup::BackupManager;
use massive_game_server_core::operational::code_generation::{
    build_code_generation_routes, CodeGenerationService,
};
use massive_game_server_core::operational::config::load_validated_server_config;
use massive_game_server_core::operational::diagnostics::{deadlock, heap_profiler};
use massive_game_server_core::operational::feature_flags::{
    build_feature_flag_routes, FeatureFlagService,
};
use massive_game_server_core::operational::monitoring::{
    alerts as monitoring_alerts, metrics as monitoring_metrics, tracing as monitoring_tracing,
};
use massive_game_server_core::scaling::HorizontalScalingCoordinator;
use massive_game_server_core::server::instance::{LiveReplayDisputeRequest, MassiveGameServer};
use massive_game_server_core::server::lifecycle;

use parking_lot::RwLock as ParkingLotRwLock;
use serde::Deserialize;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{error, info, warn, Instrument};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use uuid::Uuid;
use warp::http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri};
use warp::{Filter, Reply};

fn init_logging() -> anyhow::Result<()> {
    monitoring_tracing::init_tracing_subscriber(
        "massive_game_server_core=info,warp=info,webrtc=warn,signaling=info",
    )
}

#[derive(Clone, Default, Deserialize)]
struct WsAuthQuery {
    auth_token: Option<String>,
    token: Option<String>,
    team_id: Option<u8>,
    team: Option<String>,
    spectator: Option<String>,
    mode: Option<String>,
    is_mobile: Option<bool>,
    match_type: Option<String>,
}

impl WsAuthQuery {
    fn requested_team_id(&self) -> Option<u8> {
        if let Some(team_id) = self.team_id {
            return Some(team_id);
        }

        let team_hint = self
            .team
            .as_deref()
            .or(self.mode.as_deref())
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if team_hint == "spectator" || team_hint == "spec" {
            return Some(0);
        }
        if team_hint == "1" || team_hint == "team1" || team_hint == "red" {
            return Some(1);
        }
        if team_hint == "2" || team_hint == "team2" || team_hint == "blue" {
            return Some(2);
        }

        if self
            .spectator
            .as_deref()
            .and_then(parse_boolish_query)
            .unwrap_or(false)
        {
            return Some(0);
        }
        None
    }
}

fn parse_boolish_query(raw: &str) -> Option<bool> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

#[derive(Clone, Default, Deserialize)]
struct LiveReplayRecentQuery {
    limit: Option<usize>,
}

#[derive(Clone, Default, Deserialize)]
struct QuicControlRequest {
    op: Option<String>,
    peer_id: Option<String>,
    username: Option<String>,
    team_id: Option<u8>,
    replay_limit: Option<usize>,
    from_frame: Option<u64>,
    to_frame: Option<u64>,
    player_id: Option<String>,
    input: Option<QuicInputEnvelope>,
    inputs: Option<Vec<QuicInputEnvelope>>,
}

#[derive(Clone, Default, Deserialize)]
struct QuicInputEnvelope {
    timestamp: Option<u64>,
    sequence: Option<u32>,
    move_forward: Option<bool>,
    move_backward: Option<bool>,
    move_left: Option<bool>,
    move_right: Option<bool>,
    shooting: Option<bool>,
    reload: Option<bool>,
    rotation: Option<f32>,
    melee_attack: Option<bool>,
    change_weapon_slot: Option<u8>,
    use_ability_slot: Option<u8>,
    ping_x: Option<f32>,
    ping_y: Option<f32>,
}

impl QuicInputEnvelope {
    fn into_player_input(self) -> PlayerInputData {
        PlayerInputData {
            timestamp: self.timestamp.unwrap_or_default(),
            sequence: self.sequence.unwrap_or_default(),
            move_forward: self.move_forward.unwrap_or(false),
            move_backward: self.move_backward.unwrap_or(false),
            move_left: self.move_left.unwrap_or(false),
            move_right: self.move_right.unwrap_or(false),
            shooting: self.shooting.unwrap_or(false),
            reload: self.reload.unwrap_or(false),
            rotation: self.rotation.unwrap_or(0.0),
            melee_attack: self.melee_attack.unwrap_or(false),
            change_weapon_slot: self.change_weapon_slot.unwrap_or(0),
            use_ability_slot: self.use_ability_slot.unwrap_or(0),
            ping_x: self.ping_x.unwrap_or(0.0),
            ping_y: self.ping_y.unwrap_or(0.0),
        }
    }
}

#[derive(Clone, Default)]
struct AdminAuthConfig {
    bearer_token: Option<Arc<String>>,
    ip_allowlist: Arc<Vec<IpNet>>,
}

impl AdminAuthConfig {
    fn from_env() -> Self {
        let bearer_token = std::env::var("MGS_ADMIN_BEARER_TOKEN")
            .or_else(|_| std::env::var("MGS_ADMIN_TOKEN"))
            .ok()
            .map(|raw| raw.trim().to_owned())
            .filter(|raw| !raw.is_empty())
            .map(Arc::new);

        if bearer_token.is_some() {
            info!("Admin bearer auth enabled for /api/ops/* and /api/arena/* routes.");
        } else {
            warn!(
                "Admin bearer auth token is not configured. Protected routes will reject requests \
                (set MGS_ADMIN_BEARER_TOKEN)."
            );
        }

        let ip_allowlist = parse_admin_ip_allowlist();
        if ip_allowlist.is_empty() {
            info!("Admin IP allowlist is disabled (set MGS_ADMIN_IP_ALLOWLIST to enforce source IP restrictions).");
        } else {
            info!(
                "Admin IP allowlist enabled with {} CIDR entries.",
                ip_allowlist.len()
            );
        }

        Self {
            bearer_token,
            ip_allowlist: Arc::new(ip_allowlist),
        }
    }
}

#[derive(Debug)]
struct AdminAuthRejection {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl AdminAuthRejection {
    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "admin_auth_required",
            message: message.into(),
        }
    }

    fn service_unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "admin_auth_unconfigured",
            message: message.into(),
        }
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "admin_ip_blocked",
            message: message.into(),
        }
    }
}

impl warp::reject::Reject for AdminAuthRejection {}

fn parse_bearer_token(authorization_header: Option<&str>) -> Option<String> {
    let raw = authorization_header?.trim();
    if raw.is_empty() {
        return None;
    }
    let token = raw
        .strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))?
        .trim();
    if token.is_empty() {
        return None;
    }
    Some(token.to_owned())
}

fn constant_time_eq(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.as_bytes()
        .iter()
        .zip(right.as_bytes())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

fn is_admin_protected_path(path: &str) -> bool {
    let normalized = path.trim_end_matches('/');
    normalized == "/api/ops"
        || normalized.starts_with("/api/ops/")
        || normalized == "/api/arena"
        || normalized.starts_with("/api/arena/")
}

fn parse_list_env(var_name: &str) -> Vec<String> {
    std::env::var(var_name)
        .ok()
        .into_iter()
        .flat_map(|raw| {
            raw.split(',')
                .map(str::trim)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|item| !item.is_empty())
        .collect()
}

fn parse_admin_ip_allowlist() -> Vec<IpNet> {
    let mut entries = parse_list_env("MGS_ADMIN_IP_ALLOWLIST");
    entries.extend(parse_list_env("MGS_ADMIN_ALLOWED_IPS"));

    let mut allowlist = Vec::new();
    for entry in entries {
        if let Ok(cidr) = entry.parse::<IpNet>() {
            allowlist.push(cidr);
            continue;
        }
        if let Ok(ip) = entry.parse::<IpAddr>() {
            allowlist.push(IpNet::from(ip));
            continue;
        }
        warn!(
            "Skipping invalid admin allowlist entry '{}'. Expected IP or CIDR.",
            entry
        );
    }
    allowlist.sort_by_key(|a| a.to_string());
    allowlist.dedup_by(|a, b| a == b);
    allowlist
}

fn admin_ip_allowed(ip_allowlist: &[IpNet], source_ip: IpAddr) -> bool {
    if ip_allowlist.is_empty() {
        return true;
    }
    ip_allowlist.iter().any(|cidr| cidr.contains(&source_ip))
}

fn is_trusted_proxy(ip: IpAddr) -> bool {
    use std::net::Ipv6Addr;
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.octets()[0] == 10
                || (v4.octets()[0] == 172 && (v4.octets()[1] & 0xf0) == 16)
                || (v4.octets()[0] == 192 && v4.octets()[1] == 168)
        }
        IpAddr::V6(v6) => {
            v6 == Ipv6Addr::LOCALHOST
                || match v6.to_ipv4_mapped() {
                    Some(v4) => is_trusted_proxy(IpAddr::V4(v4)),
                    None => false,
                }
        }
    }
}

fn env_flag(var_name: &str) -> bool {
    std::env::var(var_name)
        .ok()
        .map(|raw| {
            let normalized = raw.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "yes" || normalized == "on"
        })
        .unwrap_or(false)
}

fn requires_admin_auth(
    config: AdminAuthConfig,
) -> impl Filter<Extract = ((),), Error = warp::Rejection> + Clone {
    warp::method()
        .and(warp::path::full())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::header::headers_cloned())
        .and(warp::addr::remote())
        .and_then(
            move |method: Method,
                  full_path: warp::path::FullPath,
                  authorization: Option<String>,
                  headers: HeaderMap,
                  remote_addr: Option<SocketAddr>| {
                let config = config.clone();
                async move {
                    let path = full_path.as_str();
                    if !is_admin_protected_path(path) {
                        return Err(warp::reject::not_found());
                    }
                    if method == Method::OPTIONS {
                        return Ok(());
                    }

                    let Some(expected_token) = config.bearer_token.as_ref() else {
                        return Err(warp::reject::custom(AdminAuthRejection::service_unavailable(
                            "Admin routes are disabled until MGS_ADMIN_BEARER_TOKEN is configured.",
                        )));
                    };

                    if !config.ip_allowlist.is_empty() {
                        let socket_ip = remote_addr.map(|addr| addr.ip());

                        // Only trust X-Forwarded-For / X-Real-IP if the direct
                        // connecting IP is a known trusted proxy (private/loopback).
                        let source_ip = if socket_ip.is_some_and(is_trusted_proxy) {
                            let forwarded_ip = headers
                                .get("x-forwarded-for")
                                .and_then(|value| value.to_str().ok())
                                .and_then(parse_forwarded_for_ip);
                            let real_ip = headers
                                .get("x-real-ip")
                                .and_then(|value| value.to_str().ok())
                                .and_then(|value| value.trim().parse::<IpAddr>().ok());
                            forwarded_ip.or(real_ip).or(socket_ip)
                        } else {
                            socket_ip
                        };

                        let Some(source_ip) = source_ip else {
                            return Err(warp::reject::custom(AdminAuthRejection::forbidden(
                                "Admin request source IP could not be determined.",
                            )));
                        };

                        if !admin_ip_allowed(config.ip_allowlist.as_slice(), source_ip) {
                            return Err(warp::reject::custom(AdminAuthRejection::forbidden(
                                format!(
                                    "Admin access denied for source IP {} (not in allowlist).",
                                    source_ip
                                ),
                            )));
                        }
                    }

                    let Some(provided_token) = parse_bearer_token(authorization.as_deref()) else {
                        return Err(warp::reject::custom(AdminAuthRejection::unauthorized(
                            "Missing Authorization bearer token.",
                        )));
                    };

                    if !constant_time_eq(expected_token.as_str(), provided_token.as_str()) {
                        return Err(warp::reject::custom(AdminAuthRejection::unauthorized(
                            "Invalid admin bearer token.",
                        )));
                    }

                    Ok(())
                }
            },
        )
}

async fn handle_route_rejection(rejection: warp::Rejection) -> Result<impl Reply, Infallible> {
    if let Some(admin_rejection) = rejection.find::<AdminAuthRejection>() {
        let body = warp::reply::json(&serde_json::json!({
            "ok": false,
            "error": {
                "code": admin_rejection.code,
                "message": admin_rejection.message
            }
        }));
        return Ok(warp::reply::with_status(body, admin_rejection.status));
    }

    if rejection.is_not_found() {
        let body = warp::reply::json(&serde_json::json!({
            "ok": false,
            "error": {
                "code": "not_found",
                "message": "Route not found."
            }
        }));
        return Ok(warp::reply::with_status(body, StatusCode::NOT_FOUND));
    }

    if rejection.find::<warp::reject::MethodNotAllowed>().is_some() {
        let body = warp::reply::json(&serde_json::json!({
            "ok": false,
            "error": {
                "code": "method_not_allowed",
                "message": "Method not allowed."
            }
        }));
        return Ok(warp::reply::with_status(
            body,
            StatusCode::METHOD_NOT_ALLOWED,
        ));
    }

    if let Some(err) = rejection.find::<warp::filters::body::BodyDeserializeError>() {
        let body = warp::reply::json(&serde_json::json!({
            "ok": false,
            "error": {
                "code": "invalid_json",
                "message": err.to_string()
            }
        }));
        return Ok(warp::reply::with_status(body, StatusCode::BAD_REQUEST));
    }

    if let Some(err) = rejection.find::<warp::reject::InvalidQuery>() {
        let body = warp::reply::json(&serde_json::json!({
            "ok": false,
            "error": {
                "code": "invalid_query",
                "message": err.to_string()
            }
        }));
        return Ok(warp::reply::with_status(body, StatusCode::BAD_REQUEST));
    }

    error!("Unhandled route rejection: {:?}", rejection);
    let body = warp::reply::json(&serde_json::json!({
        "ok": false,
        "error": {
            "code": "internal_error",
            "message": "Unhandled server rejection."
        }
    }));
    Ok(warp::reply::with_status(
        body,
        StatusCode::INTERNAL_SERVER_ERROR,
    ))
}

fn static_cache_control_for_path(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());

    match extension.as_deref() {
        Some("html") => "no-cache, no-store, must-revalidate",
        Some("js") | Some("mjs") | Some("css") | Some("wasm") | Some("png") | Some("jpg")
        | Some("jpeg") | Some("webp") | Some("gif") | Some("svg") | Some("ico") | Some("woff")
        | Some("woff2") | Some("ttf") | Some("otf") | Some("mp3") | Some("ogg") | Some("wav") => {
            "public, max-age=31536000, immutable"
        }
        Some("json") | Some("map") => "public, max-age=300",
        _ => "public, max-age=3600",
    }
}

fn parse_forwarded_for_ip(raw: &str) -> Option<IpAddr> {
    raw.split(',')
        .map(str::trim)
        .find(|candidate| !candidate.is_empty())
        .and_then(|candidate| candidate.parse::<IpAddr>().ok())
}

fn parse_u64_env(var_name: &str, default_value: u64) -> u64 {
    std::env::var(var_name)
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_value)
}

fn recent_frame_p95_ms(server: &MassiveGameServer) -> Option<f64> {
    let history = server.tick_durations_history.read();
    if history.is_empty() {
        return None;
    }
    let mut samples_ms: Vec<f64> = history
        .iter()
        .rev()
        .take(240)
        .map(|sample| sample.as_secs_f64() * 1000.0)
        .collect();
    if samples_ms.is_empty() {
        return None;
    }
    samples_ms.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let p95_idx = ((samples_ms.len().saturating_sub(1) as f64) * 0.95).round() as usize;
    samples_ms.get(p95_idx).copied()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // MUST be the very first line
    // MUST be the very first line
    std::panic::set_hook(Box::new(|panic_info| {
        eprintln!("PANIC: {}", panic_info);
        if let Some(location) = panic_info.location() {
            eprintln!(
                "Location: {}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            );
        }

        // Also log to file in case stderr is lost
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("panic.log")
        {
            use std::io::Write;
            use std::time::SystemTime;

            let timestamp = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            writeln!(file, "PANIC at {}: {}", timestamp, panic_info).ok();
        }

        // Print backtrace
        eprintln!("Backtrace:\n{:?}", std::backtrace::Backtrace::capture());
    }));

    if let Err(e) = init_logging() {
        eprintln!("Failed to initialize logging: {:?}", e);
        return Err(e);
    }
    if let Err(err) = monitoring_metrics::init_metrics_exporter_from_env() {
        warn!(
            "Prometheus exporter initialization failed; continuing without metrics endpoint: {}",
            err
        );
    }

    info!("Massive Game Server starting up...");

    let config = Arc::new(
        load_validated_server_config()
            .map_err(|err| anyhow::anyhow!("failed to load/validate server config: {}", err))?,
    );
    info!(
        "Server configuration loaded. Tick rate: {}",
        config.tick_rate
    );
    let scaling_coordinator = Arc::new(HorizontalScalingCoordinator::new(
        config.cluster_shard_count,
        2,
    ));
    let bootstrap_assignment = scaling_coordinator.assignment_for_match("bootstrap");
    info!(
        "Horizontal scaling coordinator ready: shards={}, local_shard={}, bootstrap_primary={}, replicas={:?}",
        scaling_coordinator.shard_count(),
        config.local_shard_id,
        bootstrap_assignment.primary_shard,
        bootstrap_assignment.replica_shards
    );

    let thread_pool_system = match ThreadPoolSystem::new(config.clone()) {
        Ok(tps) => Arc::new(tps),
        Err(e) => {
            error!("Failed to initialize thread pools: {:?}", e);
            return Err(anyhow::anyhow!("Thread pool initialization failed: {}", e));
        }
    };
    info!("Thread pools initialized.");

    let data_channels_state: DataChannelsMap = Arc::new(DashMap::new());
    let client_states_state: ClientStatesMap = Arc::new(ParkingLotRwLock::new(HashMap::new()));
    let chat_messages_state: ChatMessagesQueue =
        Arc::new(tokio::sync::RwLock::new(VecDeque::with_capacity(100)));
    let player_aois_state: Arc<DashMap<String, PlayerAoI>> = Arc::new(DashMap::new());

    let game_server_instance: ServerInstanceRef = Arc::new(MassiveGameServer::new(
        // Changed variable name for clarity
        config.clone(),
        thread_pool_system,
        data_channels_state.clone(),
        client_states_state.clone(),
        chat_messages_state.clone(),
        player_aois_state.clone(),
    ));
    info!("Game server instance created.");

    if let Ok(map_path) = std::env::var("MGS_MAP_PATH") {
        info!("Custom map loaded from: {}", map_path);
    }

    if env_flag("MGS_DIAGNOSTICS_ENABLED") {
        deadlock::spawn_frame_progress_watchdog(
            game_server_instance.frame_counter.clone(),
            std::time::Duration::from_secs(1),
            std::time::Duration::from_secs(10),
        );
        heap_profiler::spawn_heap_snapshot_logger(std::time::Duration::from_secs(30));
        info!("Background diagnostics enabled.");
    }

    let backup_manager = BackupManager::from_env();
    if backup_manager.enabled() {
        info!(
            "Automated backups enabled (interval={}s, dir from MGS_BACKUP_DIR).",
            backup_manager.interval_seconds()
        );
        let backup_manager_task = backup_manager.clone();
        let server_for_backup_task = game_server_instance.clone();
        tokio::spawn(async move {
            let mut ticker =
                tokio::time::interval(Duration::from_secs(backup_manager_task.interval_seconds()));
            // Skip immediate tick to avoid backup spike during warm startup.
            ticker.tick().await;
            loop {
                ticker.tick().await;
                if server_for_backup_task.is_shutdown_requested() {
                    info!("Backup worker observed shutdown; stopping.");
                    break;
                }
                if let Err(err) = backup_manager_task.run_once("scheduled").await {
                    warn!("Scheduled backup failed: {}", err);
                }
            }
        });
    } else {
        info!("Automated backups are disabled (set MGS_BACKUP_ENABLED=1 to enable).");
    }

    let alert_rules = monitoring_alerts::default_alert_rules_from_env();
    let alert_notifier = monitoring_alerts::AlertmanagerNotifier::new(
        monitoring_alerts::AlertmanagerConfig::from_env(),
    );
    if alert_rules.is_empty() {
        info!("Alert evaluator disabled (no threshold env vars configured).");
    } else {
        let alert_eval_interval_secs = parse_u64_env("MGS_ALERT_EVAL_INTERVAL_SECONDS", 15);
        info!(
            "Alert evaluator enabled (rules={}, interval={}s, alertmanager_webhook={}).",
            alert_rules.len(),
            alert_eval_interval_secs,
            if alert_notifier.enabled() {
                "configured"
            } else {
                "disabled"
            }
        );
        let server_for_alerts = game_server_instance.clone();
        let rules_for_alerts = alert_rules.clone();
        let notifier_for_alerts = alert_notifier.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(alert_eval_interval_secs));
            loop {
                ticker.tick().await;
                if server_for_alerts.is_shutdown_requested() {
                    info!("Alert evaluator observed shutdown; stopping.");
                    break;
                }

                let connected_players = server_for_alerts
                    .player_manager
                    .player_count()
                    .saturating_add(connected_quic_peer_count());
                let heap_snapshot = heap_profiler::collect_heap_snapshot();
                monitoring_metrics::record_memory_usage(
                    heap_snapshot.resident_bytes,
                    heap_snapshot.allocated_bytes,
                );

                let mut snapshots = vec![monitoring_alerts::MetricSnapshot {
                    name: "game_players_connected".to_owned(),
                    value: connected_players as f64,
                }];
                if let Some(frame_p95_ms) = recent_frame_p95_ms(server_for_alerts.as_ref()) {
                    snapshots.push(monitoring_alerts::MetricSnapshot {
                        name: "game_frame_time_ms_p95".to_owned(),
                        value: frame_p95_ms,
                    });
                }
                if let Some(rss_bytes) = heap_snapshot.resident_bytes {
                    snapshots.push(monitoring_alerts::MetricSnapshot {
                        name: "game_memory_rss_bytes".to_owned(),
                        value: rss_bytes as f64,
                    });
                }

                let events = monitoring_alerts::evaluate_alerts(&rules_for_alerts, &snapshots);
                if !events.is_empty() {
                    warn!("Alert thresholds crossed: {:?}", events);
                    notifier_for_alerts.notify_events(&events).await;
                }
            }
        });
    }

    let signaling_peers_state: SignalingPeers = Arc::new(DashMap::new());
    let auth_service = AuthService::new_from_env();
    let arena_service = ArenaService::new_from_env();
    let arena_service_for_worker = arena_service.clone();
    let code_generation_service = CodeGenerationService::new_from_env();
    let feature_flag_service = FeatureFlagService::new_from_env();
    let auth_routes = build_auth_routes(auth_service.clone());
    let arena_routes = build_arena_routes(arena_service.clone());
    let code_generation_routes = build_code_generation_routes(code_generation_service);
    let feature_flag_routes = build_feature_flag_routes(feature_flag_service);
    let server_for_join_stage_report = game_server_instance.clone();
    let join_stage_report_route = warp::path!("api" / "ops" / "join-stages")
        .and(warp::get())
        .and(warp::any().map(move || server_for_join_stage_report.clone()))
        .map(|server_inst: ServerInstanceRef| warp::reply::json(&server_inst.join_stage_report()));
    let server_for_join_stage_reset = game_server_instance.clone();
    let join_stage_reset_route = warp::path!("api" / "ops" / "join-stages" / "reset")
        .and(warp::post())
        .and(warp::any().map(move || server_for_join_stage_reset.clone()))
        .map(|server_inst: ServerInstanceRef| {
            server_inst.reset_join_stage_report();
            warp::reply::json(&serde_json::json!({ "ok": true }))
        });
    let server_for_live_replay_recent = game_server_instance.clone();
    let live_replay_recent_route = warp::path!("api" / "ops" / "live-replay" / "recent")
        .and(warp::get())
        .and(
            warp::query::<LiveReplayRecentQuery>()
                .or(warp::any().map(LiveReplayRecentQuery::default))
                .unify(),
        )
        .and(warp::any().map(move || server_for_live_replay_recent.clone()))
        .map(|query: LiveReplayRecentQuery, server_inst: ServerInstanceRef| {
            let limit = query.limit.unwrap_or(256).clamp(1, 4096);
            warp::reply::json(&serde_json::json!({
                "enabled": !server_inst.recent_live_replay_frames(1).is_empty() || env_flag("MGS_LIVE_REPLAY_ENABLED"),
                "frames": server_inst.recent_live_replay_frames(limit),
                "limit": limit,
            }))
        });
    let server_for_live_replay_dispute = game_server_instance.clone();
    let live_replay_dispute_route = warp::path!("api" / "ops" / "live-replay" / "dispute")
        .and(warp::post())
        .and(warp::body::json::<LiveReplayDisputeRequest>())
        .and(warp::any().map(move || server_for_live_replay_dispute.clone()))
        .map(
            |request: LiveReplayDisputeRequest, server_inst: ServerInstanceRef| {
                warp::reply::json(&server_inst.build_live_replay_dispute_report(request))
            },
        );
    let server_for_live_replay_dispute_recent = game_server_instance.clone();
    let live_replay_dispute_recent_route =
        warp::path!("api" / "ops" / "live-replay" / "disputes" / "recent")
            .and(warp::get())
            .and(
                warp::query::<LiveReplayRecentQuery>()
                    .or(warp::any().map(LiveReplayRecentQuery::default))
                    .unify(),
            )
            .and(warp::any().map(move || server_for_live_replay_dispute_recent.clone()))
            .map(
                |query: LiveReplayRecentQuery, server_inst: ServerInstanceRef| {
                    let limit = query.limit.unwrap_or(128).clamp(1, 2048);
                    warp::reply::json(&serde_json::json!({
                        "ok": true,
                        "op": "live_replay_disputes_recent",
                        "audits": server_inst.recent_live_replay_dispute_audits(limit),
                        "limit": limit,
                    }))
                },
            );
    let server_for_match_summary = game_server_instance.clone();
    let match_summary_latest_route = warp::path!("api" / "ops" / "match-summary" / "latest")
        .and(warp::get())
        .and(warp::any().map(move || server_for_match_summary.clone()))
        .map(|server_inst: ServerInstanceRef| {
            warp::reply::json(&serde_json::json!({
                "ok": true,
                "summary": server_inst.latest_match_end_summary(),
            }))
        });
    let server_for_killcam = game_server_instance.clone();
    let killcam_latest_route = warp::path!("api" / "ops" / "killcam" / String)
        .and(warp::get())
        .and(warp::any().map(move || server_for_killcam.clone()))
        .map(|player_id: String, server_inst: ServerInstanceRef| {
            warp::reply::json(&serde_json::json!({
                "ok": true,
                "player_id": player_id,
                "killcam": server_inst.latest_killcam_for_player(&player_id),
            }))
        });
    let server_for_match_type = game_server_instance.clone();
    let match_type_route = warp::path!("api" / "ops" / "match-type")
        .and(warp::get())
        .and(warp::any().map(move || server_for_match_type.clone()))
        .map(|server_inst: ServerInstanceRef| {
            warp::reply::json(&serde_json::json!({
                "ok": true,
                "match_type": server_inst.match_type.label(),
                "max_players": server_inst.effective_max_players(),
                "match_duration_secs": server_inst.match_duration_secs,
                "bot_fill_delay_secs": server_inst.match_type.bot_fill_delay_secs(),
                "min_humans_for_bot_fill": server_inst.match_type.min_humans_for_bot_fill(),
            }))
        });

    let quic_primary_only = env_flag("MGS_QUIC_PRIMARY") && env_flag("MGS_QUIC_PRIMARY_ONLY");
    if quic_primary_only {
        info!(
            "MGS_QUIC_PRIMARY_ONLY enabled: WebSocket signaling endpoint /ws will reject upgrades."
        );
    }

    let config_for_ws = config.clone();
    let signaling_peers_for_ws = signaling_peers_state.clone();
    // Pass the Arc<MassiveGameServer> directly for its components
    let player_manager_for_ws: PlayerManagerRef = game_server_instance.player_manager.clone();
    let world_partition_manager_for_ws: WorldPartitionManagerRef =
        game_server_instance.world_partition_manager.clone();
    let data_channels_for_ws = data_channels_state.clone();
    let client_states_for_ws = client_states_state.clone();
    let chat_messages_for_ws = chat_messages_state.clone();
    let player_aois_for_ws = player_aois_state.clone();
    let server_instance_for_ws = game_server_instance.clone(); // Clone Arc for WebSocket handler
    let auth_service_for_ws = auth_service.clone();
    let scaling_coordinator_for_ws = scaling_coordinator.clone();

    let signaling_route_ws = warp::path("ws")
        .and(warp::ws())
        .and(
            warp::query::<WsAuthQuery>()
                .or(warp::any().map(WsAuthQuery::default))
                .unify(),
        )
        .and(warp::header::headers_cloned())
        .and(warp::addr::remote())
        .and(warp::any().map(move || signaling_peers_for_ws.clone()))
        .and(warp::any().map(move || player_manager_for_ws.clone()))
        .and(warp::any().map(move || world_partition_manager_for_ws.clone()))
        .and(warp::any().map(move || data_channels_for_ws.clone()))
        .and(warp::any().map(move || client_states_for_ws.clone()))
        .and(warp::any().map(move || chat_messages_for_ws.clone()))
        .and(warp::any().map(move || config_for_ws.clone()))
        .and(warp::any().map(move || player_aois_for_ws.clone()))
        .and(warp::any().map(move || server_instance_for_ws.clone())) // Pass server instance Arc
        .and(warp::any().map(move || auth_service_for_ws.clone()))
        .and(warp::any().map(move || scaling_coordinator_for_ws.clone()))
        .map(
            |ws: warp::ws::Ws,
             ws_auth_query: WsAuthQuery,
             request_headers: HeaderMap,
             remote_addr: Option<SocketAddr>,
             s_peers: SignalingPeers,
             p_manager: PlayerManagerRef,
             w_p_manager: WorldPartitionManagerRef,
             d_channels: DataChannelsMap,
             c_states: ClientStatesMap,
             chats: ChatMessagesQueue,
             conf: Arc<ServerConfig>,
             p_aois: Arc<DashMap<String, PlayerAoI>>,
             server_inst: ServerInstanceRef,
             auth_service: AuthService,
             scaling_coordinator: Arc<HorizontalScalingCoordinator>| {
                // Accept server instance Arc
                let peer_id = Uuid::new_v4().to_string();
                let requested_team_id = ws_auth_query.requested_team_id();
                let auth_token = ws_auth_query
                    .auth_token
                    .or(ws_auth_query.token)
                    .unwrap_or_default();
                let auth_user_id = auth_service.resolve_user_id_from_token(&auth_token);
                if let Some(bound_user_id) = auth_user_id.as_deref() {
                    if let Some(profile) = auth_service.profile_by_user_id(bound_user_id) {
                        let routing_key = format!("user:{}", bound_user_id);
                        let assignment = scaling_coordinator
                            .assignment_for_match_with_mmr(&routing_key, profile.mmr);
                        info!(
                            "MMR shard hint for {} (band={}, mmr={:.1}): primary={}, replicas={:?}",
                            bound_user_id,
                            profile.mmr_band,
                            profile.mmr,
                            assignment.primary_shard,
                            assignment.replica_shards
                        );
                    }
                }
                let forwarded_ip = request_headers
                    .get("x-forwarded-for")
                    .and_then(|value| value.to_str().ok())
                    .and_then(parse_forwarded_for_ip);
                let real_ip = request_headers
                    .get("x-real-ip")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.trim().parse::<IpAddr>().ok());
                let client_ip = forwarded_ip
                    .or(real_ip)
                    .or_else(|| remote_addr.map(|addr| addr.ip()));
                let remote_context = monitoring_tracing::extract_remote_context(
                    request_headers
                        .get("traceparent")
                        .and_then(|value| value.to_str().ok()),
                    request_headers
                        .get("tracestate")
                        .and_then(|value| value.to_str().ok()),
                );
                let ws_upgrade_span = tracing::info_span!(
                    "ws_signaling_connection",
                    peer_id = %peer_id,
                    transport = "webrtc",
                    auth_user_id = auth_user_id.as_deref().unwrap_or("anonymous")
                );
                ws_upgrade_span.set_parent(remote_context);

                let is_mobile = ws_auth_query.is_mobile.unwrap_or(false);
                let requested_match_type = ws_auth_query.match_type.as_deref().unwrap_or("full");
                let match_type = massive_game_server_core::server::instance::MatchType::from_query_str(requested_match_type);
                info!(
                    "WS connection: peer={}, match_type={}, is_mobile={}",
                    peer_id, match_type, is_mobile
                );
                // Record human queue arrival for quick-match bot-fill delay tracking.
                if match_type == massive_game_server_core::server::instance::MatchType::QuickMatch {
                    server_inst.note_human_queue_arrival();
                }
                ws.on_upgrade(move |socket| {
                    handle_signaling_connection(
                        socket,
                        peer_id,
                        s_peers,
                        p_manager,
                        w_p_manager,
                        d_channels,
                        c_states,
                        chats,
                        conf,
                        p_aois,
                        server_inst, // Pass server instance to handler
                        auth_service,
                        auth_user_id,
                        requested_team_id,
                        client_ip,
                        is_mobile,
                    )
                    .instrument(ws_upgrade_span)
                })
            },
        )
        .boxed();

    let signaling_route = if quic_primary_only {
        warp::path("ws")
            .and(warp::path::end())
            .and(warp::get())
            .map(|| {
                warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "error": "quic_primary_only",
                        "detail": "WebSocket signaling is disabled. Use QUIC primary transport."
                    })),
                    StatusCode::UPGRADE_REQUIRED,
                )
            })
            .map(warp::reply::Reply::into_response)
            .boxed()
    } else {
        signaling_route_ws
            .map(warp::reply::Reply::into_response)
            .boxed()
    };

    let static_asset_allow_origin = std::env::var("MGS_CDN_ORIGIN")
        .ok()
        .map(|raw| raw.trim().to_owned())
        .filter(|raw| !raw.is_empty());
    if let Some(origin) = static_asset_allow_origin.as_deref() {
        info!(
            "Static asset CORS origin override enabled for CDN/cache distribution: {}",
            origin
        );
    }

    let root_route = warp::path::end()
        .and(warp::get())
        .map(|| warp::redirect::temporary(Uri::from_static("/index.html")));

    let server_for_healthz = game_server_instance.clone();
    let healthz_route = warp::path("healthz")
        .and(warp::path::end())
        .and(warp::get())
        .map(move || {
            let last_tick = server_for_healthz
                .last_tick_epoch_ms
                .load(std::sync::atomic::Ordering::Relaxed);
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_millis() as u64;
            let tick_age_ms = now_ms.saturating_sub(last_tick);

            // Consider the game loop stalled if no tick completed in the last 2 seconds.
            // A last_tick of 0 means the loop hasn't started yet which is acceptable
            // during startup; the readyz endpoint covers that case.
            let game_loop_alive = last_tick == 0 || tick_age_ms <= 2_000;

            if game_loop_alive {
                warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "ok": true,
                        "service": "massive_game_server",
                        "last_tick_age_ms": tick_age_ms,
                    })),
                    StatusCode::OK,
                )
            } else {
                warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "ok": false,
                        "service": "massive_game_server",
                        "error": "game_loop_stalled",
                        "last_tick_age_ms": tick_age_ms,
                    })),
                    StatusCode::SERVICE_UNAVAILABLE,
                )
            }
        })
        .map(warp::reply::Reply::into_response);

    let server_for_readyz = game_server_instance.clone();
    let readyz_route = warp::path("readyz")
        .and(warp::path::end())
        .and(warp::get())
        .map(move || {
            let last_tick = server_for_readyz
                .last_tick_epoch_ms
                .load(std::sync::atomic::Ordering::Relaxed);
            let frame = server_for_readyz
                .frame_counter
                .load(std::sync::atomic::Ordering::Relaxed);

            // Server is ready once the game loop has completed at least one tick.
            let ready = last_tick > 0 && frame > 0;

            if ready {
                warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "ok": true,
                        "service": "massive_game_server",
                        "frame": frame,
                    })),
                    StatusCode::OK,
                )
            } else {
                warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "ok": false,
                        "service": "massive_game_server",
                        "error": "not_ready",
                        "frame": frame,
                    })),
                    StatusCode::SERVICE_UNAVAILABLE,
                )
            }
        })
        .map(warp::reply::Reply::into_response);

    let static_files_route =
        warp::fs::dir("static_client").map(move |reply: warp::filters::fs::File| {
            let requested_path = reply.path().to_path_buf();
            let cache_control = static_cache_control_for_path(&requested_path);
            let mut response = reply.into_response();
            let headers = response.headers_mut();

            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static(cache_control),
            );
            headers.insert(header::VARY, HeaderValue::from_static("Accept-Encoding"));
            headers.insert(
                HeaderName::from_static("timing-allow-origin"),
                HeaderValue::from_static("*"),
            );
            headers.insert(
                HeaderName::from_static("cross-origin-resource-policy"),
                HeaderValue::from_static("cross-origin"),
            );

            if let Some(origin) = static_asset_allow_origin.as_deref() {
                if let Ok(header_value) = HeaderValue::from_str(origin) {
                    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, header_value);
                }
            }

            response
        });

    let admin_auth_config = AdminAuthConfig::from_env();
    let admin_routes = arena_routes
        .or(code_generation_routes)
        .or(feature_flag_routes)
        .or(join_stage_report_route)
        .or(join_stage_reset_route)
        .or(live_replay_recent_route)
        .or(live_replay_dispute_route)
        .or(live_replay_dispute_recent_route)
        .or(match_summary_latest_route)
        .or(killcam_latest_route)
        .or(match_type_route)
        .map(warp::reply::Reply::into_response)
        .boxed();

    let protected_routes = requires_admin_auth(admin_auth_config)
        .and(admin_routes)
        .map(|(), reply| reply)
        .boxed();

    let public_routes = auth_routes
        .or(signaling_route)
        .or(root_route)
        .or(healthz_route)
        .or(readyz_route)
        .or(static_files_route)
        .map(warp::reply::Reply::into_response)
        .boxed();

    let allowed_cors_origins = parse_list_env("MGS_ALLOWED_ORIGINS");
    let base_routes = protected_routes.or(public_routes).boxed();
    let recovered_routes = base_routes.recover(handle_route_rejection);

    let routes = if allowed_cors_origins.is_empty() {
        info!(
            "No cross-origin API origins configured (set MGS_ALLOWED_ORIGINS for explicit allowlist)."
        );
        recovered_routes
            .map(warp::reply::Reply::into_response)
            .boxed()
    } else {
        for origin in &allowed_cors_origins {
            info!("Allowing API CORS origin: {}", origin);
        }
        recovered_routes
            .with(
                warp::cors()
                    .allow_origins(allowed_cors_origins.iter().map(String::as_str))
                    .allow_methods(vec!["GET", "POST", "OPTIONS"])
                    .allow_headers(vec![
                        "Content-Type",
                        "Authorization",
                        "User-Agent",
                        "Sec-WebSocket-Key",
                        "Sec-WebSocket-Version",
                        "Sec-WebSocket-Extensions",
                        "Upgrade",
                        "Connection",
                    ]),
            )
            .map(warp::reply::Reply::into_response)
            .boxed()
    };

    let game_server_for_loop = Arc::clone(&game_server_instance); // Use the renamed variable
    let game_loop_handle = tokio::spawn(async move {
        info!("Starting game loop...");
        game_server_for_loop.run_game_loop().await;
        info!("Game loop stopped.");
    });

    // Periodic idle connection cleanup: evict peers that have not sent any
    // traffic for 120 seconds.  This calls stale_peer_ids() which was previously
    // defined but never wired into the runtime.
    {
        let stale_signaling_peers = signaling_peers_state.clone();
        let stale_player_manager = game_server_instance.player_manager.clone();
        let stale_data_channels = game_server_instance.data_channels_map.clone();
        let stale_client_states = game_server_instance.client_states_map.clone();
        let stale_player_aois = game_server_instance.player_aois.clone();
        let stale_auth_service = auth_service.clone();
        let stale_shutdown_flag = game_server_instance.is_shutting_down.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(30));
            let stale_threshold = Duration::from_secs(120);
            loop {
                ticker.tick().await;
                if stale_shutdown_flag.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                let stale_ids = shared_connection_manager().stale_peer_ids(stale_threshold);
                if !stale_ids.is_empty() {
                    info!(
                        "Idle connection cleanup: evicting {} stale peer(s).",
                        stale_ids.len()
                    );
                    for peer_id in &stale_ids {
                        cleanup_connection(
                            peer_id,
                            &stale_signaling_peers,
                            &stale_player_manager,
                            &stale_data_channels,
                            &stale_client_states,
                            &stale_player_aois,
                            &stale_auth_service,
                        );
                    }
                }
            }
        });
    }

    let arena_worker_shutdown_server = game_server_instance.clone();
    let arena_worker_enabled = std::env::var("MGS_ARENA_WORKER_ENABLED")
        .ok()
        .map(|raw| {
            let normalized = raw.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "yes" || normalized == "on"
        })
        .unwrap_or(false);
    if arena_worker_enabled {
        let worker_interval_ms = std::env::var("MGS_ARENA_WORKER_INTERVAL_MS")
            .ok()
            .and_then(|raw| raw.parse::<u64>().ok())
            .filter(|value| *value >= 100)
            .unwrap_or(1000);
        let worker_max_ticks = std::env::var("MGS_ARENA_WORKER_MAX_TICKS")
            .ok()
            .and_then(|raw| raw.parse::<u32>().ok())
            .filter(|value| *value > 0);
        tokio::spawn(async move {
            let mut ticker =
                tokio::time::interval(std::time::Duration::from_millis(worker_interval_ms));
            info!(
                "Arena worker enabled (interval_ms={}, max_ticks={:?}).",
                worker_interval_ms, worker_max_ticks
            );
            loop {
                ticker.tick().await;
                if arena_worker_shutdown_server.is_shutdown_requested() {
                    info!("Arena worker shutdown requested; stopping worker loop.");
                    break;
                }
                match arena_service_for_worker.worker_execute_next(worker_max_ticks, None) {
                    Ok(Some(executed)) => {
                        info!(
                            "Arena worker executed match '{}' mode={} (pending {} -> {}, draw={}, winner={:?}, objective {}:{}-{}, runtimes=({},{}) ).",
                            executed.report.match_id,
                            executed.sandbox.mode,
                            executed.pending_before,
                            executed.pending_after,
                            executed.sandbox.draw,
                            executed.sandbox.winner_model_id,
                            executed.sandbox.objective_label,
                            executed.sandbox.objective_a,
                            executed.sandbox.objective_b,
                            executed.sandbox.model_a_runtime,
                            executed.sandbox.model_b_runtime
                        );
                    }
                    Ok(None) => {}
                    Err(err) => warn!("Arena worker execute_next failed: {}", err),
                }
            }
            info!("Arena worker stopped.");
        });
    }

    let bind_host = std::env::var("MGS_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let bind_port = std::env::var("MGS_PORT")
        .ok()
        .and_then(|raw| raw.parse::<u16>().ok())
        .unwrap_or(8080);
    let server_address: SocketAddr =
        format!("{}:{}", bind_host, bind_port)
            .parse()
            .map_err(|err| {
                anyhow::anyhow!("Invalid bind address {}:{} ({})", bind_host, bind_port, err)
            })?;

    let default_quic_bind_addr = SocketAddr::new(server_address.ip(), bind_port.saturating_add(1));
    let server_for_quic = game_server_instance.clone();
    let quic_request_handler: QuicRequestHandler = Arc::new(move |payload: &[u8]| {
        let request = serde_json::from_slice::<QuicControlRequest>(payload).unwrap_or_default();
        let op = request.op.unwrap_or_else(|| "echo".to_string());

        let response = match op.as_str() {
            "healthz" => serde_json::json!({
                "ok": true,
                "op": "healthz",
                "frame": server_for_quic.frame_counter.load(std::sync::atomic::Ordering::Relaxed),
                "players": server_for_quic.player_manager.player_count(),
                "projectiles": server_for_quic.projectiles.read().len(),
                "pickups": server_for_quic.pickups.read().len(),
                "ts_ms": server_for_quic.get_server_timestamp_ms(),
            }),
            "live_replay_recent" => {
                let limit = request.replay_limit.unwrap_or(128).clamp(1, 4096);
                serde_json::json!({
                    "ok": true,
                    "op": "live_replay_recent",
                    "frames": server_for_quic.recent_live_replay_frames(limit),
                    "limit": limit,
                })
            }
            "live_replay_disputes_recent" => {
                let limit = request.replay_limit.unwrap_or(128).clamp(1, 2048);
                serde_json::json!({
                    "ok": true,
                    "op": "live_replay_disputes_recent",
                    "audits": server_for_quic.recent_live_replay_dispute_audits(limit),
                    "limit": limit,
                })
            }
            "live_replay_dispute" => {
                let report =
                    server_for_quic.build_live_replay_dispute_report(LiveReplayDisputeRequest {
                        from_frame: request.from_frame,
                        to_frame: request.to_frame,
                        limit: request.replay_limit,
                        player_id: request.player_id,
                    });
                return serde_json::to_vec(&report).ok();
            }
            "join" => {
                let peer_id = request
                    .peer_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                match peer_id {
                    Some(peer_id) => {
                        let joined = server_for_quic.register_quic_player(
                            peer_id,
                            request.username.as_deref(),
                            request.team_id,
                        );
                        serde_json::json!({
                            "ok": joined.is_some(),
                            "op": "join",
                            "player": joined,
                        })
                    }
                    None => serde_json::json!({
                        "ok": false,
                        "op": "join",
                        "error": "missing_peer_id",
                    }),
                }
            }
            "input" => {
                let peer_id = request
                    .peer_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let Some(peer_id) = peer_id else {
                    return serde_json::to_vec(&serde_json::json!({
                        "ok": false,
                        "op": "input",
                        "error": "missing_peer_id",
                    }))
                    .ok();
                };

                let mut inputs = Vec::new();
                if let Some(single_input) = request.input {
                    inputs.push(single_input.into_player_input());
                }
                if let Some(batch_inputs) = request.inputs {
                    for input in batch_inputs.into_iter().take(128) {
                        inputs.push(input.into_player_input());
                    }
                }

                let mut accepted = 0usize;
                for input in inputs {
                    if server_for_quic.enqueue_quic_input(peer_id, input) {
                        accepted += 1;
                    } else {
                        break;
                    }
                }

                serde_json::json!({
                    "ok": accepted > 0,
                    "op": "input",
                    "accepted": accepted,
                    "peer_id": peer_id,
                })
            }
            "leave" | "disconnect" => {
                if let Some(peer_id) = request
                    .peer_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    server_for_quic.remove_quic_player(peer_id);
                    serde_json::json!({
                        "ok": true,
                        "op": "leave",
                        "peer_id": peer_id,
                    })
                } else {
                    serde_json::json!({
                        "ok": false,
                        "op": "leave",
                        "error": "missing_peer_id",
                    })
                }
            }
            _ => serde_json::json!({
                "ok": true,
                "op": "echo",
                "bytes": payload.len(),
                "trace_headers": monitoring_tracing::inject_current_context_headers(),
            }),
        };

        serde_json::to_vec(&response).ok()
    });
    let quic_runtime = start_quic_runtime_from_env_with_handler(
        default_quic_bind_addr,
        Some(quic_request_handler),
    )?;
    if let Some(runtime) = quic_runtime.as_ref() {
        info!(
            "QUIC primary transport is enabled and listening on {}.",
            runtime.local_addr()
        );
    }

    if quic_primary_only {
        info!(
            "WebSocket signaling endpoint ws://{}/ws is disabled (QUIC primary only mode).",
            server_address
        );
    } else {
        info!("Signaling server listening on ws://{}/ws", server_address);
    }
    info!("Client files served from http://{}/", server_address);

    let server_for_shutdown = game_server_instance.clone();
    let (_bound_address, server) =
        warp::serve(routes).bind_with_graceful_shutdown(server_address, async move {
            lifecycle::request_shutdown_on_signal(server_for_shutdown).await;
        });
    let shutdown_started_at = Instant::now();
    server.await;
    drop(quic_runtime);

    if backup_manager.enabled() {
        if let Err(err) = backup_manager.run_once("shutdown").await {
            warn!("Final shutdown backup failed: {}", err);
        }
    }

    let mut game_loop_handle = game_loop_handle;
    let shutdown_drain_timeout_secs = parse_u64_env("MGS_SHUTDOWN_DRAIN_TIMEOUT_SECONDS", 20);
    lifecycle::drain_game_loop_with_timeout(
        &mut game_loop_handle,
        Duration::from_secs(shutdown_drain_timeout_secs),
    )
    .await;

    monitoring_metrics::record_shutdown_duration(shutdown_started_at.elapsed().as_secs_f64());
    info!("Massive Game Server shut down.");
    Ok(())
}
