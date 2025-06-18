// massive_game_server/server/src/core/types.rs
use std::collections::{VecDeque, HashSet}; 
use std::sync::Arc;
use std::time::{Instant}; // Removed unused Duration
use uuid::Uuid;
use dashmap::DashMap; 
use std::time::Duration;


pub type PlayerID = Arc<String>;
pub type EntityId = u64;

// --- Server-Side Enums ---
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ServerWeaponType {
    Pistol,
    Shotgun,
    Rifle,
    Sniper,
    Melee,
}

impl Default for ServerWeaponType {
    fn default() -> Self {
        ServerWeaponType::Pistol
    }
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
}

// --- Basic Geometric Types ---
#[derive(Clone, Debug, Copy, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub fn new(x: f32, y: f32) -> Self { Vec2 { x, y } }
    pub fn zero() -> Self { Vec2 { x: 0.0, y: 0.0 } }
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
pub const FIELD_HEALTH_ALIVE: u16    = 1 << 1;
pub const FIELD_WEAPON_AMMO: u16     = 1 << 2;
pub const FIELD_SCORE_STATS: u16     = 1 << 3;
pub const FIELD_POWERUPS: u16        = 1 << 4;
pub const FIELD_SHIELD: u16          = 1 << 5;
pub const FIELD_FLAG: u16            = 1 << 6;

// --- Game Entities (Basic Definitions) ---
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerState {
    pub id: PlayerID,
    pub username: String,
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
    pub respawn_timer: Option<f32>,
    pub reload_progress: Option<f32>,
    pub last_shot_time: Option<Instant>,

    pub speed_boost_remaining: f32,
    pub damage_boost_remaining: f32,
    pub shield_current: i32,
    pub shield_max: i32,
    pub is_carrying_flag_team_id: u8,

    pub last_valid_position: (f32, f32),
    pub violation_count: u32,

    pub changed_fields: u16,
}

impl PlayerState {
    pub fn new(id_val: String, username_val: String, initial_x: f32, initial_y: f32) -> Self {
        let arc_id = Arc::new(id_val);
        let default_weapon = ServerWeaponType::default();
        let default_ammo = Self::get_max_ammo_for_weapon(default_weapon);

        PlayerState {
            id: arc_id,
            username: username_val,
            x: initial_x,
            y: initial_y,
            velocity_x: 0.0,
            velocity_y: 0.0,
            rotation: 0.0,
            health: 100,
            max_health: 100,
            alive: true,
            last_processed_input_sequence: 0,
            input_queue: VecDeque::with_capacity(crate::core::constants::MAX_INPUT_QUEUE_SIZE_PER_PLAYER),
            score: 0,
            kills: 0,
            deaths: 0,
            team_id: 0,
            last_update_timestamp: Some(Instant::now()),
            weapon: default_weapon,
            ammo: default_ammo,
            respawn_timer: None,
            reload_progress: None,
            last_shot_time: None,
            speed_boost_remaining: 0.0,
            damage_boost_remaining: 0.0,
            shield_current: 0,
            shield_max: 0,
            is_carrying_flag_team_id: 0,
            last_valid_position: (initial_x, initial_y),
            violation_count: 0,
            changed_fields: 0xFFFF, 
        }
    }

    pub fn queue_input(&mut self, input: PlayerInputData) {
        if self.input_queue.len() >= crate::core::constants::MAX_INPUT_QUEUE_SIZE_PER_PLAYER {
            self.input_queue.pop_front();
        }
        self.input_queue.push_back(input);
    }

    pub fn mark_field_changed(&mut self, field_flag: u16) {
        self.changed_fields |= field_flag;
    }

    pub fn clear_changed_fields(&mut self) {
        self.changed_fields = 0;
    }

    pub fn get_max_ammo_for_weapon(weapon_type: ServerWeaponType) -> i32 {
        match weapon_type {
            ServerWeaponType::Pistol => 7, ServerWeaponType::Shotgun => 5,
            ServerWeaponType::Rifle => 30, ServerWeaponType::Sniper => 5,
            ServerWeaponType::Melee => 0, 
        }
    }

    pub fn get_weapon_fire_rate_seconds(weapon_type: ServerWeaponType) -> f32 {
        match weapon_type {
            ServerWeaponType::Pistol => 0.6, ServerWeaponType::Shotgun => 0.8,
            ServerWeaponType::Rifle => 0.1, ServerWeaponType::Sniper => 1.2,
            ServerWeaponType::Melee => 0.5,
        }
    }

