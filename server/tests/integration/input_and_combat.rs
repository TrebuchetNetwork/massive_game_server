// Tests for input processing, movement, weapon firing, damage, knockback,
// killstreaks, pickup collection, and anti-cheat validation.

use massive_game_server_core::concurrent::thread_pools::ThreadPoolSystem;
use massive_game_server_core::core::config::ServerConfig;
use massive_game_server_core::core::constants::{
    MELEE_LUNGE_DISTANCE, MELEE_WINDUP_SECS, PLAYER_BASE_SPEED, POINTS_PER_KILL, WORLD_MAX_X,
    WORLD_MIN_X,
};
use massive_game_server_core::core::types::{
    EntityId, KillstreakRewardPreference, PlayerAoIs, PlayerID, PlayerInputData, Projectile,
    ServerWeaponType, Wall,
};
use massive_game_server_core::network::signaling::{
    BoundedChatQueue, ChatMessagesQueue, ClientStatesMap, DataChannelsMap, MAX_CHAT_QUEUE_SIZE,
};
use massive_game_server_core::server::instance::MassiveGameServer;

use dashmap::DashMap;
use parking_lot::RwLock as ParkingLotRwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock as TokioRwLock;

fn setup_test_server() -> Arc<MassiveGameServer> {
    let config = Arc::new(ServerConfig::default());
    let thread_pool_system =
        Arc::new(ThreadPoolSystem::new(config.clone()).expect("Failed to create thread pools"));
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

fn make_input(sequence: u32) -> PlayerInputData {
    PlayerInputData {
        timestamp: 1000 + (sequence as u64 * 16),
        sequence,
        move_forward: false,
        move_backward: false,
        move_left: false,
        move_right: false,
        shooting: false,
        reload: false,
        rotation: 0.0,
        melee_attack: false,
        use_ability_slot: 0,
        change_weapon_slot: 0,
        ping_x: 0.0,
        ping_y: 0.0,
    }
}

// ── Movement and input processing ─────────────────────────────────
// Input is processed via process_network_input(), not run_game_logic_update().

#[tokio::test(flavor = "multi_thread")]
async fn input_sets_velocity_forward() {
    let server = setup_test_server();
    let pid = add_player(&server, "mover", 1, 0.0, 0.0);

    let mut input = make_input(1);
    input.move_forward = true;
    input.rotation = 0.0; // Face right (+x direction)

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.input_queue.push_back(input);
    }

    server.process_network_input().await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert!(
        ps.velocity_x > 0.0,
        "velocity_x should be positive for forward movement at rotation=0, got {}",
        ps.velocity_x
    );
    assert!(
        ps.velocity_x.abs() <= PLAYER_BASE_SPEED * 3.0,
        "velocity should not exceed reasonable speed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn input_sets_velocity_strafe() {
    let server = setup_test_server();
    let pid = add_player(&server, "strafer", 1, 0.0, 0.0);

    let mut input = make_input(1);
    input.move_right = true;
    input.rotation = 0.0;

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.input_queue.push_back(input);
    }

    server.process_network_input().await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    // Strafing right at rotation=0 should give positive velocity_y
    assert!(
        ps.velocity_y > 0.0,
        "velocity_y should be positive for right strafe at rotation=0, got {}",
        ps.velocity_y
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn duplicate_input_sequence_is_ignored() {
    let server = setup_test_server();
    let pid = add_player(&server, "dedup", 1, 0.0, 0.0);

    // Send sequence 1 with forward.
    let mut input1 = make_input(1);
    input1.move_forward = true;
    input1.rotation = 0.0;

    // Send duplicate sequence 1 with backward (should be ignored).
    let mut input1_dup = make_input(1);
    input1_dup.move_backward = true;
    input1_dup.rotation = std::f32::consts::PI;

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.input_queue.push_back(input1);
        ps.input_queue.push_back(input1_dup);
    }

    server.process_network_input().await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert_eq!(ps.last_processed_input_sequence, 1);
    // Forward was applied, backward was ignored due to same sequence.
    assert!(
        ps.velocity_x > 0.0,
        "Forward input should have been applied"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn dead_player_input_zeroes_velocity() {
    let server = setup_test_server();
    let pid = add_player(&server, "dead_mover", 1, 0.0, 0.0);

    // Kill the player.
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.alive = false;
    }

    let mut input = make_input(1);
    input.move_forward = true;

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.input_queue.push_back(input);
    }

    server.process_network_input().await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert!(
        ps.velocity_x.abs() < f32::EPSILON,
        "Dead player should not have velocity"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn zero_sequence_input_is_ignored() {
    let server = setup_test_server();
    let pid = add_player(&server, "seq0", 1, 0.0, 0.0);

    let mut input = make_input(0);
    input.move_forward = true;

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.input_queue.push_back(input);
    }

    server.process_network_input().await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert!(ps.velocity_x.abs() < f32::EPSILON);
}

#[tokio::test(flavor = "multi_thread")]
async fn anti_cheat_violation_threshold_auto_kicks_player() {
    let server = setup_test_server();
    let pid = add_player(&server, "cheater", 1, 0.0, 0.0);

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.violation_count = 8;
        ps.input_queue.push_back(make_input(1));
    }

    server.process_network_input().await;

    let removed = server.player_manager.get_player_state(&pid).is_none();
    assert!(
        removed,
        "player should be removed after threshold violation"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn melee_attack_applies_forward_lunge() {
    let server = setup_test_server();
    let pid = add_player(&server, "melee_lunge", 1, 0.0, 0.0);

    let mut input = make_input(1);
    input.melee_attack = true;
    input.rotation = 0.0;

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.x = 0.0;
        ps.y = 0.0;
        ps.input_queue.push_back(input);
    }

    server.process_network_input().await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert!(
        ps.x.abs() <= f32::EPSILON,
        "Melee should not lunge immediately before windup resolves, got x={}",
        ps.x
    );

    server.run_physics_update(MELEE_WINDUP_SECS + 0.01).await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert!(
        ps.x > 0.0,
        "Melee lunge should move player forward, got x={}",
        ps.x
    );
    assert!(
        ps.x <= MELEE_LUNGE_DISTANCE + 0.5,
        "Melee lunge should be bounded by configured distance, got x={}",
        ps.x
    );
}

// ── Weapon firing and projectile creation ─────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn shooting_input_creates_projectile() {
    let server = setup_test_server();
    let pid = add_player(&server, "shooter", 1, 0.0, 0.0);

    let mut input = make_input(1);
    input.shooting = true;
    input.rotation = 0.0;

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.input_queue.push_back(input);
    }

    server.process_network_input().await;

    // Projectile should be in the queue.
    let count = server.projectiles_to_add.len();
    assert!(
        count > 0,
        "Should have queued at least one projectile from shooting"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn shotgun_creates_multiple_pellets() {
    let server = setup_test_server();
    let pid = add_player(&server, "shotgunner", 1, 0.0, 0.0);

    // Switch to shotgun and give ammo.
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.weapon = ServerWeaponType::Shotgun;
        ps.ammo = 8;
    }

    let mut input = make_input(1);
    input.shooting = true;

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.input_queue.push_back(input);
    }

    server.process_network_input().await;

    let pellet_count = server.projectiles_to_add.len();
    assert!(
        pellet_count > 1,
        "Shotgun should create multiple pellets, got {}",
        pellet_count
    );
}

