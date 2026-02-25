use super::*;

impl MassiveGameServer {
    pub(super) fn manage_bot_population(&self) {
        let mut human_player_count = 0usize;
        self.player_manager
            .for_each_player(|player_id, player_state| {
                if self.bot_players.contains_key(player_id) || player_state.is_spectator {
                    return;
                }
                human_player_count += 1;
            });
        let current_bot_count = self.bot_players.len();

        let max_players_in_match = self.effective_max_players();
        let effective_bot_capacity = max_players_in_match.saturating_sub(self.reserved_human_slots);

        // Quick-match bot auto-fill: if the bot-fill delay has elapsed and we
        // have fewer than the minimum human count, immediately fill remaining
        // slots with bots up to the match-type cap.
        let quick_fill_target = if self.should_quick_match_bot_fill() {
            let fill_to = max_players_in_match.saturating_sub(human_player_count);
            debug!(
                "[Bot Management] QuickMatch auto-fill triggered: humans={}, fill_to={}",
                human_player_count, fill_to,
            );
            Some(fill_to)
        } else {
            None
        };

        let base_desired = if human_player_count >= effective_bot_capacity {
            0
        } else {
            (effective_bot_capacity - human_player_count).min(
                self.target_bot_count
                    .load(std::sync::atomic::Ordering::Relaxed) as usize,
            )
        };

        let desired_bot_count = match quick_fill_target {
            Some(fill) => base_desired.max(fill),
            None => base_desired,
        };

        if current_bot_count > desired_bot_count {
            let bots_to_remove_count = current_bot_count - desired_bot_count;
            debug!(
                "[Bot Management] Max players: {}, Humans: {}, Current Bots: {}, Desired Bots: {}. Removing {} bots.",
                max_players_in_match,
                human_player_count,
                current_bot_count,
                desired_bot_count,
                bots_to_remove_count
            );
            self.remove_bots(bots_to_remove_count);
        } else if current_bot_count < desired_bot_count {
            let bots_to_add_count = desired_bot_count - current_bot_count;
            debug!(
                "[Bot Management] Max players: {}, Humans: {}, Current Bots: {}, Desired Bots: {}. Adding {} bots.",
                max_players_in_match,
                human_player_count,
                current_bot_count,
                desired_bot_count,
                bots_to_add_count
            );
            self.spawn_additional_bots(bots_to_add_count);
        }
    }

    fn team_player_counts(&self) -> (usize, usize) {
        let mut team1_count = 0usize;
        let mut team2_count = 0usize;
        self.player_manager.for_each_player(|_, state| {
            if state.team_id == 1 {
                team1_count += 1;
            } else if state.team_id == 2 {
                team2_count += 1;
            }
        });
        (team1_count, team2_count)
    }

    pub fn ensure_human_join_capacity(&self, joining_peer_id: &str) -> bool {
        self.ensure_human_join_capacity_for_team(joining_peer_id, None)
    }

    pub fn ensure_human_join_capacity_for_team(
        &self,
        joining_peer_id: &str,
        joining_team: Option<u8>,
    ) -> bool {
        let participant_count = self.participant_count();
        let max_players = self.effective_max_players();
        if !self.human_priority_enabled {
            return participant_count < max_players;
        }
        if participant_count < max_players {
            return true;
        }
        let selected_bot = match joining_team {
            Some(team) if team == 1 || team == 2 => self.select_balanced_bot_for_human_join(team),
            _ => self.select_lowest_performing_bot(),
        };
        let Some(bot_id) = selected_bot else {
            warn!(
                "[Human Priority] No bot available to evict for human '{}'; server remains full.",
                joining_peer_id
            );
            return false;
        };
        self.evict_bot_for_human(&bot_id, joining_peer_id, joining_team)
    }

    fn bot_eviction_candidates(&self) -> Vec<(PlayerID, i64, u8, String)> {
        let mut candidates = Vec::with_capacity(self.bot_players.len());

        for entry in self.bot_players.iter() {
            let bot_id = entry.key().clone();
            let (rating, team_id, username) = self
                .player_manager
                .get_player_state(&bot_id)
                .map(|state| {
                    let score = state.score as i64;
                    let kills = state.kills as i64 * 25;
                    let deaths_penalty = state.deaths as i64 * 10;
                    let health_bonus = state.health.max(0) as i64;
                    (
                        score + kills + health_bonus - deaths_penalty,
                        state.team_id,
                        state.username.clone(),
                    )
                })
                .unwrap_or((i64::MIN, 0, bot_id.as_str().to_owned()));
            candidates.push((bot_id, rating, team_id, username));
        }

        candidates
    }

    fn select_lowest_performing_bot(&self) -> Option<PlayerID> {
        self.bot_eviction_candidates()
            .into_iter()
            .min_by_key(|(_, rating, _, _)| *rating)
            .map(|(bot_id, _, _, _)| bot_id)
    }

