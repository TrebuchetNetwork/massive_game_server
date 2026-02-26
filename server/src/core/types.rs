// massive_game_server/server/src/core/types.rs
use dashmap::DashMap;
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant; // Removed unused Duration

pub type PlayerID = Arc<String>;
pub type EntityId = u64;
static ENTITY_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn generate_entity_id() -> EntityId {
    loop {
        let prev = ENTITY_ID_COUNTER.fetch_add(1, AtomicOrdering::SeqCst);
        // Guard against u64 wrapping back to 0 (practically impossible at 60 Hz,
        // but defensively correct).  ID 0 is reserved as a sentinel / "no entity".
        if prev != 0 {
            return prev;
        }
        // fetch_add already advanced the counter past 0; just retry to skip it.
    }
}

// --- Server-Side Enums ---
#[derive(Debug, Clone, Copy, PartialEq)]
#[derive(Default)]
pub enum ServerWeaponType {
    #[default]
    Pistol,
    Shotgun,
    Rifle,
    Sniper,
    Melee,
}


// --- PlayerInputData ---
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerInputData {
    pub timestamp: u64,
    pub sequence: u32,
    pub move_forward: bool,
    pub move_backward: bool,
    pub move_left: bool,
    pub move_right: bool,
    pub shooting: bool,
    pub reload: bool,
    pub rotation: f32,
    pub melee_attack: bool,
    pub change_weapon_slot: u8,
    pub use_ability_slot: u8,
    pub ping_x: f32,
    pub ping_y: f32,
}

// --- Basic Geometric Types ---
#[derive(Clone, Debug, Copy, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub fn new(x: f32, y: f32) -> Self {
        Vec2 { x, y }
    }
    pub fn zero() -> Self {
        Vec2 { x: 0.0, y: 0.0 }
    }
}

#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum ZoneType {
    SlowZone,
    DamageZone,
    BoostPad,
}

#[derive(Clone, Debug)]
pub struct Zone {
    pub id: EntityId,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub zone_type: ZoneType,
    pub direction: f32,
}

impl Zone {
    #[inline]
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }
}

#[derive(Clone, Debug, Copy)]
pub struct PartitionBounds {
    pub min_x: f32,
    pub max_x: f32,
    pub min_y: f32,
    pub max_y: f32,
}

// --- PlayerState Delta Tracking Flags ---
pub const FIELD_POSITION_ROTATION: u16 = 1 << 0;
pub const FIELD_HEALTH_ALIVE: u16 = 1 << 1;
pub const FIELD_WEAPON_AMMO: u16 = 1 << 2;
pub const FIELD_SCORE_STATS: u16 = 1 << 3;
pub const FIELD_POWERUPS: u16 = 1 << 4;
pub const FIELD_SHIELD: u16 = 1 << 5;
pub const FIELD_FLAG: u16 = 1 << 6;

// --- Game Entities (Basic Definitions) ---
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerState {
    pub id: PlayerID,
    pub username: String,
    pub is_spectator: bool,
    pub x: f32,
    pub y: f32,
    pub velocity_x: f32,
    pub velocity_y: f32,
    pub rotation: f32,
    pub health: i32,
    pub max_health: i32,
    pub alive: bool,
    pub last_processed_input_sequence: u32,
    pub input_queue: VecDeque<PlayerInputData>,
    pub score: i32,
    pub kills: i32,
    pub deaths: i32,
    pub team_id: u8,
    pub last_update_timestamp: Option<Instant>,

    pub weapon: ServerWeaponType,
    pub ammo: i32,
    pub primary_weapon: ServerWeaponType,
    pub primary_ammo: i32,
    pub secondary_weapon: ServerWeaponType,
    pub secondary_ammo: i32,
    pub weapon_swap_progress: f32,
    pub pending_weapon_swap: Option<ServerWeaponType>,
    pub respawn_timer: Option<f32>,
    pub reload_progress: Option<f32>,
    pub last_shot_time: Option<Instant>,
    pub ability_1_cooldown_remaining: f32,
    pub ability_2_cooldown_remaining: f32,
    pub dash_remaining: f32,
    pub dodge_roll_remaining: f32,
    pub invulnerable_remaining: f32,
    pub ping_cooldown_remaining: f32,
    pub zone_boost_cooldown_remaining: f32,

    pub speed_boost_remaining: f32,
    pub damage_boost_remaining: f32,
    pub shield_current: i32,
    pub shield_max: i32,
    pub is_carrying_flag_team_id: u8,

    pub damage_dealt: i32,
    pub damage_taken: i32,
    pub flag_captures: i32,
    pub flag_returns: i32,
    pub kills_per_weapon: [i32; 5],

    pub last_valid_position: (f32, f32),
    pub violation_count: u32,

    pub changed_fields: u16,

    // Killstreak tracking
    pub current_streak: u32,
    pub streak_damage_boost_remaining: f32,
    pub streak_speed_boost_remaining: f32,

    // Assist tracking: (attacker_id, damage_dealt, timestamp)
    pub recent_damage_sources: Vec<(PlayerID, i32, Instant)>,

    // Sequence validation: tracks the last accepted input sequence to reject
    // replayed or suspiciously-jumped inputs at queue time.
    pub last_queued_input_sequence: u32,
}

