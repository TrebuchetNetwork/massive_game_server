// massive_game_server/server/src/main.rs
use dashmap::DashMap;
use massive_game_server_core::concurrent::thread_pools::ThreadPoolSystem;
use massive_game_server_core::core::types::PlayerAoI;
use massive_game_server_core::network::quic::control::build_quic_control_handler;
use massive_game_server_core::network::quic::{
    register_quic_disconnect_hook, start_quic_runtime_from_env_with_handler,
};
use massive_game_server_core::network::signaling::{
    configure_signaling_runtime,
    BoundedChatQueue,
    ChatMessagesQueue,
    ClientStatesMap,
    DataChannelsMap,
    ServerInstanceRef, // Added ServerInstanceRef
    SignalingPeers,
    MAX_CHAT_QUEUE_SIZE,
};
use massive_game_server_core::operational::admin_auth::{requires_admin_auth, AdminAuthConfig};
use massive_game_server_core::operational::arena::{build_arena_routes, ArenaService};
use massive_game_server_core::operational::auth::{build_auth_routes, AuthService};
use massive_game_server_core::operational::backup::BackupManager;
use massive_game_server_core::operational::code_generation::{
    build_code_generation_routes, CodeGenerationService,
};
use massive_game_server_core::operational::config::env_registry::load_app_env_config;
use massive_game_server_core::operational::config::load_validated_server_config;
use massive_game_server_core::operational::diagnostics::{deadlock, heap_profiler, panic_log};
use massive_game_server_core::operational::feature_flags::{
    build_feature_flag_routes, configure_feature_flags_runtime, FeatureFlagService,
};
use massive_game_server_core::operational::monitoring::{
    metrics as monitoring_metrics, tracing as monitoring_tracing,
};
use massive_game_server_core::routes::admin::build_ops_admin_routes;
use massive_game_server_core::routes::app::compose_http_routes;
use massive_game_server_core::routes::health::{build_healthz_route, build_readyz_route};
use massive_game_server_core::routes::static_files::{build_root_route, build_static_files_route};
use massive_game_server_core::routes::ws_signaling::{
    build_signaling_route, build_ws_security_filters,
};
use massive_game_server_core::scaling::HorizontalScalingCoordinator;
use massive_game_server_core::server::background_tasks::{
    spawn_alert_evaluator, spawn_arena_worker, spawn_backup_worker, spawn_idle_connection_cleanup,
};
use massive_game_server_core::server::instance::{configure_instance_runtime, MassiveGameServer};
use massive_game_server_core::server::lifecycle;

use parking_lot::RwLock as ParkingLotRwLock;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};
use warp::Filter;

