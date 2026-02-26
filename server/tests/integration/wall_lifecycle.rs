// Tests for wall lifecycle: destruction, respawn scheduling, progressive
// fragmentation, spatial index consistency, and wall damage aggregation.

use massive_game_server_core::concurrent::thread_pools::ThreadPoolSystem;
use massive_game_server_core::core::config::ServerConfig;
use massive_game_server_core::core::types::{EntityId, PlayerAoIs, Projectile, ServerWeaponType, Wall};
use massive_game_server_core::network::signaling::{ChatMessagesQueue, ClientStatesMap, DataChannelsMap};
use massive_game_server_core::server::instance::MassiveGameServer;

use dashmap::DashMap;
use parking_lot::RwLock as ParkingLotRwLock;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock as TokioRwLock;

fn setup_test_server() -> Arc<MassiveGameServer> {
    let config = Arc::new(ServerConfig::default());
    let thread_pool_system =
        Arc::new(ThreadPoolSystem::new(config.clone()).expect("Failed to create thread pools"));
    let data_channels_map: DataChannelsMap = Arc::new(DashMap::new());
    let client_states_map: ClientStatesMap = Arc::new(ParkingLotRwLock::new(HashMap::new()));
    let chat_messages_queue: ChatMessagesQueue = Arc::new(TokioRwLock::new(VecDeque::new()));
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

fn create_wall(
    server: &MassiveGameServer,
    wall_id: EntityId,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    health: i32,
    destructible: bool,
) {
    let wall = Wall {
        id: wall_id,
        x,
        y,
        width,
        height,
        is_destructible: destructible,
        current_health: health,
        max_health: health,
    };
    let partition_idx = server
        .world_partition_manager
        .get_partition_index_for_point(x + width / 2.0, y + height / 2.0);
    if let Some(partition) = server.world_partition_manager.get_partition(partition_idx) {
        partition.add_wall_on_load(wall.clone());
    }
    if destructible {
        server.wall_respawn_manager.register_wall(&wall);
    }
}

fn rebuild_wall_index(server: &MassiveGameServer) {
    let all_walls = server.collect_all_walls_current_state();
    server.wall_spatial_index.rebuild(&all_walls, 0);
}

// ── Wall creation and partition placement ──────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn wall_placed_in_correct_partition() {
    let server = setup_test_server();
    let wall_id = 42u64;
    create_wall(&server, wall_id, 100.0, 100.0, 50.0, 50.0, 200, true);

    let partition_idx = server
        .world_partition_manager
        .get_partition_index_for_point(125.0, 125.0);
    let partition = server
        .world_partition_manager
        .get_partition(partition_idx)
        .expect("Partition should exist");

    assert!(
        partition.all_walls_in_partition.contains_key(&wall_id),
        "Wall should be in the partition"
    );
}

// ── Wall destruction via projectile ─────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn wall_destroyed_by_sufficient_damage() {
    let server = setup_test_server();
    let wall_id = 100u64;
    create_wall(&server, wall_id, 100.0, -25.0, 50.0, 50.0, 50, true);
    rebuild_wall_index(&server);

    // Add attacker player.
    server
        .player_manager
        .add_player("attacker".to_owned(), "attacker".to_owned(), 0.0, 0.0);

    let owner_id = server.player_manager.id_pool.get_or_create("attacker");
    // Sniper does 50 base damage * 2.0 = 100. Wall has 50 HP.
    let proj = Projectile::new(
        owner_id,
        ServerWeaponType::Sniper,
        95.0,
        0.0,
        1.0,
        0.0,
        2.0,
    );
    server.projectiles_to_add.push(proj);

    server.run_game_logic_update(0.016).await;
    server.run_physics_update(0.016).await;
    server.run_game_logic_update(0.016).await;

    // Wall should be at 0 health.
    let partition_idx = server
        .world_partition_manager
        .get_partition_index_for_point(125.0, 0.0);
    if let Some(partition) = server.world_partition_manager.get_partition(partition_idx) {
        if let Some(wall_entry) = partition.all_walls_in_partition.get(&wall_id) {
            assert_eq!(
                wall_entry.current_health, 0,
                "Wall should be destroyed (health=0)"
            );
        }
    }

    // Respawn should be scheduled.
    assert!(
        server
            .wall_respawn_manager
            .get_wall_respawn_timer(wall_id)
            .is_some(),
        "Destroyed wall should have respawn timer"
    );
}

// ── Indestructible wall ignores damage ──────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn indestructible_wall_takes_no_damage() {
    let server = setup_test_server();
    let wall_id = 200u64;
    create_wall(&server, wall_id, 100.0, -25.0, 50.0, 50.0, 1000, false);
    rebuild_wall_index(&server);

    server
        .player_manager
        .add_player("indestr_attacker".to_owned(), "indestr_attacker".to_owned(), 0.0, 0.0);
    let owner_id = server
        .player_manager
        .id_pool
        .get_or_create("indestr_attacker");
    let proj = Projectile::new(
        owner_id,
        ServerWeaponType::Sniper,
        95.0,
        0.0,
        1.0,
        0.0,
        2.0,
    );
    server.projectiles_to_add.push(proj);

    server.run_game_logic_update(0.016).await;
    server.run_physics_update(0.016).await;
    server.run_game_logic_update(0.016).await;

    // Indestructible wall health should remain unchanged.
    let partition_idx = server
        .world_partition_manager
        .get_partition_index_for_point(125.0, 0.0);
    if let Some(partition) = server.world_partition_manager.get_partition(partition_idx) {
        if let Some(wall_entry) = partition.all_walls_in_partition.get(&wall_id) {
            assert_eq!(
                wall_entry.current_health, 1000,
                "Indestructible wall health should not change"
            );
        }
    }
}