    pub fn get_weapon_reload_time_seconds(weapon_type: ServerWeaponType) -> f32 {
        match weapon_type {
            ServerWeaponType::Pistol => 1.5, ServerWeaponType::Shotgun => 2.5,
            ServerWeaponType::Rifle => 2.0, ServerWeaponType::Sniper => 3.0,
            ServerWeaponType::Melee => 0.0, 
        }
    }

    pub fn get_weapon_damage(weapon_type: ServerWeaponType, damage_boost_active: bool) -> i32 {
        let base_damage = match weapon_type {
            ServerWeaponType::Pistol => 8, ServerWeaponType::Shotgun => 7, 
            ServerWeaponType::Rifle => 10, ServerWeaponType::Sniper => 50,
            ServerWeaponType::Melee => 30,
        };
        let multiplier = if damage_boost_active { 1.5 } else { 1.0 };
        (base_damage as f32 * multiplier) as i32
    }

    pub fn can_shoot(&self, current_time: Instant) -> bool {
        if !self.alive || self.reload_progress.is_some() { return false; }
        if self.weapon != ServerWeaponType::Melee && self.ammo <= 0 { return false; }
        if let Some(last_shot) = self.last_shot_time {
            let cooldown = Self::get_weapon_fire_rate_seconds(self.weapon);
            if current_time.duration_since(last_shot).as_secs_f32() < cooldown.max(crate::core::constants::MIN_SHOT_INTERVAL_SECONDS) {
                return false;
            }
        }
        true
    }

    pub fn start_reload(&mut self, _current_time: Instant) { 
        if self.reload_progress.is_some() || !self.alive || self.ammo == Self::get_max_ammo_for_weapon(self.weapon) { return; }
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
        if !self.alive { return false; }
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
        self.velocity_x = 0.0; // Added for consistency
        self.velocity_y = 0.0; // Added for consistency
        // self.is_carrying_flag_team_id = 0; // <<<< REMOVE THIS LINE (or comment it out)
        // Mark FIELD_FLAG changed if it was carried, this will be handled by the caller now.
        self.mark_field_changed(FIELD_HEALTH_ALIVE | FIELD_SCORE_STATS | FIELD_POSITION_ROTATION); // FIELD_FLAG will be marked by caller if changed
    }

    pub fn respawn(&mut self, new_x: f32, new_y: f32) {
        self.alive = true;
        self.health = self.max_health;
        self.respawn_timer = None;
        self.x = new_x; self.y = new_y;
        self.last_valid_position = (new_x, new_y);
        self.velocity_x = 0.0; self.velocity_y = 0.0;
        self.weapon = ServerWeaponType::Pistol; // <-- ADD THIS LINE TO RESET TO PISTOL
        self.ammo = Self::get_max_ammo_for_weapon(self.weapon);
        self.reload_progress = None;
        self.shield_current = 0; 
        self.mark_field_changed(FIELD_HEALTH_ALIVE | FIELD_POSITION_ROTATION | FIELD_WEAPON_AMMO | FIELD_SHIELD);
    }

