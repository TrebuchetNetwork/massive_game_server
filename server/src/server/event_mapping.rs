use crate::core::types::{GameEvent, PlayerID, ServerWeaponType, Vec2};
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
        GameEvent::FlagGrabbed { position, .. } => *position,
        GameEvent::FlagDropped { position, .. } => *position,
        GameEvent::FlagReturned { position, .. } => *position,
        GameEvent::FlagCaptured { position, .. } => *position,
        _ => Vec2::zero(),
    }
}

pub(crate) fn event_instigator_id(event: &GameEvent) -> Option<PlayerID> {
    match event {
        GameEvent::PlayerDamaged { attacker_id, .. } => attacker_id.clone(),
        GameEvent::PlayerKilled { killer_id, .. } => Some(killer_id.clone()),
        GameEvent::WeaponFired { player_id, .. } => Some(player_id.clone()),
        GameEvent::PowerupCollected { player_id, .. } => Some(player_id.clone()),
        GameEvent::FlagGrabbed { player_id, .. } => Some(player_id.clone()),
        GameEvent::FlagCaptured { capturer_id, .. } => Some(capturer_id.clone()),
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
        _ => None,
    }
}

pub(crate) fn event_weapon_type(event: &GameEvent) -> Option<ServerWeaponType> {
    match event {
        GameEvent::PlayerDamaged { weapon, .. } => Some(*weapon),
        GameEvent::PlayerKilled { weapon, .. } => Some(*weapon),
        GameEvent::WeaponFired { weapon, .. } => Some(*weapon),
        _ => None,
    }
}

pub(crate) fn event_value(event: &GameEvent) -> Option<f32> {
    match event {
        GameEvent::PlayerDamaged { damage, .. } => Some(*damage as f32),
        GameEvent::WallImpact { damage, .. } => Some(*damage as f32),
        _ => None,
    }
}

pub(crate) fn map_game_event_type_to_fb(event: &GameEvent) -> fb::GameEventType {
    match event {
        GameEvent::PlayerDamaged { .. } => fb::GameEventType::PlayerDamageEffect,
        GameEvent::PlayerKilled { .. } => fb::GameEventType::PlayerDamageEffect,
        GameEvent::ProjectileHitWall { .. } => fb::GameEventType::WallImpact,
        GameEvent::PowerupCollected { .. } => fb::GameEventType::PowerupActivated,
        GameEvent::WeaponFired { .. } => fb::GameEventType::WeaponFire,
        GameEvent::WallDestroyed { .. } => fb::GameEventType::WallDestroyed,
        GameEvent::WallImpact { .. } => fb::GameEventType::WallImpact,
        GameEvent::FlagGrabbed { .. } => fb::GameEventType::FlagGrabbed,
        GameEvent::FlagDropped { .. } => fb::GameEventType::FlagDropped,
        GameEvent::FlagReturned { .. } => fb::GameEventType::FlagReturned,
        GameEvent::FlagCaptured { .. } => fb::GameEventType::FlagCaptured,
        GameEvent::PlayerJoined { .. } | GameEvent::PlayerLeft { .. } => {
            fb::GameEventType::BulletImpact
        }
        GameEvent::MeleeHit { .. } => fb::GameEventType::PlayerDamageEffect,
        GameEvent::Footstep { .. } => fb::GameEventType::BulletImpact,
    }
}
