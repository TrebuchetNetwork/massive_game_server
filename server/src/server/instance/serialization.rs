use super::*;

pub(super) fn map_server_weapon_to_fb(server_weapon: ServerWeaponType) -> fb::WeaponType {
    match server_weapon {
        ServerWeaponType::Pistol => fb::WeaponType::Pistol,
        ServerWeaponType::Shotgun => fb::WeaponType::Shotgun,
        ServerWeaponType::Rifle => fb::WeaponType::Rifle,
        ServerWeaponType::Sniper => fb::WeaponType::Sniper,
        ServerWeaponType::Melee => fb::WeaponType::Melee,
    }
}

pub(super) fn map_core_pickup_to_fb(
    core_type: &CorePickupType,
) -> (fb::PickupType, Option<fb::WeaponType>) {
    match core_type {
        CorePickupType::Health => (fb::PickupType::Health, None),
        CorePickupType::Ammo => (fb::PickupType::Ammo, None),
        CorePickupType::WeaponCrate(server_weapon_type) => (
            fb::PickupType::WeaponCrate,
            Some(map_server_weapon_to_fb(*server_weapon_type)),
        ),
        CorePickupType::SpeedBoost => (fb::PickupType::SpeedBoost, None),
        CorePickupType::DamageBoost => (fb::PickupType::DamageBoost, None),
        CorePickupType::Shield => (fb::PickupType::Shield, None),
    }
}

#[inline]
pub(super) fn fb_safe_str<'b>(
    builder: &mut flatbuffers::FlatBufferBuilder<'b>,
    s: &str,
) -> flatbuffers::WIPOffset<&'b str> {
    // Rust strings are UTF-8. Flatbuffers create_string expects valid UTF-8.
    // The main concern could be embedded nulls if strings come from unsafe sources,
    // but Rust &str shouldn't have them.
    // For extreme safety or if data might come from FFI with potential nulls:
    // if s.contains('\0') {
    //     let cleaned_s: String = s.chars().filter(|&c| c != '\0').collect();
    //     return builder.create_string(&cleaned_s);
    // }
    builder.create_string(s)
}

#[inline]
pub(super) fn fb_safe_entity_id<'b>(
    builder: &mut flatbuffers::FlatBufferBuilder<'b>,
    id: EntityId,
) -> flatbuffers::WIPOffset<&'b str> {
    let mut buf = ItoaBuffer::new();
    builder.create_string(buf.format(id))
}

pub(super) fn create_fb_player_state_for_delta<'a>(
    builder: &mut flatbuffers::FlatBufferBuilder<'a>,
    pstate: &PlayerState,
    changed_fields: u16,
) -> flatbuffers::WIPOffset<fb::PlayerState<'a>> {
    create_fb_player_state_for_delta_ext(builder, pstate, changed_fields, false)
}

