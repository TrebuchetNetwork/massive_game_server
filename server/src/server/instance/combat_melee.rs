use super::*;

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
                                        attacker_mut_state.score += 100;
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
