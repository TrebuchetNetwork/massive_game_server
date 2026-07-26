use super::*;
use crate::operational::bot_sandbox::ExhibitionBotAction;

const EXHIBITION_ATTACK_DAMAGE: i32 = 10;
const EXHIBITION_CHARGE_DAMAGE: i32 = 16;

#[inline]
fn scale_exhibition_damage(damage: i32, percent: i32) -> i32 {
    damage
        .max(0)
        .saturating_mul(percent.max(0))
        .saturating_add(50)
        / 100
}

/// Stable per-model/per-slot/per-strategy-tick jitter. Deliberately excludes
/// the live player UUID so spawn order cannot change a fighter's mechanics.
pub(crate) fn deterministic_exhibition_damage_jitter(
    model_id: &str,
    slot: i32,
    action_tick: u32,
) -> i32 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in model_id
        .as_bytes()
        .iter()
        .copied()
        .chain(slot.to_le_bytes())
        .chain(action_tick.to_le_bytes())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash % 4) as i32 - 1
}

#[inline]
pub(crate) fn exhibition_action_damage(action: ExhibitionBotAction, jitter: i32) -> i32 {
    let base = match action {
        ExhibitionBotAction::Attack => EXHIBITION_ATTACK_DAMAGE,
        ExhibitionBotAction::Charge => EXHIBITION_CHARGE_DAMAGE,
        ExhibitionBotAction::Idle | ExhibitionBotAction::Defend | ExhibitionBotAction::Support => {
            return 0
        }
    };
    (base + jitter.clamp(-1, 2)).max(1)
}

#[inline]
fn consume_exhibition_hit(
    action: ExhibitionBotAction,
    damage_pending: &mut bool,
    selected_target_matches: bool,
    jitter: i32,
) -> i32 {
    if !*damage_pending || !selected_target_matches {
        return 0;
    }
    let damage = exhibition_action_damage(action, jitter);
    if damage <= 0 {
        return 0;
    }
    *damage_pending = false;
    damage
}

/// Apply the ABI's mitigation order: DEFEND first, then one non-stacking
/// SUPPORT shield. Integer rounding matches the deterministic evaluator.
#[inline]
pub(crate) fn mitigate_exhibition_damage(
    damage: i32,
    target_defending: bool,
    target_supported: bool,
) -> i32 {
    let after_defend = if target_defending {
        scale_exhibition_damage(damage, 40)
    } else {
        damage.max(0)
    };
    if target_supported {
        scale_exhibition_damage(after_defend, 50)
    } else {
        after_defend
    }
}

fn drop_flag_after_exhibition_self_death(
    flag: &mut ServerFlagState,
    player_id: &PlayerID,
    position: Vec2,
) -> bool {
    if flag
        .carrier_id
        .as_ref()
        .is_some_and(|carrier_id| carrier_id != player_id)
    {
        return false;
    }
    flag.status = fb::FlagStatus::Dropped;
    flag.position = position;
    flag.carrier_id = None;
    flag.respawn_timer = 30.0;
    true
}

impl MassiveGameServer {
    /// Complete the authoritative side effects of a lethal CHARGE cost. The
    /// player has already entered the normal death/respawn state; no opponent
    /// receives kill credit for this self-inflicted cost.
    pub(crate) fn finalize_exhibition_charge_self_death(
        &self,
        player_id: PlayerID,
        username: String,
        position: Vec2,
        carried_flag_team_id: u8,
    ) {
        self.global_game_events.push(
            GameEvent::PlayerKilled {
                victim_id: player_id.clone(),
                killer_id: Arc::from("arena_overcharge"),
                weapon: ServerWeaponType::Melee,
                position,
            },
            EventPriority::Normal,
        );
        self.push_kill_feed_entry(
            "Overcharge".to_owned(),
            username,
            ServerWeaponType::Melee,
            false,
        );

        if carried_flag_team_id == 0 {
            return;
        }
        {
            let mut match_info = self.match_info.write();
            let Some(flag) = match_info.flag_states.get_mut(&carried_flag_team_id) else {
                return;
            };
            if !drop_flag_after_exhibition_self_death(flag, &player_id, position) {
                return;
            }
        }
        self.global_game_events.push(
            GameEvent::FlagDropped {
                player_id,
                flag_team_id: carried_flag_team_id,
                position,
            },
            EventPriority::High,
        );
    }