impl PlayerState {
    pub fn new(id_val: String, username_val: String, initial_x: f32, initial_y: f32) -> Self {
        let arc_id = Arc::new(id_val);
        let primary_weapon = ServerWeaponType::Rifle;
        let secondary_weapon = ServerWeaponType::Pistol;
        let default_weapon = primary_weapon;
        let default_ammo = Self::get_max_ammo_for_weapon(default_weapon);

        PlayerState {
            id: arc_id,
            username: username_val,
            is_spectator: false,
            x: initial_x,
            y: initial_y,
            velocity_x: 0.0,
            velocity_y: 0.0,
            rotation: 0.0,
            health: 100,
            max_health: 100,
            alive: true,
            last_processed_input_sequence: 0,
            input_queue: VecDeque::with_capacity(
                crate::core::constants::MAX_INPUT_QUEUE_SIZE_PER_PLAYER,
            ),
            score: 0,
            kills: 0,
            deaths: 0,
            team_id: 0,
            last_update_timestamp: Some(Instant::now()),
            weapon: default_weapon,
            ammo: default_ammo,
            primary_weapon,
            primary_ammo: default_ammo,
            secondary_weapon,
            secondary_ammo: Self::get_max_ammo_for_weapon(secondary_weapon),
            weapon_swap_progress: 0.0,
            pending_weapon_swap: None,
            respawn_timer: None,
            reload_progress: None,
            last_shot_time: None,
            ability_1_cooldown_remaining: 0.0,
            ability_2_cooldown_remaining: 0.0,
            dash_remaining: 0.0,
            dodge_roll_remaining: 0.0,
            invulnerable_remaining: 0.0,
            ping_cooldown_remaining: 0.0,
            zone_boost_cooldown_remaining: 0.0,
            speed_boost_remaining: 0.0,
            damage_boost_remaining: 0.0,
            shield_current: 0,
            shield_max: 0,
            is_carrying_flag_team_id: 0,
            damage_dealt: 0,
            damage_taken: 0,
            flag_captures: 0,
            flag_returns: 0,
            kills_per_weapon: [0; 5],
            last_valid_position: (initial_x, initial_y),
            violation_count: 0,
            changed_fields: 0xFFFF,
            current_streak: 0,
            streak_damage_boost_remaining: 0.0,
            streak_speed_boost_remaining: 0.0,
            recent_damage_sources: Vec::new(),
            last_queued_input_sequence: 0,
        }
    }

    /// Queue a player input after validating its sequence number.
    /// Returns `true` if the input was accepted, `false` if rejected.
    ///
    /// Rejection reasons:
    /// - `sequence <= last_queued_input_sequence` (replay / duplicate)
    /// - `sequence > last_queued_input_sequence + MAX_INPUT_SEQUENCE_GAP` (suspicious jump)
    pub fn queue_input(&mut self, input: PlayerInputData) -> bool {
        let seq = input.sequence;
        // Reject replayed or duplicate inputs.
        if seq > 0 && seq <= self.last_queued_input_sequence {
            return false;
        }
        // Reject suspiciously large sequence jumps.
        if self.last_queued_input_sequence > 0
            && seq > self.last_queued_input_sequence + crate::core::constants::MAX_INPUT_SEQUENCE_GAP
        {
            return false;
        }

        if self.input_queue.len() >= crate::core::constants::MAX_INPUT_QUEUE_SIZE_PER_PLAYER {
            self.input_queue.pop_front();
        }
        self.last_queued_input_sequence = seq;
        self.input_queue.push_back(input);
        true
    }

    pub fn mark_field_changed(&mut self, field_flag: u16) {
        self.changed_fields |= field_flag;
    }

    pub fn clear_changed_fields(&mut self) {
        self.changed_fields = 0;
    }

    pub fn get_max_ammo_for_weapon(weapon_type: ServerWeaponType) -> i32 {
        use crate::core::constants::*;
        match weapon_type {
            ServerWeaponType::Pistol => PISTOL_MAX_AMMO,
            ServerWeaponType::Shotgun => SHOTGUN_MAX_AMMO,
            ServerWeaponType::Rifle => RIFLE_MAX_AMMO,
            ServerWeaponType::Sniper => SNIPER_MAX_AMMO,
            ServerWeaponType::Melee => MELEE_MAX_AMMO,
        }
    }

    pub fn get_weapon_fire_rate_seconds(weapon_type: ServerWeaponType) -> f32 {
        use crate::core::constants::*;
        match weapon_type {
            ServerWeaponType::Pistol => PISTOL_FIRE_RATE_SECS,
            ServerWeaponType::Shotgun => SHOTGUN_FIRE_RATE_SECS,
            ServerWeaponType::Rifle => RIFLE_FIRE_RATE_SECS,
            ServerWeaponType::Sniper => SNIPER_FIRE_RATE_SECS,
            ServerWeaponType::Melee => MELEE_FIRE_RATE_SECS,
        }
    }

    pub fn get_weapon_reload_time_seconds(weapon_type: ServerWeaponType) -> f32 {
        use crate::core::constants::*;
        match weapon_type {
            ServerWeaponType::Pistol => PISTOL_RELOAD_SECS,
            ServerWeaponType::Shotgun => SHOTGUN_RELOAD_SECS,
            ServerWeaponType::Rifle => RIFLE_RELOAD_SECS,
            ServerWeaponType::Sniper => SNIPER_RELOAD_SECS,
            ServerWeaponType::Melee => MELEE_RELOAD_SECS,
        }
    }

    pub fn get_weapon_damage(weapon_type: ServerWeaponType, damage_boost_active: bool) -> i32 {
        use crate::core::constants::*;
        let base_damage = match weapon_type {
            ServerWeaponType::Pistol => PISTOL_DAMAGE,
            ServerWeaponType::Shotgun => SHOTGUN_DAMAGE,
            ServerWeaponType::Rifle => RIFLE_DAMAGE,
            ServerWeaponType::Sniper => SNIPER_DAMAGE,
            ServerWeaponType::Melee => MELEE_DAMAGE,
        };
        let multiplier = if damage_boost_active { DAMAGE_BOOST_MULTIPLIER } else { 1.0 };
        (base_damage as f32 * multiplier) as i32
    }

