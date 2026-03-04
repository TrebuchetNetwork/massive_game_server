use super::*;
use crate::core::deterministic_rng::DeterministicRng;

#[derive(Debug, Clone, Copy)]
struct DynamicModeThresholds {
    tdm_transition_elapsed: f32,
    tdm_countdown_elapsed: f32,
    ctf_transition_remaining: f32,
    ctf_countdown_20_remaining: f32,
    ctf_countdown_10_remaining: f32,
    ctf_countdown_5_remaining: f32,
}

fn dynamic_mode_thresholds(match_duration_secs: f32) -> DynamicModeThresholds {
    let scaled_threshold = |seconds_for_full_match: f32| {
        (match_duration_secs * (seconds_for_full_match / FULL_MATCH_DURATION_SECS)).max(0.0)
    };
    DynamicModeThresholds {
        tdm_transition_elapsed: scaled_threshold(120.0),
        tdm_countdown_elapsed: scaled_threshold(105.0),
        ctf_transition_remaining: scaled_threshold(70.0),
        ctf_countdown_20_remaining: scaled_threshold(90.0),
        ctf_countdown_10_remaining: scaled_threshold(80.0),
        ctf_countdown_5_remaining: scaled_threshold(75.0),
    }
}

fn map_event_interval_from_seed(seed: u64) -> f32 {
    let mut rng = DeterministicRng::new(seed ^ 0xA5A5_A5A5_1F2E_3D4C);
    rng.gen_range_f32(MAP_EVENT_INTERVAL_MIN_SECS, MAP_EVENT_INTERVAL_MAX_SECS)
        .clamp(MAP_EVENT_INTERVAL_MIN_SECS, MAP_EVENT_INTERVAL_MAX_SECS)
}

fn hot_zone_center_from_seed(seed: u64) -> Vec2 {
    let mut rng = DeterministicRng::new(seed ^ 0x4C3D_2E1F_A5A5_A5A5);
    let margin_x = HOT_ZONE_SPAWN_MARGIN.clamp(0.0, (WORLD_MAX_X - WORLD_MIN_X) * 0.45);
    let margin_y = HOT_ZONE_SPAWN_MARGIN.clamp(0.0, (WORLD_MAX_Y - WORLD_MIN_Y) * 0.45);
    let min_x = (WORLD_MIN_X + margin_x).min(WORLD_MAX_X - 40.0);
    let max_x = (WORLD_MAX_X - margin_x).max(WORLD_MIN_X + 40.0);
    let min_y = (WORLD_MIN_Y + margin_y).min(WORLD_MAX_Y - 40.0);
    let max_y = (WORLD_MAX_Y - margin_y).max(WORLD_MIN_Y + 40.0);
    Vec2::new(
        rng.gen_range_f32(min_x, max_x),
        rng.gen_range_f32(min_y, max_y),
    )
}

impl MassiveGameServer {
    #[inline]
    fn fortress_attack_defend_map(&self) -> bool {
        self.map_name.trim().eq_ignore_ascii_case("Fortress")
    }

    #[inline]
    fn fortress_attacking_team() -> u8 {
        1
    }

    #[inline]
    fn fortress_defending_team() -> u8 {
        2
    }