    /// Resolve a landed live hit through the exhibition ABI.
    ///
    /// Model fighters get their exact ATTACK/CHARGE damage and can consume one
    /// landed hit per strategy tick. Non-attacking model actions deal zero even
    /// if an old projectile lands. Human/generic damage retains its normal
    /// amount, but is still reduced by a model fighter's DEFEND or SUPPORT.
    pub(super) fn resolve_exhibition_hit_damage(
        &self,
        attacker_id: &PlayerID,
        target_id: &PlayerID,
        normal_damage: i32,
    ) -> i32 {
        if normal_damage <= 0 {
            return 0;
        }

        let target_defending = self.bot_players.get(target_id).is_some_and(|controller| {
            controller.arena_model_id.is_some()
                && controller.arena_action == Some(ExhibitionBotAction::Defend)
        });

        // Gather IDs before looking at player snapshots so no bot-controller
        // map guard is held while another subsystem is consulted.
        let supporting_fighters: Vec<PlayerID> = self
            .bot_players
            .iter()
            .filter(|controller| {
                controller.arena_model_id.is_some()
                    && controller.arena_action == Some(ExhibitionBotAction::Support)
                    && controller.arena_support_target_id.as_ref() == Some(target_id)
            })
            .map(|controller| controller.player_id.clone())
            .collect();
        // A boolean intentionally makes multiple supporters non-stacking.
        let target_supported = supporting_fighters.iter().any(|supporter_id| {
            self.player_manager
                .get_player_state(supporter_id)
                .is_some_and(|supporter| supporter.alive && !supporter.is_spectator)
        });

        let raw_damage = match self.bot_players.get_mut(attacker_id) {
            Some(mut controller) if controller.arena_model_id.is_some() => {
                let Some(action) = controller.arena_action else {
                    return 0;
                };
                let model_id = controller
                    .arena_model_id
                    .as_deref()
                    .expect("guarded by is_some");
                let jitter = deterministic_exhibition_damage_jitter(
                    model_id,
                    controller.arena_slot,
                    controller.arena_action_tick,
                );
                let selected_target_matches =
                    controller.target_enemy_id.as_ref() == Some(target_id);
                consume_exhibition_hit(
                    action,
                    &mut controller.arena_damage_pending,
                    selected_target_matches,
                    jitter,
                )
            }
            _ => normal_damage.max(0),
        };

        mitigate_exhibition_damage(raw_damage, target_defending, target_supported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attack_and_charge_use_exact_bases_and_bounded_jitter() {
        for jitter in -8..=8 {
            let clamped = jitter.clamp(-1, 2);
            assert_eq!(
                exhibition_action_damage(ExhibitionBotAction::Attack, jitter),
                10 + clamped
            );
            assert_eq!(
                exhibition_action_damage(ExhibitionBotAction::Charge, jitter),
                16 + clamped
            );
        }
        for action in [
            ExhibitionBotAction::Idle,
            ExhibitionBotAction::Defend,
            ExhibitionBotAction::Support,
        ] {
            assert_eq!(exhibition_action_damage(action, 2), 0);
        }
    }

    #[test]
    fn jitter_is_stable_and_never_depends_on_player_uuid() {
        let first = deterministic_exhibition_damage_jitter("model/a", 3, 17);
        let second = deterministic_exhibition_damage_jitter("model/a", 3, 17);
        assert_eq!(first, second);
        for tick in 0..256 {
            assert!((-1..=2).contains(&deterministic_exhibition_damage_jitter("model/a", 3, tick)));
        }
    }

    #[test]
    fn defend_and_support_apply_in_evaluator_order() {
        assert_eq!(mitigate_exhibition_damage(10, false, false), 10);
        assert_eq!(mitigate_exhibition_damage(10, true, false), 4);
        assert_eq!(mitigate_exhibition_damage(10, false, true), 5);
        assert_eq!(mitigate_exhibition_damage(10, true, true), 2);
        // Rounding is performed after each stage, matching scale_damage.
        assert_eq!(mitigate_exhibition_damage(27, true, true), 6);
    }

    #[test]
    fn support_is_non_stacking_by_construction() {
        let once = mitigate_exhibition_damage(16, false, true);
        assert_eq!(once, 8);
        // The resolver passes one boolean regardless of supporter count.
        assert_eq!(mitigate_exhibition_damage(16, false, true), once);
    }

    #[test]
    fn lethal_charge_drops_only_the_dead_fighters_carried_flag() {
        let carrier: PlayerID = Arc::from("carrier");
        let other: PlayerID = Arc::from("other");
        let mut flag = ServerFlagState {
            team_id: 2,
            status: fb::FlagStatus::Carried,
            position: Vec2::new(1.0, 2.0),
            carrier_id: Some(carrier.clone()),
            respawn_timer: 0.0,
        };
        let drop_position = Vec2::new(30.0, 40.0);

        assert!(!drop_flag_after_exhibition_self_death(
            &mut flag,
            &other,
            drop_position
        ));
        assert_eq!(flag.carrier_id.as_ref(), Some(&carrier));
        assert!(drop_flag_after_exhibition_self_death(
            &mut flag,
            &carrier,
            drop_position
        ));
        assert_eq!(flag.status, fb::FlagStatus::Dropped);
        assert_eq!(flag.position, drop_position);
        assert!(flag.carrier_id.is_none());
        assert_eq!(flag.respawn_timer, 30.0);
    }

    #[test]
    fn successful_hit_consumes_exactly_one_attack_per_strategy_tick() {
        let mut pending = true;
        assert_eq!(
            consume_exhibition_hit(ExhibitionBotAction::Attack, &mut pending, true, 2),
            12
        );
        assert!(!pending);
        assert_eq!(
            consume_exhibition_hit(ExhibitionBotAction::Attack, &mut pending, true, 2),
            0
        );

        let mut wrong_target_pending = true;
        assert_eq!(
            consume_exhibition_hit(
                ExhibitionBotAction::Charge,
                &mut wrong_target_pending,
                false,
                0,
            ),
            0
        );
        assert!(
            wrong_target_pending,
            "a miss must not consume the strategy hit"
        );

        let mut defend_pending = true;
        assert_eq!(
            consume_exhibition_hit(ExhibitionBotAction::Defend, &mut defend_pending, true, 0,),
            0
        );
        assert!(defend_pending, "non-attacking actions cannot land damage");
    }
}