    /// Total damage multiplier including streak bonus and pickup boost
    pub fn effective_damage_multiplier(&self) -> f32 {
        use crate::core::constants::*;
        let mut mult = 1.0;
        if self.damage_boost_remaining > 0.0 {
            mult *= DAMAGE_BOOST_MULTIPLIER;
        }
        if self.streak_damage_boost_remaining > 0.0 {
            mult *= KILLSTREAK_DAMAGE_BOOST_MULTIPLIER;
        }
        mult
    }

    /// Record an incoming damage event for assist tracking
    pub fn record_incoming_damage(&mut self, attacker_id: &PlayerID, damage: i32, now: Instant) {
        use crate::core::constants::ASSIST_WINDOW_SECS;
        // Prune stale entries
        self.recent_damage_sources.retain(|(_, _, t)| now.duration_since(*t).as_secs_f32() < ASSIST_WINDOW_SECS);
        // Update existing or push new
        if let Some(entry) = self.recent_damage_sources.iter_mut().find(|(id, _, _)| id == attacker_id) {
            entry.1 += damage;
            entry.2 = now;
        } else {
            self.recent_damage_sources.push((attacker_id.clone(), damage, now));
        }
    }

    /// Get assist candidates (everyone who damaged this player in the window, excluding the killer)
    pub fn get_assist_ids(&self, killer_id: &PlayerID, now: Instant) -> Vec<PlayerID> {
        use crate::core::constants::ASSIST_WINDOW_SECS;
        self.recent_damage_sources.iter()
            .filter(|(id, _, t)| id != killer_id && now.duration_since(*t).as_secs_f32() < ASSIST_WINDOW_SECS)
            .map(|(id, _, _)| id.clone())
            .collect()
    }

    pub fn can_shoot(&self, current_time: Instant) -> bool {
        if !self.alive || self.reload_progress.is_some() || self.weapon_swap_progress > 0.0 {
            return false;
        }
        if self.weapon != ServerWeaponType::Melee && self.ammo <= 0 {
            return false;
        }
        if let Some(last_shot) = self.last_shot_time {
            let cooldown = Self::get_weapon_fire_rate_seconds(self.weapon);
            if current_time.duration_since(last_shot).as_secs_f32()
                < cooldown.max(crate::core::constants::MIN_SHOT_INTERVAL_SECONDS)
            {
                return false;
            }
        }
        true
    }

    pub fn start_reload(&mut self, _current_time: Instant) {
        if self.reload_progress.is_some()
            || !self.alive
            || self.ammo == Self::get_max_ammo_for_weapon(self.weapon)
            || self.weapon_swap_progress > 0.0
        {
            return;
        }
        let reload_duration = Self::get_weapon_reload_time_seconds(self.weapon);
        if reload_duration > 0.0 {
            self.reload_progress = Some(0.0);
            self.mark_field_changed(FIELD_WEAPON_AMMO);
        }
    }

    pub fn update_reload_progress(&mut self, delta_time: f32) {
        if let Some(progress) = &mut self.reload_progress {
            let reload_duration = Self::get_weapon_reload_time_seconds(self.weapon);
            if reload_duration > 0.0 {
                *progress += delta_time / reload_duration;
                if *progress >= 1.0 {
                    self.ammo = Self::get_max_ammo_for_weapon(self.weapon);
                    self.sync_active_weapon_to_loadout_slot();
                    self.reload_progress = None;
                    self.mark_field_changed(FIELD_WEAPON_AMMO);
                } else {
                    self.mark_field_changed(FIELD_WEAPON_AMMO);
                }
            } else {
                self.reload_progress = None;
            }
        }
    }

    pub fn apply_damage(&mut self, damage: i32) -> bool {
        if !self.alive || self.invulnerable_remaining > 0.0 {
            return false;
        }
        let mut remaining_damage = damage;

        if self.shield_current > 0 {
            let shield_damage = remaining_damage.min(self.shield_current);
            self.shield_current -= shield_damage;
            remaining_damage -= shield_damage;
            self.mark_field_changed(FIELD_SHIELD);
        }

        if remaining_damage > 0 {
            let old_health = self.health;
            self.health = (self.health - remaining_damage).max(0);
            self.damage_taken = self.damage_taken.saturating_add(remaining_damage);
            if old_health != self.health {
                self.mark_field_changed(FIELD_HEALTH_ALIVE);
            }
        }

        if self.health == 0 {
            self.die();
            return true;
        }
        false
    }

    fn die(&mut self) {
        self.alive = false;
        self.deaths += 1;
        self.respawn_timer = Some(crate::core::constants::DEFAULT_RESPAWN_DURATION_SECS);
        self.velocity_x = 0.0;
        self.velocity_y = 0.0;
        self.weapon_swap_progress = 0.0;
        self.pending_weapon_swap = None;
        self.dash_remaining = 0.0;
        self.dodge_roll_remaining = 0.0;
        self.invulnerable_remaining = 0.0;
        self.ping_cooldown_remaining = 0.0;
        self.zone_boost_cooldown_remaining = 0.0;
        // Reset streak on death
        self.current_streak = 0;
        self.streak_damage_boost_remaining = 0.0;
        self.streak_speed_boost_remaining = 0.0;
        // Clear assist tracking
        self.recent_damage_sources.clear();
        self.mark_field_changed(FIELD_HEALTH_ALIVE | FIELD_SCORE_STATS | FIELD_POSITION_ROTATION);
    }

