// massive_game_server/server/src/main.rs
use dashmap::DashMap;
use massive_game_server_core::concurrent::thread_pools::ThreadPoolSystem;
use massive_game_server_core::core::config::ServerConfig;
use massive_game_server_core::core::types::PlayerAoI;
use massive_game_server_core::network::signaling::{
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
use massive_game_server_core::operational::code_generation::{
    build_code_generation_routes, CodeGenerationService,
};
use massive_game_server_core::operational::feature_flags::{
    build_feature_flag_routes, FeatureFlagService,
};
use massive_game_server_core::server::instance::MassiveGameServer;

use parking_lot::RwLock as ParkingLotRwLock;
use serde::Deserialize;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use tracing::{error, info, warn, Level};
use tracing_subscriber::{fmt, EnvFilter};
use uuid::Uuid;
use warp::http::{header, HeaderName, HeaderValue};
use warp::{Filter, Reply};

fn init_logging() -> anyhow::Result<()> {
    let subscriber = fmt::Subscriber::builder()
        .with_max_level(Level::INFO) // Adjusted default to INFO
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            "massive_game_server_core=info,warp=info,webrtc=warn,signaling=info".into()
            // Keep this specific
        }))
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .map_err(|e| anyhow::anyhow!("Failed to set global default tracing subscriber: {}", e))?;
    info!("Tracing subscriber initialized.");
    Ok(())
}

#[derive(Clone, Default, Deserialize)]
struct WsAuthQuery {
    auth_token: Option<String>,
    token: Option<String>,
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

    info!("Massive Game Server starting up...");

    let config = Arc::new(ServerConfig::default());
    info!(
        "Server configuration loaded. Tick rate: {}",
        config.tick_rate
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

    let signaling_peers_state: SignalingPeers =
        Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
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

    let signaling_route = warp::path("ws")
        .and(warp::ws())
        .and(
            warp::query::<WsAuthQuery>()
                .or(warp::any().map(WsAuthQuery::default))
                .unify(),
        )
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
        .map(
            |ws: warp::ws::Ws,
             ws_auth_query: WsAuthQuery,
             s_peers: SignalingPeers,
             p_manager: PlayerManagerRef,
             w_p_manager: WorldPartitionManagerRef,
             d_channels: DataChannelsMap,
             c_states: ClientStatesMap,
             chats: ChatMessagesQueue,
             conf: Arc<ServerConfig>,
             p_aois: Arc<DashMap<String, PlayerAoI>>,
             server_inst: ServerInstanceRef,
             auth_service: AuthService| {
                // Accept server instance Arc
                let peer_id = Uuid::new_v4().to_string();
                let auth_token = ws_auth_query
                    .auth_token
                    .or(ws_auth_query.token)
                    .unwrap_or_default();
                let auth_user_id = auth_service.resolve_user_id_from_token(&auth_token);
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
                    )
                })
            },
        );

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

    let routes = auth_routes
        .or(arena_routes)
        .or(code_generation_routes)
        .or(feature_flag_routes)
        .or(join_stage_report_route)
        .or(join_stage_reset_route)
        .or(signaling_route)
        .or(static_files_route)
        .with(
            warp::cors()
                .allow_any_origin()
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
        );

    let game_server_for_loop = Arc::clone(&game_server_instance); // Use the renamed variable
    tokio::spawn(async move {
        info!("Starting game loop...");
        game_server_for_loop.run_game_loop().await;
        info!("Game loop stopped.");
    });

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
                match arena_service_for_worker.worker_execute_next(worker_max_ticks, None) {
                    Ok(Some(executed)) => {
                        info!(
                            "Arena worker executed match '{}' (pending {} -> {}, draw={}, winner={:?}, runtimes=({},{}) ).",
                            executed.report.match_id,
                            executed.pending_before,
                            executed.pending_after,
                            executed.sandbox.draw,
                            executed.sandbox.winner_model_id,
                            executed.sandbox.model_a_runtime,
                            executed.sandbox.model_b_runtime
                        );
                    }
                    Ok(None) => {}
                    Err(err) => warn!("Arena worker execute_next failed: {}", err),
                }
            }
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

    info!("Signaling server listening on ws://{}/ws", server_address);
    info!("Client files served from http://{}/", server_address);
    warp::serve(routes).run(server_address).await;

    info!("Massive Game Server shut down.");
    Ok(())
}
