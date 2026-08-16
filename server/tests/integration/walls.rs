// massive_game_server/server/tests/integration/walls.rs

use massive_game_server_core::concurrent::thread_pools::ThreadPoolSystem;
use massive_game_server_core::core::config::ServerConfig;
use massive_game_server_core::core::types::{
    EntityId, PlayerID, Projectile, ServerWeaponType, Wall,
};
use massive_game_server_core::server::instance::MassiveGameServer;
// Removed unused DataChannelsMap, ClientStatesMap, ChatMessagesQueue (type aliases)
// Removed unused PlayerAoIs
use massive_game_server_core::core::types::PlayerAoIs;
use massive_game_server_core::network::signaling::{
    BoundedChatQueue, ChatMessagesQueue, MAX_CHAT_QUEUE_SIZE,
};

use dashmap::DashMap;
use parking_lot::RwLock as ParkingLotRwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock as TokioRwLock;
use tracing::info; // For debug logging in test

struct TestServerContext {
    server: Arc<MassiveGameServer>,
}

async fn setup_test_server() -> TestServerContext {
    // Initialize logging for tests if it hasn't been, to see debug messages
    // This is a simple way; a more robust solution might use a once_cell or dedicated test setup.
    // let _ = tracing_subscriber::fmt().with_max_level(tracing::Level::DEBUG).try_init();

    let config = Arc::new(ServerConfig::default());
    let thread_pools = Arc::new(
        ThreadPoolSystem::new(config.clone()).expect("Failed to create thread pools for test"),
    );

    let data_channels_map = Arc::new(DashMap::new());
    let client_states_map = Arc::new(ParkingLotRwLock::new(HashMap::new()));

    let chat_messages_queue: ChatMessagesQueue =
        Arc::new(TokioRwLock::new(BoundedChatQueue::new(MAX_CHAT_QUEUE_SIZE)));
    let player_aois: PlayerAoIs = Arc::new(DashMap::new());

    let server = Arc::new(MassiveGameServer::new(
        config,
        thread_pools,
        data_channels_map,
        client_states_map,
        chat_messages_queue,
        player_aois,
    ));
    TestServerContext { server }
}

fn add_player(server: &MassiveGameServer, peer_id: &str, team_id: u8, x: f32, y: f32) -> PlayerID {
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
    player_id
}

fn create_destructible_wall(
    server_context: &TestServerContext,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    health: i32,
) -> EntityId {
    let wall_id = rand::random::<u64>();
    let wall = Wall {
        id: wall_id,
        x,
        y,
        width,
        height,
        is_destructible: true,
        current_health: health,
        max_health: health,
    };

    let partition_idx = server_context
        .server
        .world_partition_manager
        .get_partition_index_for_point(x + width / 2.0, y + height / 2.0);
    if let Some(partition) = server_context
        .server
        .world_partition_manager
        .get_partition(partition_idx)
    {
        partition.add_wall_on_load(wall.clone());
    } else {
        panic!("Test setup error: Could not find partition to add wall during create_destructible_wall.");
    }
    server_context
        .server
        .wall_respawn_manager
        .register_wall(&wall);
    info!(
        "[Test] Created wall ID {} at ({}, {}) with health {}",
        wall_id, x, y, health
    );
    wall_id
}