    pub fn respawn(&mut self, new_x: f32, new_y: f32) {
        self.alive = true;
        self.health = self.max_health;
        self.respawn_timer = None;
        self.x = new_x;
        self.y = new_y;
        self.last_valid_position = (new_x, new_y);
        self.velocity_x = 0.0;
        self.velocity_y = 0.0;
        self.primary_ammo = Self::get_max_ammo_for_weapon(self.primary_weapon);
        self.secondary_ammo = Self::get_max_ammo_for_weapon(self.secondary_weapon);
        self.weapon = self.primary_weapon;
        self.ammo = self.primary_ammo;
        self.weapon_swap_progress = 0.0;
        self.pending_weapon_swap = None;
        self.reload_progress = None;
        self.ability_1_cooldown_remaining = 0.0;
        self.ability_2_cooldown_remaining = 0.0;
        self.dash_remaining = 0.0;
        self.dodge_roll_remaining = 0.0;
        self.invulnerable_remaining = 0.0;
        self.ping_cooldown_remaining = 0.0;
        self.zone_boost_cooldown_remaining = 0.0;
        self.shield_current = 0;
        self.is_carrying_flag_team_id = 0;
        self.current_streak = 0;
        self.streak_damage_boost_remaining = 0.0;
        self.streak_speed_boost_remaining = 0.0;
        self.recent_damage_sources.clear();
        self.mark_field_changed(
            FIELD_HEALTH_ALIVE
                | FIELD_POSITION_ROTATION
                | FIELD_WEAPON_AMMO
                | FIELD_SHIELD
                | FIELD_FLAG,
        );
    }

    pub fn update_timers(&mut self, delta_time: f32) {
        let mut changed_health_alive = false;
        let mut changed_powerups = false;
        let mut changed_weapon_state = false;

        if !self.alive {
            if let Some(timer) = &mut self.respawn_timer {
                *timer -= delta_time;
                if *timer <= 0.0 {
                    self.respawn_timer = Some(0.0);
                }
                changed_health_alive = true;
            }
        }

        if self.speed_boost_remaining > 0.0 {
            self.speed_boost_remaining = (self.speed_boost_remaining - delta_time).max(0.0);
            changed_powerups = true;
        }
        if self.damage_boost_remaining > 0.0 {
            self.damage_boost_remaining = (self.damage_boost_remaining - delta_time).max(0.0);
            changed_powerups = true;
        }
        if self.ability_1_cooldown_remaining > 0.0 {
            self.ability_1_cooldown_remaining =
                (self.ability_1_cooldown_remaining - delta_time).max(0.0);
            changed_powerups = true;
        }
        if self.ability_2_cooldown_remaining > 0.0 {
            self.ability_2_cooldown_remaining =
                (self.ability_2_cooldown_remaining - delta_time).max(0.0);
            changed_powerups = true;
        }
        if self.dash_remaining > 0.0 {
            self.dash_remaining = (self.dash_remaining - delta_time).max(0.0);
            changed_powerups = true;
        }
        if self.dodge_roll_remaining > 0.0 {
            self.dodge_roll_remaining = (self.dodge_roll_remaining - delta_time).max(0.0);
            changed_powerups = true;
        }
        if self.invulnerable_remaining > 0.0 {
            self.invulnerable_remaining = (self.invulnerable_remaining - delta_time).max(0.0);
            changed_powerups = true;
        }
        if self.ping_cooldown_remaining > 0.0 {
            self.ping_cooldown_remaining = (self.ping_cooldown_remaining - delta_time).max(0.0);
            changed_powerups = true;
        }
        if self.zone_boost_cooldown_remaining > 0.0 {
            self.zone_boost_cooldown_remaining =
                (self.zone_boost_cooldown_remaining - delta_time).max(0.0);
            changed_powerups = true;
        }
        if self.streak_damage_boost_remaining > 0.0 {
            self.streak_damage_boost_remaining = (self.streak_damage_boost_remaining - delta_time).max(0.0);
            changed_powerups = true;
        }
        if self.streak_speed_boost_remaining > 0.0 {
            self.streak_speed_boost_remaining = (self.streak_speed_boost_remaining - delta_time).max(0.0);
            changed_powerups = true;
        }
        if self.weapon_swap_progress > 0.0 {
            self.weapon_swap_progress = (self.weapon_swap_progress - delta_time).max(0.0);
            changed_weapon_state = true;
            if self.weapon_swap_progress <= 0.0 {
                if let Some(next_weapon) = self.pending_weapon_swap.take() {
                    self.commit_pending_weapon_swap(next_weapon);
                }
            }
        }

        if changed_health_alive {
            self.mark_field_changed(FIELD_HEALTH_ALIVE);
        }
        if changed_powerups {
            self.mark_field_changed(FIELD_POWERUPS);
        }
        if changed_weapon_state {
            self.mark_field_changed(FIELD_WEAPON_AMMO);
        }

        let old_reload_progress = self.reload_progress;
        self.update_reload_progress(delta_time);
        if self.reload_progress != old_reload_progress {
            self.mark_field_changed(FIELD_WEAPON_AMMO);
        }
    }

    pub fn start_weapon_swap_to_slot(&mut self, slot: u8) -> bool {
        if self.weapon_swap_progress > 0.0 {
            return false;
        }

        let target_weapon = match slot {
            1 => self.primary_weapon,
            2 => self.secondary_weapon,
            _ => return false,
        };

        if self.weapon == target_weapon {
            return false;
        }

        self.sync_active_weapon_to_loadout_slot();
        self.pending_weapon_swap = Some(target_weapon);
        self.weapon_swap_progress = crate::core::constants::WEAPON_SWAP_DURATION_SECS;
        self.reload_progress = None;
        self.mark_field_changed(FIELD_WEAPON_AMMO);
        true
    }

    pub fn replace_active_slot_weapon(&mut self, weapon: ServerWeaponType) {
        if self.active_loadout_slot() == 2 {
            self.secondary_weapon = weapon;
            self.secondary_ammo = Self::get_max_ammo_for_weapon(weapon);
        } else {
            self.primary_weapon = weapon;
            self.primary_ammo = Self::get_max_ammo_for_weapon(weapon);
        }
        self.weapon = weapon;
        self.ammo = Self::get_max_ammo_for_weapon(weapon);
        self.pending_weapon_swap = None;
        self.weapon_swap_progress = 0.0;
        self.reload_progress = None;
        self.mark_field_changed(FIELD_WEAPON_AMMO);
    }

