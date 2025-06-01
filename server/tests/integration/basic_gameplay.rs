// massive_game_server/server/tests/integration/basic_gameplay.rs

use massive_game_server_core::core::types::{Projectile, Wall, ServerWeaponType, EntityId, PlayerID, Vec2, EventPriority, PlayerState, PlayerInputData}; // Added PlayerState, PlayerInputData
use massive_game_server_core::server::instance::MassiveGameServer;
use massive_game_server_core::core::config::ServerConfig;
use massive_game_server_core::concurrent::thread_pools::ThreadPoolSystem;
use massive_game_server_core::network::signaling::{DataChannelsMap, ClientStatesMap, ChatMessagesQueue}; 
use massive_game_server_core::core::types::PlayerAoIs; 
use massive_game_server_core::systems::physics::collision; 
use massive_game_server_core::core::constants::PLAYER_RADIUS; // Import PLAYER_RADIUS

use std::sync::Arc;
use dashmap::DashMap;
use tokio::sync::RwLock as TokioRwLock;
use std::collections::VecDeque;
use tracing::info; 
use std::time::SystemTime; // For PlayerInputData timestamp

// Helper function to set up a test server instance
fn setup_test_server() -> Arc<MassiveGameServer> {
    info!("[Test Setup] Setting up test server instance...");
    let config = Arc::new(ServerConfig::default());
    let thread_pool_system = Arc::new(ThreadPoolSystem::new(config.clone()).expect("Failed to create thread pools"));
    
    let data_channels_map: DataChannelsMap = Arc::new(DashMap::new());
    let client_states_map: ClientStatesMap = Arc::new(DashMap::new());
    let chat_messages_queue: ChatMessagesQueue = Arc::new(TokioRwLock::new(VecDeque::new()));
    let player_aois: PlayerAoIs = Arc::new(DashMap::new());

    let server = Arc::new(MassiveGameServer::new(
        config,
        thread_pool_system,
        data_channels_map,
        client_states_map,
        chat_messages_queue,
        player_aois,
    ));
    info!("[Test Setup] Test server instance created.");
    server
}

// Helper function to create a destructible wall
fn create_destructible_wall(
    server: &MassiveGameServer,
    x: f32, y: f32, width: f32, height: f32, health: i32
) -> EntityId {
    let wall_id = rand::random::<u64>();
    let wall = Wall {
        id: wall_id,
        x, y, width, height,
        is_destructible: true,
        current_health: health,
        max_health: health,
    };

    let partition_idx = server.world_partition_manager.get_partition_index_for_point(x + width / 2.0, y + height / 2.0);
    if let Some(partition) = server.world_partition_manager.get_partition(partition_idx) {
        partition.all_walls_in_partition.insert(wall.id, wall.clone());
        info!("[Test Wall] Added wall ID {} to partition {}", wall_id, partition_idx);
    } else {
        panic!("[Test Wall] Test setup error: Could not find partition {} to add wall {}.", partition_idx, wall_id);
    }
    server.wall_respawn_manager.register_wall(&wall);
    info!("[Test Wall] Created and registered wall ID {} at ({}, {}) with health {}", wall_id, x, y, health);
    wall_id
}

// Helper function to create a projectile
fn create_projectile(
    server: &MassiveGameServer,
    owner_player_id_str: &str,
    weapon_type: ServerWeaponType,
    start_x: f32, start_y: f32,
    dir_x: f32, dir_y: f32,
    damage_multiplier: f32,
) -> Projectile {
    if server.player_manager.get_player_state(&server.player_manager.id_pool.get_or_create(owner_player_id_str)).is_none() {
        info!("[Test Projectile] Owner {} not in PlayerManager, adding for test.", owner_player_id_str);
        server.player_manager.add_player(owner_player_id_str.to_string(), owner_player_id_str.to_string(), 0.0, 0.0);
    }
    
    let owner_id_arc = server.player_manager.id_pool.get_or_create(owner_player_id_str);
    let projectile = Projectile::new(owner_id_arc, weapon_type, start_x, start_y, dir_x, dir_y, damage_multiplier);
    info!("[Test Projectile] Created projectile ID {} for owner {}, weapon {:?}, damage {}, multiplier {}", projectile.id, owner_player_id_str, weapon_type, projectile.damage, damage_multiplier);
    projectile
}

