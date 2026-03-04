use super::*;

/// Pure geometric check: is the target within the melee cone arc?
/// Returns true if the angle between the attacker's facing direction and
/// the direction to the target is within half the arc angle.
#[inline]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn is_within_melee_arc(
    attacker_x: f32,
    attacker_y: f32,
    attacker_rotation: f32,
    target_x: f32,
    target_y: f32,
    arc_half_angle: f32,
) -> bool {
    let dx = target_x - attacker_x;
    let dy = target_y - attacker_y;
    let angle_to_target = dy.atan2(dx);
    let mut angle_diff =
        (angle_to_target - attacker_rotation).rem_euclid(2.0 * std::f32::consts::PI);
    if angle_diff > std::f32::consts::PI {
        angle_diff = 2.0 * std::f32::consts::PI - angle_diff;
    }
    angle_diff <= arc_half_angle
}

/// Pure geometric check: is the target within melee range (squared distance)?
#[inline]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn is_within_melee_range(
    attacker_x: f32,
    attacker_y: f32,
    target_x: f32,
    target_y: f32,
    max_range: f32,
) -> bool {
    let dx = target_x - attacker_x;
    let dy = target_y - attacker_y;
    (dx * dx + dy * dy) < max_range * max_range
}

impl MassiveGameServer {
    // Extracted melee processing logic
    pub(super) fn process_melee_hits(&self, melee_hit_events: Vec<GameEvent>) {
        for event in melee_hit_events {
            if let GameEvent::MeleeHit {
                attacker_id,
                position: _attack_pos,
                ..
            } = event
            {
                let melee_range_sq = crate::core::constants::MELEE_MAX_RANGE
                    * crate::core::constants::MELEE_MAX_RANGE;
                let melee_arc_angle_rad = crate::core::constants::MELEE_CONE_HALF_ANGLE_RAD * 2.0;
                let melee_damage = crate::core::constants::MELEE_DAMAGE;

                // Get attacker info
                let (
                    attacker_pos_x,
                    attacker_pos_y,
                    attacker_rot,
                    attacker_team_id,
                    attacker_username,
                    attacker_is_spectator,
                ) = {
                    if let Some(attacker_state_guard) =
                        self.player_manager.get_player_state(&attacker_id)
                    {
                        (
                            attacker_state_guard.x,
                            attacker_state_guard.y,
                            attacker_state_guard.rotation,
                            attacker_state_guard.team_id,
                            attacker_state_guard.username.clone(),
                            attacker_state_guard.is_spectator,
                        )
                    } else {
                        continue; // Attacker not found
                    }
                };
                if attacker_is_spectator {
                    continue;
                }

                // Use spatial index for nearby players
                let melee_check_radius = 70.0;
                let nearby_player_ids = self.spatial_index.query_nearby_players(
                    attacker_pos_x,
                    attacker_pos_y,
                    melee_check_radius,
                );

                enum MeleeResolution {
                    Hit {
                        died: bool,
                        target_position: Vec2,
                        target_username: String,
                        victim_was_carrying_flag_id: u8,
                    },
                    Parried {
                        defender_id: PlayerID,
                        defender_position: Vec2,
                        defender_username: String,
                    },
                }

                // Process each potential target
                for target_id_arc_nearby in nearby_player_ids {
                    if target_id_arc_nearby == attacker_id {
                        continue;
                    }

                    // Collect all the data we need from the target before applying damage
                    let target_hit_data = {
                        if let Some(mut target_state_entry) = self
                            .player_manager
                            .get_player_state_mut(&target_id_arc_nearby)
                        {
                            let target_state = &mut *target_state_entry;

                            if !target_state.alive
                                || target_state.is_spectator
                                || (target_state.team_id != 0
                                    && attacker_team_id != 0
                                    && target_state.team_id == attacker_team_id)
                            {
                                continue; // Skip dead or same-team targets
                            }

                            let dx = target_state.x - attacker_pos_x;
                            let dy = target_state.y - attacker_pos_y;
                            let dist_sq = dx * dx + dy * dy;

                            if dist_sq >= melee_range_sq {
                                continue; // Out of range
                            }

                            if !self.has_clear_line_of_sight(
                                attacker_pos_x,
                                attacker_pos_y,
                                target_state.x,
                                target_state.y,
                            ) {
                                continue; // Wall blocks melee reach
                            }

                            let angle_to_target = dy.atan2(dx);
                            let mut angle_diff = (angle_to_target - attacker_rot)
                                .rem_euclid(2.0 * std::f32::consts::PI);
                            if angle_diff > std::f32::consts::PI {
                                angle_diff = 2.0 * std::f32::consts::PI - angle_diff;
                            }

                            if angle_diff > melee_arc_angle_rad / 2.0 {
                                continue; // Outside melee arc
                            }

                            let parry_active = target_state.weapon == ServerWeaponType::Melee
                                && target_state
                                    .last_shot_time
                                    .map(|last_melee_time| {
                                        last_melee_time.elapsed().as_secs_f32()
                                            <= crate::core::constants::MELEE_PARRY_WINDOW_SECS
                                    })
                                    .unwrap_or(false);
                            if parry_active
                                && is_within_melee_arc(
                                    target_state.x,
                                    target_state.y,
                                    target_state.rotation,
                                    attacker_pos_x,
                                    attacker_pos_y,
                                    crate::core::constants::MELEE_PARRY_CONE_HALF_ANGLE_RAD,
                                )
                            {
                                let defender_position = Vec2::new(target_state.x, target_state.y);
                                let defender_username = target_state.username.clone();
                                Some(MeleeResolution::Parried {
                                    defender_id: target_id_arc_nearby.clone(),
                                    defender_position,
                                    defender_username,
                                })
                            } else {
                                info!("[Melee] {} attempting to hit {} (dist_sq: {:.1}, angle_diff: {:.2} rad).",
                                      attacker_id.as_ref(), target_id_arc_nearby.as_ref(), dist_sq, angle_diff);

                                // Apply damage and collect necessary data
                                let died = target_state.apply_damage(melee_damage);
                                let target_position = Vec2::new(target_state.x, target_state.y);
                                let target_username = target_state.username.clone();
                                let victim_was_carrying_flag_id = if died {
                                    target_state.is_carrying_flag_team_id
                                } else {
                                    0
                                };

                                if died {
                                    // Reset flag carry state on the victim
                                    target_state.is_carrying_flag_team_id = 0;
                                    target_state.mark_field_changed(FIELD_FLAG);
                                }

                                Some(MeleeResolution::Hit {
                                    died,
                                    target_position,
                                    target_username,
                                    victim_was_carrying_flag_id,
                                })
                            }
                        } else {
                            None
                        }
                    };

                    // Now process the hit results without holding any mutable borrows
                    if let Some(target_resolution) = target_hit_data {
                        if let MeleeResolution::Parried {
                            defender_id,
                            defender_position,
                            defender_username,
                        } = target_resolution
                        {
                            let mut counter_damage_applied = 0;
                            let mut parry_impact_position =
                                Vec2::new(attacker_pos_x, attacker_pos_y);
                            if let Some(mut attacker_state_entry) =
                                self.player_manager.get_player_state_mut(&attacker_id)
                            {
                                let attacker_state = &mut *attacker_state_entry;
                                let max_nonlethal = (attacker_state.health - 1).max(0);
                                let counter_damage =
                                    crate::core::constants::MELEE_PARRY_COUNTER_DAMAGE
                                        .min(max_nonlethal);
                                if counter_damage > 0
                                    && attacker_state.alive
                                    && attacker_state.invulnerable_remaining <= 0.0
                                {
                                    let _ = attacker_state.apply_damage(counter_damage);
                                    counter_damage_applied = counter_damage;
                                }

                                let away_dx = attacker_state.x - defender_position.x;
                                let away_dy = attacker_state.y - defender_position.y;
                                let away_len = (away_dx * away_dx + away_dy * away_dy).sqrt();
                                let (dir_x, dir_y) = if away_len > 0.001 {
                                    (away_dx / away_len, away_dy / away_len)
                                } else {
                                    (-attacker_rot.cos(), -attacker_rot.sin())
                                };
                                let knockback_dist =
                                    crate::core::constants::MELEE_PARRY_KNOCKBACK_DISTANCE.max(0.0);
                                let next_x = (attacker_state.x + dir_x * knockback_dist).clamp(
                                    WORLD_MIN_X + PLAYER_RADIUS,
                                    WORLD_MAX_X - PLAYER_RADIUS,
                                );
                                let next_y = (attacker_state.y + dir_y * knockback_dist).clamp(
                                    WORLD_MIN_Y + PLAYER_RADIUS,
                                    WORLD_MAX_Y - PLAYER_RADIUS,
                                );
                                if self.has_clear_line_of_sight(
                                    attacker_state.x,
                                    attacker_state.y,
                                    next_x,
                                    next_y,
                                ) && !self.position_overlaps_any_wall(next_x, next_y)
                                {
                                    attacker_state.x = next_x;
                                    attacker_state.y = next_y;
                                    attacker_state.mark_field_changed(FIELD_POSITION_ROTATION);
                                }
                                parry_impact_position =
                                    Vec2::new(attacker_state.x, attacker_state.y);
                            }

                            if counter_damage_applied > 0 {
                                if let Some(mut defender_state) =
                                    self.player_manager.get_player_state_mut(&defender_id)
                                {
                                    defender_state.record_damage_dealt(counter_damage_applied);
                                    defender_state.mark_field_changed(FIELD_SCORE_STATS);
                                }
                                self.global_game_events.push(
                                    GameEvent::PlayerDamaged {
                                        target_id: attacker_id.clone(),
                                        attacker_id: Some(defender_id.clone()),
                                        damage: counter_damage_applied,
                                        weapon: ServerWeaponType::Melee,
                                        position: parry_impact_position,
                                    },
                                    EventPriority::Normal,
                                );
                            }

                            self.global_game_events.push(
                                GameEvent::WeaponFired {
                                    player_id: defender_id,
                                    weapon: ServerWeaponType::Melee,
                                    position: defender_position,
                                },
                                EventPriority::Normal,
                            );
                            info!(
                                "[Melee] {} parried {} and countered for {} damage.",
                                defender_username, attacker_username, counter_damage_applied
                            );
                            break;
                        }

                        let MeleeResolution::Hit {
                            died,
                            target_position,
                            target_username,
                            victim_was_carrying_flag_id,
                        } = target_resolution
                        else {
                            continue;
                        };

                        if let Some(mut attacker_state_entry) =
                            self.player_manager.get_player_state_mut(&attacker_id)
                        {
                            attacker_state_entry.record_damage_dealt(melee_damage);
                            attacker_state_entry.mark_field_changed(FIELD_SCORE_STATS);
                        }

                        // Push damage event
                        self.global_game_events.push(
                            GameEvent::PlayerDamaged {
                                target_id: target_id_arc_nearby.clone(),
                                attacker_id: Some(attacker_id.clone()),
                                damage: melee_damage,
                                weapon: ServerWeaponType::Melee,
                                position: target_position,
                            },
                            EventPriority::Normal,
                        );

                        if died {
                            // Update attacker stats
                            if attacker_id != target_id_arc_nearby {
                                // Get victim team for friendly fire check
                                let victim_team = self
                                    .player_manager
                                    .get_player_state(&target_id_arc_nearby)
                                    .map(|p| p.team_id)
                                    .unwrap_or(0);

                                if let Some(mut attacker_mut_state_entry) =
                                    self.player_manager.get_player_state_mut(&attacker_id)
                                {
                                    let attacker_mut_state = &mut *attacker_mut_state_entry;
                                    let mut killstreak_event: Option<(u32, Vec2)> = None;
                                    attacker_mut_state.kills += 1;
                                    attacker_mut_state
                                        .record_kill_with_weapon(ServerWeaponType::Melee);

                                    // Check for friendly fire
                                    if attacker_team_id != 0
                                        && victim_team != 0
                                        && attacker_team_id == victim_team
                                    {
                                        // Friendly fire: double negative score
                                        attacker_mut_state.score -= 200;
                                        info!("Friendly fire penalty (melee): {} killed teammate {}, -200 score",
                                              attacker_username, target_username);
                                    } else {
                                        // Normal kill: positive score
                                        let kill_points =
                                            self.hot_zone_kill_points_at_position(target_position);
                                        attacker_mut_state.score += kill_points;
                                        if kill_points > POINTS_PER_KILL {
                                            info!(
                                                "Hot zone bonus: {} gained {} points for melee elimination at ({:.1}, {:.1})",
                                                attacker_username, kill_points, target_position.x, target_position.y
                                            );
                                        }
                                        let streak = self.advance_killstreak(attacker_mut_state);
                                        if streak
                                            >= crate::core::constants::KILLSTREAK_DAMAGE_BOOST_THRESHOLD
                                        {
                                            killstreak_event = Some((
                                                streak,
                                                Vec2::new(attacker_mut_state.x, attacker_mut_state.y),
                                            ));
                                        }
                                    }

                                    attacker_mut_state.mark_field_changed(FIELD_SCORE_STATS);
                                    if let Some((streak, position)) = killstreak_event {
                                        self.global_game_events.push(
                                            GameEvent::Killstreak {
                                                player_id: attacker_id.clone(),
                                                streak,
                                                position,
                                            },
                                            EventPriority::High,
                                        );
                                    }
                                }
                            }

                            // Push kill event
                            self.global_game_events.push(
                                GameEvent::PlayerKilled {
                                    victim_id: target_id_arc_nearby.clone(),
                                    killer_id: attacker_id.clone(),
                                    weapon: ServerWeaponType::Melee,
                                    position: target_position,
                                },
                                EventPriority::High,
                            );

                            // Losing team respawn reduction for melee kills
                            {
                                let victim_team = self
                                    .player_manager
                                    .get_player_state(&target_id_arc_nearby)
                                    .map(|p| p.team_id)
                                    .unwrap_or(0);
                                if victim_team != 0 {
                                    let match_info_guard = self.match_info.read();
                                    let victim_team_score = match_info_guard
                                        .team_scores
                                        .get(&victim_team)
                                        .cloned()
                                        .unwrap_or(0);
                                    let max_enemy_score = match_info_guard
                                        .team_scores
                                        .iter()
                                        .filter(|(&tid, _)| tid != victim_team)
                                        .map(|(_, &s)| s)
                                        .max()
                                        .unwrap_or(0);
                                    drop(match_info_guard);

                                    let deficit = max_enemy_score - victim_team_score;
                                    if deficit > 0 {
                                        let reduction_ticks = (deficit / 5).max(0) as f32;
                                        let reduction = reduction_ticks
                                            * LOSING_TEAM_RESPAWN_REDUCTION_PER_5PTS;
                                        if let Some(mut victim_entry) = self
                                            .player_manager
                                            .get_player_state_mut(&target_id_arc_nearby)
                                        {
                                            if let Some(ref mut timer) = victim_entry.respawn_timer
                                            {
                                                *timer = (*timer - reduction).max(0.5);
                                            }
                                        }
                                    }
                                }
                            }

                            // Update kill feed
                            self.capture_killcam_for_victim(
                                &target_id_arc_nearby,
                                &target_username,
                                &attacker_id,
                                ServerWeaponType::Melee,
                            );

                            self.push_kill_feed_entry(
                                attacker_username.clone(),
                                target_username,
                                ServerWeaponType::Melee,
                                false,
                            );

                            // Handle flag dropping if victim was carrying a flag
                            if victim_was_carrying_flag_id != 0 {
                                let mut match_info_guard = self.match_info.write();

                                // Award score to attacker's team if applicable
                                if let Some(attacker_state_for_score) =
                                    self.player_manager.get_player_state(&attacker_id)
                                {
                                    if attacker_state_for_score.team_id != 0
                                        && attacker_state_for_score.team_id
                                            != victim_was_carrying_flag_id
                                    {
                                        let team_score_mut_ref = match_info_guard
                                            .team_scores
                                            .entry(attacker_state_for_score.team_id)
                                            .or_insert(0);
                                        *team_score_mut_ref += 1;
                                        info!("Team {} scored +1 via melee kill on flag carrier by {}",
                                              attacker_state_for_score.team_id, attacker_id.as_ref());
                                    }
                                }

                                // Drop the flag
                                if let Some(flag_state) = match_info_guard
                                    .flag_states
                                    .get_mut(&victim_was_carrying_flag_id)
                                {
                                    flag_state.status = fb::FlagStatus::Dropped;
                                    flag_state.position = target_position;
                                    flag_state.carrier_id = None;
                                    flag_state.respawn_timer = 30.0;

                                    // Push flag dropped event after releasing match_info lock
                                    drop(match_info_guard);

                                    self.global_game_events.push(
                                        GameEvent::FlagDropped {
                                            player_id: target_id_arc_nearby.clone(),
                                            flag_team_id: victim_was_carrying_flag_id,
                                            position: target_position,
                                        },
                                        EventPriority::High,
                                    );

                                    info!(
                                        "(Melee Kill) Flag of team {} dropped at ({:.1}, {:.1})",
                                        victim_was_carrying_flag_id,
                                        target_position.x,
                                        target_position.y
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MELEE_MAX_RANGE: f32 = crate::core::constants::MELEE_MAX_RANGE;
    // The arc used in process_melee_hits divides by 2 to get the half-angle.
    // MELEE_CONE_HALF_ANGLE_RAD is the half-angle (pi/6 = 30 degrees, 60° total cone).
    const ARC_HALF_ANGLE: f32 = crate::core::constants::MELEE_CONE_HALF_ANGLE_RAD;

    // ── Range tests ─────────────────────────────────────────────────

    #[test]
    fn within_range_at_zero_distance() {
        assert!(is_within_melee_range(0.0, 0.0, 0.0, 0.0, MELEE_MAX_RANGE));
    }

    #[test]
    fn within_range_just_inside() {
        assert!(is_within_melee_range(0.0, 0.0, 29.0, 0.0, MELEE_MAX_RANGE));
    }

    #[test]
    fn out_of_range_at_boundary() {
        // At exactly max_range, dist_sq == range_sq, the check is strict <
        assert!(!is_within_melee_range(
            0.0,
            0.0,
            MELEE_MAX_RANGE,
            0.0,
            MELEE_MAX_RANGE
        ));
    }

    #[test]
    fn out_of_range_far_away() {
        assert!(!is_within_melee_range(
            0.0,
            0.0,
            100.0,
            0.0,
            MELEE_MAX_RANGE
        ));
    }

    #[test]
    fn within_range_diagonal() {
        // diagonal distance = sqrt(20^2 + 20^2) = ~28.28 < 30
        assert!(is_within_melee_range(0.0, 0.0, 20.0, 20.0, MELEE_MAX_RANGE));
    }

    // ── Cone angle tests ─────────────────────────────────────────────

    #[test]
    fn within_arc_directly_ahead() {
        // Attacker facing right (rotation=0), target directly to the right
        assert!(is_within_melee_arc(
            0.0,
            0.0,
            0.0,
            10.0,
            0.0,
            ARC_HALF_ANGLE
        ));
    }

    #[test]
    fn within_arc_at_edge() {
        // Target at almost exactly the arc boundary
        let angle = ARC_HALF_ANGLE - 0.01;
        let tx = 10.0 * angle.cos();
        let ty = 10.0 * angle.sin();
        assert!(is_within_melee_arc(0.0, 0.0, 0.0, tx, ty, ARC_HALF_ANGLE));
    }

    #[test]
    fn outside_arc_just_beyond() {
        // Target just outside the arc boundary
        let angle = ARC_HALF_ANGLE + 0.05;
        let tx = 10.0 * angle.cos();
        let ty = 10.0 * angle.sin();
        assert!(!is_within_melee_arc(0.0, 0.0, 0.0, tx, ty, ARC_HALF_ANGLE));
    }

    #[test]
    fn outside_arc_behind_attacker() {
        // Target directly behind the attacker
        assert!(!is_within_melee_arc(
            0.0,
            0.0,
            0.0,
            -10.0,
            0.0,
            ARC_HALF_ANGLE
        ));
    }

    #[test]
    fn within_arc_with_rotated_attacker() {
        // Attacker facing up (rotation=PI/2), target directly above
        let rot = std::f32::consts::FRAC_PI_2;
        assert!(is_within_melee_arc(
            0.0,
            0.0,
            rot,
            0.0,
            10.0,
            ARC_HALF_ANGLE
        ));
    }

    #[test]
    fn cone_check_uses_melee_half_angle_from_constants() {
        // Verify with the MELEE_CONE_HALF_ANGLE_RAD from constants
        let half = crate::core::constants::MELEE_CONE_HALF_ANGLE_RAD;
        // Target at exactly half angle should be within
        let tx = 10.0 * (half - 0.01_f32).cos();
        let ty = 10.0 * (half - 0.01_f32).sin();
        assert!(is_within_melee_arc(0.0, 0.0, 0.0, tx, ty, half));
        // Target beyond half angle should be outside
        let tx2 = 10.0 * (half + 0.05_f32).cos();
        let ty2 = 10.0 * (half + 0.05_f32).sin();
        assert!(!is_within_melee_arc(0.0, 0.0, 0.0, tx2, ty2, half));
    }
}