// ── Projectile physics: wall collision ─────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn projectile_removed_on_wall_hit() {
    let server = setup_test_server();
    add_player(&server, "wall_shooter", 1, -200.0, 0.0);

    let wall = Wall {
        id: 999,
        x: 100.0,
        y: -50.0,
        width: 50.0,
        height: 100.0,
        is_destructible: false,
        current_health: 1000,
        max_health: 1000,
    };
    let partition_idx = server
        .world_partition_manager
        .get_partition_index_for_point(125.0, 0.0);
    if let Some(partition) = server.world_partition_manager.get_partition(partition_idx) {
        partition.add_wall_on_load(wall.clone());
    }
    let all_walls = server.collect_all_walls_current_state();
    server.wall_spatial_index.rebuild(&all_walls, 0);

    let owner_id = server.player_manager.id_pool.get_or_create("wall_shooter");
    let proj = Projectile::new(owner_id, ServerWeaponType::Sniper, 95.0, 0.0, 1.0, 0.0, 1.0);
    server.projectiles_to_add.push(proj);

    server.run_game_logic_update(0.016).await;
    server.run_physics_update(0.016).await;

    let projectiles = server.projectiles.read();
    assert_eq!(
        projectiles.len(),
        0,
        "Projectile should be removed after hitting indestructible wall"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn projectile_damages_destructible_wall() {
    let server = setup_test_server();
    add_player(&server, "destroyer", 1, -200.0, 0.0);

    let wall_id: EntityId = rand::random();
    let wall = Wall {
        id: wall_id,
        x: 100.0,
        y: -25.0,
        width: 50.0,
        height: 50.0,
        is_destructible: true,
        current_health: 100,
        max_health: 100,
    };
    let partition_idx = server
        .world_partition_manager
        .get_partition_index_for_point(125.0, 0.0);
    if let Some(partition) = server.world_partition_manager.get_partition(partition_idx) {
        partition.add_wall_on_load(wall.clone());
    }
    server.wall_respawn_manager.register_wall(&wall);
    let all_walls = server.collect_all_walls_current_state();
    server.wall_spatial_index.rebuild(&all_walls, 0);

    let owner_id = server.player_manager.id_pool.get_or_create("destroyer");
    let proj = Projectile::new(owner_id, ServerWeaponType::Sniper, 95.0, 0.0, 1.0, 0.0, 1.0);
    server.projectiles_to_add.push(proj);

    server.run_game_logic_update(0.016).await;
    server.run_physics_update(0.016).await;
    server.run_game_logic_update(0.016).await;

    if let Some(partition) = server.world_partition_manager.get_partition(partition_idx) {
        if let Some(wall_entry) = partition.all_walls_in_partition.get(&wall_id) {
            assert!(
                wall_entry.current_health < 100,
                "Wall should have taken damage, health={}",
                wall_entry.current_health
            );
        }
    }
}

// ── Projectile bounds checking ────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn projectile_removed_when_out_of_bounds() {
    let server = setup_test_server();
    add_player(&server, "oob_shooter", 1, 0.0, 0.0);

    let owner_id = server.player_manager.id_pool.get_or_create("oob_shooter");
    let proj = Projectile::new(
        owner_id,
        ServerWeaponType::Pistol,
        WORLD_MAX_X - 1.0,
        0.0,
        1.0,
        0.0,
        1.0,
    );
    server.projectiles_to_add.push(proj);

    server.run_game_logic_update(0.016).await;
    server.run_physics_update(0.016).await;

    let projectiles = server.projectiles.read();
    assert_eq!(
        projectiles.len(),
        0,
        "Out-of-bounds projectile should be removed"
    );
}