// Helper function to simulate processing a projectile-wall collision
fn process_projectile_wall_collision(
    server: &MassiveGameServer,
    projectile: &Projectile,
    wall_id: EntityId,
) {
    info!("[Test Collision] Processing collision: projectile ID {} vs wall ID {}", projectile.id, wall_id);
    let mut wall_found_and_processed = false;
    for partition_arc in server.world_partition_manager.get_partitions_for_processing() {
        if let Some(mut wall_entry) = partition_arc.all_walls_in_partition.get_mut(&wall_id) {
            let wall_mut = wall_entry.value_mut();
            info!("[Test Collision] Wall ID {} found in partition {}. Current health: {}", wall_id, partition_arc.id, wall_mut.current_health);
            if let Some(event) = collision::handle_projectile_wall_collision(
                projectile,
                wall_id,
                wall_mut, 
                &server.wall_respawn_manager,
            ) {
                info!("[Test Collision] Collision event generated: {:?}", event);
                server.global_game_events.push(event, EventPriority::Normal);
            }
            wall_found_and_processed = true;
            break; 
        }
    }
    if !wall_found_and_processed {
        panic!("[Test Collision] Test error: Wall ID {} not found for collision processing.", wall_id);
    }
}

// Helper function for the new test: checks if player is significantly inside a wall
fn is_player_colliding_with_wall(player_x: f32, player_y: f32, walls: &[Wall]) -> Option<EntityId> {
    for wall in walls {
        if wall.is_destructible && wall.current_health <= 0 { // Skip destroyed walls
            continue;
        }
        // Check if player's circle intersects wall rectangle
        let closest_x = player_x.clamp(wall.x, wall.x + wall.width);
        let closest_y = player_y.clamp(wall.y, wall.y + wall.height);

        let distance_x = player_x - closest_x;
        let distance_y = player_y - closest_y;
        
        if (distance_x * distance_x + distance_y * distance_y) < (PLAYER_RADIUS * PLAYER_RADIUS) {
            info!("[Collision Check] Player at ({:.2}, {:.2}) intersects wall ID {} ({:.2},{:.2} w:{:.2},h:{:.2})",
                    player_x, player_y, wall.id, wall.x, wall.y, wall.width, wall.height);
            return Some(wall.id); 
        }
    }
    None
}


// --- Test Cases ---

#[test]
fn test_wall_creation_and_registration() {
    let server = setup_test_server();
    let wall_id = create_destructible_wall(&server, 100.0, 100.0, 50.0, 50.0, 100);
    
    let mut wall_exists_in_partition = false;
    let partition_idx = server.world_partition_manager.get_partition_index_for_point(125.0, 125.0);
    if let Some(partition) = server.world_partition_manager.get_partition(partition_idx) {
        if partition.all_walls_in_partition.contains_key(&wall_id) {
            wall_exists_in_partition = true;
        }
    }
    assert!(wall_exists_in_partition, "Wall was not added to a partition.");
    info!("[Test Result] test_wall_creation_and_registration: Wall exists in partition verified.");

    assert!(server.wall_respawn_manager.is_wall_template_registered_for_test(wall_id), "Wall template not registered in WallRespawnManager.");
    info!("[Test Result] test_wall_creation_and_registration: Wall template registration verified.");
}