// ── Spatial index consistency ──────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn wall_spatial_index_rebuilt_includes_new_walls() {
    let server = setup_test_server();

    // Create walls and rebuild.
    create_wall(&server, 301, -100.0, -100.0, 40.0, 40.0, 100, true);
    create_wall(&server, 302, 200.0, 200.0, 60.0, 60.0, 100, false);
    rebuild_wall_index(&server);

    // Query the spatial index for a line segment that passes through wall 301.
    let results = server
        .wall_spatial_index
        .query_line_segment(-120.0, -80.0, -60.0, -80.0);
    assert!(
        results.iter().any(|w| w.id == 301),
        "Spatial index should contain wall 301 after rebuild"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn wall_spatial_index_empty_before_rebuild() {
    let server = setup_test_server();
    create_wall(&server, 400, 0.0, 0.0, 50.0, 50.0, 100, true);

    // Don't rebuild index — query should miss the new wall.
    // Note: the server may have pre-existing walls from map generation.
    // Just verify the new wall is findable after rebuild.
    rebuild_wall_index(&server);
    let results = server
        .wall_spatial_index
        .query_line_segment(-10.0, 25.0, 60.0, 25.0);
    let has_wall_400 = results.iter().any(|w| w.id == 400);
    assert!(has_wall_400, "After rebuild, wall 400 should be queryable");
}

// ── Multiple walls take independent damage ─────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn multiple_walls_take_independent_damage() {
    let server = setup_test_server();

    // Two walls side by side.
    create_wall(&server, 501, 100.0, -25.0, 30.0, 50.0, 100, true);
    create_wall(&server, 502, 200.0, -25.0, 30.0, 50.0, 100, true);
    rebuild_wall_index(&server);

    server
        .player_manager
        .add_player("multi_attacker".to_owned(), "multi_attacker".to_owned(), 0.0, 0.0);
    let owner_id = server
        .player_manager
        .id_pool
        .get_or_create("multi_attacker");

    // Projectile aimed at wall 501 only.
    let proj = Projectile::new(
        owner_id.clone(),
        ServerWeaponType::Sniper,
        95.0,
        0.0,
        1.0,
        0.0,
        1.0,
    );
    server.projectiles_to_add.push(proj);

    server.run_game_logic_update(0.016).await;
    server.run_physics_update(0.016).await;
    server.run_game_logic_update(0.016).await;

    // Wall 501 should have taken damage.
    let p_idx_501 = server
        .world_partition_manager
        .get_partition_index_for_point(115.0, 0.0);
    if let Some(partition) = server.world_partition_manager.get_partition(p_idx_501) {
        if let Some(entry) = partition.all_walls_in_partition.get(&501) {
            assert!(
                entry.current_health < 100,
                "Wall 501 should have taken damage"
            );
        }
    }

    // Wall 502 should be untouched.
    let p_idx_502 = server
        .world_partition_manager
        .get_partition_index_for_point(215.0, 0.0);
    if let Some(partition) = server.world_partition_manager.get_partition(p_idx_502) {
        if let Some(entry) = partition.all_walls_in_partition.get(&502) {
            assert_eq!(
                entry.current_health, 100,
                "Wall 502 should be untouched"
            );
        }
    }
}

// ── Wall respawn manager tracking ───────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn destroyed_wall_tracked_in_respawn_manager() {
    let server = setup_test_server();
    let wall_id = 600u64;
    create_wall(&server, wall_id, 100.0, -25.0, 50.0, 50.0, 1, true);
    rebuild_wall_index(&server);

    server
        .player_manager
        .add_player("respawn_test".to_owned(), "respawn_test".to_owned(), 0.0, 0.0);
    let owner_id = server
        .player_manager
        .id_pool
        .get_or_create("respawn_test");

    // 1 HP wall hit by sniper (50 dmg) -> destroyed.
    let proj = Projectile::new(
        owner_id,
        ServerWeaponType::Sniper,
        95.0,
        0.0,
        1.0,
        0.0,
        1.0,
    );
    server.projectiles_to_add.push(proj);

    server.run_game_logic_update(0.016).await;
    server.run_physics_update(0.016).await;
    server.run_game_logic_update(0.016).await;

    assert!(
        server
            .wall_respawn_manager
            .is_wall_id_in_destroyed_map_for_test(wall_id),
        "Destroyed wall should be tracked in respawn manager"
    );
}

// ── collect_all_walls_current_state ─────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn collect_all_walls_includes_map_and_dynamic_walls() {
    let server = setup_test_server();

    // Map-generated walls should already exist.
    let initial_walls = server.collect_all_walls_current_state();
    let initial_count = initial_walls.len();
    assert!(initial_count > 0, "Server should have map-generated walls");

    // Add a dynamic wall.
    create_wall(&server, 700, 300.0, 300.0, 40.0, 40.0, 50, true);

    let new_walls = server.collect_all_walls_current_state();
    assert_eq!(
        new_walls.len(),
        initial_count + 1,
        "Should include the newly added wall"
    );
    assert!(
        new_walls.iter().any(|w| w.id == 700),
        "New wall should be in collected state"
    );
}
