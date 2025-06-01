// massive_game_server/server/src/main.rs
use massive_game_server_core::server::instance::MassiveGameServer;
use massive_game_server_core::concurrent::thread_pools::ThreadPoolSystem;
use massive_game_server_core::core::config::ServerConfig;
use massive_game_server_core::network::signaling::{
    handle_signaling_connection, ChatMessagesQueue, ClientStatesMap,
    DataChannelsMap, PlayerManagerRef, SignalingPeers, WorldPartitionManagerRef, ServerInstanceRef, // Added ServerInstanceRef
};
use massive_game_server_core::core::types::PlayerAoI;
use dashmap::DashMap;

use std::collections::{VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, Level};
use tracing_subscriber::{EnvFilter, fmt};
use warp::Filter;
use uuid::Uuid;
use parking_lot::RwLock as ParkingLotRwLock;
use std::collections::HashMap;





fn init_logging() -> anyhow::Result<()> {
    let subscriber = fmt::Subscriber::builder()
        .with_max_level(Level::INFO) // Adjusted default to INFO
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            "massive_game_server_core=info,warp=info,webrtc=warn,signaling=info".into() // Keep this specific
        }))
        .finish();

    tracing::subscriber::set_global_default(subscriber)
        .map_err(|e| anyhow::anyhow!("Failed to set global default tracing subscriber: {}", e))?;
    info!("Tracing subscriber initialized.");
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {

    // MUST be the very first line
    // MUST be the very first line
    std::panic::set_hook(Box::new(|panic_info| {
        eprintln!("PANIC: {}", panic_info);
        if let Some(location) = panic_info.location() {
            eprintln!("Location: {}:{}:{}", 
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
            
            writeln!(file, "PANIC at {}: {}", 
                timestamp,
                panic_info
            ).ok();
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
    info!("Server configuration loaded. Tick rate: {}", config.tick_rate);

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
    let chat_messages_state: ChatMessagesQueue = Arc::new(tokio::sync::RwLock::new(VecDeque::with_capacity(100)));
    let player_aois_state: Arc<DashMap<String, PlayerAoI>> = Arc::new(DashMap::new());


    let game_server_instance: ServerInstanceRef = Arc::new(MassiveGameServer::new( // Changed variable name for clarity
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

    let config_for_ws = config.clone();
    let signaling_peers_for_ws = signaling_peers_state.clone();
    // Pass the Arc<MassiveGameServer> directly for its components
    let player_manager_for_ws: PlayerManagerRef = game_server_instance.player_manager.clone();
    let world_partition_manager_for_ws: WorldPartitionManagerRef = game_server_instance.world_partition_manager.clone();
    let data_channels_for_ws = data_channels_state.clone();
    let client_states_for_ws = client_states_state.clone();
    let chat_messages_for_ws = chat_messages_state.clone();
    let player_aois_for_ws = player_aois_state.clone();
    let server_instance_for_ws = game_server_instance.clone(); // Clone Arc for WebSocket handler


    let signaling_route = warp::path("ws")
        .and(warp::ws())
        .and(warp::any().map(move || signaling_peers_for_ws.clone()))
        .and(warp::any().map(move || player_manager_for_ws.clone()))
        .and(warp::any().map(move || world_partition_manager_for_ws.clone()))
        .and(warp::any().map(move || data_channels_for_ws.clone()))
        .and(warp::any().map(move || client_states_for_ws.clone()))
        .and(warp::any().map(move || chat_messages_for_ws.clone()))
        .and(warp::any().map(move || config_for_ws.clone()))
        .and(warp::any().map(move || player_aois_for_ws.clone()))
        .and(warp::any().map(move || server_instance_for_ws.clone())) // Pass server instance Arc
        .map(
            |ws: warp::ws::Ws,
             s_peers: SignalingPeers,
             p_manager: PlayerManagerRef,
             w_p_manager: WorldPartitionManagerRef,
             d_channels: DataChannelsMap,
             c_states: ClientStatesMap,
             chats: ChatMessagesQueue,
             conf: Arc<ServerConfig>,
             p_aois: Arc<DashMap<String, PlayerAoI>>,
             server_inst: ServerInstanceRef| { // Accept server instance Arc
                let peer_id = Uuid::new_v4().to_string();
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
                    )
                })
            },
        );

    let static_files_route = warp::fs::dir("static_client")
        .map(|reply: warp::filters::fs::File| {
            if reply.path().extension().map_or(false, |ext| ext == "html") {
                warp::reply::with_header(reply, "Cache-Control", "no-cache, no-store, must-revalidate")
            } else {
                warp::reply::with_header(reply, "Cache-Control", "public, max-age=3600")
            }
        });

    let routes = signaling_route
        .or(static_files_route)
        .with(warp::cors().allow_any_origin().allow_methods(vec!["GET", "POST", "OPTIONS"]).allow_headers(vec!["Content-Type", "User-Agent", "Sec-WebSocket-Key", "Sec-WebSocket-Version", "Sec-WebSocket-Extensions", "Upgrade", "Connection"]));

    let game_server_for_loop = Arc::clone(&game_server_instance); // Use the renamed variable
    tokio::spawn(async move {
        info!("Starting game loop...");
        game_server_for_loop.run_game_loop().await;
        info!("Game loop stopped.");
    });

    let server_address = ([0, 0, 0, 0], 8080);
    info!("Signaling server listening on ws://0.0.0.0:8080/ws");
    info!("Client files served from [http://0.0.0.0:8080/](http://0.0.0.0:8080/)");
    warp::serve(routes).run(server_address).await;

    info!("Massive Game Server shut down.");
    Ok(())
}