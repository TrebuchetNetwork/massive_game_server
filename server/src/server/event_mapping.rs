use crate::core::types::{GameEvent, PlayerID, ServerWeaponType, SurfaceType, Vec2};
use crate::flatbuffers_generated::game_protocol as fb;

pub(crate) fn event_position(event: &GameEvent) -> Vec2 {
    match event {
        GameEvent::PlayerDamaged { position, .. } => *position,
        GameEvent::PlayerKilled { position, .. } => *position,
        GameEvent::ProjectileHitWall { position, .. } => *position,
        GameEvent::PowerupCollected { position, .. } => *position,
        GameEvent::WeaponFired { position, .. } => *position,
        GameEvent::WallDestroyed { position, .. } => *position,
        GameEvent::WallImpact { position, .. } => *position,
        GameEvent::MeleeHit { position, .. } => *position,
        GameEvent::Footstep { position, .. } => *position,
        GameEvent::WeaponMilestone { position, .. } => *position,
        GameEvent::FlagGrabbed { position, .. } => *position,
        GameEvent::FlagDropped { position, .. } => *position,
        GameEvent::FlagReturned { position, .. } => *position,
        GameEvent::FlagCaptured { position, .. } => *position,
        GameEvent::TeamPing { position, .. } => *position,
        GameEvent::ShieldBroken { position, .. } => *position,
        GameEvent::PowerupExpiring { position, .. } => *position,
        _ => Vec2::zero(),
    }
}

pub(crate) fn event_instigator_id(event: &GameEvent) -> Option<PlayerID> {
    match event {
        GameEvent::PlayerDamaged { attacker_id, .. } => attacker_id.clone(),
        GameEvent::PlayerKilled { killer_id, .. } => Some(killer_id.clone()),
        GameEvent::WeaponFired { player_id, .. } => Some(player_id.clone()),
        GameEvent::MeleeHit { attacker_id, .. } => Some(attacker_id.clone()),
        GameEvent::Footstep { player_id, .. } => Some(player_id.clone()),
        GameEvent::PowerupCollected { player_id, .. } => Some(player_id.clone()),
        GameEvent::FlagGrabbed { player_id, .. } => Some(player_id.clone()),
        GameEvent::FlagCaptured { capturer_id, .. } => Some(capturer_id.clone()),
        GameEvent::TeamPing { player_id, .. } => Some(player_id.clone()),
        GameEvent::Killstreak { player_id, .. } => Some(player_id.clone()),
        GameEvent::AssistKill { assister_id, .. } => Some(assister_id.clone()),
        GameEvent::ShieldBroken { player_id, .. } => Some(player_id.clone()),
        GameEvent::PowerupExpiring { player_id, .. } => Some(player_id.clone()),
        GameEvent::WeaponMilestone { player_id, .. } => Some(player_id.clone()),
        _ => None,
    }
}

pub(crate) fn event_target_id(event: &GameEvent) -> Option<String> {
    match event {
        GameEvent::PlayerDamaged { target_id, .. } => Some(target_id.to_string()),
        GameEvent::PlayerKilled { victim_id, .. } => Some(victim_id.to_string()),
        GameEvent::ProjectileHitWall { wall_id, .. } => Some(wall_id.to_string()),
        GameEvent::PowerupCollected { pickup_id, .. } => Some(pickup_id.to_string()),
        GameEvent::WallDestroyed { wall_id, .. } => Some(wall_id.to_string()),
        GameEvent::WallImpact { wall_id, .. } => Some(wall_id.to_string()),
        GameEvent::MeleeHit { target_id, .. } => target_id.as_ref().map(|id| id.to_string()),
        GameEvent::FlagDropped { flag_team_id, .. } => Some(flag_team_id.to_string()),
        GameEvent::FlagReturned { flag_team_id, .. } => Some(flag_team_id.to_string()),
        GameEvent::TeamPing { team_id, .. } => Some(team_id.to_string()),
        GameEvent::AssistKill { victim_id, .. } => Some(victim_id.to_string()),
        GameEvent::PowerupExpiring { powerup, .. } => Some(powerup.clone()),
        _ => None,
    }
}

pub(crate) fn event_weapon_type(event: &GameEvent) -> Option<ServerWeaponType> {
    match event {
        GameEvent::PlayerDamaged { weapon, .. } => Some(*weapon),
        GameEvent::PlayerKilled { weapon, .. } => Some(*weapon),
        GameEvent::WeaponFired { weapon, .. } => Some(*weapon),
        GameEvent::MeleeHit { .. } => Some(ServerWeaponType::Melee),
        GameEvent::WeaponMilestone { weapon, .. } => Some(*weapon),
        _ => None,
    }
}

pub(crate) fn event_value(event: &GameEvent) -> Option<f32> {
    match event {
        GameEvent::PlayerDamaged { damage, .. } => Some(*damage as f32),
        GameEvent::WallImpact { damage, .. } => Some(*damage as f32),
        GameEvent::Killstreak { streak, .. } => Some(*streak as f32),
        GameEvent::AssistKill { points, .. } => Some(*points as f32),
        GameEvent::PowerupExpiring {
            seconds_remaining, ..
        } => Some(*seconds_remaining),
        GameEvent::WeaponMilestone { milestone, .. } => Some(*milestone as f32),
        _ => None,
    }
}