    fn commit_pending_weapon_swap(&mut self, weapon: ServerWeaponType) {
        self.weapon = weapon;
        self.ammo = if self.secondary_weapon == weapon {
            self.secondary_ammo
        } else {
            self.primary_ammo
        };
        self.mark_field_changed(FIELD_WEAPON_AMMO);
    }

    fn active_loadout_slot(&self) -> u8 {
        if self.weapon == self.secondary_weapon && self.weapon != self.primary_weapon {
            2
        } else {
            1
        }
    }

    pub fn sync_active_weapon_to_loadout_slot(&mut self) {
        match self.active_loadout_slot() {
            2 => {
                self.secondary_weapon = self.weapon;
                self.secondary_ammo = self.ammo.max(0);
            }
            _ => {
                self.primary_weapon = self.weapon;
                self.primary_ammo = self.ammo.max(0);
            }
        }
    }

    #[inline]
    pub fn record_damage_dealt(&mut self, damage: i32) {
        self.damage_dealt = self.damage_dealt.saturating_add(damage.max(0));
    }

    #[inline]
    pub fn record_damage_taken(&mut self, damage: i32) {
        self.damage_taken = self.damage_taken.saturating_add(damage.max(0));
    }

    #[inline]
    pub fn record_kill_with_weapon(&mut self, weapon: ServerWeaponType) {
        let idx = Self::weapon_index(weapon);
        self.kills_per_weapon[idx] = self.kills_per_weapon[idx].saturating_add(1);
    }

    pub fn reset_match_stats(&mut self) {
        self.damage_dealt = 0;
        self.damage_taken = 0;
        self.flag_captures = 0;
        self.flag_returns = 0;
        self.kills_per_weapon = [0; 5];
        self.current_streak = 0;
        self.streak_damage_boost_remaining = 0.0;
        self.streak_speed_boost_remaining = 0.0;
        self.recent_damage_sources.clear();
    }

