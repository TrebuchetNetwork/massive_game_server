use massive_game_server_core::concurrent::thread_pools::ThreadPoolSystem;
use massive_game_server_core::core::config::ServerConfig;
use massive_game_server_core::core::constants::{
    FULL_MATCH_DURATION_SECS, LATE_PHASE_FINAL_STAND_FULL_MATCH_REMAINING_SECS,
    LATE_PHASE_SUPPLY_WARNING_FULL_MATCH_REMAINING_SECS,
    LATE_PHASE_ZONE_SURGE_FULL_MATCH_REMAINING_SECS,
};
use massive_game_server_core::core::types::PlayerAoIs;
use massive_game_server_core::flatbuffers_generated::game_protocol as fb;
use massive_game_server_core::network::signaling::{
    BoundedChatQueue, ChatMessagesQueue, ClientStatesMap, DataChannelsMap, MAX_CHAT_QUEUE_SIZE,
};
use massive_game_server_core::operational::config::env_registry::load_app_env_config;
use massive_game_server_core::server::instance::{configure_instance_runtime, MassiveGameServer};

use dashmap::DashMap;
use parking_lot::RwLock as ParkingLotRwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock as TokioRwLock;

fn setup_test_server() -> Arc<MassiveGameServer> {
    let config = Arc::new(ServerConfig::default());
    let thread_pool_system =
        Arc::new(ThreadPoolSystem::new(config.clone()).expect("failed to create thread pools"));
    let data_channels_map: DataChannelsMap = Arc::new(DashMap::new());
    let client_states_map: ClientStatesMap = Arc::new(ParkingLotRwLock::new(HashMap::new()));
    let chat_messages_queue: ChatMessagesQueue =
        Arc::new(TokioRwLock::new(BoundedChatQueue::new(MAX_CHAT_QUEUE_SIZE)));
    let player_aois: PlayerAoIs = Arc::new(DashMap::new());

    Arc::new(MassiveGameServer::new(
        config,
        thread_pool_system,
        data_channels_map,
        client_states_map,
        chat_messages_queue,
        player_aois,
    ))
}

fn add_player(server: &MassiveGameServer, peer_id: &str, team_id: u8, x: f32, y: f32) {
    server
        .player_manager
        .add_player(peer_id.to_owned(), peer_id.to_owned(), x, y);
    let player_id = server.player_manager.id_pool.get_or_create(peer_id);
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&player_id) {
        ps.team_id = team_id;
        ps.x = x;
        ps.y = y;
        ps.alive = true;
    }
}

fn scaled_threshold(match_duration_secs: f32, seconds_for_full_match: f32) -> f32 {
    (match_duration_secs * (seconds_for_full_match / FULL_MATCH_DURATION_SECS)).max(0.0)
}

#[tokio::test(flavor = "multi_thread")]
async fn full_match_transitions_ffa_to_tdm_to_ctf_and_triggers_late_phase_events() {
    let instance_env = temp_env::with_var("MGS_DYNAMIC_MODE_TRANSITIONS", Some("1"), || {
        load_app_env_config()
            .expect("load app env config")
            .instance
            .clone()
    });
    configure_instance_runtime(&instance_env);

    let server = setup_test_server();
    add_player(&server, "player1", 1, 0.0, 0.0);
    add_player(&server, "player2", 2, 50.0, 50.0);
    server.run_game_logic_update(0.016).await;

    {
        let mut match_info = server.match_info.write();
        match_info.match_state = fb::MatchStateType::Active;
        match_info.game_mode = fb::GameModeType::FreeForAll;
        match_info.flag_states.clear();
        match_info.hot_zone_active = false;
        match_info.hot_zone_event_count = 0;
        match_info.hot_zone_elapsed_secs = 0.0;
        match_info.late_phase_supply_warning_triggered = false;
        match_info.late_phase_zone_surge_triggered = false;
        match_info.late_phase_final_stand_triggered = false;
    }

    let duration = server.match_duration_secs;
    let tdm_transition_elapsed = scaled_threshold(duration, 120.0);
    let ctf_transition_remaining = scaled_threshold(duration, 70.0);
    let supply_warning_remaining = scaled_threshold(
        duration,
        LATE_PHASE_SUPPLY_WARNING_FULL_MATCH_REMAINING_SECS,
    );
    let zone_surge_remaining =
        scaled_threshold(duration, LATE_PHASE_ZONE_SURGE_FULL_MATCH_REMAINING_SECS);
    let final_stand_remaining =
        scaled_threshold(duration, LATE_PHASE_FINAL_STAND_FULL_MATCH_REMAINING_SECS);

    {
        let mut match_info = server.match_info.write();
        match_info.time_remaining = duration - tdm_transition_elapsed + 0.01;
        match_info.game_mode = fb::GameModeType::FreeForAll;
    }
    server.run_game_logic_update(0.02).await;
    assert_eq!(
        server.match_info.read().game_mode,
        fb::GameModeType::TeamDeathmatch,
        "full match should transition from FFA to TDM after elapsed threshold"
    );

    {
        let mut match_info = server.match_info.write();
        match_info.time_remaining = ctf_transition_remaining + 0.01;
    }
    server.run_game_logic_update(0.02).await;
    {
        let match_info = server.match_info.read();
        assert_eq!(
            match_info.game_mode,
            fb::GameModeType::CaptureTheFlag,
            "full match should transition from TDM to CTF near endgame"
        );
        assert_eq!(
            match_info.flag_states.len(),
            2,
            "CTF transition should initialize both team flags"
        );
    }

    {
        let mut match_info = server.match_info.write();
        match_info.time_remaining = supply_warning_remaining + 0.01;
        match_info.late_phase_supply_warning_triggered = false;
    }
    server.run_game_logic_update(0.02).await;
    assert!(
        server.match_info.read().late_phase_supply_warning_triggered,
        "supply warning should trigger at late-phase threshold"
    );

    {
        let mut match_info = server.match_info.write();
        match_info.time_remaining = zone_surge_remaining + 0.01;
        match_info.late_phase_zone_surge_triggered = false;
        match_info.hot_zone_active = false;
    }
    server.run_game_logic_update(0.02).await;
    {
        let match_info = server.match_info.read();
        assert!(
            match_info.late_phase_zone_surge_triggered,
            "zone surge should trigger at late-phase threshold"
        );
        assert!(
            match_info.hot_zone_active,
            "zone surge should activate a hot zone"
        );
    }

    {
        let mut match_info = server.match_info.write();
        match_info.time_remaining = final_stand_remaining + 0.01;
        match_info.late_phase_final_stand_triggered = false;
    }
    server.run_game_logic_update(0.02).await;
    assert!(
        server.match_info.read().late_phase_final_stand_triggered,
        "final stand should trigger at the final late-phase threshold"
    );
}