#[test]
fn test_wall_destruction_and_respawn_scheduling() {
    let server = setup_test_server();
    let wall_health = 100;
    let wall_id = create_destructible_wall(&server, 150.0, 150.0, 20.0, 20.0, wall_health);
    let player_owner_id = "player_destroyer";
    server.player_manager.add_player(player_owner_id.to_string(), player_owner_id.to_string(), 0.0, 0.0);

    let projectile1 = create_projectile(&server, player_owner_id, ServerWeaponType::Sniper, 150.0, 140.0, 0.0, 1.0, 2.0);
    assert_eq!(projectile1.damage, 100, "Projectile damage for one-shot setup is incorrect.");

    process_projectile_wall_collision(&server, &projectile1, wall_id);

    let mut final_health = -1;
    let partition_idx = server.world_partition_manager.get_partition_index_for_point(150.0 + 10.0, 150.0 + 10.0);
    if let Some(partition) = server.world_partition_manager.get_partition(partition_idx) {
        if let Some(wall_data) = partition.all_walls_in_partition.get(&wall_id) {
            final_health = wall_data.current_health;
        }
    }
    assert_eq!(final_health, 0, "Wall health was not reduced to 0 after destruction. Current health: {}", final_health);
    info!("[Test Result] test_wall_destruction_and_respawn_scheduling: Wall health is 0 after one-shot.");

    let respawn_timer_opt = server.wall_respawn_manager.get_wall_respawn_timer(wall_id);
    assert!(respawn_timer_opt.is_some(), "Wall was not scheduled for respawn after destruction.");
    if let Some(timer) = respawn_timer_opt {
        assert!(timer.as_secs() >= 29 && timer.as_secs() <= 91, "Wall respawn timer ({}s) is out of expected range (30-90s).", timer.as_secs());
        info!("[Test Result] test_wall_destruction_and_respawn_scheduling: Wall respawn timer is {:?}.", timer);
    }

    let projectile2 = create_projectile(&server, "player_hitter_destroyed", ServerWeaponType::Pistol, 150.0, 140.0, 0.0, 1.0, 1.0);
    process_projectile_wall_collision(&server, &projectile2, wall_id);

    let mut health_after_second_hit = -1;
    if let Some(partition) = server.world_partition_manager.get_partition(partition_idx) {
        if let Some(wall_data) = partition.all_walls_in_partition.get(&wall_id) {
            health_after_second_hit = wall_data.current_health;
        }
    }
    assert_eq!(health_after_second_hit, 0, "Wall health changed after being hit while already destroyed.");
    info!("[Test Result] test_wall_destruction_and_respawn_scheduling: Wall health remains 0 after second hit.");
    
    let respawn_timer_after_second_hit_opt = server.wall_respawn_manager.get_wall_respawn_timer(wall_id);
    assert!(respawn_timer_after_second_hit_opt.is_some(), "Respawn timer disappeared after second hit on destroyed wall.");
    if let Some(timer_after_second_hit) = respawn_timer_after_second_hit_opt {
         if let Some(initial_timer) = respawn_timer_opt {
            let diff_ms = if initial_timer > timer_after_second_hit { 
                (initial_timer - timer_after_second_hit).as_millis() 
            } else { 
                (timer_after_second_hit - initial_timer).as_millis() 
            };
            assert!(diff_ms < 100, "Respawn timer was significantly altered ({}ms diff) by hitting an already destroyed wall.", diff_ms);
            info!("[Test Result] test_wall_destruction_and_respawn_scheduling: Respawn timer stable after second hit.");
         }
    }
}

#[test]
fn test_wall_destruction_scenario() {
    let server = setup_test_server();
    let wall_id = create_destructible_wall(&server, 100.0, 100.0, 50.0, 50.0, 100);
    let player1_id = "p1_multi_hit";
    let player2_id = "p2_multi_hit";
    let player3_id = "p3_multi_hit";
    server.player_manager.add_player(player1_id.to_string(), player1_id.to_string(), 0.0, 0.0);
    server.player_manager.add_player(player2_id.to_string(), player2_id.to_string(), 0.0, 0.0);
    server.player_manager.add_player(player3_id.to_string(), player3_id.to_string(), 0.0, 0.0);

    let projectile1 = create_projectile(&server, player1_id, ServerWeaponType::Rifle, 90.0, 90.0, 1.0, 0.0, 1.0);
    assert_eq!(projectile1.damage, 10, "Projectile1 damage mismatch for Rifle (expected 10).");
    process_projectile_wall_collision(&server, &projectile1, wall_id);
    
    let mut health_after_hit1 = -1;
    let partition_idx = server.world_partition_manager.get_partition_index_for_point(125.0, 125.0);
    if let Some(partition) = server.world_partition_manager.get_partition(partition_idx) {
        if let Some(wall_data) = partition.all_walls_in_partition.get(&wall_id) {
            health_after_hit1 = wall_data.current_health;
        }
    }
    assert_eq!(health_after_hit1, 90, "Wall health after first rifle hit is incorrect (expected 90).");
    info!("[Test Result] test_wall_destruction_scenario: Health after Rifle hit: {}", health_after_hit1);

    let projectile2 = create_projectile(&server, player2_id, ServerWeaponType::Sniper, 90.0, 90.0, 1.0, 0.0, 1.0);
    assert_eq!(projectile2.damage, 50, "Projectile2 damage mismatch for Sniper (expected 50).");
    process_projectile_wall_collision(&server, &projectile2, wall_id);
    
    let mut health_after_hit2 = -1;
    if let Some(partition) = server.world_partition_manager.get_partition(partition_idx) {
        if let Some(wall_data) = partition.all_walls_in_partition.get(&wall_id) {
            health_after_hit2 = wall_data.current_health;
        }
    }
    assert_eq!(health_after_hit2, 40, "Wall health after sniper hit is incorrect (expected 40).");
    info!("[Test Result] test_wall_destruction_scenario: Health after Sniper hit: {}", health_after_hit2);

    let projectile3 = create_projectile(&server, player3_id, ServerWeaponType::Sniper, 90.0, 90.0, 1.0, 0.0, 1.0);
    assert_eq!(projectile3.damage, 50, "Projectile3 damage mismatch for Sniper (expected 50).");
    process_projectile_wall_collision(&server, &projectile3, wall_id);

    let mut health_after_destruction = -1;
    if let Some(partition) = server.world_partition_manager.get_partition(partition_idx) {
        if let Some(wall_data) = partition.all_walls_in_partition.get(&wall_id) {
            health_after_destruction = wall_data.current_health;
        }
    }
    assert_eq!(health_after_destruction, 0, "Wall health should be 0 after final hit.");
    info!("[Test Result] test_wall_destruction_scenario: Health after final Sniper hit: {}", health_after_destruction);
    
    assert!(server.wall_respawn_manager.get_wall_respawn_timer(wall_id).is_some(), "Destroyed wall not scheduled for respawn.");
    info!("[Test Result] test_wall_destruction_scenario: Wall respawn scheduled.");
}

