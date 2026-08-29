use massive_game_server_core::concurrent::thread_pools::ThreadPoolSystem;
use massive_game_server_core::core::config::ServerConfig;
use massive_game_server_core::core::types::{
    generate_entity_id, CorePickupType, Pickup, PlayerAoIs, PlayerID, ServerWeaponType, ZoneType,
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

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_pickup_collection_has_single_winner() {
    let server = setup_test_server();
    let p1 = add_player(&server, "pickup_p1", 1, 100.0, 100.0);
    let p2 = add_player(&server, "pickup_p2", 2, 102.0, 100.0);

    // The first logic tick transitions Waiting -> Active. Per the fresh-match
    // contract every participant is revived and fully healed in place at
    // match start, so mid-match state must be staged after this tick.
    server.run_game_logic_update(1.0 / 60.0).await;
    assert_eq!(
        server.match_info.read().match_state,
        massive_game_server_core::flatbuffers_generated::game_protocol::MatchStateType::Active,
        "match should be Active after the first logic tick"
    );
    for player_id in [&p1, &p2] {
        let player = server
            .player_manager
            .get_player_state(player_id)
            .expect("player should be tracked after match start");
        assert!(player.alive, "match start should revive participants");
        assert_eq!(
            player.health, player.max_health,
            "match start should restore full health"
        );
    }

    // Stage mid-match damage now that the match-start reset has run.
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&p1) {
        ps.health = 10;
        ps.max_health = 100;
    }
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&p2) {
        ps.health = 10;
        ps.max_health = 100;
    }

    let staged_pickup_id = generate_entity_id();
    {
        let mut pickups = server.pickups.write();
        pickups.clear();
        pickups.push(Pickup::new(
            staged_pickup_id,
            101.0,
            100.0,
            CorePickupType::Health,
        ));
    }

    server.run_game_logic_update(1.0 / 60.0).await;

    let p1_health = server
        .player_manager
        .get_player_state(&p1)
        .map(|ps| ps.health)
        .unwrap_or_default();
    let p2_health = server
        .player_manager
        .get_player_state(&p2)
        .map(|ps| ps.health)
        .unwrap_or_default();

    let winners = [p1_health, p2_health]
        .into_iter()
        .filter(|health| *health == 60)
        .count();
    assert_eq!(winners, 1, "exactly one player should collect the pickup");

    // An active match may spawn additional event pickups on the same tick;
    // only the staged pickup's outcome is under test.
    let pickups = server.pickups.read();
    let staged = pickups
        .iter()
        .find(|pickup| pickup.id == staged_pickup_id)
        .expect("staged pickup should still be tracked");
    assert!(
        !staged.is_active,
        "pickup should be deactivated after a successful collection"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn damage_zone_kills_player_after_four_seconds_of_exposure() {
    let server = setup_test_server();
    let damage_zone = server
        .zones
        .iter()
        .find(|zone| zone.zone_type == ZoneType::DamageZone)
        .expect("default map should include at least one damage zone");
    let center_x = damage_zone.x + damage_zone.width * 0.5;
    let center_y = damage_zone.y + damage_zone.height * 0.5;

    let player_id = add_player(&server, "zone_target", 1, center_x, center_y);
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&player_id) {
        ps.health = 100;
        ps.shield_current = 0;
        ps.shield_max = 0;
        ps.velocity_x = 0.0;
        ps.velocity_y = 0.0;
    }

    for _ in 0..4 {
        server.run_physics_update(1.0).await;
    }

    let player = server
        .player_manager
        .get_player_state(&player_id)
        .expect("player state should remain tracked after zone death");
    assert_eq!(player.health, 0);
    assert!(!player.alive, "damage zone should kill the player");
    assert_eq!(player.deaths, 1);
    assert!(
        player.respawn_timer.is_some(),
        "zone death should start the respawn timer"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn weapon_crate_pickup_swaps_active_weapon_and_refills_ammo() {
    let server = setup_test_server();
    let player_id = add_player(&server, "weapon_crate", 1, 200.0, 200.0);

    // The first logic tick transitions Waiting -> Active. The fresh-match
    // contract respawns every participant in place, which includes resetting
    // the active weapon to the primary slot with full ammo.
    server.run_game_logic_update(1.0 / 60.0).await;
    {
        let player = server
            .player_manager
            .get_player_state(&player_id)
            .expect("player should be tracked after match start");
        assert!(player.alive, "match start should revive participants");
        assert_eq!(
            player.weapon, player.primary_weapon,
            "match start should reset the active weapon to the primary slot"
        );
        assert_eq!(
            player.ammo,
            massive_game_server_core::core::types::PlayerState::get_max_ammo_for_weapon(
                player.primary_weapon
            ),
            "match start should refill primary ammo"
        );
    }

    // Stage the mid-match weapon state now that the match-start reset has run.
    if let Some(mut ps) = server.player_manager.get_player_state_mut(&player_id) {
        ps.primary_weapon = ServerWeaponType::Rifle;
        ps.primary_ammo = 5;
        ps.secondary_weapon = ServerWeaponType::Pistol;
        ps.secondary_ammo = 1;
        ps.weapon = ServerWeaponType::Pistol;
        ps.ammo = 1;
    }

    {
        let mut pickups = server.pickups.write();
        pickups.clear();
        pickups.push(Pickup::new(
            generate_entity_id(),
            200.0,
            200.0,
            CorePickupType::WeaponCrate(ServerWeaponType::Sniper),
        ));
    }

    server.run_game_logic_update(1.0 / 60.0).await;

    let player = server
        .player_manager
        .get_player_state(&player_id)
        .expect("player should still exist after collecting a weapon crate");
    assert_eq!(player.weapon, ServerWeaponType::Sniper);
    assert_eq!(player.secondary_weapon, ServerWeaponType::Sniper);
    assert_eq!(
        player.ammo,
        massive_game_server_core::core::types::PlayerState::get_max_ammo_for_weapon(
            ServerWeaponType::Sniper
        )
    );
    assert_eq!(player.secondary_ammo, player.ammo);
}