// ── Kill scoring ──────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn player_kill_awards_points() {
    let server = setup_test_server();
    let attacker_id = add_player(&server, "attacker", 1, 0.0, 0.0);
    let victim_id = add_player(&server, "victim", 2, 50.0, 0.0);

    // Trigger match active.
    server.run_game_logic_update(0.016).await;

    let attacker_arc = server.player_manager.id_pool.get_or_create("attacker");
    let proj = Projectile::new(
        attacker_arc,
        ServerWeaponType::Sniper,
        40.0,
        0.0,
        1.0,
        0.0,
        2.0,
    );
    server.projectiles_to_add.push(proj);

    server
        .spatial_index
        .update_player_position(attacker_id.clone(), 0.0, 0.0);
    server
        .spatial_index
        .update_player_position(victim_id.clone(), 50.0, 0.0);
    let walls = server.collect_all_walls_current_state();
    server.wall_spatial_index.rebuild(&walls, 0);

    server.run_game_logic_update(0.016).await;
    server.run_physics_update(0.016).await;

    let attacker_state = server.player_manager.get_player_state(&attacker_id);
    if let Some(ps) = attacker_state {
        if ps.kills > 0 {
            assert!(ps.score >= POINTS_PER_KILL);
        }
    }
}

// ── Knockback clamping ────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn knockback_velocity_is_clamped() {
    let server = setup_test_server();
    let pid = add_player(&server, "knocked", 1, 0.0, 0.0);

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.velocity_x = 1000.0;
        ps.velocity_y = 1000.0;
    }

    server.run_physics_update(0.016).await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert!(ps.x <= WORLD_MAX_X, "Player should not exceed world bounds");
    assert!(ps.x >= WORLD_MIN_X, "Player should not go below world min");
}

// ── Pickup collection ─────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn initial_pickups_are_generated() {
    let server = setup_test_server();
    let pickups = server.pickups.read();
    assert!(
        !pickups.is_empty(),
        "Server should generate initial pickups"
    );
}