#[tokio::test]
async fn test_player_spawning_and_movement() {
    let server = setup_test_server();
    info!("[Spawn Test] Starting test_player_spawning_and_movement");

    // Add some walls to make spawning non-trivial
    create_destructible_wall(&server, 0.0, 50.0, 200.0, 20.0, 100); // Horizontal wall
    create_destructible_wall(&server, -50.0, 0.0, 20.0, 200.0, 100); // Vertical wall
    create_destructible_wall(&server, 200.0, -100.0, 50.0, 50.0, 100); // Small box

    // Collect all active walls once for collision checks within the loop
    // This assumes walls aren't destroyed and respawned rapidly within this test's scope.
    // For a pure spawn test, this is generally fine.
    let mut static_walls_for_respawn: Vec<Wall> = Vec::new();
    let mut destructible_walls_for_respawn: Vec<Wall> = Vec::new();
    
    server.world_partition_manager.get_partitions_for_processing().iter().for_each(|p| {
        p.all_walls_in_partition.iter().for_each(|entry| {
            let wall = entry.value();
            if wall.is_destructible {
                destructible_walls_for_respawn.push(wall.clone());
            } else {
                static_walls_for_respawn.push(wall.clone());
            }
        });
    });


    let num_spawn_attempts = 100;
    for i in 0..num_spawn_attempts {
        let player_id_str = format!("spawn_test_player_{}", i);
        let player_id_arc_for_respawn = Arc::new(player_id_str.clone()); // Create PlayerID for respawn manager

        // Get a spawn position using the server's respawn manager
        let enemy_positions: Vec<(Vec2, PlayerID)> = Vec::new(); // No enemies for this specific spawn check
        
        // Refresh the current state of destructible walls for each spawn attempt,
        // as previous iterations might have (theoretically, though not in this test's actions) changed them.
        let current_destructible_walls_state: Vec<Wall> = server.collect_all_walls_current_state()
            .into_iter()
            .filter(|w| w.is_destructible)
            .collect();

        let spawn_pos = server.respawn_manager.get_respawn_position_with_walls(
            &player_id_arc_for_respawn,
            None, // No specific team
            &enemy_positions,
            &static_walls_for_respawn, // Static walls don't change
            &current_destructible_walls_state,
        );

        info!("[Spawn Test Iteration {}] Suggested spawn at ({:.2}, {:.2}) for player {}", i, spawn_pos.x, spawn_pos.y, player_id_str);

        // Add player to the server at the chosen spawn position
        let player_id_arc = server.player_manager.add_player(
            player_id_str.clone(),
            format!("SpawnTest{}", i),
            spawn_pos.x,
            spawn_pos.y
        ).expect("Failed to add player for spawn test");

        // Initial Position Check: Ensure player is not spawned inside any active wall
        let initial_player_state_snapshot = server.player_manager.get_player_state(&player_id_arc).unwrap().clone();
        let all_current_walls_after_spawn = server.collect_all_walls_current_state();

        if let Some(colliding_wall_id) = is_player_colliding_with_wall(initial_player_state_snapshot.x, initial_player_state_snapshot.y, &all_current_walls_after_spawn) {
            panic!("Player {} spawned colliding with wall ID {} at ({:.2}, {:.2}). Spawn point was ({:.2}, {:.2})", 
                   player_id_str, colliding_wall_id, initial_player_state_snapshot.x, initial_player_state_snapshot.y, spawn_pos.x, spawn_pos.y);
        }
        info!("[Spawn Test Iteration {}] Player {} spawned successfully at ({:.2}, {:.2}), no initial collision.", i, player_id_str, initial_player_state_snapshot.x, initial_player_state_snapshot.y);

        // Movement Check
        let initial_x = initial_player_state_snapshot.x;
        let initial_y = initial_player_state_snapshot.y;
        let movement_rotation = rand::random::<f32>() * 2.0 * std::f32::consts::PI; // Try a random direction

        {
            let mut p_state_mut = server.player_manager.get_player_state_mut(&player_id_arc).unwrap();
            p_state_mut.rotation = movement_rotation;
            let input = PlayerInputData {
                timestamp: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis() as u64, // More realistic timestamp
                sequence: p_state_mut.last_processed_input_sequence + 1, // Increment sequence
                move_forward: true, move_backward: false, move_left: false, move_right: false,
                shooting: false, reload: false, rotation: p_state_mut.rotation, 
                melee_attack: false, change_weapon_slot: 0, use_ability_slot: 0,
            };
            p_state_mut.queue_input(input);
            info!("[Spawn Test Iteration {}] Player {} queued move input (rot: {:.2}). Initial pos: ({:.2}, {:.2})", i, player_id_str, movement_rotation, initial_x, initial_y);
        }
        
        // Run server ticks to process input and movement
        server.process_network_input().await; 
        server.run_physics_update(0.032).await; // Slightly longer tick for more noticeable movement
        server.synchronize_state().await; // Ensure spatial index, etc., are updated

        let final_player_state_snapshot = server.player_manager.get_player_state(&player_id_arc).unwrap().clone();
        info!("[Spawn Test Iteration {}] Player {} final pos: ({:.2}, {:.2})", i, player_id_str, final_player_state_snapshot.x, final_player_state_snapshot.y);

        let pos_changed = (final_player_state_snapshot.x - initial_x).abs() > 0.1 || // Increased threshold for noticeable change
                          (final_player_state_snapshot.y - initial_y).abs() > 0.1;

        if !pos_changed {
            // If position didn't change significantly, check if they are right next to a wall in the direction they tried to move.
            let intended_move_dir = Vec2::new(movement_rotation.cos(), movement_rotation.sin());
            // Check a point slightly further in front
            let point_in_front = Vec2::new(initial_x + intended_move_dir.x * (PLAYER_RADIUS + 5.0), 
                                           initial_y + intended_move_dir.y * (PLAYER_RADIUS + 5.0));
            
            let mut very_near_wall_in_front = false;
            if let Some(_wall_id) = is_player_colliding_with_wall(point_in_front.x, point_in_front.y, &all_current_walls_after_spawn) {
                 very_near_wall_in_front = true;
            }
            
            assert!(very_near_wall_in_front, 
                    "Player {} (spawned at {:.2},{:.2}) did not move significantly from ({:.2}, {:.2}) to ({:.2}, {:.2}) and was not blocked in front (tried rot: {:.2}). Velocity: ({:.2}, {:.2})", 
                    player_id_str, spawn_pos.x, spawn_pos.y, initial_x, initial_y, final_player_state_snapshot.x, final_player_state_snapshot.y, movement_rotation, final_player_state_snapshot.velocity_x, final_player_state_snapshot.velocity_y);
            info!("[Spawn Test Iteration {}] Player {} did not move, but was blocked as expected.", i, player_id_str);
        } else {
            info!("[Spawn Test Iteration {}] Player {} moved successfully.", i, player_id_str);
        }
        
        // Final check: ensure player is not stuck inside a wall after movement
        if let Some(colliding_wall_id_after_move) = is_player_colliding_with_wall(final_player_state_snapshot.x, final_player_state_snapshot.y, &all_current_walls_after_spawn) {
             panic!("Player {} ended up colliding with wall ID {} at ({:.2}, {:.2}) after movement. Initial spawn: ({:.2}, {:.2}), attempted rot: {:.2}", 
                   player_id_str, colliding_wall_id_after_move, final_player_state_snapshot.x, final_player_state_snapshot.y, spawn_pos.x, spawn_pos.y, movement_rotation);
        }
        info!("[Spawn Test Iteration {}] Player {} final position is clear of walls.", i, player_id_str);

        // Clean up: remove player for the next iteration
        server.player_manager.remove_player(&player_id_str);
    }
    info!("[Spawn Test] Completed {} spawn and movement attempts.", num_spawn_attempts);
}


/*
// Ensure this method (or similar) exists in your actual WallRespawnManager:
// in src/systems/respawn.rs
impl WallRespawnManager {
    pub fn is_wall_template_registered_for_test(&self, wall_id: EntityId) -> bool {
        self.wall_templates.contains_key(&wall_id)
    }
}
*/