    fn select_balanced_bot_for_human_join(&self, joining_team: u8) -> Option<PlayerID> {
        let candidates = self.bot_eviction_candidates();
        if candidates.is_empty() {
            return None;
        }
        if joining_team != 1 && joining_team != 2 {
            return candidates
                .into_iter()
                .min_by_key(|(_, rating, _, _)| *rating)
                .map(|(bot_id, _, _, _)| bot_id);
        }

        let (team1_count, team2_count) = self.team_player_counts();
        candidates
            .into_iter()
            .map(|(bot_id, rating, bot_team, _)| {
                let mut projected_team1 =
                    team1_count as i64 + if joining_team == 1 { 1 } else { 0 };
                let mut projected_team2 =
                    team2_count as i64 + if joining_team == 2 { 1 } else { 0 };
                if bot_team == 1 {
                    projected_team1 -= 1;
                } else if bot_team == 2 {
                    projected_team2 -= 1;
                }
                let imbalance = (projected_team1 - projected_team2).abs();
                (bot_id, imbalance, rating)
            })
            .min_by(|lhs, rhs| lhs.1.cmp(&rhs.1).then_with(|| lhs.2.cmp(&rhs.2)))
            .map(|(bot_id, _, _)| bot_id)
    }

    fn enqueue_system_chat_message(&self, message: String) {
        let entry = ChatMessage {
            seq: next_chat_message_seq(),
            player_id: self.player_manager.id_pool.get_or_create("system"),
            username: "System".to_owned(),
            message: message.chars().take(160).collect(),
            timestamp: self.get_server_timestamp_ms(),
        };

        if let Ok(mut chat_q_guard) = self.chat_messages_queue.try_write() {
            chat_q_guard.push_back(entry);
            if chat_q_guard.len() > MAX_CHAT_MESSAGES_HISTORY {
                chat_q_guard.pop_front();
            }
            return;
        }

        let queue = self.chat_messages_queue.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let mut chat_q_guard = queue.write().await;
                chat_q_guard.push_back(entry);
                if chat_q_guard.len() > MAX_CHAT_MESSAGES_HISTORY {
                    chat_q_guard.pop_front();
                }
            });
        } else {
            warn!(
                "[System Chat] Dropped announcement without runtime: {}",
                message
            );
        }
    }

    pub(super) fn push_kill_feed_entry(
        &self,
        killer_name: String,
        victim_name: String,
        weapon: ServerWeaponType,
    ) {
        let mut kill_feed_guard = self.kill_feed.write();
        kill_feed_guard.push_back(ServerKillFeedEntry {
            killer_name,
            victim_name,
            weapon,
            timestamp: self.frame_counter.load(AtomicOrdering::Relaxed),
        });
        if kill_feed_guard.len() > MAX_KILL_FEED_HISTORY {
            kill_feed_guard.pop_front();
        }
    }

    fn evict_bot_for_human(
        &self,
        bot_id: &PlayerID,
        joining_peer_id: &str,
        joining_team: Option<u8>,
    ) -> bool {
        let bot_snapshot = self
            .bot_eviction_candidates()
            .into_iter()
            .find(|(candidate_bot_id, _, _, _)| candidate_bot_id == bot_id)
            .map(|(_, _, team_id, username)| (team_id, username));

        if self.bot_players.remove(bot_id).is_none() {
            return false;
        }

        self.player_manager.remove_player(bot_id.as_str());
        self.data_channels_map.remove(bot_id.as_str());
        self.client_states_map.write().remove(bot_id.as_str());
        self.player_aois.remove(bot_id.as_str());

        info!(
            "[Human Priority] Evicted bot '{}' to free a slot for human '{}'.",
            bot_id, joining_peer_id
        );

        if joining_peer_id != "bot_population_manager" {
            let (bot_team, bot_name) =
                bot_snapshot.unwrap_or((0, format!("Bot {}", bot_id.as_str())));
            let mut announcement = format!(
                "{} was rotated out to free a slot for {}.",
                bot_name, joining_peer_id
            );
            if let Some(team) = joining_team {
                if (team == 1 || team == 2) && (bot_team == 1 || bot_team == 2) {
                    announcement.push_str(&format!(
                        " Team balance: joiner T{}, removed bot T{}.",
                        team, bot_team
                    ));
                }
            }
            self.enqueue_system_chat_message(announcement);
            let joiner_short = &joining_peer_id[..joining_peer_id.len().min(6)];
            self.push_kill_feed_entry(
                format!("Human {}", joiner_short),
                bot_name,
                ServerWeaponType::Melee,
            );
        }

        true
    }

    fn spawn_additional_bots(&self, count_to_add: usize) {
        if count_to_add == 0 {
            return;
        }
        info!(
            "[Bot Management] Attempting to spawn {} additional bots...",
            count_to_add
        );

        let team_spawn_areas = crate::world::map_generator::MapGenerator::get_team_spawn_areas();
        let mut rng = rand::thread_rng();
        let bot_names = [
            "Alpha", "Beta", "Gamma", "Delta", "Echo", "Foxtrot", "Golf", "Hotel", "India",
            "Juliet", "Kilo", "Lima", "Mike", "November", "Oscar", "Papa", "Quebec", "Romeo",
            "Sierra", "Tango", "Uniform", "Victor", "Whiskey", "Xray", "Yankee", "Zulu",
        ];

        for _i in 0..count_to_add {
            let current_total_players = self.player_manager.player_count();
            let max_players = self.effective_max_players();
            if current_total_players >= max_players {
                info!(
                    "[Bot Management] Max player limit ({}) reached, stopping additional bot spawn. Current players: {}",
                    max_players,
                    current_total_players
                );
                break;
            }

            let bot_name_num = self.bot_name_counter.fetch_add(1, AtomicOrdering::SeqCst);
            let bot_base_name = bot_names
                .get(bot_name_num as usize % bot_names.len())
                .unwrap_or(&"Extra");
            let bot_name = format!(
                "Bot {}{}",
                bot_base_name,
                if bot_name_num >= bot_names.len() as u64 {
                    (bot_name_num / bot_names.len() as u64).to_string()
                } else {
                    "".to_string()
                }
            );

            let bot_player_id_str = format!("bot_{}", uuid::Uuid::new_v4());

            let mut team1_player_count = 0;
            let mut team2_player_count = 0;
            self.player_manager.for_each_player(|_id, p_state| {
                if p_state.team_id == 1 {
                    team1_player_count += 1;
                } else if p_state.team_id == 2 {
                    team2_player_count += 1;
                }
            });

            let team_id = if team1_player_count <= team2_player_count {
                1
            } else {
                2
            };

            let potential_spawns_for_team: Vec<Vec2> = team_spawn_areas
                .iter()
                .filter(|(_, sp_team_id)| *sp_team_id == team_id as u8)
                .map(|(pos, _)| *pos)
                .collect();

            let spawn_pos = if !potential_spawns_for_team.is_empty() {
                let base_spawn =
                    potential_spawns_for_team[rng.gen_range(0..potential_spawns_for_team.len())];
                let offset_radius = 50.0;
                let angle = rng.gen_range(0.0..2.0 * std::f32::consts::PI);
                let offset_x = offset_radius * angle.cos();
                let offset_y = offset_radius * angle.sin();
                Vec2::new(
                    (base_spawn.x + offset_x)
                        .clamp(WORLD_MIN_X + PLAYER_RADIUS, WORLD_MAX_X - PLAYER_RADIUS),
                    (base_spawn.y + offset_y)
                        .clamp(WORLD_MIN_Y + PLAYER_RADIUS, WORLD_MAX_Y - PLAYER_RADIUS),
                )
            } else {
                self.respawn_manager.get_respawn_position(
                    self,
                    &Arc::new(bot_player_id_str.clone()),
                    Some(team_id as u8),
                    &[],
                )
            };

            if let Some(player_id_arc) = self.player_manager.add_player(
                bot_player_id_str.clone(),
                bot_name.clone(),
                spawn_pos.x,
                spawn_pos.y,
            ) {
                if let Some(mut p_state_entry) =
                    self.player_manager.get_player_state_mut(&player_id_arc)
                {
                    let p_state = &mut *p_state_entry;
                    p_state.team_id = team_id;
                    p_state.mark_field_changed(FIELD_SCORE_STATS | FIELD_FLAG);
                }

                let bot_controller = BotController {
                    player_id: player_id_arc.clone(),
                    target_position: None,
                    target_enemy_id: None,
                    last_decision_time: Instant::now(),
                    ai_update_accumulator_secs: 0.0,
                    behavior_state: BotBehaviorState::Idle,
                    current_path: VecDeque::new(),
                    path_recalculation_timer: Instant::now(),
                    last_weapon_switch_time: Instant::now(),
                    last_position: Vec2::new(spawn_pos.x, spawn_pos.y),
                    stuck_timer: 0.0,
                    stuck_check_position: Vec2::new(spawn_pos.x, spawn_pos.y),
                    personality: crate::systems::ai::optimized_bot_ai::BotPersonality::random(),
                };
                self.bot_players.insert(player_id_arc, bot_controller);
                debug!(
                    "[Bot Management] Spawned additional bot: {} (ID: {}) on team {} at ({:.1}, {:.1}). Total players: {}",
                    bot_name,
                    bot_player_id_str,
                    team_id,
                    spawn_pos.x,
                    spawn_pos.y,
                    self.player_manager.player_count()
                );
            } else {
                error!(
                    "[Bot Management] Failed to add bot {} to player manager.",
                    bot_name
                );
            }
        }
    }

    fn remove_bots(&self, count: usize) {
        let mut removed_count = 0;
        while removed_count < count {
            let Some(bot_key) = self.select_lowest_performing_bot() else {
                break;
            };
            if self.evict_bot_for_human(&bot_key, "bot_population_manager", None) {
                removed_count += 1;
            } else {
                break;
            }
        }
    }
}