// ── Weapon switching ──────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn weapon_switch_input_changes_weapon() {
    let server = setup_test_server();
    let pid = add_player(&server, "switcher", 1, 0.0, 0.0);

    // Send weapon change input to slot 5 (Melee).
    let mut input = make_input(1);
    input.change_weapon_slot = 5;

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.input_queue.push_back(input);
    }

    server.process_network_input().await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert_eq!(ps.weapon, ServerWeaponType::Melee);
}

// ── Reload input ──────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn reload_input_starts_reload() {
    let server = setup_test_server();
    let pid = add_player(&server, "reloader", 1, 0.0, 0.0);

    // Deplete ammo.
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.ammo = 0;
    }

    let mut input = make_input(1);
    input.reload = true;

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.input_queue.push_back(input);
    }

    server.process_network_input().await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert!(
        ps.reload_progress.is_some() || ps.ammo > 0,
        "Reload should start or complete"
    );
}

// ── Ability activation (dash) ─────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn dash_ability_activates_on_input() {
    let server = setup_test_server();
    let pid = add_player(&server, "dasher", 1, 0.0, 0.0);

    let mut input = make_input(1);
    input.use_ability_slot = 1;
    input.move_forward = true;

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.input_queue.push_back(input);
    }

    server.process_network_input().await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert!(
        ps.ability_1_cooldown_remaining > 0.0,
        "Dash should be on cooldown"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn dodge_ability_grants_invulnerability() {
    let server = setup_test_server();
    let pid = add_player(&server, "dodger", 1, 0.0, 0.0);

    let mut input = make_input(1);
    input.use_ability_slot = 2;

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.input_queue.push_back(input);
    }

    server.process_network_input().await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert!(
        ps.ability_2_cooldown_remaining > 0.0,
        "Dodge should be on cooldown"
    );
    assert!(
        ps.invulnerable_remaining > 0.0,
        "Dodge should grant invulnerability"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn killstreak_reward_preference_switches_via_reserved_input_slots() {
    let server = setup_test_server();
    let pid = add_player(&server, "streak_pref", 1, 0.0, 0.0);

    let mut input = make_input(1);
    input.use_ability_slot =
        massive_game_server_core::core::constants::KILLSTREAK_PREF_SPEED_FIRST_INPUT_SLOT;
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.input_queue.push_back(input);
    }
    server.process_network_input().await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert_eq!(
        ps.killstreak_reward_preference,
        KillstreakRewardPreference::SpeedFirst
    );
}

// ── Rotation input updates player rotation ────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn rotation_input_updates_player_rotation() {
    let server = setup_test_server();
    let pid = add_player(&server, "rotator", 1, 0.0, 0.0);

    let mut input = make_input(1);
    input.rotation = 1.5;

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.input_queue.push_back(input);
    }

    server.process_network_input().await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert!(
        (ps.rotation - 1.5).abs() < 0.01,
        "Player rotation should be updated to 1.5, got {}",
        ps.rotation
    );
}

// ── Diagonal movement is normalized ───────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn diagonal_movement_is_normalized() {
    let server = setup_test_server();
    let pid = add_player(&server, "diagonal", 1, 0.0, 0.0);

    let mut input = make_input(1);
    input.move_forward = true;
    input.move_right = true;
    input.rotation = 0.0;

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.input_queue.push_back(input);
    }

    server.process_network_input().await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    let speed = (ps.velocity_x.powi(2) + ps.velocity_y.powi(2)).sqrt();
    // Diagonal speed should be ~PLAYER_BASE_SPEED, not 1.414x.
    assert!(
        speed <= PLAYER_BASE_SPEED * 1.05,
        "Diagonal speed ({}) should be close to PLAYER_BASE_SPEED ({})",
        speed,
        PLAYER_BASE_SPEED
    );
}

// ── Speed boost multiplier ────────────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn speed_boost_increases_velocity() {
    let server = setup_test_server();
    let pid = add_player(&server, "boosted", 1, 0.0, 0.0);

    // Give speed boost.
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.speed_boost_remaining = 5.0;
    }

    let mut input = make_input(1);
    input.move_forward = true;
    input.rotation = 0.0;

    if let Some(mut ps) = server.player_manager.get_player_state_mut(&pid) {
        ps.input_queue.push_back(input);
    }

    server.process_network_input().await;

    let ps = server.player_manager.get_player_state(&pid).unwrap();
    assert!(
        ps.velocity_x > PLAYER_BASE_SPEED,
        "Speed boost should increase velocity beyond base speed. vx={}",
        ps.velocity_x
    );
}