    pub fn update_timers(&mut self, delta_time: f32) {
        let mut changed_health_alive = false;
        let mut changed_powerups = false;

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

        if changed_health_alive { self.mark_field_changed(FIELD_HEALTH_ALIVE); }
        if changed_powerups { self.mark_field_changed(FIELD_POWERUPS); }

        let old_reload_progress = self.reload_progress;
        self.update_reload_progress(delta_time);
        if self.reload_progress != old_reload_progress { 
             self.mark_field_changed(FIELD_WEAPON_AMMO);
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
        let id = Uuid::new_v4().as_u128() as u64; 
        
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
    PlayerJoined { player_id: PlayerID },
    PlayerLeft { player_id: PlayerID },
    PlayerDamaged { target_id: PlayerID, attacker_id: Option<PlayerID>, damage: i32, weapon: ServerWeaponType, position: Vec2 },
    PlayerKilled { victim_id: PlayerID, killer_id: PlayerID, weapon: ServerWeaponType, position: Vec2 },
    ProjectileHitWall { projectile_id: EntityId, wall_id: EntityId, position: Vec2 }, 
    PowerupCollected { player_id: PlayerID, pickup_id: EntityId, pickup_type: CorePickupType, position: Vec2 },
    WeaponFired { player_id: PlayerID, weapon: ServerWeaponType, position: Vec2 },
    WallDestroyed { wall_id: EntityId, position: Vec2 },
    WallImpact { wall_id: EntityId, position: Vec2, damage: i32 },
    MeleeHit { attacker_id: PlayerID, target_id: Option<PlayerID>, position: Vec2 },
    Footstep { player_id: PlayerID, position: Vec2, surface_type: u8 },
    FlagGrabbed { player_id: PlayerID, flag_team_id: u8, position: Vec2 },
    FlagDropped { player_id: PlayerID, flag_team_id: u8, position: Vec2 },
    FlagReturned { player_id: PlayerID, flag_team_id: u8, position: Vec2 },
    FlagCaptured { capturer_id: PlayerID, captured_flag_team_id: u8, capturing_team_id: u8, position: Vec2 },
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
            id, x, y, pickup_type,
            is_active: true,
            respawn_timer: None,
        }
    }
    pub fn get_respawn_duration(&self) -> f32 {
        match self.pickup_type {
            CorePickupType::Health | CorePickupType::Ammo => 10.0,
            CorePickupType::WeaponCrate(_) => 15.0,
            CorePickupType::SpeedBoost | CorePickupType::DamageBoost | CorePickupType::Shield => 20.0,
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


#[derive(Clone, Debug)] pub struct DeltaState { }
#[derive(Debug, Clone)] pub struct NetworkConnection { pub last_heartbeat: Instant }
impl NetworkConnection {
    pub fn send_zero_copy(&self, _bytes: Vec<u8>) -> Result<(), String> { Ok(()) }
    pub fn poll_input(&self) -> Option<PlayerInputData> { None }
}
#[derive(Clone, Debug)] pub struct BoundaryUpdate { pub player_id: PlayerID, pub action: BoundaryAction, pub position: (f32, f32) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum BoundaryAction { Enter, Leave, Update }
#[derive(Clone, Debug)] pub struct BoundarySnapshot { pub players: Vec<(PlayerID, f32, f32)>, pub version: u64, pub timestamp: Instant }
impl Default for BoundarySnapshot { fn default() -> Self { BoundarySnapshot { players: Vec::new(), version: 0, timestamp: Instant::now() } } }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Direction { North, South, East, West, NorthEast, NorthWest, SouthEast, SouthWest }
#[derive(Clone, Debug)] pub enum EventPriority { High, Normal, Low }
//pub struct FlatBufferBuilder<'a> { _phantom: std::marker::PhantomData<&'a u8> }
/*impl<'a> FlatBufferBuilder<'a> {
    pub fn new() -> Self { FlatBufferBuilder { _phantom: std::marker::PhantomData } }
    pub fn with_capacity(_cap: usize) -> Self { Self::new() }
    pub fn reset(&mut self) {}
    pub fn finished_data(&self) -> &[u8] { &[] }
}*/
pub struct PerformanceMetrics;
impl PerformanceMetrics { pub fn new() -> Self { PerformanceMetrics } pub fn get_average_frame_time(&self) -> f64 { 0.016 } pub fn get_cpu_usage(&self) -> f64 { 50.0 } }
pub struct NumaAwareServer;
impl NumaAwareServer { pub fn new() -> Result<Self, String> { Ok(NumaAwareServer) } }
pub type ThreadId = std::thread::ThreadId;
#[derive(Clone, Debug)] pub struct ThreadState { pub last_progress: Instant }
impl ThreadState { pub fn new() -> Self { ThreadState { last_progress: Instant::now() }} }
pub struct PrometheusHistogram; impl PrometheusHistogram { pub fn observe(&self, _val: f64) {} }
pub struct PrometheusGauge; impl PrometheusGauge { pub fn set(&self, _val: f64) {} }
pub struct PrometheusCounter; impl PrometheusCounter { pub fn inc(&self) {} }

#[derive(Clone)]
pub struct RTCDataChannel { 
    inner: Arc<webrtc::data_channel::RTCDataChannel>,
}

impl RTCDataChannel {
    pub fn new(inner: Arc<webrtc::data_channel::RTCDataChannel>) -> Self {
        RTCDataChannel { inner }
    }

    pub fn label(&self) -> &str {
        self.inner.label()
    }

    pub async fn send(&self, data: &bytes::Bytes) -> Result<(), String> {
        self.inner.send(data).await
            .map(|_bytes_sent| ()) 
            .map_err(|e| e.to_string())
    }
}