    #[inline]
    fn weapon_index(weapon: ServerWeaponType) -> usize {
        match weapon {
            ServerWeaponType::Pistol => 0,
            ServerWeaponType::Shotgun => 1,
            ServerWeaponType::Rifle => 2,
            ServerWeaponType::Sniper => 3,
            ServerWeaponType::Melee => 4,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Projectile {
    pub id: EntityId,
    pub owner_id: PlayerID,
    pub weapon_type: ServerWeaponType,
    pub x: f32,
    pub y: f32,
    pub velocity_x: f32,
    pub velocity_y: f32,
    pub damage: i32,
    pub creation_time: Instant,
    pub max_lifetime_secs: f32,
}

impl Projectile {
    pub fn new(
        owner_id: PlayerID,
        weapon_type: ServerWeaponType,
        start_x: f32,
        start_y: f32,
        direction_x: f32,
        direction_y: f32,
        damage_multiplier: f32,
    ) -> Self {
        let id = generate_entity_id();

        // Get speed and lifetime for weapon
        let (speed, lifetime) = match weapon_type {
            ServerWeaponType::Pistol => (450.0, 2.0),
            ServerWeaponType::Shotgun => (400.0, 1.2),
            ServerWeaponType::Rifle => (550.0, 2.5),
            ServerWeaponType::Sniper => (700.0, 4.0),
            ServerWeaponType::Melee => (0.0, 0.0),
        };

        // Use PlayerState::get_weapon_damage for consistent damage calculation
        let has_damage_boost = damage_multiplier > 1.0;
        let damage = PlayerState::get_weapon_damage(weapon_type, has_damage_boost);

        Projectile {
            id,
            owner_id,
            weapon_type,
            x: start_x,
            y: start_y,
            velocity_x: direction_x * speed,
            velocity_y: direction_y * speed,
            damage,
            creation_time: Instant::now(),
            max_lifetime_secs: lifetime,
        }
    }
    pub fn should_remove(&self) -> bool {
        self.creation_time.elapsed().as_secs_f32() > self.max_lifetime_secs
    }
}

#[derive(Clone, Debug)]
pub enum GameEvent {
    PlayerJoined {
        player_id: PlayerID,
    },
    PlayerLeft {
        player_id: PlayerID,
    },
    PlayerDamaged {
        target_id: PlayerID,
        attacker_id: Option<PlayerID>,
        damage: i32,
        weapon: ServerWeaponType,
        position: Vec2,
    },
    PlayerKilled {
        victim_id: PlayerID,
        killer_id: PlayerID,
        weapon: ServerWeaponType,
        position: Vec2,
    },
    ProjectileHitWall {
        projectile_id: EntityId,
        wall_id: EntityId,
        position: Vec2,
    },
    PowerupCollected {
        player_id: PlayerID,
        pickup_id: EntityId,
        pickup_type: CorePickupType,
        position: Vec2,
    },
    WeaponFired {
        player_id: PlayerID,
        weapon: ServerWeaponType,
        position: Vec2,
    },
    WallDestroyed {
        wall_id: EntityId,
        position: Vec2,
    },
    WallImpact {
        wall_id: EntityId,
        position: Vec2,
        damage: i32,
    },
    MeleeHit {
        attacker_id: PlayerID,
        target_id: Option<PlayerID>,
        position: Vec2,
    },
    Footstep {
        player_id: PlayerID,
        position: Vec2,
        surface_type: u8,
    },
    FlagGrabbed {
        player_id: PlayerID,
        flag_team_id: u8,
        position: Vec2,
    },
    FlagDropped {
        player_id: PlayerID,
        flag_team_id: u8,
        position: Vec2,
    },
    FlagReturned {
        player_id: PlayerID,
        flag_team_id: u8,
        position: Vec2,
    },
    FlagCaptured {
        capturer_id: PlayerID,
        captured_flag_team_id: u8,
        capturing_team_id: u8,
        position: Vec2,
    },
    TeamPing {
        player_id: PlayerID,
        team_id: u8,
        position: Vec2,
    },
    Killstreak {
        player_id: PlayerID,
        streak: u32,
        position: Vec2,
    },
    AssistKill {
        assister_id: PlayerID,
        victim_id: PlayerID,
        points: i32,
    },
}

#[derive(Clone, Debug)]
pub struct Wall {
    pub id: EntityId,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub is_destructible: bool,
    pub current_health: i32,
    pub max_health: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CorePickupType {
    Health,
    Ammo,
    WeaponCrate(ServerWeaponType),
    SpeedBoost,
    DamageBoost,
    Shield,
}

#[derive(Clone, Debug)]
pub struct Pickup {
    pub id: EntityId,
    pub x: f32,
    pub y: f32,
    pub pickup_type: CorePickupType,
    pub is_active: bool,
    pub respawn_timer: Option<f32>,
}
impl Pickup {
    pub fn new(id: EntityId, x: f32, y: f32, pickup_type: CorePickupType) -> Self {
        Pickup {
            id,
            x,
            y,
            pickup_type,
            is_active: true,
            respawn_timer: None,
        }
    }
    pub fn get_respawn_duration(&self) -> f32 {
        match self.pickup_type {
            CorePickupType::Health | CorePickupType::Ammo => 10.0,
            CorePickupType::WeaponCrate(_) => 15.0,
            CorePickupType::SpeedBoost | CorePickupType::DamageBoost | CorePickupType::Shield => {
                20.0
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchState {
    WaitingForPlayers,
    InProgress,
    Ended,
}

#[derive(Clone)]
#[allow(dead_code)]
struct MatchStatus {
    state: MatchState,
    time_remaining: Duration,
    team1_score: i32,
    team2_score: i32,
    winning_team: Option<u8>,
}

#[derive(Clone, Debug)]
pub struct PlayerAoI {
    pub visible_players: HashSet<PlayerID>,
    pub visible_projectiles: HashSet<EntityId>,
    pub visible_pickups: HashSet<EntityId>,
    pub visible_walls: HashSet<EntityId>,
    pub last_update: Instant,
}

impl Default for PlayerAoI {
    fn default() -> Self {
        Self::new()
    }
}

impl PlayerAoI {
    pub fn new() -> Self {
        PlayerAoI {
            visible_players: HashSet::new(),
            visible_projectiles: HashSet::new(),
            visible_pickups: HashSet::new(),
            visible_walls: HashSet::new(),
            last_update: Instant::now(),
        }
    }
}

pub type PlayerAoIs = Arc<DashMap<String, PlayerAoI>>;

#[derive(Clone, Debug)]
pub struct DeltaState {}
#[derive(Debug, Clone)]
pub struct NetworkConnection {
    pub last_heartbeat: Instant,
}
impl NetworkConnection {
    pub fn send_zero_copy(&self, _bytes: Vec<u8>) -> Result<(), String> {
        Ok(())
    }
    pub fn poll_input(&self) -> Option<PlayerInputData> {
        None
    }
}
#[derive(Clone, Debug)]
pub struct BoundaryUpdate {
    pub player_id: PlayerID,
    pub action: BoundaryAction,
    pub position: (f32, f32),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundaryAction {
    Enter,
    Leave,
    Update,
}
#[derive(Clone, Debug)]
pub struct BoundarySnapshot {
    pub players: Vec<(PlayerID, f32, f32)>,
    pub version: u64,
    pub timestamp: Instant,
}
impl Default for BoundarySnapshot {
    fn default() -> Self {
        BoundarySnapshot {
            players: Vec::new(),
            version: 0,
            timestamp: Instant::now(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    North,
    South,
    East,
    West,
    NorthEast,
    NorthWest,
    SouthEast,
    SouthWest,
}
#[derive(Clone, Debug)]
pub enum EventPriority {
    High,
    Normal,
    Low,
}
//pub struct FlatBufferBuilder<'a> { _phantom: std::marker::PhantomData<&'a u8> }
/*impl<'a> FlatBufferBuilder<'a> {
    pub fn new() -> Self { FlatBufferBuilder { _phantom: std::marker::PhantomData } }
    pub fn with_capacity(_cap: usize) -> Self { Self::new() }
    pub fn reset(&mut self) {}
    pub fn finished_data(&self) -> &[u8] { &[] }
}*/
pub struct PerformanceMetrics;
impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl PerformanceMetrics {
    pub fn new() -> Self {
        PerformanceMetrics
    }
    pub fn get_average_frame_time(&self) -> f64 {
        0.016
    }
    pub fn get_cpu_usage(&self) -> f64 {
        50.0
    }
}
pub struct NumaAwareServer;
impl NumaAwareServer {
    pub fn new() -> Result<Self, String> {
        Ok(NumaAwareServer)
    }
}
pub type ThreadId = std::thread::ThreadId;
#[derive(Clone, Debug)]
pub struct ThreadState {
    pub last_progress: Instant,
}
impl Default for ThreadState {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreadState {
    pub fn new() -> Self {
        ThreadState {
            last_progress: Instant::now(),
        }
    }
}
pub struct PrometheusHistogram;
impl PrometheusHistogram {
    pub fn observe(&self, _val: f64) {}
}
pub struct PrometheusGauge;
impl PrometheusGauge {
    pub fn set(&self, _val: f64) {}
}
pub struct PrometheusCounter;
impl PrometheusCounter {
    pub fn inc(&self) {}
}

#[derive(Clone)]
enum RTCDataChannelBackend {
    Real(Arc<webrtc::data_channel::RTCDataChannel>),
    MockCounter(Arc<AtomicU64>),
}

#[derive(Clone)]
pub struct RTCDataChannel {
    backend: RTCDataChannelBackend,
}

impl RTCDataChannel {
    pub fn new(inner: Arc<webrtc::data_channel::RTCDataChannel>) -> Self {
        RTCDataChannel {
            backend: RTCDataChannelBackend::Real(inner),
        }
    }

    pub fn new_mock_counter(counter: Arc<AtomicU64>) -> Self {
        RTCDataChannel {
            backend: RTCDataChannelBackend::MockCounter(counter),
        }
    }

    pub fn label(&self) -> &str {
        match &self.backend {
            RTCDataChannelBackend::Real(inner) => inner.label(),
            RTCDataChannelBackend::MockCounter(_) => "mock-data-channel",
        }
    }

    pub fn is_open(&self) -> bool {
        match &self.backend {
            RTCDataChannelBackend::Real(inner) => {
                inner.ready_state()
                    == webrtc::data_channel::data_channel_state::RTCDataChannelState::Open
            }
            RTCDataChannelBackend::MockCounter(_) => true,
        }
    }

    pub async fn send(&self, data: &bytes::Bytes) -> Result<(), String> {
        match &self.backend {
            RTCDataChannelBackend::Real(inner) => inner
                .send(data)
                .await
                .map(|_bytes_sent| ())
                .map_err(|e| e.to_string()),
            RTCDataChannelBackend::MockCounter(counter) => {
                let _ = data;
                counter.fetch_add(1, AtomicOrdering::Relaxed);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn make_player(name: &str) -> PlayerState {
        PlayerState::new(name.to_string(), name.to_string(), 100.0, 100.0)
    }

    // ── apply_damage tests ─────────────────────────────────────────

    #[test]
    fn apply_damage_reduces_health() {
        let mut p = make_player("p1");
        let died = p.apply_damage(30);
        assert!(!died);
        assert_eq!(p.health, 70);
        assert!(p.alive);
    }

    #[test]
    fn apply_damage_kills_at_zero() {
        let mut p = make_player("p1");
        let died = p.apply_damage(100);
        assert!(died);
        assert_eq!(p.health, 0);
        assert!(!p.alive);
        assert_eq!(p.deaths, 1);
    }

    #[test]
    fn apply_damage_overkill_clamps_to_zero() {
        let mut p = make_player("p1");
        let died = p.apply_damage(500);
        assert!(died);
        assert_eq!(p.health, 0);
        assert!(!p.alive);
    }

    #[test]
    fn apply_damage_shield_absorbs_fully() {
        let mut p = make_player("p1");
        p.shield_current = 50;
        p.shield_max = 50;
        let died = p.apply_damage(30);
        assert!(!died);
        assert_eq!(p.shield_current, 20);
        assert_eq!(p.health, 100); // no health lost
    }

    #[test]
    fn apply_damage_shield_absorbs_partially() {
        let mut p = make_player("p1");
        p.shield_current = 10;
        p.shield_max = 50;
        let died = p.apply_damage(30);
        assert!(!died);
        assert_eq!(p.shield_current, 0);
        assert_eq!(p.health, 80); // 30 - 10 shield = 20 to health
    }

    #[test]
    fn apply_damage_shield_overflow_kills() {
        let mut p = make_player("p1");
        p.shield_current = 10;
        p.health = 20;
        let died = p.apply_damage(50);
        assert!(died);
        assert_eq!(p.shield_current, 0);
        assert_eq!(p.health, 0);
    }

    #[test]
    fn apply_damage_invulnerable_blocks_all() {
        let mut p = make_player("p1");
        p.invulnerable_remaining = 2.0;
        let died = p.apply_damage(100);
        assert!(!died);
        assert_eq!(p.health, 100);
        assert!(p.alive);
    }

    #[test]
    fn apply_damage_dead_player_no_effect() {
        let mut p = make_player("p1");
        p.alive = false;
        let died = p.apply_damage(50);
        assert!(!died);
        assert_eq!(p.health, 100); // unchanged
    }

    #[test]
    fn apply_damage_tracks_damage_taken() {
        let mut p = make_player("p1");
        p.apply_damage(25);
        p.apply_damage(15);
        assert_eq!(p.damage_taken, 40);
    }

    #[test]
    fn apply_damage_marks_changed_fields_on_death() {
        let mut p = make_player("p1");
        p.clear_changed_fields();
        p.apply_damage(100);
        // die() sets FIELD_HEALTH_ALIVE | FIELD_SCORE_STATS | FIELD_POSITION_ROTATION
        assert_ne!(p.changed_fields & FIELD_HEALTH_ALIVE, 0);
        assert_ne!(p.changed_fields & FIELD_SCORE_STATS, 0);
    }

    // ── can_shoot tests ───────────────────────────────────────────

    #[test]
    fn can_shoot_alive_with_ammo() {
        let p = make_player("p1");
        let now = Instant::now();
        assert!(p.can_shoot(now));
    }

    #[test]
    fn can_shoot_dead_player_cannot() {
        let mut p = make_player("p1");
        p.alive = false;
        assert!(!p.can_shoot(Instant::now()));
    }

    #[test]
    fn can_shoot_reloading_blocks() {
        let mut p = make_player("p1");
        p.reload_progress = Some(0.5);
        assert!(!p.can_shoot(Instant::now()));
    }

    #[test]
    fn can_shoot_weapon_swap_blocks() {
        let mut p = make_player("p1");
        p.weapon_swap_progress = 0.2;
        assert!(!p.can_shoot(Instant::now()));
    }

    #[test]
    fn can_shoot_zero_ammo_blocks_ranged() {
        let mut p = make_player("p1");
        p.weapon = ServerWeaponType::Pistol;
        p.ammo = 0;
        assert!(!p.can_shoot(Instant::now()));
    }

    #[test]
    fn can_shoot_melee_ignores_ammo() {
        let mut p = make_player("p1");
        p.weapon = ServerWeaponType::Melee;
        p.ammo = 0;
        assert!(p.can_shoot(Instant::now()));
    }

    #[test]
    fn can_shoot_cooldown_not_elapsed() {
        let mut p = make_player("p1");
        p.weapon = ServerWeaponType::Sniper;
        p.ammo = 5;
        let shot_time = Instant::now();
        p.last_shot_time = Some(shot_time);
        // Sniper fire rate is 1.2s; check immediately after should fail
        assert!(!p.can_shoot(shot_time));
    }

    #[test]
    fn can_shoot_cooldown_elapsed() {
        let mut p = make_player("p1");
        p.weapon = ServerWeaponType::Pistol;
        p.ammo = 5;
        let shot_time = Instant::now();
        p.last_shot_time = Some(shot_time);
        // Pistol fire rate is 0.45s; well past cooldown
        let future_time = shot_time + Duration::from_millis(500);
        assert!(p.can_shoot(future_time));
    }

    // ── assist tracking tests ─────────────────────────────────────

    #[test]
    fn record_incoming_damage_creates_entry() {
        let mut p = make_player("victim");
        let attacker_id: PlayerID = Arc::new("attacker1".to_string());
        let now = Instant::now();
        p.record_incoming_damage(&attacker_id, 25, now);
        assert_eq!(p.recent_damage_sources.len(), 1);
        assert_eq!(p.recent_damage_sources[0].1, 25);
    }

    #[test]
    fn record_incoming_damage_accumulates_same_attacker() {
        let mut p = make_player("victim");
        let attacker_id: PlayerID = Arc::new("attacker1".to_string());
        let now = Instant::now();
        p.record_incoming_damage(&attacker_id, 20, now);
        p.record_incoming_damage(&attacker_id, 15, now);
        assert_eq!(p.recent_damage_sources.len(), 1);
        assert_eq!(p.recent_damage_sources[0].1, 35); // accumulated
    }

    #[test]
    fn record_incoming_damage_separate_attackers() {
        let mut p = make_player("victim");
        let a1: PlayerID = Arc::new("a1".to_string());
        let a2: PlayerID = Arc::new("a2".to_string());
        let now = Instant::now();
        p.record_incoming_damage(&a1, 20, now);
        p.record_incoming_damage(&a2, 30, now);
        assert_eq!(p.recent_damage_sources.len(), 2);
    }

    #[test]
    fn get_assist_ids_excludes_killer() {
        let mut p = make_player("victim");
        let killer: PlayerID = Arc::new("killer".to_string());
        let assister: PlayerID = Arc::new("assister".to_string());
        let now = Instant::now();
        p.record_incoming_damage(&killer, 60, now);
        p.record_incoming_damage(&assister, 30, now);
        let assists = p.get_assist_ids(&killer, now);
        assert_eq!(assists.len(), 1);
        assert_eq!(*assists[0], "assister");
    }

    #[test]
    fn get_assist_ids_stale_entries_pruned() {
        let mut p = make_player("victim");
        let old_attacker: PlayerID = Arc::new("old".to_string());
        let recent_attacker: PlayerID = Arc::new("recent".to_string());
        let killer: PlayerID = Arc::new("killer".to_string());
        let old_time = Instant::now();
        p.record_incoming_damage(&old_attacker, 20, old_time);
        // Simulate time passing beyond ASSIST_WINDOW_SECS (5s)
        let now = old_time + Duration::from_secs(6);
        p.record_incoming_damage(&recent_attacker, 30, now);
        let assists = p.get_assist_ids(&killer, now);
        // old_attacker should be expired
        assert_eq!(assists.len(), 1);
        assert_eq!(*assists[0], "recent");
    }

    // ── effective_damage_multiplier / streak tests ────────────────

    #[test]
    fn effective_damage_multiplier_no_boosts() {
        let p = make_player("p1");
        assert!((p.effective_damage_multiplier() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn effective_damage_multiplier_damage_boost_only() {
        let mut p = make_player("p1");
        p.damage_boost_remaining = 5.0;
        let mult = p.effective_damage_multiplier();
        assert!((mult - crate::core::constants::DAMAGE_BOOST_MULTIPLIER).abs() < f32::EPSILON);
    }

    #[test]
    fn effective_damage_multiplier_streak_boost_only() {
        let mut p = make_player("p1");
        p.streak_damage_boost_remaining = 10.0;
        let mult = p.effective_damage_multiplier();
        assert!((mult - crate::core::constants::KILLSTREAK_DAMAGE_BOOST_MULTIPLIER).abs() < f32::EPSILON);
    }

    #[test]
    fn effective_damage_multiplier_both_boosts_stack() {
        let mut p = make_player("p1");
        p.damage_boost_remaining = 5.0;
        p.streak_damage_boost_remaining = 10.0;
        let mult = p.effective_damage_multiplier();
        let expected = crate::core::constants::DAMAGE_BOOST_MULTIPLIER
            * crate::core::constants::KILLSTREAK_DAMAGE_BOOST_MULTIPLIER;
        assert!((mult - expected).abs() < 0.001);
    }

    // ── die / respawn / reset tests ──────────────────────────────

    #[test]
    fn die_resets_streak_and_state() {
        let mut p = make_player("p1");
        p.current_streak = 5;
        p.streak_damage_boost_remaining = 10.0;
        p.apply_damage(100); // triggers die()
        assert_eq!(p.current_streak, 0);
        assert_eq!(p.streak_damage_boost_remaining, 0.0);
        assert!(!p.alive);
        assert!(p.respawn_timer.is_some());
    }

    #[test]
    fn respawn_restores_full_state() {
        let mut p = make_player("p1");
        p.apply_damage(100); // kill
        p.respawn(200.0, 300.0);
        assert!(p.alive);
        assert_eq!(p.health, 100);
        assert_eq!(p.x, 200.0);
        assert_eq!(p.y, 300.0);
        assert!(p.respawn_timer.is_none());
        assert_eq!(p.current_streak, 0);
        assert_eq!(p.shield_current, 0);
    }
}