/// Extended version that supports optional mobile quantization.
/// When `quantize_for_mobile` is true, position/velocity/rotation values are
/// snapped to a coarser grid before being written as f32 into the FlatBuffer.
/// This reduces delta-compression entropy and saves bandwidth for mobile clients.
pub(super) fn create_fb_player_state_for_delta_ext<'a>(
    builder: &mut flatbuffers::FlatBufferBuilder<'a>,
    pstate: &PlayerState,
    changed_fields: u16,
    quantize_for_mobile: bool,
) -> flatbuffers::WIPOffset<fb::PlayerState<'a>> {
    use crate::core::constants::{quantize_position, quantize_rotation, quantize_velocity};

    let is_full_state = changed_fields == 0xFFFF || changed_fields == u8::MAX as u16;
    let has_position_delta = is_full_state || (changed_fields & FIELD_POSITION_ROTATION) != 0;
    let has_health_delta = is_full_state || (changed_fields & FIELD_HEALTH_ALIVE) != 0;
    let has_weapon_delta = is_full_state || (changed_fields & FIELD_WEAPON_AMMO) != 0;
    let has_score_delta = is_full_state || (changed_fields & FIELD_SCORE_STATS) != 0;
    let has_powerup_delta = is_full_state || (changed_fields & FIELD_POWERUPS) != 0;
    let has_shield_delta = is_full_state || (changed_fields & FIELD_SHIELD) != 0;
    let has_flag_delta = is_full_state || (changed_fields & FIELD_FLAG) != 0;

    let id_fb = fb_safe_str(builder, pstate.id.as_ref());
    let username_fb = if is_full_state || has_score_delta {
        Some(fb_safe_str(builder, &pstate.username))
    } else {
        None
    };
    let weapon_fb = if has_weapon_delta {
        map_server_weapon_to_fb(pstate.weapon)
    } else {
        fb::WeaponType::Pistol
    };

    // Apply mobile quantization: snap to coarser grid to reduce entropy
    let (px, py, rot, vx, vy) = if has_position_delta && quantize_for_mobile {
        (
            quantize_position(pstate.x),
            quantize_position(pstate.y),
            quantize_rotation(pstate.rotation),
            quantize_velocity(pstate.velocity_x),
            quantize_velocity(pstate.velocity_y),
        )
    } else if has_position_delta {
        (
            pstate.x,
            pstate.y,
            pstate.rotation,
            pstate.velocity_x,
            pstate.velocity_y,
        )
    } else {
        (0.0, 0.0, 0.0, 0.0, 0.0)
    };

    fb::PlayerState::create(
        builder,
        &fb::PlayerStateArgs {
            id: Some(id_fb),
            username: username_fb,
            x: px,
            y: py,
            rotation: rot,
            velocity_x: vx,
            velocity_y: vy,
            health: if has_health_delta { pstate.health } else { 0 },
            max_health: if has_health_delta {
                pstate.max_health
            } else {
                0
            },
            alive: if has_health_delta {
                pstate.alive
            } else {
                false
            },
            respawn_timer: if has_health_delta {
                pstate.respawn_timer.unwrap_or(-1.0)
            } else {
                0.0
            },
            weapon: weapon_fb,
            ammo: if has_weapon_delta { pstate.ammo } else { 0 },
            reload_progress: if has_weapon_delta {
                pstate.reload_progress.unwrap_or(-1.0)
            } else {
                0.0
            },
            score: if has_score_delta { pstate.score } else { 0 },
            kills: if has_score_delta { pstate.kills } else { 0 },
            deaths: if has_score_delta { pstate.deaths } else { 0 },
            team_id: if has_score_delta {
                pstate.team_id as i8
            } else {
                0
            },
            speed_boost_remaining: if has_powerup_delta {
                pstate.speed_boost_remaining
            } else {
                0.0
            },
            damage_boost_remaining: if has_powerup_delta {
                pstate.damage_boost_remaining
            } else {
                0.0
            },
            shield_current: if has_shield_delta {
                pstate.shield_current
            } else {
                0
            },
            shield_max: if has_shield_delta {
                pstate.shield_max
            } else {
                0
            },
            is_carrying_flag_team_id: if has_flag_delta {
                pstate.is_carrying_flag_team_id as i8
            } else {
                0
            },
            ability_1_cooldown_remaining: if has_powerup_delta {
                pstate.ability_1_cooldown_remaining
            } else {
                0.0
            },
            ability_2_cooldown_remaining: if has_powerup_delta {
                pstate.ability_2_cooldown_remaining
            } else {
                0.0
            },
            invulnerable_remaining: if has_powerup_delta {
                pstate.invulnerable_remaining
            } else {
                0.0
            },
            secondary_weapon: if has_weapon_delta {
                map_server_weapon_to_fb(pstate.secondary_weapon)
            } else {
                fb::WeaponType::Pistol
            },
            weapon_swap_progress: if has_weapon_delta {
                pstate.weapon_swap_progress
            } else {
                0.0
            },
            current_streak: if has_score_delta {
                pstate.current_streak
            } else {
                0
            },
            primary_weapon: if has_weapon_delta {
                map_server_weapon_to_fb(pstate.primary_weapon)
            } else {
                fb::WeaponType::Rifle
            },
        },
    )
}

pub(super) fn build_chat_game_message_bytes(chat_entry: &ChatMessage) -> Bytes {
    let mut chat_builder = flatbuffers::FlatBufferBuilder::with_capacity(256);

    let player_id_fb = chat_builder.create_string(chat_entry.player_id.as_ref());
    let username_fb = chat_builder.create_string(&chat_entry.username);
    let message_fb = chat_builder.create_string(&chat_entry.message);

    let chat_payload_offset = fb::ChatMessage::create(
        &mut chat_builder,
        &fb::ChatMessageArgs {
            seq: chat_entry.seq,
            player_id: Some(player_id_fb),
            username: Some(username_fb),
            message: Some(message_fb),
            timestamp: chat_entry.timestamp,
        },
    );

    let game_message_offset = fb::GameMessage::create(
        &mut chat_builder,
        &fb::GameMessageArgs {
            msg_type: fb::MessageType::Chat,
            actual_message_type: fb::MessagePayload::ChatMessage,
            actual_message: Some(chat_payload_offset.as_union_value()),
            protocol_version: GAME_PROTOCOL_VERSION,
        },
    );

    chat_builder.finish(game_message_offset, None);
    let (buffer, root_index) = chat_builder.collapse();
    Bytes::from(buffer).slice(root_index..)
}

pub(super) fn build_game_event_fb<'a>(
    builder: &mut flatbuffers::FlatBufferBuilder<'a>,
    event: &GameEvent,
) -> Option<flatbuffers::WIPOffset<fb::GameEvent<'a>>> {
    let event_type = map_game_event_type_to_fb(event)?;
    let event_pos = event_position(event);
    let pos_fb = fb::Vec2::create(
        builder,
        &fb::Vec2Args {
            x: event_pos.x,
            y: event_pos.y,
        },
    );
    let instigator_id_fb = event_instigator_id(event).map(|id| builder.create_string(id.as_ref()));
    let target_id_fb = event_target_id(event).map(|id| builder.create_string(&id));
    let weapon_type_fb =
        event_weapon_type(event).map_or(fb::WeaponType::Pistol, map_server_weapon_to_fb);

    Some(fb::GameEvent::create(
        builder,
        &fb::GameEventArgs {
            event_type,
            position: Some(pos_fb),
            instigator_id: instigator_id_fb,
            target_id: target_id_fb,
            weapon_type: weapon_type_fb,
            value: event_value(event).unwrap_or(0.0),
        },
    ))
}
