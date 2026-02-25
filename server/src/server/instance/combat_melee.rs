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
    let mut angle_diff = (angle_to_target - attacker_rotation)
        .rem_euclid(2.0 * std::f32::consts::PI);
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
                let melee_range_sq = 30.0 * 30.0;
                let melee_arc_angle_rad = std::f32::consts::FRAC_PI_3;
                let melee_damage = 30;

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

                            info!("[Melee] {} attempting to hit {} (dist_sq: {:.1}, angle_diff: {:.2} rad).",
                                  attacker_id.as_str(), target_id_arc_nearby.as_str(), dist_sq, angle_diff);

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

                            Some((
                                died,
                                target_position,
                                target_username,
                                victim_was_carrying_flag_id,
                            ))
                        } else {
                            None
                        }
                    };

                    // Now process the hit results without holding any mutable borrows
                    if let Some((
                        died,
                        target_position,
                        target_username,
                        victim_was_carrying_flag_id,
                    )) = target_hit_data
                    {
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
                                        attacker_mut_state.score += POINTS_PER_KILL;
                                    }

                                    attacker_mut_state.mark_field_changed(FIELD_SCORE_STATS);
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
                                              attacker_state_for_score.team_id, attacker_id.as_str());
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

    const MELEE_MAX_RANGE: f32 = 30.0;
    // The arc used in process_melee_hits is FRAC_PI_3 / 2 = pi/6 as half-angle
    // (60 degree total cone). We test with that value from the actual code.
    const ARC_HALF_ANGLE: f32 = std::f32::consts::FRAC_PI_3 / 2.0; // ~0.5236 rad = 30 degrees

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
        assert!(!is_within_melee_range(0.0, 0.0, MELEE_MAX_RANGE, 0.0, MELEE_MAX_RANGE));
    }

    #[test]
    fn out_of_range_far_away() {
        assert!(!is_within_melee_range(0.0, 0.0, 100.0, 0.0, MELEE_MAX_RANGE));
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
        assert!(is_within_melee_arc(0.0, 0.0, 0.0, 10.0, 0.0, ARC_HALF_ANGLE));
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
        assert!(!is_within_melee_arc(0.0, 0.0, 0.0, -10.0, 0.0, ARC_HALF_ANGLE));
    }

    #[test]
    fn within_arc_with_rotated_attacker() {
        // Attacker facing up (rotation=PI/2), target directly above
        let rot = std::f32::consts::FRAC_PI_2;
        assert!(is_within_melee_arc(0.0, 0.0, rot, 0.0, 10.0, ARC_HALF_ANGLE));
    }

    #[test]
    fn cone_check_uses_frac_pi_4_from_constants() {
        // Verify with the MELEE_CONE_HALF_ANGLE_RAD from constants (pi/4)
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