fn fire_projectile_at_wall(
    server_context: &TestServerContext,
    wall_id: EntityId,
    _intended_damage: i32,
    projectile_owner_id_str: &str,
) {
    let wall_opt = server_context
        .server
        .collect_all_walls_current_state()
        .into_iter()
        .find(|w| w.id == wall_id);

    if let Some(wall_data) = wall_opt {
        let player_id_arc = server_context
            .server
            .player_manager
            .id_pool
            .get_or_create(projectile_owner_id_str);

        // Adjust projectile starting position to ensure collision within one tick.
        // Wall X is wall_data.x. Projectile needs to cross this.
        // Sniper speed is 700. Delta_time is 0.016. Distance covered = 700 * 0.016 = 11.2 units.
        // Start projectile just before the wall, e.g., 5 units before.
        let projectile_start_x = wall_data.x - 5.0;
        let projectile_start_y = wall_data.y + wall_data.height / 2.0;

        info!(
            "[Test] Firing projectile at wall ID {}. Wall X: {}. Projectile Start X: {}",
            wall_id, wall_data.x, projectile_start_x
        );

        let projectile = Projectile::new(
            player_id_arc,
            ServerWeaponType::Sniper,
            projectile_start_x,
            projectile_start_y,
            1.0,
            0.0,
            2.0, // Damage multiplier (50 base * 2.0 = 100 damage)
        );

        server_context.server.projectiles_to_add.push(projectile);
    } else {
        panic!(
            "Test setup error: Wall with ID {} not found for firing projectile.",
            wall_id
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn player_slides_along_wall_on_diagonal_impact() {
    let test_ctx = setup_test_server().await;
    let server = &test_ctx.server;

    // Indestructible vertical wall: x in [500, 520], y in [300, 500].
    // Location chosen away from the generated map's SlowZone (x in [-95, 95]),
    // DamageZones (|x| in [140, 260], y in [-70, 70]) and boost pads (y in [-220, 220]).
    // The spatial index is rebuilt with ONLY this wall so procedurally generated
    // map walls cannot interfere with the assertion.
    let wall = Wall {
        id: 424242,
        x: 500.0,
        y: 300.0,
        width: 20.0,
        height: 200.0,
        is_destructible: false,
        current_health: 1000,
        max_health: 1000,
    };
    server.wall_spatial_index.rebuild(&[wall], 0);

    // Player just left of the wall surface (PLAYER_RADIUS = 15), moving diagonally
    // into it. Speed ~141 < WALL_SLAM_STUN_SPEED_THRESHOLD (170), so no slam stun.
    let start_x = 500.0 - 15.0 - 1.0; // 1 unit of clearance from the wall face
    let pid = add_player(server, "wall_slider", 1, start_x, 400.0);
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.velocity_x = 100.0;
        ps.velocity_y = 100.0;
    }

    server.run_physics_update(0.016).await;

    let ps = server
        .player_manager
        .get_player_state(&pid)
        .expect("player state missing after physics update");

    assert!(
        ps.velocity_y >= 80.0,
        "tangential velocity component should be preserved while sliding, got velocity_y={}",
        ps.velocity_y
    );
    assert!(
        ps.velocity_x.abs() < 1.0,
        "normal velocity component should be cancelled by the wall, got velocity_x={}",
        ps.velocity_x
    );
    assert!(
        ps.x <= 500.0 - 15.0 + 0.001,
        "player must be resolved out of wall penetration, got x={}",
        ps.x
    );
    assert_eq!(
        ps.last_valid_position,
        (ps.x, ps.y),
        "last_valid_position must be synced to the resolved slide position (anti-cheat relies on it)"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn wall_slam_stun_stops_player_on_impact_tick() {
    let test_ctx = setup_test_server().await;
    let server = &test_ctx.server;

    // Same isolated wall setup as the slide test.
    let wall = Wall {
        id: 424243,
        x: 500.0,
        y: 300.0,
        width: 20.0,
        height: 200.0,
        is_destructible: false,
        current_health: 1000,
        max_health: 1000,
    };
    server.wall_spatial_index.rebuild(&[wall], 0);

    // Head-on impact at 200 u/s >= WALL_SLAM_STUN_SPEED_THRESHOLD (170): the slam
    // path must keep the original behavior — full position revert + zeroed velocity —
    // because the stun itself does not stop the player (bots have no input pass that
    // would zero it later).
    let start_x = 500.0 - 15.0 - 1.0;
    let pid = add_player(server, "wall_slammer", 1, start_x, 400.0);
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.velocity_x = 200.0;
        ps.velocity_y = 0.0;
    }

    server.run_physics_update(0.016).await;

    let ps = server
        .player_manager
        .get_player_state(&pid)
        .expect("player state missing after physics update");

    assert!(
        ps.is_wall_slam_stunned() && ps.wall_slam_stun_remaining > 0.0,
        "slam stun should be applied, wall_slam_stun_remaining={}",
        ps.wall_slam_stun_remaining
    );
    assert_eq!(
        (ps.x, ps.y),
        (start_x, 400.0),
        "slam impact must revert the position to the pre-collision tick position"
    );
    assert_eq!(
        (ps.velocity_x, ps.velocity_y),
        (0.0, 0.0),
        "slam impact must zero velocity on the impact tick"
    );
    assert_eq!(
        ps.last_valid_position,
        (start_x, 400.0),
        "last_valid_position must follow the slam revert"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn duplicate_wall_destruction_is_ignored() {
    let test_ctx = setup_test_server().await;
    let server = &test_ctx.server;

    let attacker_id_str = "test_attacker_duplicate_destroy";
    server.player_manager.add_player(
        attacker_id_str.to_string(),
        "Attacker".to_string(),
        0.0,
        0.0,
    );

    let wall_id = create_destructible_wall(&test_ctx, 100.0, 100.0, 50.0, 50.0, 100);

    // Rebuild the wall spatial index so the new wall is visible to projectile collision queries.
    let all_walls = server.collect_all_walls_current_state();
    server.wall_spatial_index.rebuild(&all_walls, 0);

    fire_projectile_at_wall(&test_ctx, wall_id, 100, attacker_id_str);
    fire_projectile_at_wall(&test_ctx, wall_id, 100, attacker_id_str);

    server.run_game_logic_update(0.016).await;
    info!(
        "[Test] After first game_logic_update. Projectiles in add queue: {}",
        server.projectiles_to_add.len()
    );
    // It's useful to see how many projectiles are in the main list after this, but that list is private.
    // We can infer by checking `projectiles_to_add` is empty.

    server.run_physics_update(0.016).await;
    info!("[Test] After physics_update.");

    server.run_game_logic_update(0.016).await;
    info!("[Test] After second game_logic_update.");

    let wall_center_x = 100.0 + 50.0 / 2.0;
    let wall_center_y = 100.0 + 50.0 / 2.0;
    let partition_idx = server
        .world_partition_manager
        .get_partition_index_for_point(wall_center_x, wall_center_y);

    let final_health = if let Some(partition) =
        server.world_partition_manager.get_partition(partition_idx)
    {
        if let Some(wall_data_entry) = partition.all_walls_in_partition.get(&wall_id) {
            let current = wall_data_entry.value().current_health;
            info!(
                "[Test] Wall ID {} found in partition. Final health from DashMap: {}",
                wall_id, current
            );
            current
        } else {
            // If wall is destroyed, it might be removed from all_walls_in_partition if your logic does that,
            // or its health is just 0. The test expects to find it and check health.
            // If it's NOT found, it implies it might have been removed upon destruction, which is not what this test asserts.
            // This test asserts health is 0.
            let all_walls_in_server_state = server.collect_all_walls_current_state();
            let wall_still_exists = all_walls_in_server_state.iter().any(|w| w.id == wall_id);
            panic!("Test assertion setup error: Wall ID {} not found in partition's DashMap after firing. Does it still exist in server state? {}", wall_id, wall_still_exists);
        }
    } else {
        panic!(
            "Test assertion setup error: Partition not found for wall ID {}.",
            wall_id
        );
    };
    assert_eq!(
        final_health, 0,
        "Wall health was not reduced to 0 after projectile hits. Current health: {}",
        final_health
    );

    assert!(
        server
            .wall_respawn_manager
            .get_wall_respawn_timer(wall_id)
            .is_some(),
        "Wall {} should be scheduled for respawn.",
        wall_id
    );

    assert!(
        server
            .wall_respawn_manager
            .is_wall_id_in_destroyed_map_for_test(wall_id),
        "Wall ID {} should be present in the destroyed_walls tracking map.",
        wall_id
    );
}