fn init_logging() -> anyhow::Result<()> {
    monitoring_tracing::init_tracing_subscriber(
        "massive_game_server_core=info,warp=info,webrtc=warn,signaling=info",
    )
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    panic_log::install_panic_logging_hook();

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
    let app_env = load_app_env_config()
        .map_err(|err| anyhow::anyhow!("invalid environment configuration: {}", err))?;
    configure_signaling_runtime(&app_env.signaling);
    configure_instance_runtime(&app_env.instance);
    configure_feature_flags_runtime(&app_env.admin_auth);

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
    let chat_messages_state: ChatMessagesQueue = Arc::new(tokio::sync::RwLock::new(
        BoundedChatQueue::new(MAX_CHAT_QUEUE_SIZE),
    ));
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

    if let Some(map_path) = app_env.map_path.as_deref() {
        info!("Custom map loaded from: {}", map_path);
    }

    if app_env.diagnostics.enabled {
        deadlock::spawn_frame_progress_watchdog(
            game_server_instance.frame_counter.clone(),
            std::time::Duration::from_millis(app_env.diagnostics.frame_watchdog_check_ms),
            std::time::Duration::from_millis(app_env.diagnostics.frame_watchdog_stale_ms),
        );
        heap_profiler::spawn_heap_snapshot_logger(std::time::Duration::from_secs(30));
        info!("Background diagnostics enabled.");
    }

    let backup_manager = BackupManager::from_env_config(&app_env.backup);
    spawn_backup_worker(backup_manager.clone(), game_server_instance.clone());
    spawn_alert_evaluator(game_server_instance.clone());

    let signaling_peers_state: SignalingPeers = Arc::new(DashMap::new());
    let auth_service = AuthService::new_from_env_config_with_cookie_security(
        &app_env.auth,
        app_env.ws_security.behind_tls_proxy,
    );
    let arena_service = ArenaService::new_from_env();
    let arena_service_for_worker = arena_service.clone();
    let code_generation_service = CodeGenerationService::new_from_env();
    let feature_flag_service = FeatureFlagService::new_from_env_config(&app_env.feature_flags);
    let auth_routes = build_auth_routes(auth_service.clone());
    // Start the background GDPR deletion processor (runs hourly)
    auth_service.clone().start_deletion_processor();
    let arena_routes = build_arena_routes(arena_service.clone());
    let code_generation_routes = build_code_generation_routes(code_generation_service);
    let feature_flag_routes = build_feature_flag_routes(feature_flag_service);
    let ops_admin_routes = build_ops_admin_routes(
        game_server_instance.clone(),
        app_env.live_replay_enabled,
        backup_manager.clone(),
    );

    let quic_primary_only = app_env.quic_primary_only;
    if quic_primary_only {
        info!(
            "MGS_QUIC_PRIMARY_ONLY enabled: WebSocket signaling endpoint /ws will reject upgrades."
        );
    }

    let ws_security = build_ws_security_filters(
        signaling_peers_state.clone(),
        game_server_instance.effective_max_players() as u64,
        &app_env.ws_security,
    );
    let behind_tls_proxy = ws_security.behind_tls_proxy;

    let signaling_route = build_signaling_route(
        config.clone(),
        signaling_peers_state.clone(),
        game_server_instance.player_manager.clone(),
        game_server_instance.world_partition_manager.clone(),
        data_channels_state.clone(),
        client_states_state.clone(),
        chat_messages_state.clone(),
        player_aois_state.clone(),
        game_server_instance.clone(),
        auth_service.clone(),
        scaling_coordinator.clone(),
        ws_security.ws_require_auth,
        quic_primary_only,
        ws_security.origin_check_filter,
        ws_security.ws_connection_cap_filter,
    );

    let static_asset_allow_origin = app_env.cdn_origin.clone();
    if let Some(origin) = static_asset_allow_origin.as_deref() {
        info!(
            "Static asset CORS origin override enabled for CDN/cache distribution: {}",
            origin
        );
    }

    let root_route = build_root_route();
    let healthz_route = build_healthz_route(game_server_instance.clone());
    let readyz_route = build_readyz_route(game_server_instance.clone());
    let static_files_route = build_static_files_route(static_asset_allow_origin.clone());

    let admin_auth_config = AdminAuthConfig::from_env_config(&app_env.admin_auth);
    let admin_routes = arena_routes
        .or(code_generation_routes)
        .or(feature_flag_routes)
        .or(ops_admin_routes)
        .map(warp::reply::Reply::into_response)
        .boxed();

    let protected_routes = requires_admin_auth(admin_auth_config)
        .and(admin_routes)
        .map(|(), reply| reply)
        .boxed();

    let public_routes = auth_routes.map(warp::reply::Reply::into_response).boxed();
    // Routes that should not be subject to HTTP CORS middleware:
    // - WebSocket signaling has explicit origin/transport guards.
    // - Static/root/health routes should be reachable directly in local mode.
    let static_routes = signaling_route
        .or(root_route)
        .or(healthz_route)
        .or(readyz_route)
        .or(static_files_route)
        .map(warp::reply::Reply::into_response)
        .boxed();

    let allowed_cors_origins = app_env.allowed_cors_origins.clone();
    let routes = compose_http_routes(
        protected_routes,
        public_routes,
        static_routes,
        allowed_cors_origins,
        behind_tls_proxy,
    );

    let game_server_for_loop = Arc::clone(&game_server_instance); // Use the renamed variable
    let game_loop_handle = tokio::spawn(async move {
        info!("Starting game loop...");
        game_server_for_loop.run_game_loop().await;
        info!("Game loop stopped.");
    });

    spawn_idle_connection_cleanup(
        signaling_peers_state.clone(),
        game_server_instance.player_manager.clone(),
        game_server_instance.data_channels_map.clone(),
        game_server_instance.client_states_map.clone(),
        game_server_instance.player_aois.clone(),
        auth_service.clone(),
        game_server_instance.is_shutting_down.clone(),
    );
    spawn_arena_worker(
        game_server_instance.clone(),
        arena_service_for_worker,
        &app_env.arena_worker,
    );

    let bind_host = app_env.network_bind.host.clone();
    let bind_port = app_env.network_bind.port;
    let server_address: SocketAddr =
        format!("{}:{}", bind_host, bind_port)
            .parse()
            .map_err(|err| {
                anyhow::anyhow!("Invalid bind address {}:{} ({})", bind_host, bind_port, err)
            })?;

    let default_quic_bind_addr = SocketAddr::new(server_address.ip(), bind_port.saturating_add(1));
    let server_for_quic = game_server_instance.clone();
    let auth_service_for_quic = auth_service.clone();
    let server_for_quic_disconnect = game_server_instance.clone();
    let auth_service_for_quic_disconnect = auth_service.clone();
    register_quic_disconnect_hook(Arc::new(move |peer_id: &str| {
        server_for_quic_disconnect.remove_quic_player(peer_id);
        auth_service_for_quic_disconnect.clear_peer_binding(peer_id);
    }));
    let quic_request_handler =
        build_quic_control_handler(server_for_quic.clone(), auth_service_for_quic.clone());
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
    lifecycle::drain_game_loop_with_timeout(
        &mut game_loop_handle,
        Duration::from_secs(app_env.shutdown_drain_timeout_secs),
    )
    .await;

    monitoring_metrics::record_shutdown_duration(shutdown_started_at.elapsed().as_secs_f64());
    info!("Massive Game Server shut down.");
    Ok(())
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
