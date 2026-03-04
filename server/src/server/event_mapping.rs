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
        GameEvent::TeamPing { position, .. } => *position,
        _ => Vec2::zero(),
    }
}

pub(crate) fn event_instigator_id(event: &GameEvent) -> Option<PlayerID> {
    match event {
        GameEvent::PlayerDamaged { attacker_id, .. } => attacker_id.clone(),
        GameEvent::PlayerKilled { killer_id, .. } => Some(killer_id.clone()),
        GameEvent::WeaponFired { player_id, .. } => Some(player_id.clone()),
        GameEvent::MeleeHit { attacker_id, .. } => Some(attacker_id.clone()),
        GameEvent::PowerupCollected { player_id, .. } => Some(player_id.clone()),
        GameEvent::FlagGrabbed { player_id, .. } => Some(player_id.clone()),
        GameEvent::FlagCaptured { capturer_id, .. } => Some(capturer_id.clone()),
        GameEvent::TeamPing { player_id, .. } => Some(player_id.clone()),
        GameEvent::Killstreak { player_id, .. } => Some(player_id.clone()),
        GameEvent::AssistKill { assister_id, .. } => Some(assister_id.clone()),
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
        _ => None,
    }
}

pub(crate) fn event_weapon_type(event: &GameEvent) -> Option<ServerWeaponType> {
    match event {
        GameEvent::PlayerDamaged { weapon, .. } => Some(*weapon),
        GameEvent::PlayerKilled { weapon, .. } => Some(*weapon),
        GameEvent::WeaponFired { weapon, .. } => Some(*weapon),
        GameEvent::MeleeHit { .. } => Some(ServerWeaponType::Melee),
        _ => None,
    }
}

pub(crate) fn event_value(event: &GameEvent) -> Option<f32> {
    match event {
        GameEvent::PlayerDamaged { damage, .. } => Some(*damage as f32),
        GameEvent::WallImpact { damage, .. } => Some(*damage as f32),
        GameEvent::Killstreak { streak, .. } => Some(*streak as f32),
        GameEvent::AssistKill { points, .. } => Some(*points as f32),
        _ => None,
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
        // These events currently do not have explicit FlatBuffer variants in game.fbs.
        // Skip serialization rather than misclassifying them as BulletImpact.
        GameEvent::PlayerJoined { .. }
        | GameEvent::PlayerLeft { .. }
        | GameEvent::Footstep { .. } => None,
    }
}