pub(crate) fn event_falloff_multiplier(event: &GameEvent) -> f32 {
    match event {
        GameEvent::PlayerDamaged {
            falloff_multiplier, ..
        } => (*falloff_multiplier).clamp(0.0, 1.0),
        _ => 1.0,
    }
}

pub(crate) fn map_surface_type_to_fb(surface_type: u8) -> fb::SurfaceType {
    match surface_type {
        x if x == SurfaceType::Metal.as_u8() => fb::SurfaceType::Metal,
        x if x == SurfaceType::Wood.as_u8() => fb::SurfaceType::Wood,
        x if x == SurfaceType::Glass.as_u8() => fb::SurfaceType::Glass,
        _ => fb::SurfaceType::Concrete,
    }
}

pub(crate) fn event_surface_type(event: &GameEvent) -> fb::SurfaceType {
    match event {
        GameEvent::WallImpact { surface_type, .. } | GameEvent::Footstep { surface_type, .. } => {
            map_surface_type_to_fb(*surface_type)
        }
        _ => fb::SurfaceType::Concrete,
    }
}

pub(crate) fn should_serialize_game_event(event: &GameEvent) -> bool {
    map_game_event_type_to_fb(event).is_some()
}

pub(crate) fn map_game_event_type_to_fb(event: &GameEvent) -> Option<fb::GameEventType> {
    match event {
        GameEvent::PlayerDamaged { .. } => Some(fb::GameEventType::PlayerDamageEffect),
        GameEvent::PlayerKilled { .. } => Some(fb::GameEventType::PlayerDamageEffect),
        GameEvent::ProjectileHitWall { .. } => Some(fb::GameEventType::WallImpact),
        GameEvent::PowerupCollected { .. } => Some(fb::GameEventType::PowerupActivated),
        GameEvent::WeaponFired { .. } => Some(fb::GameEventType::WeaponFire),
        GameEvent::MeleeHit { .. } => Some(fb::GameEventType::WeaponFire),
        GameEvent::WallDestroyed { .. } => Some(fb::GameEventType::WallDestroyed),
        GameEvent::WallImpact { .. } => Some(fb::GameEventType::WallImpact),
        GameEvent::FlagGrabbed { .. } => Some(fb::GameEventType::FlagGrabbed),
        GameEvent::FlagDropped { .. } => Some(fb::GameEventType::FlagDropped),
        GameEvent::FlagReturned { .. } => Some(fb::GameEventType::FlagReturned),
        GameEvent::FlagCaptured { .. } => Some(fb::GameEventType::FlagCaptured),
        GameEvent::TeamPing { .. } => Some(fb::GameEventType::TeamPing),
        GameEvent::Killstreak { .. } => Some(fb::GameEventType::Killstreak),
        GameEvent::AssistKill { .. } => Some(fb::GameEventType::AssistKill),
        GameEvent::ShieldBroken { .. } => Some(fb::GameEventType::ShieldBroken),
        GameEvent::PowerupExpiring { .. } => Some(fb::GameEventType::PowerupExpiring),
        GameEvent::Footstep { .. } => Some(fb::GameEventType::Footstep),
        GameEvent::WeaponMilestone { .. } => Some(fb::GameEventType::WeaponMilestone),
        // These events currently do not have explicit FlatBuffer variants in game.fbs.
        // Skip serialization rather than misclassifying them as BulletImpact.
        GameEvent::PlayerJoined { .. } | GameEvent::PlayerLeft { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::SurfaceType;
    use std::sync::Arc;

    #[test]
    fn footstep_events_serialize_to_footstep_type() {
        let event = GameEvent::Footstep {
            player_id: Arc::from("p1"),
            position: Vec2::new(10.0, 20.0),
            surface_type: SurfaceType::Metal.as_u8(),
        };
        assert_eq!(
            map_game_event_type_to_fb(&event),
            Some(fb::GameEventType::Footstep)
        );
    }

    #[test]
    fn wall_impact_surface_type_maps_to_flatbuffer_enum() {
        let event = GameEvent::WallImpact {
            wall_id: 7,
            position: Vec2::new(0.0, 0.0),
            damage: 12,
            surface_type: SurfaceType::Wood.as_u8(),
        };
        assert_eq!(event_surface_type(&event), fb::SurfaceType::Wood);
    }

    #[test]
    fn weapon_milestone_events_serialize_to_milestone_type() {
        let event = GameEvent::WeaponMilestone {
            player_id: Arc::from("p1"),
            weapon: ServerWeaponType::Sniper,
            milestone: 50,
            position: Vec2::new(5.0, 9.0),
        };
        assert_eq!(
            map_game_event_type_to_fb(&event),
            Some(fb::GameEventType::WeaponMilestone)
        );
        assert_eq!(event_value(&event), Some(50.0));
    }
}