    fn game_mode_label(mode: fb::GameModeType) -> &'static str {
        match mode {
            fb::GameModeType::FreeForAll => "FreeForAll",
            fb::GameModeType::TeamDeathmatch => "TeamDeathmatch",
            fb::GameModeType::CaptureTheFlag => "CaptureTheFlag",
            _ => "Unknown",
        }
    }

    #[inline]
    fn ctf_player_label(&self, player_id: &PlayerID) -> String {
        self.player_manager
            .get_player_state(player_id)
            .map(|state| state.username.clone())
            .unwrap_or_else(|| player_id.as_ref().to_owned())
    }

    fn broadcast_dynamic_mode_event(
        &self,
        phase: &str,
        from_mode: fb::GameModeType,
        to_mode: fb::GameModeType,
        seconds_remaining: Option<u32>,
        time_remaining: f32,
    ) {
        let payload = serde_json::json!({
            "phase": phase,
            "from_mode": Self::game_mode_label(from_mode),
            "to_mode": Self::game_mode_label(to_mode),
            "seconds_remaining": seconds_remaining,
            "time_remaining": time_remaining.max(0.0),
        });
        if let Some(packet) = self.build_system_event_packet("mode_transition", Some(&payload)) {
            self.enqueue_direct_packet_for_all_players(packet);
        }
    }

    fn broadcast_ctf_overtime_event(&self, round: u8, duration_secs: f32) {
        let payload = serde_json::json!({
            "phase": "ctf_overtime",
            "round": round,
            "duration_secs": duration_secs.max(0.0),
            "time_remaining": duration_secs.max(0.0),
        });
        if let Some(packet) = self.build_system_event_packet("ctf_overtime", Some(&payload)) {
            self.enqueue_direct_packet_for_all_players(packet);
        }
    }

    fn broadcast_fortress_phase_event(
        &self,
        phase: &str,
        attacking_team: u8,
        defending_team: u8,
        time_remaining: f32,
        outcome: Option<&str>,
    ) {
        let payload = serde_json::json!({
            "phase": phase,
            "attacking_team": attacking_team,
            "defending_team": defending_team,
            "time_remaining": time_remaining.max(0.0),
            "outcome": outcome,
        });
        if let Some(packet) = self.build_system_event_packet("fortress_phase", Some(&payload)) {
            self.enqueue_direct_packet_for_all_players(packet);
        }
    }

    fn next_map_event_interval_secs(&self, event_sequence: u64) -> f32 {
        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        map_event_interval_from_seed(frame ^ event_sequence.wrapping_mul(0x9E37_79B9_7F4A_7C15))
    }

    fn next_hot_zone_center(&self, event_sequence: u64) -> Vec2 {
        let frame = self.frame_counter.load(AtomicOrdering::Relaxed);
        hot_zone_center_from_seed(frame ^ event_sequence.wrapping_mul(0x94D0_49BB_1331_11EB))
    }

    fn spawn_map_event_supply_drop(&self, center: Vec2, event_index: u64) -> usize {
        let seed = self.frame_counter.load(AtomicOrdering::Relaxed)
            ^ event_index.wrapping_mul(0xD6E8_FEB8_6659_FD93)
            ^ ((center.x.to_bits() as u64) << 17)
            ^ ((center.y.to_bits() as u64) << 1);
        let mut rng = DeterministicRng::new(seed);
        let pickup_types = [
            CorePickupType::DamageBoost,
            CorePickupType::Shield,
            CorePickupType::SpeedBoost,
            CorePickupType::WeaponCrate(ServerWeaponType::Shotgun),
            CorePickupType::WeaponCrate(ServerWeaponType::Sniper),
            CorePickupType::Health,
            CorePickupType::Ammo,
        ];

        let mut spawned = Vec::with_capacity(MAP_EVENT_SUPPLY_DROP_PICKUPS);
        {
            let mut pickups = self.pickups.write();
            for idx in 0..MAP_EVENT_SUPPLY_DROP_PICKUPS {
                let angle = rng.gen_range_f32(0.0, 2.0 * std::f32::consts::PI);
                let radius = rng.gen_range_f32(
                    MAP_EVENT_SUPPLY_DROP_INNER_RADIUS,
                    MAP_EVENT_SUPPLY_DROP_OUTER_RADIUS,
                );
                let spawn_x =
                    (center.x + radius * angle.cos()).clamp(WORLD_MIN_X + 40.0, WORLD_MAX_X - 40.0);
                let spawn_y =
                    (center.y + radius * angle.sin()).clamp(WORLD_MIN_Y + 40.0, WORLD_MAX_Y - 40.0);
                let overlaps_wall = self
                    .wall_spatial_index
                    .query_radius(spawn_x, spawn_y, PLAYER_RADIUS + 8.0)
                    .iter()
                    .any(|wall| {
                        let closest_x = spawn_x.clamp(wall.x, wall.x + wall.width);
                        let closest_y = spawn_y.clamp(wall.y, wall.y + wall.height);
                        let dx = spawn_x - closest_x;
                        let dy = spawn_y - closest_y;
                        dx * dx + dy * dy < PLAYER_RADIUS * PLAYER_RADIUS
                    });
                if overlaps_wall {
                    continue;
                }

                let pickup = Pickup::new(
                    generate_entity_id(),
                    spawn_x,
                    spawn_y,
                    pickup_types[idx % pickup_types.len()].clone(),
                );
                pickups.push(pickup.clone());
                spawned.push(pickup);
            }
        }

        for pickup in &spawned {
            self.upsert_pickup_in_partition_index(pickup);
        }
        spawned.len()
    }

    fn trigger_center_supply_drop_map_event(&self, event_index: u64, next_interval_secs: f32) {
        let center = Vec2::new(
            (WORLD_MIN_X + WORLD_MAX_X) * 0.5,
            (WORLD_MIN_Y + WORLD_MAX_Y) * 0.5,
        );
        let spawned_pickups = self.spawn_map_event_supply_drop(center, event_index);
        let payload = serde_json::json!({
            "phase": "triggered",
            "event_type": "center_supply_drop",
            "event_index": event_index.saturating_add(1),
            "x": center.x,
            "y": center.y,
            "spawned_pickups": spawned_pickups,
            "next_event_secs": next_interval_secs.max(0.0),
            "ping_kind": "defend",
        });
        if let Some(packet) = self.build_system_event_packet("map_event", Some(&payload)) {
            self.enqueue_direct_packet_for_all_players(packet);
        }
        info!(
            "Map event #{} triggered: center supply drop (pickups={}, next_in={:.1}s).",
            event_index.saturating_add(1),
            spawned_pickups,
            next_interval_secs
        );
    }

    fn trigger_hot_zone_map_event(&self, event_index: u64, center: Vec2) {
        let spawned_pickups =
            self.spawn_map_event_supply_drop(center, event_index ^ 0xBADC_0FFE_D00D_F00D);
        let payload = serde_json::json!({
            "phase": "triggered",
            "event_type": "hot_zone",
            "event_index": event_index.saturating_add(1),
            "x": center.x,
            "y": center.y,
            "radius": HOT_ZONE_RADIUS,
            "bonus_multiplier": HOT_ZONE_POINTS_MULTIPLIER,
            "spawned_pickups": spawned_pickups,
            "next_event_secs": HOT_ZONE_ROTATE_INTERVAL_SECS.max(0.0),
            "ping_kind": "enemy",
        });
        if let Some(packet) = self.build_system_event_packet("map_event", Some(&payload)) {
            self.enqueue_direct_packet_for_all_players(packet);
        }
        info!(
            "Map event #{} triggered: hot zone at ({:.1}, {:.1}), radius={:.1}, pickups={}.",
            event_index.saturating_add(1),
            center.x,
            center.y,
            HOT_ZONE_RADIUS,
            spawned_pickups
        );
    }

    pub(super) fn hot_zone_bonus_multiplier_at_position(&self, position: Vec2) -> f32 {
        let match_info_guard = self.match_info.read();
        if !match_info_guard.hot_zone_active
            || match_info_guard.match_state != fb::MatchStateType::Active
        {
            return 1.0;
        }
        let dx = position.x - match_info_guard.hot_zone_center.x;
        let dy = position.y - match_info_guard.hot_zone_center.y;
        if (dx * dx + dy * dy)
            <= match_info_guard.hot_zone_radius * match_info_guard.hot_zone_radius
        {
            HOT_ZONE_POINTS_MULTIPLIER
        } else {
            1.0
        }
    }

    pub(super) fn hot_zone_kill_points_at_position(&self, position: Vec2) -> i32 {
        let multiplier = self.hot_zone_bonus_multiplier_at_position(position);
        let scaled = (POINTS_PER_KILL as f32 * multiplier).round() as i32;
        scaled.max(POINTS_PER_KILL)
    }

    pub(super) fn update_match_state_authoritative(&self, delta_time: f32) {
        let mut match_info_guard = self.match_info.write();
        let player_count = self.participant_count();
        let connected_client_count = self
            .data_channels_map
            .len()
            .saturating_add(connected_quic_peer_count());
        let effective_participant_count = player_count;
        let fortress_attack_defend_mode = self.fortress_attack_defend_map();
        let dynamic_mode_transitions =
            env_bool_value("MGS_DYNAMIC_MODE_TRANSITIONS") && !fortress_attack_defend_mode;

        match match_info_guard.match_state {
            fb::MatchStateType::Waiting => {
                if effective_participant_count >= MIN_PLAYERS_TO_START {
                    match_info_guard.match_state = fb::MatchStateType::Active;
                    match_info_guard.time_remaining = self.match_duration_secs;
                    match_info_guard.team_scores.clear();
                    match_info_guard.ctf_overtime_round = 0;
                    match_info_guard.map_event_count = 0;
                    match_info_guard.map_event_elapsed_secs = 0.0;
                    match_info_guard.map_event_interval_secs = self.next_map_event_interval_secs(0);
                    match_info_guard.hot_zone_active = false;
                    match_info_guard.hot_zone_event_count = 0;
                    match_info_guard.hot_zone_elapsed_secs = HOT_ZONE_ROTATE_INTERVAL_SECS;
                    match_info_guard.hot_zone_center = Vec2::new(0.0, 0.0);
                    match_info_guard.hot_zone_radius = HOT_ZONE_RADIUS;
                    if fortress_attack_defend_mode {
                        match_info_guard.game_mode = fb::GameModeType::CaptureTheFlag;
                    } else if dynamic_mode_transitions {
                        match_info_guard.game_mode = fb::GameModeType::FreeForAll;
                    }
                    info!(
                        "Match starting! Mode: {:?}, players={}, connected_clients={}, effective_participants={}",
                        match_info_guard.game_mode,
                        player_count,
                        connected_client_count,
                        effective_participant_count
                    );
                    if match_info_guard.game_mode == fb::GameModeType::CaptureTheFlag {
                        self.initialize_ctf_flags(&mut match_info_guard);
                        if fortress_attack_defend_mode {
                            self.broadcast_fortress_phase_event(
                                "round_start",
                                Self::fortress_attacking_team(),
                                Self::fortress_defending_team(),
                                match_info_guard.time_remaining,
                                None,
                            );
                        }
                    }
                    self.player_manager.for_each_player_mut(|_id, p_state| {
                        if p_state.is_spectator {
                            return;
                        }
                        p_state.score = 0;
                        p_state.kills = 0;
                        p_state.deaths = 0;
                        p_state.reset_match_stats();
                        p_state.is_carrying_flag_team_id = 0;
                        p_state.mark_field_changed(FIELD_SCORE_STATS | FIELD_FLAG);
                    });
                    self.kill_feed.write().clear();
                }
            }
            fb::MatchStateType::Active => {
                let mut map_event_to_trigger: Option<(u64, f32)> = None;
                let mut hot_zone_to_trigger: Option<(u64, Vec2)> = None;
                let previous_time_remaining = match_info_guard.time_remaining;
                match_info_guard.time_remaining -= delta_time;
                if dynamic_mode_transitions {
                    // Scale legacy FullMatch thresholds (120s elapsed, 70s remaining)
                    // relative to current match duration so short formats still
                    // transition through all dynamic phases.
                    let thresholds = dynamic_mode_thresholds(self.match_duration_secs);

                    let elapsed =
                        (self.match_duration_secs - match_info_guard.time_remaining).max(0.0);
                    let previous_elapsed =
                        (self.match_duration_secs - previous_time_remaining).max(0.0);
                    if match_info_guard.game_mode == fb::GameModeType::FreeForAll
                        && previous_elapsed < thresholds.tdm_countdown_elapsed
                        && elapsed >= thresholds.tdm_countdown_elapsed
                    {
                        self.broadcast_dynamic_mode_event(
                            "countdown",
                            fb::GameModeType::FreeForAll,
                            fb::GameModeType::TeamDeathmatch,
                            Some(15),
                            match_info_guard.time_remaining,
                        );
                    }
                    if elapsed >= thresholds.tdm_transition_elapsed
                        && match_info_guard.time_remaining > thresholds.ctf_transition_remaining
                        && match_info_guard.game_mode == fb::GameModeType::FreeForAll
                    {
                        let previous_mode = match_info_guard.game_mode;
                        match_info_guard.game_mode = fb::GameModeType::TeamDeathmatch;
                        info!(
                            "Dynamic mode transition: {} -> {}",
                            Self::game_mode_label(previous_mode),
                            Self::game_mode_label(fb::GameModeType::TeamDeathmatch)
                        );
                        self.broadcast_dynamic_mode_event(
                            "transition",
                            previous_mode,
                            fb::GameModeType::TeamDeathmatch,
                            None,
                            match_info_guard.time_remaining,
                        );
                    } else if previous_time_remaining > thresholds.ctf_transition_remaining
                        && match_info_guard.game_mode != fb::GameModeType::CaptureTheFlag
                    {
                        if previous_time_remaining > thresholds.ctf_countdown_20_remaining
                            && match_info_guard.time_remaining
                                <= thresholds.ctf_countdown_20_remaining
                        {
                            self.broadcast_dynamic_mode_event(
                                "countdown",
                                match_info_guard.game_mode,
                                fb::GameModeType::CaptureTheFlag,
                                Some(20),
                                match_info_guard.time_remaining,
                            );
                        }
                        if previous_time_remaining > thresholds.ctf_countdown_10_remaining
                            && match_info_guard.time_remaining
                                <= thresholds.ctf_countdown_10_remaining
                        {
                            self.broadcast_dynamic_mode_event(
                                "countdown",
                                match_info_guard.game_mode,
                                fb::GameModeType::CaptureTheFlag,
                                Some(10),
                                match_info_guard.time_remaining,
                            );
                        }
                        if previous_time_remaining > thresholds.ctf_countdown_5_remaining
                            && match_info_guard.time_remaining
                                <= thresholds.ctf_countdown_5_remaining
                        {
                            self.broadcast_dynamic_mode_event(
                                "countdown",
                                match_info_guard.game_mode,
                                fb::GameModeType::CaptureTheFlag,
                                Some(5),
                                match_info_guard.time_remaining,
                            );
                        }
                    }
                    if match_info_guard.time_remaining <= thresholds.ctf_transition_remaining
                        && match_info_guard.game_mode != fb::GameModeType::CaptureTheFlag
                    {
                        let previous_mode = match_info_guard.game_mode;
                        match_info_guard.game_mode = fb::GameModeType::CaptureTheFlag;
                        self.initialize_ctf_flags(&mut match_info_guard);
                        info!(
                            "Dynamic mode transition: {} -> {}",
                            Self::game_mode_label(previous_mode),
                            Self::game_mode_label(fb::GameModeType::CaptureTheFlag)
                        );
                        self.broadcast_dynamic_mode_event(
                            "transition",
                            previous_mode,
                            fb::GameModeType::CaptureTheFlag,
                            None,
                            match_info_guard.time_remaining,
                        );
                    }
                }
                let safe_delta = delta_time.max(0.0);
                if safe_delta > 0.0 {
                    match_info_guard.map_event_elapsed_secs += safe_delta;
                    let current_interval = match_info_guard
                        .map_event_interval_secs
                        .clamp(MAP_EVENT_INTERVAL_MIN_SECS, MAP_EVENT_INTERVAL_MAX_SECS);
                    if match_info_guard.map_event_elapsed_secs >= current_interval {
                        let event_index = match_info_guard.map_event_count as u64;
                        match_info_guard.map_event_count =
                            match_info_guard.map_event_count.saturating_add(1);
                        match_info_guard.map_event_elapsed_secs = 0.0;
                        let next_interval =
                            self.next_map_event_interval_secs(event_index.saturating_add(1));
                        match_info_guard.map_event_interval_secs = next_interval;
                        map_event_to_trigger = Some((event_index, next_interval));
                    }

                    match_info_guard.hot_zone_elapsed_secs += safe_delta;
                    if !match_info_guard.hot_zone_active
                        || match_info_guard.hot_zone_elapsed_secs >= HOT_ZONE_ROTATE_INTERVAL_SECS
                    {
                        let hot_zone_index = match_info_guard.hot_zone_event_count as u64;
                        let hot_zone_center = self.next_hot_zone_center(hot_zone_index);
                        match_info_guard.hot_zone_event_count =
                            match_info_guard.hot_zone_event_count.saturating_add(1);
                        match_info_guard.hot_zone_elapsed_secs = 0.0;
                        match_info_guard.hot_zone_active = true;
                        match_info_guard.hot_zone_center = hot_zone_center;
                        match_info_guard.hot_zone_radius = HOT_ZONE_RADIUS;
                        hot_zone_to_trigger = Some((hot_zone_index, hot_zone_center));
                    }
                }
                if match_info_guard.game_mode == fb::GameModeType::TeamDeathmatch {
                    let team1_score = match_info_guard.team_scores.get(&1).cloned().unwrap_or(0);
                    let team2_score = match_info_guard.team_scores.get(&2).cloned().unwrap_or(0);
                    if team1_score >= TDM_KILL_LIMIT || team2_score >= TDM_KILL_LIMIT {
                        match_info_guard.match_state = fb::MatchStateType::Ended;
                        info!(
                            "TDM kill limit reached (limit={}, team1={}, team2={}).",
                            TDM_KILL_LIMIT, team1_score, team2_score
                        );
                        drop(match_info_guard);
                        self.capture_match_end_summary("tdm_kill_limit");
                        return;
                    }
                }
                if match_info_guard.time_remaining <= 0.0 {
                    if match_info_guard.game_mode == fb::GameModeType::CaptureTheFlag {
                        if fortress_attack_defend_mode {
                            let attacking_team = Self::fortress_attacking_team();
                            let defending_team = Self::fortress_defending_team();
                            match_info_guard.team_scores.insert(attacking_team, 0);
                            match_info_guard.team_scores.insert(defending_team, 1);
                            match_info_guard.match_state = fb::MatchStateType::Ended;
                            self.broadcast_fortress_phase_event(
                                "round_end",
                                attacking_team,
                                defending_team,
                                0.0,
                                Some("defenders_hold"),
                            );
                            info!(
                                "Fortress Attack/Defend ended: defenders (team {}) held.",
                                defending_team
                            );
                            drop(match_info_guard);
                            self.capture_match_end_summary("fortress_defenders_hold");
                            return;
                        }
                        let team1_score =
                            match_info_guard.team_scores.get(&1).cloned().unwrap_or(0);
                        let team2_score =
                            match_info_guard.team_scores.get(&2).cloned().unwrap_or(0);
                        if team1_score == team2_score && match_info_guard.ctf_overtime_round == 0 {
                            match_info_guard.ctf_overtime_round = 1;
                            match_info_guard.time_remaining = CTF_OVERTIME_DURATION_SECS;
                            info!(
                                "CTF overtime triggered ({}-{} tie). Extending by {:.1}s.",
                                team1_score, team2_score, CTF_OVERTIME_DURATION_SECS
                            );
                            self.broadcast_ctf_overtime_event(
                                match_info_guard.ctf_overtime_round,
                                CTF_OVERTIME_DURATION_SECS,
                            );
                            return;
                        }
                    }
                    match_info_guard.match_state = fb::MatchStateType::Ended;
                    info!("Match ended! (Time up)");
                    if match_info_guard.game_mode == fb::GameModeType::TeamDeathmatch
                        || match_info_guard.game_mode == fb::GameModeType::CaptureTheFlag
                    {
                        let team1_score =
                            match_info_guard.team_scores.get(&1).cloned().unwrap_or(0);
                        let team2_score =
                            match_info_guard.team_scores.get(&2).cloned().unwrap_or(0);

                        if team1_score > team2_score {
                            info!(
                                "Team 1 wins with {} points vs Team 2's {} points!",
                                team1_score, team2_score
                            );
                        } else if team2_score > team1_score {
                            info!(
                                "Team 2 wins with {} points vs Team 1's {} points!",
                                team2_score, team1_score
                            );
                        } else if team1_score == team2_score && team1_score > 0 {
                            info!(
                                "Match ended in a draw! Both teams scored {} points.",
                                team1_score
                            );
                        } else {
                            info!("Match ended with no winner (0-0).");
                        }
                    }
                    drop(match_info_guard);
                    self.capture_match_end_summary("time_expired");
                    return;
                }
                if map_event_to_trigger.is_some() || hot_zone_to_trigger.is_some() {
                    let map_event = map_event_to_trigger;
                    let hot_zone_event = hot_zone_to_trigger;
                    drop(match_info_guard);
                    if let Some((event_index, next_interval_secs)) = map_event {
                        self.trigger_center_supply_drop_map_event(event_index, next_interval_secs);
                    }
                    if let Some((event_index, center)) = hot_zone_event {
                        self.trigger_hot_zone_map_event(event_index, center);
                    }
                }
            }
            fb::MatchStateType::Ended => {
                match_info_guard.time_remaining -= delta_time;
                if match_info_guard.time_remaining <= -10.0 {
                    match_info_guard.match_state = fb::MatchStateType::Waiting;
                    self.reset_match_state(&mut match_info_guard);
                    info!("Match reset to Waiting.");
                }
            }
            _ => {}
        }
    }

    pub(super) fn process_ctf_logic_authoritative(&self, delta_time: f32) {
        let mut match_info_write_guard = self.match_info.write();
        if match_info_write_guard.game_mode == fb::GameModeType::CaptureTheFlag
            && match_info_write_guard.match_state == fb::MatchStateType::Active
        {
            let fortress_attack_defend_mode = self.fortress_attack_defend_map();
            let fortress_attacking_team = Self::fortress_attacking_team();
            let fortress_defending_team = Self::fortress_defending_team();
            for flag_state in match_info_write_guard.flag_states.values_mut() {
                if flag_state.status == fb::FlagStatus::Dropped && flag_state.respawn_timer > 0.0 {
                    flag_state.respawn_timer -= delta_time;
                    if flag_state.respawn_timer <= 0.0 {
                        flag_state.respawn_timer = 0.0;
                        flag_state.status = fb::FlagStatus::AtBase;
                        flag_state.position = Self::get_flag_base_position(flag_state.team_id);
                        flag_state.carrier_id = None;
                        self.global_game_events.push(
                            GameEvent::FlagReturned {
                                player_id: Arc::from("server".to_string()),
                                flag_team_id: flag_state.team_id,
                                position: flag_state.position,
                            },
                            EventPriority::High,
                        );
                        info!("Flag of team {} auto-returned to base.", flag_state.team_id);
                    }
                }
            }

            #[derive(Clone, Copy)]
            struct CtfPlayerSnapshot {
                x: f32,
                y: f32,
                team_id: u8,
                alive: bool,
                is_spectator: bool,
                is_carrying_flag_team_id: u8,
            }

            let mut player_snapshots: Vec<(PlayerID, CtfPlayerSnapshot)> = Vec::new();
            self.player_manager.for_each_player(|id, state| {
                player_snapshots.push((
                    id.clone(),
                    CtfPlayerSnapshot {
                        x: state.x,
                        y: state.y,
                        team_id: state.team_id,
                        alive: state.alive,
                        is_spectator: state.is_spectator,
                        is_carrying_flag_team_id: state.is_carrying_flag_team_id,
                    },
                ));
            });

            for (player_id_arc, player_state_snapshot) in player_snapshots.iter() {
                if !player_state_snapshot.alive || player_state_snapshot.is_spectator {
                    continue;
                }

                if player_state_snapshot.is_carrying_flag_team_id == 0 {
                    for flag_state in match_info_write_guard.flag_states.values_mut() {
                        let can_interact = match flag_state.status {
                            fb::FlagStatus::AtBase => true,
                            fb::FlagStatus::Dropped => {
                                if flag_state.team_id == player_state_snapshot.team_id {
                                    true
                                } else {
                                    flag_state.respawn_timer <= 0.0
                                }
                            }
                            _ => false,
                        };

                        if can_interact {
                            let dx = player_state_snapshot.x - flag_state.position.x;
                            let dy = player_state_snapshot.y - flag_state.position.y;
                            if (dx * dx + dy * dy)
                                < (PICKUP_COLLECTION_RADIUS * PICKUP_COLLECTION_RADIUS)
                            {
                                if flag_state.team_id != player_state_snapshot.team_id {
                                    if fortress_attack_defend_mode
                                        && (player_state_snapshot.team_id
                                            != fortress_attacking_team
                                            || flag_state.team_id != fortress_defending_team)
                                    {
                                        continue;
                                    }
                                    flag_state.status = fb::FlagStatus::Carried;
                                    flag_state.carrier_id = Some(player_id_arc.clone());
                                    if let Some(mut p_state_mut_entry) =
                                        self.player_manager.get_player_state_mut(player_id_arc)
                                    {
                                        let p_state_mut = &mut *p_state_mut_entry;
                                        p_state_mut.is_carrying_flag_team_id = flag_state.team_id;
                                        p_state_mut.mark_field_changed(FIELD_FLAG);
                                    }
                                    self.global_game_events.push(
                                        GameEvent::FlagGrabbed {
                                            player_id: player_id_arc.clone(),
                                            flag_team_id: flag_state.team_id,
                                            position: flag_state.position,
                                        },
                                        EventPriority::High,
                                    );
                                    info!(
                                        "Player {} grabbed flag of team {}",
                                        self.ctf_player_label(player_id_arc),
                                        flag_state.team_id
                                    );
                                    break;
                                } else if flag_state.status == fb::FlagStatus::Dropped
                                    && flag_state.team_id == player_state_snapshot.team_id
                                {
                                    flag_state.status = fb::FlagStatus::AtBase;
                                    flag_state.position =
                                        Self::get_flag_base_position(flag_state.team_id);
                                    flag_state.carrier_id = None;
                                    flag_state.respawn_timer = 0.0;
                                    if let Some(mut p_state_mut_entry) =
                                        self.player_manager.get_player_state_mut(player_id_arc)
                                    {
                                        let p_state_mut = &mut *p_state_mut_entry;
                                        p_state_mut.flag_returns =
                                            p_state_mut.flag_returns.saturating_add(1);
                                        p_state_mut.score += POINTS_FLAG_RETURN;
                                        p_state_mut.mark_field_changed(FIELD_SCORE_STATS);
                                    }
                                    self.global_game_events.push(
                                        GameEvent::FlagReturned {
                                            player_id: player_id_arc.clone(),
                                            flag_team_id: flag_state.team_id,
                                            position: flag_state.position,
                                        },
                                        EventPriority::High,
                                    );
                                    info!(
                                        "Player {} returned own team {}'s flag.",
                                        self.ctf_player_label(player_id_arc),
                                        flag_state.team_id
                                    );
                                    break;
                                }
                            }
                        }
                    }
                }

                if player_state_snapshot.is_carrying_flag_team_id != 0
                    && player_state_snapshot.is_carrying_flag_team_id
                        != player_state_snapshot.team_id
                {
                    let own_player_team_id = player_state_snapshot.team_id;

                    let own_flag_at_base = match_info_write_guard
                        .flag_states
                        .get(&own_player_team_id)
                        .is_some_and(|ofs| ofs.status == fb::FlagStatus::AtBase);

                    if own_flag_at_base {
                        let own_flag_base_pos = Self::get_flag_base_position(own_player_team_id);
                        let dx = player_state_snapshot.x - own_flag_base_pos.x;
                        let dy = player_state_snapshot.y - own_flag_base_pos.y;

                        if (dx * dx + dy * dy)
                            < (PICKUP_COLLECTION_RADIUS * PICKUP_COLLECTION_RADIUS)
                        {
                            let captured_flag_team_id =
                                player_state_snapshot.is_carrying_flag_team_id;

                            if let Some(captured_flag) = match_info_write_guard
                                .flag_states
                                .get_mut(&captured_flag_team_id)
                            {
                                captured_flag.status = fb::FlagStatus::AtBase;
                                captured_flag.position =
                                    Self::get_flag_base_position(captured_flag_team_id);
                                captured_flag.carrier_id = None;
                            }

                            if let Some(mut p_state_mut_entry) =
                                self.player_manager.get_player_state_mut(player_id_arc)
                            {
                                let p_state_mut = &mut *p_state_mut_entry;
                                p_state_mut.is_carrying_flag_team_id = 0;
                                p_state_mut.mark_field_changed(FIELD_FLAG);
                                p_state_mut.score += POINTS_FLAG_CAPTURE;
                                p_state_mut.flag_captures =
                                    p_state_mut.flag_captures.saturating_add(1);
                                p_state_mut.mark_field_changed(FIELD_SCORE_STATS);
                            }

                            let current_score = if fortress_attack_defend_mode {
                                match_info_write_guard
                                    .team_scores
                                    .insert(fortress_attacking_team, 1);
                                match_info_write_guard
                                    .team_scores
                                    .insert(fortress_defending_team, 0);
                                1
                            } else {
                                let team_score_mut_ref = match_info_write_guard
                                    .team_scores
                                    .entry(own_player_team_id)
                                    .or_insert(0);
                                *team_score_mut_ref += 1;
                                *team_score_mut_ref
                            };

                            self.global_game_events.push(
                                GameEvent::FlagCaptured {
                                    capturer_id: player_id_arc.clone(),
                                    captured_flag_team_id,
                                    capturing_team_id: own_player_team_id,
                                    position: own_flag_base_pos,
                                },
                                EventPriority::High,
                            );
                            info!(
                                "Player {} captured team {}'s flag for team {}! (Score: {})",
                                self.ctf_player_label(player_id_arc),
                                captured_flag_team_id,
                                own_player_team_id,
                                current_score
                            );

                            if fortress_attack_defend_mode {
                                let fortress_capture_valid = own_player_team_id
                                    == fortress_attacking_team
                                    && captured_flag_team_id == fortress_defending_team;
                                if fortress_capture_valid {
                                    match_info_write_guard.match_state = fb::MatchStateType::Ended;
                                    self.broadcast_fortress_phase_event(
                                        "round_end",
                                        fortress_attacking_team,
                                        fortress_defending_team,
                                        0.0,
                                        Some("attackers_captured"),
                                    );
                                    info!(
                                        "Fortress Attack/Defend ended: attackers (team {}) captured objective.",
                                        fortress_attacking_team
                                    );
                                    drop(match_info_write_guard);
                                    self.capture_match_end_summary("fortress_attackers_captured");
                                    return;
                                }
                            }

                            let overtime_capture = match_info_write_guard.ctf_overtime_round > 0;
                            if !fortress_attack_defend_mode
                                && (overtime_capture || current_score >= CTF_CAPTURE_LIMIT)
                            {
                                match_info_write_guard.match_state = fb::MatchStateType::Ended;
                                if overtime_capture {
                                    info!(
                                        "Team {} wins CTF in overtime with the decisive capture.",
                                        own_player_team_id
                                    );
                                } else {
                                    info!(
                                        "Team {} wins by capturing {} flags!",
                                        own_player_team_id, current_score
                                    );
                                }
                                drop(match_info_write_guard);
                                self.capture_match_end_summary(if overtime_capture {
                                    "ctf_overtime_capture"
                                } else {
                                    "ctf_score_limit"
                                });
                                return;
                            }
                        }
                    }
                }
            }
        }
    }

    fn initialize_ctf_flags(&self, match_info: &mut ServerMatchInfo) {
        match_info.flag_states.clear();
        let team1_flag_pos = Self::get_flag_base_position(1);
        match_info.flag_states.insert(
            1,
            ServerFlagState {
                team_id: 1,
                status: fb::FlagStatus::AtBase,
                position: team1_flag_pos,
                carrier_id: None,
                respawn_timer: 0.0,
            },
        );
        let team2_flag_pos = Self::get_flag_base_position(2);
        match_info.flag_states.insert(
            2,
            ServerFlagState {
                team_id: 2,
                status: fb::FlagStatus::AtBase,
                position: team2_flag_pos,
                carrier_id: None,
                respawn_timer: 0.0,
            },
        );
        info!(
            "CTF Flags initialized. T1 at {:?}, T2 at {:?}",
            team1_flag_pos, team2_flag_pos
        );
    }

    pub fn get_flag_base_position(team_id: u8) -> Vec2 {
        if team_id == 1 {
            Vec2::new(WORLD_MIN_X + 100.0, 0.0)
        } else if team_id == 2 {
            Vec2::new(WORLD_MAX_X - 100.0, 0.0)
        } else {
            Vec2::new(0.0, 0.0)
        }
    }

    fn reset_match_state(&self, match_info: &mut ServerMatchInfo) {
        match_info.time_remaining = self.match_duration_secs;
        match_info.ctf_overtime_round = 0;
        match_info.map_event_count = 0;
        match_info.map_event_elapsed_secs = 0.0;
        match_info.map_event_interval_secs = self.next_map_event_interval_secs(0);
        match_info.hot_zone_active = false;
        match_info.hot_zone_event_count = 0;
        match_info.hot_zone_elapsed_secs = 0.0;
        match_info.hot_zone_center = Vec2::new(0.0, 0.0);
        match_info.hot_zone_radius = HOT_ZONE_RADIUS;
        match_info.flag_states.clear();
        if match_info.match_state == fb::MatchStateType::Waiting
            && match_info.game_mode == fb::GameModeType::CaptureTheFlag
        {
            self.initialize_ctf_flags(match_info);
        }
        self.player_manager.for_each_player_mut(|_id, pstate| {
            // Reset individual player stats but keep their contribution to team score
            pstate.score = 0;
            pstate.kills = 0;
            pstate.deaths = 0;
            pstate.reset_match_stats();
            pstate.is_carrying_flag_team_id = 0;
            pstate.mark_field_changed(FIELD_SCORE_STATS | FIELD_FLAG);
        });
        self.kill_feed.write().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn dynamic_mode_thresholds_match_legacy_full_match_values() {
        let thresholds = dynamic_mode_thresholds(FULL_MATCH_DURATION_SECS);
        assert!((thresholds.tdm_transition_elapsed - 120.0).abs() < 0.001);
        assert!((thresholds.tdm_countdown_elapsed - 105.0).abs() < 0.001);
        assert!((thresholds.ctf_transition_remaining - 70.0).abs() < 0.001);
        assert!((thresholds.ctf_countdown_20_remaining - 90.0).abs() < 0.001);
        assert!((thresholds.ctf_countdown_10_remaining - 80.0).abs() < 0.001);
        assert!((thresholds.ctf_countdown_5_remaining - 75.0).abs() < 0.001);
    }

    #[test]
    fn dynamic_mode_thresholds_scale_for_mobile_blitz() {
        let thresholds = dynamic_mode_thresholds(MOBILE_BLITZ_DURATION_SECS);
        assert!((thresholds.tdm_transition_elapsed - 72.0).abs() < 0.001);
        assert!((thresholds.tdm_countdown_elapsed - 63.0).abs() < 0.001);
        assert!((thresholds.ctf_transition_remaining - 42.0).abs() < 0.001);
        assert!((thresholds.ctf_countdown_20_remaining - 54.0).abs() < 0.001);
        assert!((thresholds.ctf_countdown_10_remaining - 48.0).abs() < 0.001);
        assert!((thresholds.ctf_countdown_5_remaining - 45.0).abs() < 0.001);
    }

    #[test]
    fn map_event_interval_stays_within_bounds() {
        for seed in [0_u64, 1, 2, 17, 98_765, u64::MAX - 1] {
            let interval = map_event_interval_from_seed(seed);
            assert!(interval >= MAP_EVENT_INTERVAL_MIN_SECS);
            assert!(interval <= MAP_EVENT_INTERVAL_MAX_SECS);
        }
    }

    #[test]
    fn map_event_interval_varies_across_seeds() {
        let mut distinct_intervals = BTreeSet::new();
        for seed in 0_u64..24 {
            let interval = map_event_interval_from_seed(seed);
            distinct_intervals.insert((interval * 100.0).round() as i32);
        }
        assert!(distinct_intervals.len() > 1);
    }

    #[test]
    fn hot_zone_center_stays_within_world_bounds() {
        for seed in [0_u64, 1, 2, 17, 98_765, u64::MAX - 1] {
            let center = hot_zone_center_from_seed(seed);
            assert!(center.x >= WORLD_MIN_X);
            assert!(center.x <= WORLD_MAX_X);
            assert!(center.y >= WORLD_MIN_Y);
            assert!(center.y <= WORLD_MAX_Y);
        }
    }

    #[test]
    fn hot_zone_center_varies_across_seeds() {
        let mut distinct = BTreeSet::new();
        for seed in 0_u64..24 {
            let center = hot_zone_center_from_seed(seed);
            distinct.insert((
                (center.x * 10.0).round() as i32,
                (center.y * 10.0).round() as i32,
            ));
        }
        assert!(distinct.len() > 1);
    }
}
