use crate::core::constants::PICKUP_COLLECTION_RADIUS;
use crate::core::simd;
use crate::core::types::{
    CorePickupType, Pickup, PlayerID, PlayerState, FIELD_HEALTH_ALIVE, FIELD_POWERUPS,
    FIELD_SHIELD, FIELD_WEAPON_AMMO,
};

#[derive(Clone, Debug)]
pub(crate) struct PickupCollectionCandidate {
    pub player_id: PlayerID,
    pub pickup_index: usize,
}

pub(crate) fn collect_pickup_candidates(
    players: &[(PlayerID, f32, f32)],
    pickups: &[Pickup],
) -> Vec<PickupCollectionCandidate> {
    if players.is_empty() || pickups.is_empty() {
        return Vec::new();
    }

    let mut active_pickup_indices = Vec::new();
    let mut active_pickup_xs = Vec::new();
    let mut active_pickup_ys = Vec::new();
    for (pickup_index, pickup) in pickups.iter().enumerate() {
        if pickup.is_active {
            active_pickup_indices.push(pickup_index);
            active_pickup_xs.push(pickup.x);
            active_pickup_ys.push(pickup.y);
        }
    }

    if active_pickup_indices.is_empty() {
        return Vec::new();
    }

    let pickup_radius_sq = PICKUP_COLLECTION_RADIUS * PICKUP_COLLECTION_RADIUS;
    let mut out = Vec::with_capacity(players.len());
    for (player_id, player_x, player_y) in players {
        if let Some(active_index) = simd::first_index_within_radius(
            &active_pickup_xs,
            &active_pickup_ys,
            *player_x,
            *player_y,
            pickup_radius_sq,
        ) {
            out.push(PickupCollectionCandidate {
                player_id: player_id.clone(),
                pickup_index: active_pickup_indices[active_index],
            });
        }
    }

    out
}

pub(crate) fn apply_pickup_effect(
    player_state: &mut PlayerState,
    pickup_type: &CorePickupType,
) -> bool {
    match pickup_type {
        CorePickupType::Health => {
            if player_state.health < player_state.max_health {
                player_state.health = (player_state.health + 50).min(player_state.max_health);
                player_state.mark_field_changed(FIELD_HEALTH_ALIVE);
                true
            } else {
                false
            }
        }
        CorePickupType::Ammo => {
            player_state.ammo = PlayerState::get_max_ammo_for_weapon(player_state.weapon);
            player_state.mark_field_changed(FIELD_WEAPON_AMMO);
            true
        }
        CorePickupType::WeaponCrate(weapon) => {
            player_state.weapon = *weapon;
            player_state.ammo = PlayerState::get_max_ammo_for_weapon(*weapon);
            player_state.reload_progress = None;
            player_state.mark_field_changed(FIELD_WEAPON_AMMO);
            true
        }
        CorePickupType::SpeedBoost => {
            player_state.speed_boost_remaining = 10.0;
            player_state.mark_field_changed(FIELD_POWERUPS);
            true
        }
        CorePickupType::DamageBoost => {
            player_state.damage_boost_remaining = 10.0;
            player_state.mark_field_changed(FIELD_POWERUPS);
            true
        }
        CorePickupType::Shield => {
            player_state.shield_max = 50;
            player_state.shield_current = player_state.shield_max;
            player_state.mark_field_changed(FIELD_SHIELD);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::ServerWeaponType;

    #[test]
    fn collect_pickup_candidates_prefers_first_active_pickup() {
        let players = vec![(PlayerID::from("p1".to_string()), 10.0, 10.0)];
        let pickups = vec![
            Pickup::new(1, 12.0, 10.0, CorePickupType::Ammo),
            Pickup::new(2, 13.0, 10.0, CorePickupType::Health),
        ];

        let candidates = collect_pickup_candidates(&players, &pickups);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].pickup_index, 0);
        assert_eq!(candidates[0].player_id.as_str(), "p1");
    }

    #[test]
    fn apply_pickup_effect_updates_player_state() {
        let mut player = PlayerState::new("p1".to_string(), "Player".to_string(), 0.0, 0.0);
        player.health = 10;
        player.max_health = 100;
        player.weapon = ServerWeaponType::Pistol;
        player.ammo = 1;
        player.clear_changed_fields();

        let applied_health = apply_pickup_effect(&mut player, &CorePickupType::Health);
        assert!(applied_health);
        assert_eq!(player.health, 60);
        assert_ne!(player.changed_fields & FIELD_HEALTH_ALIVE, 0);

        player.clear_changed_fields();
        let applied_weapon = apply_pickup_effect(
            &mut player,
            &CorePickupType::WeaponCrate(ServerWeaponType::Rifle),
        );
        assert!(applied_weapon);
        assert_eq!(player.weapon, ServerWeaponType::Rifle);
        assert_eq!(
            player.ammo,
            PlayerState::get_max_ammo_for_weapon(ServerWeaponType::Rifle)
        );
        assert_ne!(player.changed_fields & FIELD_WEAPON_AMMO, 0);
    }
}
