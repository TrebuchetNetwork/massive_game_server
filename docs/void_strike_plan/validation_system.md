# Automatic Game Validation System
## Comprehensive Design Document for Massive Game Server

**Version:** 1.0  
**Last Updated:** 2026-02-27  
**Project:** Trebuchet Network - Massive Game Server  
**Target:** 200v200 Player Support with Zero-Tolerance Glitch Detection

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Architecture Overview](#2-architecture-overview)
3. [Game State Integrity Checks](#3-game-state-integrity-checks)
4. [Performance Monitoring](#4-performance-monitoring)
5. [Synchronization Validation](#5-synchronization-validation)
6. [Anomaly Detection](#6-anomaly-detection)
7. [Automated Test Suite](#7-automated-test-suite)
8. [Alerting System](#8-alerting-system)
9. [Implementation Guide](#9-implementation-guide)
10. [Appendix: Threshold Reference](#10-appendix-threshold-reference)

---

## 1. Executive Summary

### 1.1 Purpose

This document defines a comprehensive automatic validation system for the Massive Game Server that ensures:

- **Zero gameplay glitches** through continuous state validation
- **Cheat detection** via position, speed, and action validation
- **Performance guarantees** with real-time monitoring
- **Synchronization integrity** between server and clients
- **Automated quality assurance** through extensive test coverage

### 1.2 Key Metrics

| Metric | Target | Critical Threshold |
|--------|--------|-------------------|
| Server Tick Rate | 60 Hz | < 55 Hz |
| Position Validation | 99.99% | < 99.9% |
| Sync Latency | < 50ms | > 100ms |
| Memory Growth | < 1MB/min | > 5MB/min |
| False Positive Rate | < 0.1% | > 1% |

### 1.3 System Components

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        GAME VALIDATION SYSTEM                                │
├─────────────────────────────────────────────────────────────────────────────┤
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐    │
│  │   Game State │  │  Performance │  │     Sync     │  │   Anomaly    │    │
│  │   Integrity  │  │   Monitor    │  │  Validation  │  │  Detection   │    │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘    │
│         │                 │                 │                 │            │
│         └─────────────────┴────────┬────────┴─────────────────┘            │
│                                    │                                        │
│                         ┌──────────▼──────────┐                            │
│                         │   Validation Hub    │                            │
│                         │  (Event Aggregator) │                            │
│                         └──────────┬──────────┘                            │
│                                    │                                        │
│         ┌──────────────────────────┼──────────────────────────┐             │
│         │                          │                          │             │
│  ┌──────▼──────┐          ┌────────▼────────┐      ┌──────────▼──────┐     │
│  │   Alerting  │          │  Test Framework │      │   Dashboard     │     │
│  │   System    │          │   (CI/CD)       │      │   & Metrics     │     │
│  └─────────────┘          └─────────────────┘      └─────────────────┘     │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Architecture Overview

### 2.1 Core Principles

1. **Fail-Fast**: Detect issues within 100ms of occurrence
2. **Non-Intrusive**: Validation overhead < 5% of CPU
3. **Deterministic**: Same inputs always produce same validation results
4. **Observable**: All validation events logged with full context
5. **Recoverable**: Graceful degradation on validation failures

### 2.2 Module Structure

```rust
// server/src/validation/mod.rs

pub mod integrity;
pub mod performance;
pub mod synchronization;
pub mod anomaly;
pub mod alerting;
pub mod metrics;

use std::sync::Arc;
use tokio::sync::RwLock;

/// Central validation coordinator
pub struct ValidationSystem {
    pub integrity: Arc<RwLock<IntegrityValidator>>,
    pub performance: Arc<RwLock<PerformanceMonitor>>,
    pub sync: Arc<RwLock<SyncValidator>>,
    pub anomaly: Arc<RwLock<AnomalyDetector>>,
    pub alerting: Arc<RwLock<AlertingSystem>>,
    pub metrics: Arc<RwLock<MetricsCollector>>,
}

impl ValidationSystem {
    pub fn new(config: ValidationConfig) -> Self {
        Self {
            integrity: Arc::new(RwLock::new(IntegrityValidator::new(&config))),
            performance: Arc::new(RwLock::new(PerformanceMonitor::new(&config))),
            sync: Arc::new(RwLock::new(SyncValidator::new(&config))),
            anomaly: Arc::new(RwLock::new(AnomalyDetector::new(&config))),
            alerting: Arc::new(RwLock::new(AlertingSystem::new(&config))),
            metrics: Arc::new(RwLock::new(MetricsCollector::new())),
        }
    }
}
```

### 2.3 Configuration

```rust
// server/src/validation/config.rs

#[derive(Clone, Debug)]
pub struct ValidationConfig {
    // Integrity thresholds
    pub max_position_delta: f32,
    pub max_speed_multiplier: f32,
    pub violation_threshold: u32,
    
    // Performance thresholds
    pub min_tick_rate: u32,
    pub max_memory_growth_mb_per_min: f32,
    pub max_latency_ms: u32,
    
    // Sync thresholds
    pub max_sync_latency_ms: u32,
    pub max_state_divergence: f32,
    
    // Anomaly detection
    pub anomaly_z_score_threshold: f32,
    pub anomaly_window_size: usize,
    
    // Alerting
    pub alert_cooldown_seconds: u64,
    pub webhook_url: Option<String>,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            max_position_delta: 3.0,
            max_speed_multiplier: 1.08,
            violation_threshold: 3,
            min_tick_rate: 55,
            max_memory_growth_mb_per_min: 1.0,
            max_latency_ms: 100,
            max_sync_latency_ms: 50,
            max_state_divergence: 0.01,
            anomaly_z_score_threshold: 3.0,
            anomaly_window_size: 100,
            alert_cooldown_seconds: 60,
            webhook_url: None,
        }
    }
}
```

---

## 3. Game State Integrity Checks

### 3.1 Position Validation

Detects speed hacks, teleportation, and impossible movement.

```rust
// server/src/validation/integrity/position.rs

use crate::core::constants::*;
use crate::core::types::{PlayerId, Position, Velocity};
use crate::entities::player::Player;
use std::collections::HashMap;

/// Position validation result
#[derive(Debug, Clone, PartialEq)]
pub enum PositionValidationResult {
    Valid,
    SpeedViolation { expected: f32, actual: f32 },
    TeleportDetected { from: Position, to: Position, delta: f32 },
    OutOfBounds { position: Position },
    AccelerationHack { delta_v: f32 },
}

/// Tracks player movement history for validation
pub struct PositionValidator {
    /// Last validated position per player
    last_positions: HashMap<PlayerId, (Position, u64)>,
    /// Velocity history for acceleration detection
    velocity_history: HashMap<PlayerId, Vec<(Velocity, u64)>>,
    /// Consecutive violation counter
    violation_counts: HashMap<PlayerId, u32>,
    /// Tolerance multiplier (configurable)
    speed_tolerance: f32,
}

impl PositionValidator {
    pub fn new() -> Self {
        Self {
            last_positions: HashMap::new(),
            velocity_history: HashMap::new(),
            violation_counts: HashMap::new(),
            speed_tolerance: speed_hack_tolerance(),
        }
    }

    /// Validates a player's new position
    pub fn validate_position(
        &mut self,
        player_id: PlayerId,
        new_position: Position,
        velocity: Velocity,
        tick: u64,
    ) -> PositionValidationResult {
        // Check world bounds first
        if !self.is_within_bounds(&new_position) {
            return PositionValidationResult::OutOfBounds { position: new_position };
        }

        let Some((last_pos, last_tick)) = self.last_positions.get(&player_id) else {
            // First position - accept and record
            self.last_positions.insert(player_id, (new_position, tick));
            return PositionValidationResult::Valid;
        };

        let delta_ticks = tick.saturating_sub(*last_tick).max(1);
        let delta_time = delta_ticks as f32 * TICK_DURATION_MS as f32 / 1000.0;
        
        // Calculate actual distance traveled
        let delta_pos = new_position.distance_to(last_pos);
        let actual_speed = delta_pos / delta_time;

        // Check for teleportation (instant large movement)
        let max_teleport_distance = PLAYER_BASE_SPEED * delta_time * self.speed_tolerance * 2.0;
        if delta_pos > max_teleport_distance && delta_ticks <= 2 {
            return PositionValidationResult::TeleportDetected {
                from: *last_pos,
                to: new_position,
                delta: delta_pos,
            };
        }

        // Check speed limit
        let max_speed = PLAYER_BASE_SPEED * self.speed_tolerance;
        if actual_speed > max_speed {
            *self.violation_counts.entry(player_id).or_insert(0) += 1;
            
            if self.violation_counts[&player_id] >= POSITION_VALIDATION_VIOLATION_THRESHOLD {
                return PositionValidationResult::SpeedViolation {
                    expected: max_speed,
                    actual: actual_speed,
                };
            }
        } else {
            // Reset violation counter on valid movement
            self.violation_counts.insert(player_id, 0);
        }

        // Check acceleration (rate of velocity change)
        if let Some(result) = self.check_acceleration(player_id, velocity, tick) {
            return result;
        }

        // Record valid position
        self.last_positions.insert(player_id, (new_position, tick));
        PositionValidationResult::Valid
    }

    /// Checks for impossible acceleration (acceleration hack)
    fn check_acceleration(
        &mut self,
        player_id: PlayerId,
        current_velocity: Velocity,
        tick: u64,
    ) -> Option<PositionValidationResult> {
        let history = self.velocity_history.entry(player_id).or_default();
        
        // Keep last 5 velocity samples
        history.push((current_velocity, tick));
        if history.len() > 5 {
            history.remove(0);
        }

        if history.len() >= 2 {
            let (prev_vel, prev_tick) = history[history.len() - 2];
            let delta_tick = tick.saturating_sub(prev_tick).max(1);
            let delta_time = delta_tick as f32 * TICK_DURATION_MS as f32 / 1000.0;
            
            let velocity_delta = current_velocity.magnitude() - prev_vel.magnitude();
            let acceleration = velocity_delta.abs() / delta_time;

            if acceleration > MAX_ACCELERATION_PER_TICK {
                return Some(PositionValidationResult::AccelerationHack { delta_v: acceleration });
            }
        }

        None
    }

    fn is_within_bounds(&self, pos: &Position) -> bool {
        pos.x >= WORLD_MIN_X - BOUNDARY_ZONE_WIDTH
            && pos.x <= WORLD_MAX_X + BOUNDARY_ZONE_WIDTH
            && pos.y >= WORLD_MIN_Y - BOUNDARY_ZONE_WIDTH
            && pos.y <= WORLD_MAX_Y + BOUNDARY_ZONE_WIDTH
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_movement() {
        let mut validator = PositionValidator::new();
        let player_id = PlayerId(1);
        let pos1 = Position { x: 0.0, y: 0.0 };
        let pos2 = Position { x: 10.0, y: 0.0 };
        let velocity = Velocity { x: 100.0, y: 0.0 };

        validator.validate_position(player_id, pos1, velocity, 1);
        let result = validator.validate_position(player_id, pos2, velocity, 2);
        
        assert!(matches!(result, PositionValidationResult::Valid));
    }

    #[test]
    fn test_speed_violation() {
        let mut validator = PositionValidator::new();
        let player_id = PlayerId(1);
        let pos1 = Position { x: 0.0, y: 0.0 };
        let pos2 = Position { x: 500.0, y: 0.0 }; // Too far for 1 tick
        let velocity = Velocity { x: 5000.0, y: 0.0 };

        validator.validate_position(player_id, pos1, velocity, 1);
        
        // Need 3 consecutive violations
        for tick in 2..=4 {
            let _ = validator.validate_position(player_id, pos2, velocity, tick);
        }
        
        let result = validator.validate_position(player_id, pos2, velocity, 5);
        assert!(matches!(result, PositionValidationResult::SpeedViolation { .. }));
    }

    #[test]
    fn test_teleport_detection() {
        let mut validator = PositionValidator::new();
        let player_id = PlayerId(1);
        let pos1 = Position { x: 0.0, y: 0.0 };
        let pos2 = Position { x: 1000.0, y: 0.0 }; // Instant teleport
        let velocity = Velocity { x: 0.0, y: 0.0 };

        validator.validate_position(player_id, pos1, velocity, 1);
        let result = validator.validate_position(player_id, pos2, velocity, 2);
        
        assert!(matches!(result, PositionValidationResult::TeleportDetected { .. }));
    }
}
```

### 3.2 Health & Ammo Consistency

```rust
// server/src/validation/integrity/resources.rs

use crate::core::types::{PlayerId, AmmoCount, Health};
use std::collections::HashMap;

/// Resource validation for health and ammo
pub struct ResourceValidator {
    /// Last known valid state per player
    player_states: HashMap<PlayerId, PlayerResourceState>,
    /// Damage history for validation
    damage_log: HashMap<PlayerId, Vec<DamageEvent>>,
    /// Ammo consumption log
    ammo_log: HashMap<PlayerId, Vec<AmmoEvent>>,
}

#[derive(Clone, Debug)]
struct PlayerResourceState {
    health: Health,
    max_health: Health,
    ammo: AmmoCount,
    max_ammo: AmmoCount,
    last_update_tick: u64,
}

#[derive(Clone, Debug)]
struct DamageEvent {
    tick: u64,
    amount: f32,
    source: DamageSource,
}

#[derive(Clone, Debug)]
struct AmmoEvent {
    tick: u64,
    delta: i32, // negative for consumption, positive for pickup
}

#[derive(Clone, Debug)]
pub enum DamageSource {
    Projectile { owner: PlayerId },
    Explosion,
    Environment,
    Fall,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ResourceValidationResult {
    Valid,
    HealthMismatch { expected: Health, actual: Health },
    AmmoMismatch { expected: AmmoCount, actual: AmmoCount },
    ImpossibleHeal { amount: f32 },
    NegativeHealth { value: Health },
    AmmoRegenTooFast { delta: i32, time: u64 },
}

impl ResourceValidator {
    pub fn new() -> Self {
        Self {
            player_states: HashMap::new(),
            damage_log: HashMap::new(),
            ammo_log: HashMap::new(),
        }
    }

    /// Validates health change
    pub fn validate_health_change(
        &mut self,
        player_id: PlayerId,
        new_health: Health,
        tick: u64,
    ) -> ResourceValidationResult {
        let Some(state) = self.player_states.get(&player_id) else {
            // First time seeing player - initialize state
            self.player_states.insert(player_id, PlayerResourceState {
                health: new_health,
                max_health: 100.0,
                ammo: 30,
                max_ammo: 100,
                last_update_tick: tick,
            });
            return ResourceValidationResult::Valid;
        };

        // Check for negative health
        if new_health < 0.0 {
            return ResourceValidationResult::NegativeHealth { value: new_health };
        }

        // Check for overheal
        if new_health > state.max_health {
            return ResourceValidationResult::ImpossibleHeal { 
                amount: new_health - state.health 
            };
        }

        // Validate damage taken matches expected
        let expected_damage = self.calculate_expected_damage(player_id, tick);
        let actual_change = new_health - state.health;

        if actual_change > 0.0 && actual_change > expected_damage * 0.1 {
            // Healing without valid source
            return ResourceValidationResult::ImpossibleHeal { amount: actual_change };
        }

        // Update state
        let mut new_state = state.clone();
        new_state.health = new_health;
        new_state.last_update_tick = tick;
        self.player_states.insert(player_id, new_state);

        ResourceValidationResult::Valid
    }

    /// Validates ammo change
    pub fn validate_ammo_change(
        &mut self,
        player_id: PlayerId,
        new_ammo: AmmoCount,
        tick: u64,
    ) -> ResourceValidationResult {
        let Some(state) = self.player_states.get(&player_id) else {
            return ResourceValidationResult::Valid;
        };

        let delta = new_ammo as i32 - state.ammo as i32;

        // Check for ammo regeneration speed
        if delta > 0 {
            let time_since_update = tick - state.last_update_tick;
            let max_regen = self.calculate_max_ammo_regen(time_since_update);
            
            if delta > max_regen {
                return ResourceValidationResult::AmmoRegenTooFast { delta, time: time_since_update };
            }
        }

        // Update state
        let mut new_state = state.clone();
        new_state.ammo = new_ammo;
        new_state.last_update_tick = tick;
        self.player_states.insert(player_id, new_state);

        // Log ammo event
        self.ammo_log
            .entry(player_id)
            .or_default()
            .push(AmmoEvent { tick, delta });

        ResourceValidationResult::Valid
    }

    fn calculate_expected_damage(&self, player_id: PlayerId, tick: u64) -> f32 {
        let Some(log) = self.damage_log.get(&player_id) else {
            return 0.0;
        };

        log.iter()
            .filter(|e| e.tick >= tick.saturating_sub(10)) // Last 10 ticks
            .map(|e| e.amount)
            .sum()
    }

    fn calculate_max_ammo_regen(&self, ticks: u64) -> i32 {
        // Ammo pickups give 10-30 ammo, max 1 pickup per 60 ticks
        let max_pickups = (ticks / 60) as i32 + 1;
        max_pickups * 30
    }

    pub fn record_damage(&mut self, player_id: PlayerId, amount: f32, source: DamageSource, tick: u64) {
        self.damage_log
            .entry(player_id)
            .or_default()
            .push(DamageEvent { tick, amount, source });

        // Trim old entries
        if let Some(log) = self.damage_log.get_mut(&player_id) {
            log.retain(|e| e.tick >= tick.saturating_sub(100));
        }
    }
}
```

### 3.3 Score Validation

```rust
// server/src/validation/integrity/score.rs

use crate::core::types::{PlayerId, Score};
use std::collections::HashMap;

/// Score validation to prevent score manipulation
pub struct ScoreValidator {
    player_scores: HashMap<PlayerId, ScoreState>,
    kill_log: HashMap<PlayerId, Vec<KillEvent>>,
}

#[derive(Clone, Debug)]
struct ScoreState {
    score: Score,
    kills: u32,
    deaths: u32,
    assists: u32,
    last_update_tick: u64,
}

#[derive(Clone, Debug)]
struct KillEvent {
    tick: u64,
    victim: PlayerId,
    weapon: WeaponType,
}

#[derive(Clone, Debug)]
pub enum WeaponType {
    Primary,
    Secondary,
    Melee,
    Explosive,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScoreValidationResult {
    Valid,
    ScoreMismatch { expected: Score, actual: Score },
    ImpossibleKill { reason: ImpossibleKillReason },
    RapidKills { count: u32, window_ticks: u64 },
    SuicideScoring,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImpossibleKillReason {
    OutOfRange,
    ThroughWall,
    NoLineOfSight,
    AlreadyDead,
    Invulnerable,
}

impl ScoreValidator {
    pub fn new() -> Self {
        Self {
            player_scores: HashMap::new(),
            kill_log: HashMap::new(),
        }
    }

    /// Validates a score update
    pub fn validate_score_update(
        &mut self,
        player_id: PlayerId,
        new_score: Score,
        kills: u32,
        deaths: u32,
        tick: u64,
    ) -> ScoreValidationResult {
        let Some(state) = self.player_scores.get(&player_id) else {
            // Initialize new player
            self.player_scores.insert(player_id, ScoreState {
                score: new_score,
                kills,
                deaths,
                assists: 0,
                last_update_tick: tick,
            });
            return ScoreValidationResult::Valid;
        };

        // Check for rapid kills (potential aimbot)
        let kill_delta = kills - state.kills;
        if kill_delta > 0 {
            let recent_kills = self.count_recent_kills(player_id, tick, 30); // 0.5 second window
            if recent_kills >= 3 {
                return ScoreValidationResult::RapidKills { 
                    count: recent_kills, 
                    window_ticks: 30 
                };
            }
        }

        // Validate score calculation
        let expected_score = self.calculate_expected_score(state, kills, deaths);
        let score_diff = (new_score - expected_score).abs();
        
        if score_diff > 1.0 {
            return ScoreValidationResult::ScoreMismatch { 
                expected: expected_score, 
                actual: new_score 
            };
        }

        // Update state
        let mut new_state = state.clone();
        new_state.score = new_score;
        new_state.kills = kills;
        new_state.deaths = deaths;
        new_state.last_update_tick = tick;
        self.player_scores.insert(player_id, new_state);

        ResourceValidationResult::Valid
    }

    fn count_recent_kills(&self, player_id: PlayerId, current_tick: u64, window: u64) -> u32 {
        let Some(log) = self.kill_log.get(&player_id) else {
            return 0;
        };

        log.iter()
            .filter(|e| e.tick >= current_tick.saturating_sub(window))
            .count() as u32
    }

    fn calculate_expected_score(&self, state: &ScoreState, kills: u32, deaths: u32) -> Score {
        let kill_points = (kills - state.kills) as f32 * 100.0;
        let death_penalty = (deaths - state.deaths) as f32 * 50.0;
        state.score + kill_points - death_penalty
    }

    pub fn record_kill(&mut self, killer: PlayerId, victim: PlayerId, weapon: WeaponType, tick: u64) {
        self.kill_log
            .entry(killer)
            .or_default()
            .push(KillEvent { tick, victim, weapon });

        // Trim old entries
        if let Some(log) = self.kill_log.get_mut(&killer) {
            log.retain(|e| e.tick >= tick.saturating_sub(600)); // Keep last 10 seconds
        }
    }
}
```

### 3.4 Round State Consistency

```rust
// server/src/validation/integrity/round.rs

use crate::core::types::{RoundId, TeamId};
use std::collections::HashMap;

/// Round state validation for game mode integrity
pub struct RoundValidator {
    current_round: Option<RoundState>,
    round_history: Vec<RoundState>,
    team_scores: HashMap<TeamId, u32>,
}

#[derive(Clone, Debug)]
struct RoundState {
    round_id: RoundId,
    start_tick: u64,
    end_tick: Option<u64>,
    winning_team: Option<TeamId>,
    state: RoundPhase,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RoundPhase {
    Warmup,
    PreRound,
    InProgress,
    PostRound,
    Intermission,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RoundValidationResult {
    Valid,
    InvalidStateTransition { from: RoundPhase, to: RoundPhase },
    PrematureEnd { elapsed_ticks: u64, minimum: u64 },
    ScoreMismatch { team: TeamId, expected: u32, actual: u32 },
    FlagStateInconsistent { flag_id: u32, expected_carrier: Option<u32> },
}

impl RoundValidator {
    pub fn new() -> Self {
        Self {
            current_round: None,
            round_history: Vec::new(),
            team_scores: HashMap::new(),
        }
    }

    /// Validates a round state transition
    pub fn validate_state_transition(
        &mut self,
        new_phase: RoundPhase,
        tick: u64,
    ) -> RoundValidationResult {
        let Some(current) = &self.current_round else {
            // First round
            self.current_round = Some(RoundState {
                round_id: RoundId(1),
                start_tick: tick,
                end_tick: None,
                winning_team: None,
                state: new_phase,
            });
            return RoundValidationResult::Valid;
        };

        // Validate transition
        if !self.is_valid_transition(&current.state, &new_phase) {
            return RoundValidationResult::InvalidStateTransition {
                from: current.state.clone(),
                to: new_phase,
            };
        }

        // Check minimum round duration for end states
        if new_phase == RoundPhase::PostRound || new_phase == RoundPhase::Intermission {
            let elapsed = tick - current.start_tick;
            let minimum_ticks = 60 * 60; // 60 seconds at 60Hz
            
            if elapsed < minimum_ticks {
                return RoundValidationResult::PrematureEnd {
                    elapsed_ticks: elapsed,
                    minimum: minimum_ticks,
                };
            }
        }

        // Update state
        let mut new_state = current.clone();
        new_state.state = new_phase;
        if new_phase == RoundPhase::PostRound {
            new_state.end_tick = Some(tick);
        }
        self.current_round = Some(new_state);

        RoundValidationResult::Valid
    }

    fn is_valid_transition(&self, from: &RoundPhase, to: &RoundPhase) -> bool {
        use RoundPhase::*;
        
        match (from, to) {
            (Warmup, PreRound) => true,
            (PreRound, InProgress) => true,
            (InProgress, PostRound) => true,
            (PostRound, Intermission) => true,
            (Intermission, PreRound) => true,
            (Intermission, Warmup) => true,
            // Same state is always valid
            (a, b) if a == b => true,
            _ => false,
        }
    }

    /// Validates team score update
    pub fn validate_team_score(
        &mut self,
        team: TeamId,
        new_score: u32,
        tick: u64,
    ) -> RoundValidationResult {
        let expected_score = self.team_scores.get(&team).copied().unwrap_or(0);
        
        // Score can only increase by 1 per round in most game modes
        if new_score > expected_score + 1 {
            return RoundValidationResult::ScoreMismatch {
                team,
                expected: expected_score,
                actual: new_score,
            };
        }

        self.team_scores.insert(team, new_score);
        RoundValidationResult::Valid
    }
}
```



---

## 4. Performance Monitoring

### 4.1 Tick Rate Monitor

```rust
// server/src/validation/performance/tick_rate.rs

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Monitors server tick rate to detect performance degradation
pub struct TickRateMonitor {
    /// History of tick timestamps
    tick_history: VecDeque<Instant>,
    /// Current calculated tick rate
    current_tick_rate: f32,
    /// Window size for averaging (in seconds)
    window_size: Duration,
    /// Minimum acceptable tick rate
    min_tick_rate: f32,
    /// Consecutive violations counter
    violation_count: u32,
    /// Alert threshold for consecutive violations
    violation_threshold: u32,
}

#[derive(Debug, Clone)]
pub struct TickRateStats {
    pub current: f32,
    pub average: f32,
    pub minimum: f32,
    pub maximum: f32,
    pub std_dev: f32,
    pub violation_count: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TickRateAlert {
    Normal,
    Warning { current: f32, expected: f32 },
    Critical { current: f32, expected: f32, duration: Duration },
    Recovered { previous: f32, current: f32 },
}

impl TickRateMonitor {
    pub fn new(min_tick_rate: f32, violation_threshold: u32) -> Self {
        Self {
            tick_history: VecDeque::new(),
            current_tick_rate: min_tick_rate,
            window_size: Duration::from_secs(5),
            min_tick_rate,
            violation_count: 0,
            violation_threshold,
        }
    }

    /// Records a tick and updates statistics
    pub fn record_tick(&mut self, now: Instant) -> TickRateAlert {
        self.tick_history.push_back(now);
        
        // Remove old entries outside the window
        let cutoff = now - self.window_size;
        while let Some(&oldest) = self.tick_history.front() {
            if oldest < cutoff {
                self.tick_history.pop_front();
            } else {
                break;
            }
        }

        // Calculate current tick rate
        self.current_tick_rate = self.calculate_tick_rate();

        // Check for violations
        if self.current_tick_rate < self.min_tick_rate {
            self.violation_count += 1;
            
            if self.violation_count >= self.violation_threshold {
                let duration = self.estimate_violation_duration();
                return TickRateAlert::Critical {
                    current: self.current_tick_rate,
                    expected: self.min_tick_rate,
                    duration,
                };
            }
            
            return TickRateAlert::Warning {
                current: self.current_tick_rate,
                expected: self.min_tick_rate,
            };
        } else if self.violation_count > 0 {
            // Recovery
            let previous = self.min_tick_rate * 0.9; // Approximate
            self.violation_count = 0;
            return TickRateAlert::Recovered {
                previous,
                current: self.current_tick_rate,
            };
        }

        TickRateAlert::Normal
    }

    fn calculate_tick_rate(&self) -> f32 {
        if self.tick_history.len() < 2 {
            return self.min_tick_rate;
        }

        let duration = self.tick_history.back().unwrap() - self.tick_history.front().unwrap();
        let seconds = duration.as_secs_f32();
        
        if seconds > 0.0 {
            (self.tick_history.len() as f32 - 1.0) / seconds
        } else {
            self.min_tick_rate
        }
    }

    fn estimate_violation_duration(&self) -> Duration {
        Duration::from_secs(self.violation_count as u64)
    }

    pub fn get_stats(&self) -> TickRateStats {
        let rates: Vec<f32> = self.calculate_recent_rates();
        
        TickRateStats {
            current: self.current_tick_rate,
            average: Self::calculate_average(&rates),
            minimum: rates.iter().cloned().fold(f32::INFINITY, f32::min),
            maximum: rates.iter().cloned().fold(0.0, f32::max),
            std_dev: Self::calculate_std_dev(&rates),
            violation_count: self.violation_count,
        }
    }

    fn calculate_recent_rates(&self) -> Vec<f32> {
        let mut rates = Vec::new();
        let ticks: Vec<_> = self.tick_history.iter().cloned().collect();
        
        for window in ticks.windows(2) {
            let delta = window[1] - window[0];
            let rate = 1.0 / delta.as_secs_f32();
            rates.push(rate);
        }
        
        rates
    }

    fn calculate_average(values: &[f32]) -> f32 {
        if values.is_empty() {
            return 0.0;
        }
        values.iter().sum::<f32>() / values.len() as f32
    }

    fn calculate_std_dev(values: &[f32]) -> f32 {
        if values.len() < 2 {
            return 0.0;
        }
        
        let avg = Self::calculate_average(values);
        let variance = values.iter()
            .map(|v| (v - avg).powi(2))
            .sum::<f32>() / values.len() as f32;
        
        variance.sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tick_rate_calculation() {
        let mut monitor = TickRateMonitor::new(60.0, 3);
        let base = Instant::now();
        
        // Simulate 60Hz ticks
        for i in 0..120 {
            monitor.record_tick(base + Duration::from_millis(i * 1000 / 60));
        }
        
        let stats = monitor.get_stats();
        assert!(stats.current >= 55.0 && stats.current <= 65.0);
    }

    #[test]
    fn test_tick_rate_violation() {
        let mut monitor = TickRateMonitor::new(60.0, 3);
        let base = Instant::now();
        
        // Simulate slow ticks (30Hz)
        for i in 0..10 {
            let alert = monitor.record_tick(base + Duration::from_millis(i * 1000 / 30));
            
            if i >= 3 {
                assert!(matches!(alert, TickRateAlert::Warning { .. }));
            }
        }
    }
}
```

### 4.2 Memory Leak Detection

```rust
// server/src/validation/performance/memory.rs

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Monitors memory usage to detect leaks
pub struct MemoryMonitor {
    /// Memory samples over time
    samples: VecDeque<MemorySample>,
    /// Sample window duration
    window_size: Duration,
    /// Maximum acceptable growth rate (MB/minute)
    max_growth_rate: f32,
    /// Baseline memory at startup
    baseline_memory: usize,
    /// Last alert time to prevent spam
    last_alert: Option<Instant>,
    /// Alert cooldown
    alert_cooldown: Duration,
}

#[derive(Clone, Debug)]
struct MemorySample {
    timestamp: Instant,
    heap_used: usize,
    heap_allocated: usize,
    resident: usize,
}

#[derive(Debug, Clone)]
pub struct MemoryStats {
    pub current_mb: f32,
    pub baseline_mb: f32,
    pub growth_rate_mb_per_min: f32,
    pub peak_mb: f32,
    pub allocation_count: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MemoryAlert {
    Normal,
    Growing { rate: f32, threshold: f32 },
    LeakDetected { rate: f32, duration: Duration },
    Critical { usage_mb: f32, limit_mb: f32 },
}

impl MemoryMonitor {
    pub fn new(max_growth_rate_mb_per_min: f32) -> Self {
        let baseline = Self::get_current_memory();
        
        Self {
            samples: VecDeque::new(),
            window_size: Duration::from_secs(60),
            max_growth_rate: max_growth_rate_mb_per_min,
            baseline_memory: baseline,
            last_alert: None,
            alert_cooldown: Duration::from_secs(300), // 5 minutes
        }
    }

    /// Records a memory sample
    pub fn record_sample(&mut self) -> MemoryAlert {
        let sample = MemorySample {
            timestamp: Instant::now(),
            heap_used: Self::get_current_memory(),
            heap_allocated: Self::get_allocated_memory(),
            resident: Self::get_resident_memory(),
        };

        self.samples.push_back(sample);
        
        // Remove old samples
        let cutoff = Instant::now() - self.window_size;
        while let Some(front) = self.samples.front() {
            if front.timestamp < cutoff {
                self.samples.pop_front();
            } else {
                break;
            }
        }

        self.check_memory_health()
    }

    fn check_memory_health(&mut self) -> MemoryAlert {
        if self.samples.len() < 2 {
            return MemoryAlert::Normal;
        }

        let growth_rate = self.calculate_growth_rate();
        let current_usage = self.samples.back().unwrap().heap_used as f32 / 1024.0 / 1024.0;

        // Check for critical memory usage (> 4GB)
        if current_usage > 4096.0 {
            return MemoryAlert::Critical {
                usage_mb: current_usage,
                limit_mb: 4096.0,
            };
        }

        // Check growth rate
        if growth_rate > self.max_growth_rate * 5.0 {
            // Potential leak
            if self.can_alert() {
                self.last_alert = Some(Instant::now());
                return MemoryAlert::LeakDetected {
                    rate: growth_rate,
                    duration: self.window_size,
                };
            }
        } else if growth_rate > self.max_growth_rate {
            if self.can_alert() {
                self.last_alert = Some(Instant::now());
                return MemoryAlert::Growing {
                    rate: growth_rate,
                    threshold: self.max_growth_rate,
                };
            }
        }

        MemoryAlert::Normal
    }

    fn calculate_growth_rate(&self) -> f32 {
        if self.samples.len() < 2 {
            return 0.0;
        }

        let first = self.samples.front().unwrap();
        let last = self.samples.back().unwrap();
        
        let memory_delta = last.heap_used as f32 - first.heap_used as f32;
        let time_delta = (last.timestamp - first.timestamp).as_secs_f32() / 60.0; // in minutes
        
        if time_delta > 0.0 {
            memory_delta / 1024.0 / 1024.0 / time_delta // MB per minute
        } else {
            0.0
        }
    }

    fn can_alert(&self) -> bool {
        match self.last_alert {
            None => true,
            Some(last) => Instant::now() - last > self.alert_cooldown,
        }
    }

    pub fn get_stats(&self) -> MemoryStats {
        let current = self.samples.back()
            .map(|s| s.heap_used as f32 / 1024.0 / 1024.0)
            .unwrap_or(0.0);
        
        let peak = self.samples.iter()
            .map(|s| s.heap_used as f32 / 1024.0 / 1024.0)
            .fold(0.0, f32::max);

        MemoryStats {
            current_mb: current,
            baseline_mb: self.baseline_memory as f32 / 1024.0 / 1024.0,
            growth_rate_mb_per_min: self.calculate_growth_rate(),
            peak_mb: peak,
            allocation_count: self.samples.len() as u64,
        }
    }

    // Platform-specific memory getters
    #[cfg(target_os = "linux")]
    fn get_current_memory() -> usize {
        use std::fs;
        if let Ok(content) = fs::read_to_string("/proc/self/status") {
            for line in content.lines() {
                if line.starts_with("VmRSS:") {
                    let kb: usize = line.split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    return kb * 1024;
                }
            }
        }
        0
    }

    #[cfg(not(target_os = "linux"))]
    fn get_current_memory() -> usize {
        // Fallback - use jemalloc stats if available
        0
    }

    #[cfg(target_os = "linux")]
    fn get_allocated_memory() -> usize {
        use std::fs;
        if let Ok(content) = fs::read_to_string("/proc/self/status") {
            for line in content.lines() {
                if line.starts_with("VmSize:") {
                    let kb: usize = line.split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    return kb * 1024;
                }
            }
        }
        0
    }

    #[cfg(not(target_os = "linux"))]
    fn get_allocated_memory() -> usize {
        0
    }

    fn get_resident_memory() -> usize {
        Self::get_current_memory()
    }
}
```

### 4.3 Network Latency Monitor

```rust
// server/src/validation/performance/network.rs

use crate::core::types::PlayerId;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Monitors network latency for all connected players
pub struct NetworkLatencyMonitor {
    /// Per-player latency statistics
    player_latencies: HashMap<PlayerId, LatencyStats>,
    /// Global latency statistics
    global_stats: GlobalLatencyStats,
    /// Maximum acceptable latency
    max_latency_ms: u32,
    /// Alert threshold for consecutive high latency
    alert_threshold: u32,
}

#[derive(Clone, Debug)]
struct LatencyStats {
    /// Recent RTT measurements
    rtt_samples: VecDeque<Duration>,
    /// Last update time
    last_update: Instant,
    /// Consecutive high latency count
    violation_count: u32,
    /// Average latency
    average_ms: f32,
    /// Jitter (standard deviation)
    jitter_ms: f32,
}

#[derive(Clone, Debug, Default)]
struct GlobalLatencyStats {
    average_ms: f32,
    p50_ms: f32,
    p95_ms: f32,
    p99_ms: f32,
    max_ms: f32,
    player_count: usize,
}

#[derive(Debug, Clone)]
pub struct LatencyReport {
    pub player_id: PlayerId,
    pub current_rtt_ms: f32,
    pub average_rtt_ms: f32,
    pub jitter_ms: f32,
    pub packet_loss_percent: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LatencyAlert {
    Normal,
    Elevated { player_id: PlayerId, rtt_ms: f32 },
    High { player_id: PlayerId, rtt_ms: f32, duration: Duration },
    Critical { player_id: PlayerId, rtt_ms: f32 },
    Disconnected { player_id: PlayerId },
}

impl NetworkLatencyMonitor {
    pub fn new(max_latency_ms: u32, alert_threshold: u32) -> Self {
        Self {
            player_latencies: HashMap::new(),
            global_stats: GlobalLatencyStats::default(),
            max_latency_ms,
            alert_threshold,
        }
    }

    /// Records an RTT measurement for a player
    pub fn record_rtt(&mut self, player_id: PlayerId, rtt: Duration) -> Option<LatencyAlert> {
        let stats = self.player_latencies.entry(player_id).or_insert_with(|| LatencyStats {
            rtt_samples: VecDeque::with_capacity(100),
            last_update: Instant::now(),
            violation_count: 0,
            average_ms: 0.0,
            jitter_ms: 0.0,
        });

        stats.rtt_samples.push_back(rtt);
        if stats.rtt_samples.len() > 100 {
            stats.rtt_samples.pop_front();
        }

        stats.last_update = Instant::now();
        
        // Recalculate statistics
        let rtts_ms: Vec<f32> = stats.rtt_samples.iter()
            .map(|d| d.as_secs_f32() * 1000.0)
            .collect();
        
        stats.average_ms = Self::calculate_average(&rtts_ms);
        stats.jitter_ms = Self::calculate_std_dev(&rtts_ms);

        // Check for high latency
        let rtt_ms = rtt.as_secs_f32() * 1000.0;
        
        if rtt_ms > self.max_latency_ms as f32 {
            stats.violation_count += 1;
            
            if stats.violation_count >= self.alert_threshold {
                return Some(LatencyAlert::High {
                    player_id,
                    rtt_ms,
                    duration: Duration::from_secs(stats.violation_count as u64),
                });
            }
            
            return Some(LatencyAlert::Elevated { player_id, rtt_ms });
        } else {
            if stats.violation_count > 0 {
                stats.violation_count = stats.violation_count.saturating_sub(1);
            }
        }

        None
    }

    /// Updates global statistics
    pub fn update_global_stats(&mut self) {
        let all_averages: Vec<f32> = self.player_latencies.values()
            .map(|s| s.average_ms)
            .collect();

        if !all_averages.is_empty() {
            self.global_stats.average_ms = Self::calculate_average(&all_averages);
            self.global_stats.p50_ms = Self::calculate_percentile(&all_averages, 0.50);
            self.global_stats.p95_ms = Self::calculate_percentile(&all_averages, 0.95);
            self.global_stats.p99_ms = Self::calculate_percentile(&all_averages, 0.99);
            self.global_stats.max_ms = all_averages.iter().cloned().fold(0.0, f32::max);
            self.global_stats.player_count = all_averages.len();
        }
    }

    /// Checks for disconnected or stale players
    pub fn check_stale_players(&mut self, timeout: Duration) -> Vec<LatencyAlert> {
        let now = Instant::now();
        let mut alerts = Vec::new();
        let mut to_remove = Vec::new();

        for (player_id, stats) in &self.player_latencies {
            if now - stats.last_update > timeout {
                alerts.push(LatencyAlert::Disconnected { player_id: *player_id });
                to_remove.push(*player_id);
            }
        }

        for player_id in to_remove {
            self.player_latencies.remove(&player_id);
        }

        alerts
    }

    pub fn get_player_report(&self, player_id: PlayerId) -> Option<LatencyReport> {
        self.player_latencies.get(&player_id).map(|stats| LatencyReport {
            player_id,
            current_rtt_ms: stats.rtt_samples.back()
                .map(|d| d.as_secs_f32() * 1000.0)
                .unwrap_or(0.0),
            average_rtt_ms: stats.average_ms,
            jitter_ms: stats.jitter_ms,
            packet_loss_percent: 0.0, // TODO: Implement packet loss tracking
        })
    }

    pub fn get_global_stats(&self) -> &GlobalLatencyStats {
        &self.global_stats
    }

    fn calculate_average(values: &[f32]) -> f32 {
        if values.is_empty() {
            return 0.0;
        }
        values.iter().sum::<f32>() / values.len() as f32
    }

    fn calculate_std_dev(values: &[f32]) -> f32 {
        if values.len() < 2 {
            return 0.0;
        }
        
        let avg = Self::calculate_average(values);
        let variance = values.iter()
            .map(|v| (v - avg).powi(2))
            .sum::<f32>() / values.len() as f32;
        
        variance.sqrt()
    }

    fn calculate_percentile(sorted_values: &[f32], percentile: f32) -> f32 {
        if sorted_values.is_empty() {
            return 0.0;
        }
        
        let mut sorted = sorted_values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        
        let index = (percentile * (sorted.len() - 1) as f32) as usize;
        sorted[index.min(sorted.len() - 1)]
    }
}
```

### 4.4 FPS Monitor (Client-Side)

```rust
// server/src/validation/performance/client_fps.rs

use crate::core::types::PlayerId;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Monitors client-side FPS reported by players
pub struct ClientFPSMonitor {
    /// Per-player FPS statistics
    player_fps: HashMap<PlayerId, FPSStats>,
    /// Minimum acceptable FPS
    min_fps: u32,
    /// Alert threshold
    alert_threshold: u32,
}

#[derive(Clone, Debug)]
struct FPSStats {
    /// Recent FPS samples
    samples: VecDeque<u32>,
    /// Last update time
    last_update: Instant,
    /// Consecutive low FPS count
    violation_count: u32,
    /// Average FPS
    average: f32,
    /// Minimum FPS observed
    minimum: u32,
}

#[derive(Debug, Clone)]
pub struct FPSReport {
    pub player_id: PlayerId,
    pub current_fps: u32,
    pub average_fps: f32,
    pub minimum_fps: u32,
    pub one_percent_lows: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FPSAlert {
    Normal,
    Low { player_id: PlayerId, fps: u32 },
    Critical { player_id: PlayerId, fps: u32, duration: Duration },
    Stuttering { player_id: PlayerId, variance: f32 },
}

impl ClientFPSMonitor {
    pub fn new(min_fps: u32, alert_threshold: u32) -> Self {
        Self {
            player_fps: HashMap::new(),
            min_fps,
            alert_threshold,
        }
    }

    /// Records an FPS sample from a client
    pub fn record_fps(&mut self, player_id: PlayerId, fps: u32) -> Option<FPSAlert> {
        let stats = self.player_fps.entry(player_id).or_insert_with(|| FPSStats {
            samples: VecDeque::with_capacity(60),
            last_update: Instant::now(),
            violation_count: 0,
            average: fps as f32,
            minimum: fps,
        });

        stats.samples.push_back(fps);
        if stats.samples.len() > 60 {
            stats.samples.pop_front();
        }

        stats.last_update = Instant::now();
        stats.minimum = stats.minimum.min(fps);

        // Recalculate average
        stats.average = stats.samples.iter().sum::<u32>() as f32 / stats.samples.len() as f32;

        // Check for low FPS
        if fps < self.min_fps {
            stats.violation_count += 1;
            
            if stats.violation_count >= self.alert_threshold {
                return Some(FPSAlert::Critical {
                    player_id,
                    fps,
                    duration: Duration::from_secs(stats.violation_count as u64),
                });
            }
            
            return Some(FPSAlert::Low { player_id, fps });
        } else {
            if stats.violation_count > 0 {
                stats.violation_count = stats.violation_count.saturating_sub(1);
            }
        }

        // Check for stuttering (high variance)
        if stats.samples.len() >= 10 {
            let variance = self.calculate_variance(stats);
            if variance > 100.0 {
                return Some(FPSAlert::Stuttering { player_id, variance });
            }
        }

        None
    }

    fn calculate_variance(&self, stats: &FPSStats) -> f32 {
        let mean = stats.average;
        let variance = stats.samples.iter()
            .map(|&s| (s as f32 - mean).powi(2))
            .sum::<f32>() / stats.samples.len() as f32;
        variance
    }

    pub fn get_report(&self, player_id: PlayerId) -> Option<FPSReport> {
        self.player_fps.get(&player_id).map(|stats| {
            let mut sorted: Vec<_> = stats.samples.iter().cloned().collect();
            sorted.sort();
            
            let one_percent_idx = (sorted.len() as f32 * 0.01) as usize;
            let one_percent_lows = sorted.get(one_percent_idx).copied().unwrap_or(0) as f32;

            FPSReport {
                player_id,
                current_fps: *stats.samples.back().unwrap_or(&0),
                average_fps: stats.average,
                minimum_fps: stats.minimum,
                one_percent_lows,
            }
        })
    }

    /// Removes stale entries
    pub fn cleanup_stale(&mut self, timeout: Duration) {
        let now = Instant::now();
        self.player_fps.retain(|_, stats| {
            now - stats.last_update < timeout
        });
    }
}
```



---

## 5. Synchronization Validation

### 5.1 Server-Client State Sync

```rust
// server/src/validation/sync/state_sync.rs

use crate::core::types::{PlayerId, Position, EntityId};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Validates server-client state synchronization
pub struct StateSyncValidator {
    /// Expected client state per player
    client_states: HashMap<PlayerId, ClientStateSnapshot>,
    /// Server authoritative state
    server_states: HashMap<EntityId, EntityState>,
    /// Maximum allowed divergence
    max_divergence: f32,
    /// Sync check interval
    check_interval: Duration,
    /// Last sync check time
    last_check: Instant,
}

#[derive(Clone, Debug)]
struct ClientStateSnapshot {
    player_position: Position,
    entity_states: HashMap<EntityId, EntityState>,
    timestamp: Instant,
    sequence_number: u64,
}

#[derive(Clone, Debug, PartialEq)]
struct EntityState {
    position: Position,
    velocity: (f32, f32),
    health: f32,
    flags: u32,
}

#[derive(Debug, Clone)]
pub struct SyncDivergence {
    pub entity_id: EntityId,
    pub field: String,
    pub server_value: f32,
    pub client_value: f32,
    pub delta: f32,
}

#[derive(Debug, Clone)]
pub struct SyncReport {
    pub player_id: PlayerId,
    pub divergence_count: usize,
    pub max_divergence: f32,
    pub divergences: Vec<SyncDivergence>,
    pub sync_latency_ms: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SyncAlert {
    Normal,
    MinorDivergence { player_id: PlayerId, count: usize },
    MajorDivergence { player_id: PlayerId, divergences: Vec<SyncDivergence> },
    DesyncDetected { player_id: PlayerId, correction: Position },
    Critical { player_id: PlayerId, message: String },
}

impl StateSyncValidator {
    pub fn new(max_divergence: f32, check_interval_ms: u64) -> Self {
        Self {
            client_states: HashMap::new(),
            server_states: HashMap::new(),
            max_divergence,
            check_interval: Duration::from_millis(check_interval_ms),
            last_check: Instant::now(),
        }
    }

    /// Records server authoritative state
    pub fn record_server_state(
        &mut self,
        entity_id: EntityId,
        position: Position,
        velocity: (f32, f32),
        health: f32,
        flags: u32,
    ) {
        self.server_states.insert(entity_id, EntityState {
            position,
            velocity,
            health,
            flags,
        });
    }

    /// Records client-reported state
    pub fn record_client_state(
        &mut self,
        player_id: PlayerId,
        position: Position,
        sequence_number: u64,
    ) {
        self.client_states.insert(player_id, ClientStateSnapshot {
            player_position: position,
            entity_states: HashMap::new(),
            timestamp: Instant::now(),
            sequence_number,
        });
    }

    /// Performs sync validation
    pub fn validate_sync(&mut self, player_id: PlayerId) -> Option<SyncAlert> {
        let now = Instant::now();
        
        // Only check at intervals
        if now - self.last_check < self.check_interval {
            return None;
        }
        self.last_check = now;

        let Some(client_state) = self.client_states.get(&player_id) else {
            return None;
        };

        let player_entity_id = EntityId(player_id.0);
        let Some(server_state) = self.server_states.get(&player_entity_id) else {
            return None;
        };

        let mut divergences = Vec::new();

        // Check position divergence
        let position_delta = client_state.player_position.distance_to(&server_state.position);
        if position_delta > self.max_divergence {
            divergences.push(SyncDivergence {
                entity_id: player_entity_id,
                field: "position".to_string(),
                server_value: server_state.position.x,
                client_value: client_state.player_position.x,
                delta: position_delta,
            });
        }

        // Classify alert severity
        if divergences.is_empty() {
            Some(SyncAlert::Normal)
        } else if position_delta > self.max_divergence * 10.0 {
            Some(SyncAlert::DesyncDetected {
                player_id,
                correction: server_state.position,
            })
        } else if divergences.len() > 5 {
            Some(SyncAlert::MajorDivergence { player_id, divergences })
        } else {
            Some(SyncAlert::MinorDivergence { 
                player_id, 
                count: divergences.len() 
            })
        }
    }

    /// Generates a full sync report for a player
    pub fn generate_report(&self, player_id: PlayerId) -> Option<SyncReport> {
        let client_state = self.client_states.get(&player_id)?;
        let player_entity_id = EntityId(player_id.0);
        let server_state = self.server_states.get(&player_entity_id)?;

        let sync_latency = (Instant::now() - client_state.timestamp).as_secs_f32() * 1000.0;
        
        let position_delta = client_state.player_position.distance_to(&server_state.position);
        
        Some(SyncReport {
            player_id,
            divergence_count: if position_delta > self.max_divergence { 1 } else { 0 },
            max_divergence: position_delta,
            divergences: Vec::new(),
            sync_latency_ms: sync_latency,
        })
    }

    /// Forces a state correction for a desynced player
    pub fn force_correction(&self, player_id: PlayerId) -> Option<Position> {
        let player_entity_id = EntityId(player_id.0);
        self.server_states.get(&player_entity_id).map(|s| s.position)
    }
}
```

### 5.2 AOI (Area of Interest) Correctness

```rust
// server/src/validation/sync/aoi.rs

use crate::core::types::{PlayerId, Position, EntityId};
use crate::scaling::aoi::AOIConfig;
use std::collections::{HashMap, HashSet};

/// Validates AOI (Area of Interest) correctness
pub struct AOIValidator {
    /// Expected visible entities per player
    expected_visibility: HashMap<PlayerId, HashSet<EntityId>>,
    /// Actual visible entities (from client reports)
    actual_visibility: HashMap<PlayerId, HashSet<EntityId>>,
    /// AOI configuration
    aoi_config: AOIConfig,
    /// Entity positions
    entity_positions: HashMap<EntityId, Position>,
}

#[derive(Debug, Clone)]
pub struct AOIError {
    pub player_id: PlayerId,
    pub error_type: AOIErrorType,
    pub entity_id: EntityId,
    pub distance: f32,
    pub aoi_radius: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AOIErrorType {
    /// Entity should be visible but isn't
    MissingEntity,
    /// Entity is visible but shouldn't be
    ExtraEntity,
    /// Entity visibility changed too quickly (flickering)
    Flickering,
}

#[derive(Debug, Clone)]
pub struct AOIReport {
    pub player_id: PlayerId,
    pub expected_count: usize,
    pub actual_count: usize,
    pub missing_entities: Vec<EntityId>,
    pub extra_entities: Vec<EntityId>,
    pub correctness_percent: f32,
}

impl AOIValidator {
    pub fn new(aoi_config: AOIConfig) -> Self {
        Self {
            expected_visibility: HashMap::new(),
            actual_visibility: HashMap::new(),
            aoi_config,
            entity_positions: HashMap::new(),
        }
    }

    /// Updates entity position
    pub fn update_entity_position(&mut self, entity_id: EntityId, position: Position) {
        self.entity_positions.insert(entity_id, position);
    }

    /// Calculates expected visibility for a player
    pub fn calculate_expected_visibility(&mut self, player_id: PlayerId, player_pos: Position) {
        let mut visible = HashSet::new();

        for (entity_id, entity_pos) in &self.entity_positions {
            let distance = player_pos.distance_to(entity_pos);
            
            if distance <= self.aoi_config.radius {
                visible.insert(*entity_id);
            }
        }

        self.expected_visibility.insert(player_id, visible);
    }

    /// Records actual visibility from client
    pub fn record_actual_visibility(
        &mut self,
        player_id: PlayerId,
        visible_entities: HashSet<EntityId>,
    ) {
        self.actual_visibility.insert(player_id, visible_entities);
    }

    /// Validates AOI correctness for a player
    pub fn validate_aoi(&self, player_id: PlayerId) -> Vec<AOIError> {
        let mut errors = Vec::new();

        let expected = match self.expected_visibility.get(&player_id) {
            Some(e) => e,
            None => return errors,
        };

        let actual = match self.actual_visibility.get(&player_id) {
            Some(a) => a,
            None => return errors,
        };

        // Check for missing entities
        for entity_id in expected {
            if !actual.contains(entity_id) {
                let entity_pos = self.entity_positions.get(entity_id).unwrap();
                let player_pos = self.entity_positions.get(&EntityId(player_id.0)).unwrap();
                
                errors.push(AOIError {
                    player_id,
                    error_type: AOIErrorType::MissingEntity,
                    entity_id: *entity_id,
                    distance: player_pos.distance_to(entity_pos),
                    aoi_radius: self.aoi_config.radius,
                });
            }
        }

        // Check for extra entities
        for entity_id in actual {
            if !expected.contains(entity_id) {
                let entity_pos = self.entity_positions.get(entity_id).unwrap();
                let player_pos = self.entity_positions.get(&EntityId(player_id.0)).unwrap();
                
                errors.push(AOIError {
                    player_id,
                    error_type: AOIErrorType::ExtraEntity,
                    entity_id: *entity_id,
                    distance: player_pos.distance_to(entity_pos),
                    aoi_radius: self.aoi_config.radius,
                });
            }
        }

        errors
    }

    /// Generates AOI report for a player
    pub fn generate_report(&self, player_id: PlayerId) -> Option<AOIReport> {
        let expected = self.expected_visibility.get(&player_id)?;
        let actual = self.actual_visibility.get(&player_id)?;

        let missing: Vec<_> = expected.difference(actual).cloned().collect();
        let extra: Vec<_> = actual.difference(expected).cloned().collect();

        let union_size = expected.union(actual).count();
        let intersection_size = expected.intersection(actual).count();
        
        let correctness = if union_size > 0 {
            (intersection_size as f32 / union_size as f32) * 100.0
        } else {
            100.0
        };

        Some(AOIReport {
            player_id,
            expected_count: expected.len(),
            actual_count: actual.len(),
            missing_entities: missing,
            extra_entities: extra,
            correctness_percent: correctness,
        })
    }

    /// Validates AOI for all players and returns aggregate stats
    pub fn validate_all(&self) -> AOIValidationSummary {
        let mut total_errors = 0;
        let mut total_expected = 0;
        let mut total_actual = 0;

        for player_id in self.expected_visibility.keys() {
            let errors = self.validate_aoi(*player_id);
            total_errors += errors.len();
            
            if let Some(expected) = self.expected_visibility.get(player_id) {
                total_expected += expected.len();
            }
            if let Some(actual) = self.actual_visibility.get(player_id) {
                total_actual += actual.len();
            }
        }

        AOIValidationSummary {
            player_count: self.expected_visibility.len(),
            total_errors,
            total_expected,
            total_actual,
            accuracy_percent: if total_expected > 0 {
                ((total_expected - total_errors) as f32 / total_expected as f32) * 100.0
            } else {
                100.0
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct AOIValidationSummary {
    pub player_count: usize,
    pub total_errors: usize,
    pub total_expected: usize,
    pub total_actual: usize,
    pub accuracy_percent: f32,
}
```

### 5.3 Projectile Hit Validation

```rust
// server/src/validation/sync/projectile.rs

use crate::core::types::{PlayerId, Position, EntityId};
use crate::entities::projectile::Projectile;
use std::collections::HashMap;
use std::time::Instant;

/// Validates projectile hit detection and registration
pub struct ProjectileValidator {
    /// Active projectiles
    active_projectiles: HashMap<u32, ProjectileState>,
    /// Hit validation history
    hit_history: Vec<HitEvent>,
    /// Maximum allowed hit distance (for lag compensation)
    max_hit_distance: f32,
    /// Maximum projectile lifetime
    max_lifetime_ms: u64,
}

#[derive(Clone, Debug)]
struct ProjectileState {
    id: u32,
    owner: PlayerId,
    spawn_position: Position,
    velocity: (f32, f32),
    spawn_time: Instant,
    damage: f32,
}

#[derive(Clone, Debug)]
struct HitEvent {
    tick: u64,
    projectile_id: u32,
    victim: PlayerId,
    position: Position,
    damage: f32,
    validated: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HitValidationResult {
    Valid,
    OutOfRange { actual: f32, max: f32 },
    ExpiredProjectile { lifetime_ms: u64 },
    InvalidVictim { reason: String },
    AlreadyHit,
    NoLineOfSight,
    ThroughWall,
}

impl ProjectileValidator {
    pub fn new(max_hit_distance: f32, max_lifetime_ms: u64) -> Self {
        Self {
            active_projectiles: HashMap::new(),
            hit_history: Vec::new(),
            max_hit_distance,
            max_lifetime_ms,
        }
    }

    /// Registers a new projectile
    pub fn register_projectile(&mut self, projectile: &Projectile, owner: PlayerId) {
        let state = ProjectileState {
            id: projectile.id,
            owner,
            spawn_position: projectile.position,
            velocity: projectile.velocity,
            spawn_time: Instant::now(),
            damage: projectile.damage,
        };

        self.active_projectiles.insert(projectile.id, state);
    }

    /// Validates a hit report
    pub fn validate_hit(
        &mut self,
        projectile_id: u32,
        victim: PlayerId,
        hit_position: Position,
        victim_position: Position,
        tick: u64,
    ) -> HitValidationResult {
        let Some(projectile) = self.active_projectiles.get(&projectile_id) else {
            return HitValidationResult::ExpiredProjectile { lifetime_ms: u64::MAX };
        };

        // Check projectile lifetime
        let lifetime = (Instant::now() - projectile.spawn_time).as_millis() as u64;
        if lifetime > self.max_lifetime_ms {
            return HitValidationResult::ExpiredProjectile { lifetime_ms: lifetime };
        }

        // Check if victim was already hit by this projectile
        if self.was_already_hit(projectile_id, victim) {
            return HitValidationResult::AlreadyHit;
        }

        // Calculate expected hit position based on projectile trajectory
        let expected_position = self.calculate_expected_position(projectile, lifetime);
        
        // Validate hit distance
        let hit_distance = expected_position.distance_to(&hit_position);
        if hit_distance > self.max_hit_distance {
            return HitValidationResult::OutOfRange {
                actual: hit_distance,
                max: self.max_hit_distance,
            };
        }

        // Validate victim proximity
        let victim_distance = hit_position.distance_to(&victim_position);
        if victim_distance > 50.0 { // Player radius + tolerance
            return HitValidationResult::InvalidVictim {
                reason: format!("Victim too far from hit: {} units", victim_distance),
            };
        }

        // Record valid hit
        self.hit_history.push(HitEvent {
            tick,
            projectile_id,
            victim,
            position: hit_position,
            damage: projectile.damage,
            validated: true,
        });

        HitValidationResult::Valid
    }

    fn calculate_expected_position(&self, projectile: &ProjectileState, lifetime_ms: u64) -> Position {
        let t = lifetime_ms as f32 / 1000.0;
        Position {
            x: projectile.spawn_position.x + projectile.velocity.0 * t,
            y: projectile.spawn_position.y + projectile.velocity.1 * t,
        }
    }

    fn was_already_hit(&self, projectile_id: u32, victim: PlayerId) -> bool {
        self.hit_history.iter().any(|h| {
            h.projectile_id == projectile_id && h.victim == victim
        })
    }

    /// Cleans up expired projectiles
    pub fn cleanup(&mut self) {
        let now = Instant::now();
        self.active_projectiles.retain(|_, p| {
            (now - p.spawn_time).as_millis() as u64 <= self.max_lifetime_ms
        });

        // Trim hit history
        if self.hit_history.len() > 10000 {
            self.hit_history.drain(0..self.hit_history.len() - 5000);
        }
    }

    /// Gets hit statistics
    pub fn get_stats(&self) -> HitStatistics {
        let total_hits = self.hit_history.len();
        let validated_hits = self.hit_history.iter().filter(|h| h.validated).count();

        HitStatistics {
            total_hits,
            validated_hits,
            active_projectiles: self.active_projectiles.len(),
            validation_rate: if total_hits > 0 {
                (validated_hits as f32 / total_hits as f32) * 100.0
            } else {
                100.0
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct HitStatistics {
    pub total_hits: usize,
    pub validated_hits: usize,
    pub active_projectiles: usize,
    pub validation_rate: f32,
}
```



---

## 6. Anomaly Detection

### 6.1 Statistical Outlier Detection

```rust
// server/src/validation/anomaly/statistical.rs

use crate::core::types::PlayerId;
use std::collections::{HashMap, VecDeque};

/// Statistical anomaly detection using Z-score and IQR methods
pub struct StatisticalAnomalyDetector {
    /// Per-player metric history
    player_metrics: HashMap<PlayerId, PlayerMetrics>,
    /// Global metric baselines
    global_baselines: GlobalBaselines,
    /// Z-score threshold for anomaly flagging
    z_score_threshold: f32,
    /// Window size for calculations
    window_size: usize,
}

#[derive(Clone, Debug)]
struct PlayerMetrics {
    /// Kill history
    kills: VecDeque<u64>,
    /// Death history
    deaths: VecDeque<u64>,
    /// Score history over time
    score_history: VecDeque<(u64, f32)>,
    /// Accuracy samples (hits / shots)
    accuracy_samples: VecDeque<f32>,
    /// Movement speed samples
    speed_samples: VecDeque<f32>,
    /// Reaction time samples (ms)
    reaction_times: VecDeque<f32>,
}

#[derive(Clone, Debug, Default)]
struct GlobalBaselines {
    average_kills_per_minute: f32,
    average_accuracy: f32,
    average_reaction_time_ms: f32,
    average_score_per_minute: f32,
}

#[derive(Debug, Clone)]
pub struct AnomalyReport {
    pub player_id: PlayerId,
    pub anomaly_type: AnomalyType,
    pub severity: AnomalySeverity,
    pub z_score: f32,
    pub expected_value: f32,
    pub actual_value: f32,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AnomalyType {
    KillRate,
    Accuracy,
    ReactionTime,
    ScoreGain,
    MovementPattern,
    MultiMetric,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AnomalySeverity {
    Low,      // Z-score 2.0-3.0
    Medium,   // Z-score 3.0-4.0
    High,     // Z-score 4.0-5.0
    Critical, // Z-score > 5.0
}

impl StatisticalAnomalyDetector {
    pub fn new(z_score_threshold: f32, window_size: usize) -> Self {
        Self {
            player_metrics: HashMap::new(),
            global_baselines: GlobalBaselines::default(),
            z_score_threshold,
            window_size,
        }
    }

    /// Records a kill event
    pub fn record_kill(&mut self, player_id: PlayerId, tick: u64) {
        let metrics = self.player_metrics.entry(player_id).or_default();
        metrics.kills.push_back(tick);
        
        // Trim old entries (keep last 10 minutes at 60Hz)
        while metrics.kills.len() > self.window_size {
            metrics.kills.pop_front();
        }
    }

    /// Records accuracy sample
    pub fn record_accuracy(&mut self, player_id: PlayerId, accuracy: f32) {
        let metrics = self.player_metrics.entry(player_id).or_default();
        metrics.accuracy_samples.push_back(accuracy);
        
        while metrics.accuracy_samples.len() > self.window_size {
            metrics.accuracy_samples.pop_front();
        }
    }

    /// Records reaction time
    pub fn record_reaction_time(&mut self, player_id: PlayerId, reaction_ms: f32) {
        let metrics = self.player_metrics.entry(player_id).or_default();
        metrics.reaction_times.push_back(reaction_ms);
        
        while metrics.reaction_times.len() > self.window_size {
            metrics.reaction_times.pop_front();
        }
    }

    /// Detects anomalies for a player
    pub fn detect_anomalies(&self, player_id: PlayerId) -> Vec<AnomalyReport> {
        let mut reports = Vec::new();
        let Some(metrics) = self.player_metrics.get(&player_id) else {
            return reports;
        };

        // Check kill rate anomaly
        if let Some(report) = self.check_kill_rate_anomaly(player_id, metrics) {
            reports.push(report);
        }

        // Check accuracy anomaly
        if let Some(report) = self.check_accuracy_anomaly(player_id, metrics) {
            reports.push(report);
        }

        // Check reaction time anomaly
        if let Some(report) = self.check_reaction_time_anomaly(player_id, metrics) {
            reports.push(report);
        }

        reports
    }

    fn check_kill_rate_anomaly(&self, player_id: PlayerId, metrics: &PlayerMetrics) -> Option<AnomalyReport> {
        if metrics.kills.len() < 10 {
            return None;
        }

        let kill_rate = metrics.kills.len() as f32 / self.window_size as f32 * 3600.0; // per hour
        let global_avg = self.global_baselines.average_kills_per_minute * 60.0;
        
        if global_avg <= 0.0 {
            return None;
        }

        let z_score = (kill_rate - global_avg) / (global_avg * 0.5); // Assume 50% std dev

        if z_score > self.z_score_threshold {
            Some(AnomalyReport {
                player_id,
                anomaly_type: AnomalyType::KillRate,
                severity: self.z_score_to_severity(z_score),
                z_score,
                expected_value: global_avg,
                actual_value: kill_rate,
                confidence: (z_score / 5.0).min(1.0),
            })
        } else {
            None
        }
    }

    fn check_accuracy_anomaly(&self, player_id: PlayerId, metrics: &PlayerMetrics) -> Option<AnomalyReport> {
        if metrics.accuracy_samples.len() < 20 {
            return None;
        }

        let avg_accuracy: f32 = metrics.accuracy_samples.iter().sum::<f32>() 
            / metrics.accuracy_samples.len() as f32;
        
        let global_avg = self.global_baselines.average_accuracy;
        
        if global_avg <= 0.0 {
            return None;
        }

        let z_score = (avg_accuracy - global_avg) / 0.1; // Assume 10% std dev

        if z_score > self.z_score_threshold {
            Some(AnomalyReport {
                player_id,
                anomaly_type: AnomalyType::Accuracy,
                severity: self.z_score_to_severity(z_score),
                z_score,
                expected_value: global_avg,
                actual_value: avg_accuracy,
                confidence: (z_score / 5.0).min(1.0),
            })
        } else {
            None
        }
    }

    fn check_reaction_time_anomaly(&self, player_id: PlayerId, metrics: &PlayerMetrics) -> Option<AnomalyReport> {
        if metrics.reaction_times.len() < 10 {
            return None;
        }

        let avg_reaction: f32 = metrics.reaction_times.iter().sum::<f32>() 
            / metrics.reaction_times.len() as f32;
        
        let global_avg = self.global_baselines.average_reaction_time_ms;
        
        if global_avg <= 0.0 {
            return None;
        }

        // Lower reaction time is suspicious
        let z_score = (global_avg - avg_reaction) / (global_avg * 0.3);

        if z_score > self.z_score_threshold {
            Some(AnomalyReport {
                player_id,
                anomaly_type: AnomalyType::ReactionTime,
                severity: self.z_score_to_severity(z_score),
                z_score,
                expected_value: global_avg,
                actual_value: avg_reaction,
                confidence: (z_score / 5.0).min(1.0),
            })
        } else {
            None
        }
    }

    fn z_score_to_severity(&self, z_score: f32) -> AnomalySeverity {
        match z_score {
            s if s < 3.0 => AnomalySeverity::Low,
            s if s < 4.0 => AnomalySeverity::Medium,
            s if s < 5.0 => AnomalySeverity::High,
            _ => AnomalySeverity::Critical,
        }
    }

    /// Updates global baselines from all player data
    pub fn update_baselines(&mut self) {
        let mut total_kills = 0usize;
        let mut total_accuracy = 0.0f32;
        let mut total_reaction = 0.0f32;
        let mut player_count = 0usize;

        for metrics in self.player_metrics.values() {
            total_kills += metrics.kills.len();
            
            if !metrics.accuracy_samples.is_empty() {
                total_accuracy += metrics.accuracy_samples.iter().sum::<f32>() 
                    / metrics.accuracy_samples.len() as f32;
            }
            
            if !metrics.reaction_times.is_empty() {
                total_reaction += metrics.reaction_times.iter().sum::<f32>() 
                    / metrics.reaction_times.len() as f32;
            }
            
            player_count += 1;
        }

        if player_count > 0 {
            self.global_baselines.average_kills_per_minute = 
                total_kills as f32 / player_count as f32 / self.window_size as f32 * 3600.0;
            self.global_baselines.average_accuracy = total_accuracy / player_count as f32;
            self.global_baselines.average_reaction_time_ms = total_reaction / player_count as f32;
        }
    }
}

impl Default for PlayerMetrics {
    fn default() -> Self {
        Self {
            kills: VecDeque::with_capacity(1000),
            deaths: VecDeque::with_capacity(1000),
            score_history: VecDeque::with_capacity(1000),
            accuracy_samples: VecDeque::with_capacity(1000),
            speed_samples: VecDeque::with_capacity(1000),
            reaction_times: VecDeque::with_capacity(1000),
        }
    }
}
```

### 6.2 Impossible Action Detection

```rust
// server/src/validation/anomaly/impossible_actions.rs

use crate::core::types::{PlayerId, Position};
use crate::core::constants::*;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Detects physically impossible actions
pub struct ImpossibleActionDetector {
    /// Action history per player
    action_history: HashMap<PlayerId, ActionHistory>,
    /// Impossible action thresholds
    thresholds: ImpossibleActionThresholds,
}

#[derive(Clone, Debug)]
struct ActionHistory {
    last_shot_time: Option<Instant>,
    shot_count_in_window: u32,
    last_position: Option<Position>,
    last_velocity: Option<(f32, f32)>,
    consecutive_headshots: u32,
    wallbang_kills: u32,
    actions: Vec<TimestampedAction>,
}

#[derive(Clone, Debug)]
struct TimestampedAction {
    timestamp: Instant,
    action_type: ActionType,
    details: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ActionType {
    Shot,
    Kill,
    Headshot,
    Wallbang,
    Teleport,
    RapidTurn,
}

#[derive(Clone, Debug)]
pub struct ImpossibleActionThresholds {
    /// Max shots per second (fire rate)
    pub max_shots_per_second: u32,
    /// Max headshot percentage (suspicious if > this)
    pub max_headshot_percent: f32,
    /// Max wallbang percentage
    pub max_wallbang_percent: f32,
    /// Max turn speed (degrees per second)
    pub max_turn_speed: f32,
    /// Min time between impossible actions before alert
    pub min_action_interval: Duration,
}

#[derive(Debug, Clone)]
pub struct ImpossibleActionReport {
    pub player_id: PlayerId,
    pub action_type: ActionType,
    pub timestamp: Instant,
    pub details: String,
    pub severity: ActionSeverity,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActionSeverity {
    Suspicious,
    LikelyCheat,
    ConfirmedCheat,
}

impl ImpossibleActionDetector {
    pub fn new(thresholds: ImpossibleActionThresholds) -> Self {
        Self {
            action_history: HashMap::new(),
            thresholds,
        }
    }

    /// Records a shot fired
    pub fn record_shot(&mut self, player_id: PlayerId) -> Option<ImpossibleActionReport> {
        let history = self.action_history.entry(player_id).or_default();
        let now = Instant::now();

        // Check fire rate
        if let Some(last_shot) = history.last_shot_time {
            let elapsed = now - last_shot;
            let shots_per_second = 1.0 / elapsed.as_secs_f32();
            
            if shots_per_second > self.thresholds.max_shots_per_second as f32 {
                return Some(ImpossibleActionReport {
                    player_id,
                    action_type: ActionType::Shot,
                    timestamp: now,
                    details: format!("Fire rate exceeded: {:.1} shots/sec", shots_per_second),
                    severity: ActionSeverity::LikelyCheat,
                    evidence: vec![
                        format!("Max allowed: {} shots/sec", self.thresholds.max_shots_per_second),
                        format!("Actual: {:.1} shots/sec", shots_per_second),
                    ],
                });
            }
        }

        history.last_shot_time = Some(now);
        history.shot_count_in_window += 1;

        // Reset window after 1 second
        if history.actions.len() > 60 {
            history.shot_count_in_window = 1;
        }

        history.actions.push(TimestampedAction {
            timestamp: now,
            action_type: ActionType::Shot,
            details: "Shot fired".to_string(),
        });

        None
    }

    /// Records a kill
    pub fn record_kill(
        &mut self,
        player_id: PlayerId,
        is_headshot: bool,
        is_wallbang: bool,
    ) -> Option<ImpossibleActionReport> {
        let history = self.action_history.entry(player_id).or_default();
        let now = Instant::now();

        // Check headshot streak
        if is_headshot {
            history.consecutive_headshots += 1;
            
            if history.consecutive_headshots >= 5 {
                return Some(ImpossibleActionReport {
                    player_id,
                    action_type: ActionType::Headshot,
                    timestamp: now,
                    details: format!("{} consecutive headshots", history.consecutive_headshots),
                    severity: ActionSeverity::Suspicious,
                    evidence: vec![
                        format!("Consecutive headshots: {}", history.consecutive_headshots),
                        "Probability: < 0.1%".to_string(),
                    ],
                });
            }
        } else {
            history.consecutive_headshots = 0;
        }

        // Check wallbang streak
        if is_wallbang {
            history.wallbang_kills += 1;
            
            // Calculate wallbang percentage
            let total_kills = history.actions.iter()
                .filter(|a| a.action_type == ActionType::Kill)
                .count() as f32;
            
            if total_kills > 10.0 {
                let wallbang_percent = history.wallbang_kills as f32 / total_kills * 100.0;
                
                if wallbang_percent > self.thresholds.max_wallbang_percent {
                    return Some(ImpossibleActionReport {
                        player_id,
                        action_type: ActionType::Wallbang,
                        timestamp: now,
                        details: format!("{:.1}% wallbang kills", wallbang_percent),
                        severity: ActionSeverity::Suspicious,
                        evidence: vec![
                            format!("Wallbang kills: {}", history.wallbang_kills),
                            format!("Total kills: {}", total_kills),
                        ],
                    });
                }
            }
        }

        history.actions.push(TimestampedAction {
            timestamp: now,
            action_type: ActionType::Kill,
            details: if is_headshot { "Headshot kill".to_string() } else { "Kill".to_string() },
        });

        None
    }

    /// Records a rapid turn (potential aimbot)
    pub fn record_turn(&mut self, player_id: PlayerId, degrees: f32, time_ms: f32) -> Option<ImpossibleActionReport> {
        let turn_speed = degrees / time_ms * 1000.0; // degrees per second
        
        if turn_speed > self.thresholds.max_turn_speed {
            return Some(ImpossibleActionReport {
                player_id,
                action_type: ActionType::RapidTurn,
                timestamp: Instant::now(),
                details: format!("Impossible turn: {:.0}° in {:.0}ms", degrees, time_ms),
                severity: ActionSeverity::LikelyCheat,
                evidence: vec![
                    format!("Turn speed: {:.0}°/sec", turn_speed),
                    format!("Max human speed: {:.0}°/sec", self.thresholds.max_turn_speed),
                ],
            });
        }

        None
    }

    /// Cleans up old history entries
    pub fn cleanup(&mut self, max_age: Duration) {
        let now = Instant::now();
        
        for history in self.action_history.values_mut() {
            history.actions.retain(|a| now - a.timestamp < max_age);
        }
    }
}

impl Default for ActionHistory {
    fn default() -> Self {
        Self {
            last_shot_time: None,
            shot_count_in_window: 0,
            last_position: None,
            last_velocity: None,
            consecutive_headshots: 0,
            wallbang_kills: 0,
            actions: Vec::new(),
        }
    }
}

impl Default for ImpossibleActionThresholds {
    fn default() -> Self {
        Self {
            max_shots_per_second: 10,      // Most weapons fire slower
            max_headshot_percent: 80.0,    // Even pros rarely exceed 70%
            max_wallbang_percent: 30.0,    // Unusual to have many wallbangs
            max_turn_speed: 360.0,         // One full rotation per second max
            min_action_interval: Duration::from_secs(1),
        }
    }
}
```

### 6.3 Bot Behavior Validation

```rust
// server/src/validation/anomaly/bot_validation.rs

use crate::core::types::{PlayerId, Position};
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// Validates bot behavior to ensure AI is functioning correctly
pub struct BotValidator {
    /// Bot state tracking
    bot_states: HashMap<PlayerId, BotState>,
    /// Validation thresholds
    thresholds: BotValidationThresholds,
}

#[derive(Clone, Debug)]
struct BotState {
    is_bot: bool,
    spawn_time: Instant,
    position_history: VecDeque<(Instant, Position)>,
    decision_history: VecDeque<BotDecision>,
    stuck_count: u32,
    last_stuck_check: Instant,
    pathfinding_failures: u32,
    action_counts: HashMap<String, u32>,
}

#[derive(Clone, Debug)]
struct BotDecision {
    timestamp: Instant,
    decision_type: String,
    context: String,
}

#[derive(Clone, Debug)]
pub struct BotValidationThresholds {
    /// Max time bot can be stuck
    pub max_stuck_duration: Duration,
    /// Max pathfinding failures before alert
    pub max_pathfinding_failures: u32,
    /// Min actions per minute
    pub min_actions_per_minute: u32,
    /// Max position variance (bots shouldn't be too predictable)
    pub min_position_variance: f32,
}

#[derive(Debug, Clone)]
pub struct BotValidationReport {
    pub bot_id: PlayerId,
    pub is_valid: bool,
    pub issues: Vec<BotIssue>,
    pub health_score: f32, // 0.0 - 100.0
}

#[derive(Debug, Clone)]
pub struct BotIssue {
    pub issue_type: BotIssueType,
    pub severity: BotIssueSeverity,
    pub description: String,
    pub recommendation: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BotIssueType {
    Stuck,
    PathfindingFailure,
    Inactive,
    PredictableMovement,
    DecisionLoop,
    SpawnFailure,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BotIssueSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

impl BotValidator {
    pub fn new(thresholds: BotValidationThresholds) -> Self {
        Self {
            bot_states: HashMap::new(),
            thresholds,
        }
    }

    /// Registers a bot
    pub fn register_bot(&mut self, bot_id: PlayerId) {
        self.bot_states.insert(bot_id, BotState {
            is_bot: true,
            spawn_time: Instant::now(),
            position_history: VecDeque::with_capacity(100),
            decision_history: VecDeque::with_capacity(100),
            stuck_count: 0,
            last_stuck_check: Instant::now(),
            pathfinding_failures: 0,
            action_counts: HashMap::new(),
        });
    }

    /// Records bot position
    pub fn record_position(&mut self, bot_id: PlayerId, position: Position) {
        let Some(state) = self.bot_states.get_mut(&bot_id) else {
            return;
        };

        let now = Instant::now();
        state.position_history.push_back((now, position));

        // Keep last 100 positions
        while state.position_history.len() > 100 {
            state.position_history.pop_front();
        }

        // Check if stuck (no movement in last 5 seconds)
        if now - state.last_stuck_check > Duration::from_secs(5) {
            self.check_if_stuck(bot_id);
            state.last_stuck_check = now;
        }
    }

    /// Records a bot decision
    pub fn record_decision(&mut self, bot_id: PlayerId, decision_type: &str, context: &str) {
        let Some(state) = self.bot_states.get_mut(&bot_id) else {
            return;
        };

        let now = Instant::now();
        
        state.decision_history.push_back(BotDecision {
            timestamp: now,
            decision_type: decision_type.to_string(),
            context: context.to_string(),
        });

        // Keep last 100 decisions
        while state.decision_history.len() > 100 {
            state.decision_history.pop_front();
        }

        // Count action types
        *state.action_counts.entry(decision_type.to_string()).or_insert(0) += 1;
    }

    /// Records pathfinding failure
    pub fn record_pathfinding_failure(&mut self, bot_id: PlayerId) {
        if let Some(state) = self.bot_states.get_mut(&bot_id) {
            state.pathfinding_failures += 1;
        }
    }

    /// Validates a bot's behavior
    pub fn validate_bot(&self, bot_id: PlayerId) -> BotValidationReport {
        let Some(state) = self.bot_states.get(&bot_id) else {
            return BotValidationReport {
                bot_id,
                is_valid: false,
                issues: vec![BotIssue {
                    issue_type: BotIssueType::SpawnFailure,
                    severity: BotIssueSeverity::Critical,
                    description: "Bot not registered in validator".to_string(),
                    recommendation: "Check bot spawn logic".to_string(),
                }],
                health_score: 0.0,
            };
        };

        let mut issues = Vec::new();
        let mut health_score = 100.0;

        // Check for stuck bot
        if state.stuck_count > 3 {
            issues.push(BotIssue {
                issue_type: BotIssueType::Stuck,
                severity: BotIssueSeverity::Error,
                description: format!("Bot stuck {} times", state.stuck_count),
                recommendation: "Check navigation mesh and obstacle avoidance".to_string(),
            });
            health_score -= 30.0;
        }

        // Check pathfinding failures
        if state.pathfinding_failures > self.thresholds.max_pathfinding_failures {
            issues.push(BotIssue {
                issue_type: BotIssueType::PathfindingFailure,
                severity: BotIssueSeverity::Warning,
                description: format!("{} pathfinding failures", state.pathfinding_failures),
                recommendation: "Validate navmesh and pathfinder configuration".to_string(),
            });
            health_score -= 20.0;
        }

        // Check activity level
        let age = Instant::now() - state.spawn_time;
        let actions_per_minute = if age.as_secs() > 0 {
            state.decision_history.len() as f32 / age.as_secs_f32() * 60.0
        } else {
            0.0
        };

        if actions_per_minute < self.thresholds.min_actions_per_minute as f32 {
            issues.push(BotIssue {
                issue_type: BotIssueType::Inactive,
                severity: BotIssueSeverity::Warning,
                description: format!("Low activity: {:.1} actions/min", actions_per_minute),
                recommendation: "Check bot AI decision rate".to_string(),
            });
            health_score -= 15.0;
        }

        // Check for decision loops
        if let Some(issue) = self.detect_decision_loop(state) {
            issues.push(issue);
            health_score -= 25.0;
        }

        BotValidationReport {
            bot_id,
            is_valid: issues.is_empty(),
            issues,
            health_score: health_score.max(0.0),
        }
    }

    fn check_if_stuck(&mut self, bot_id: PlayerId) {
        let Some(state) = self.bot_states.get_mut(&bot_id) else {
            return;
        };

        if state.position_history.len() < 10 {
            return;
        }

        // Calculate total distance moved in last 10 positions
        let recent: Vec<_> = state.position_history.iter().rev().take(10).collect();
        let mut total_distance = 0.0;
        
        for i in 1..recent.len() {
            total_distance += recent[i].1.distance_to(&recent[i-1].1);
        }

        // If moved less than 10 units in 5 seconds, consider stuck
        if total_distance < 10.0 {
            state.stuck_count += 1;
        } else {
            state.stuck_count = state.stuck_count.saturating_sub(1);
        }
    }

    fn detect_decision_loop(&self, state: &BotState) -> Option<BotIssue> {
        if state.decision_history.len() < 20 {
            return None;
        }

        let recent: Vec<_> = state.decision_history.iter().rev().take(20).collect();
        
        // Check for repeating pattern of 3-5 decisions
        for pattern_len in 3..=5 {
            if recent.len() >= pattern_len * 3 {
                let pattern: Vec<_> = recent.iter().take(pattern_len)
                    .map(|d| d.decision_type.clone())
                    .collect();
                
                let mut repeats = 0;
                for chunk in recent.chunks(pattern_len) {
                    if chunk.len() == pattern_len {
                        let matches = chunk.iter().zip(&pattern)
                            .all(|(a, b)| a.decision_type == *b);
                        if matches {
                            repeats += 1;
                        }
                    }
                }
                
                if repeats >= 3 {
                    return Some(BotIssue {
                        issue_type: BotIssueType::DecisionLoop,
                        severity: BotIssueSeverity::Warning,
                        description: format!("Detected decision loop: {:?}", pattern),
                        recommendation: "Add randomness to bot decision making".to_string(),
                    });
                }
            }
        }

        None
    }

    /// Validates all bots and returns aggregate stats
    pub fn validate_all_bots(&self) -> BotValidationSummary {
        let mut total_bots = 0;
        let mut valid_bots = 0;
        let mut total_issues = 0;
        let mut total_health = 0.0;

        for bot_id in self.bot_states.keys() {
            let report = self.validate_bot(*bot_id);
            total_bots += 1;
            
            if report.is_valid {
                valid_bots += 1;
            }
            
            total_issues += report.issues.len();
            total_health += report.health_score;
        }

        BotValidationSummary {
            total_bots,
            valid_bots,
            invalid_bots: total_bots - valid_bots,
            total_issues,
            average_health: if total_bots > 0 { total_health / total_bots as f32 } else { 0.0 },
        }
    }
}

#[derive(Debug, Clone)]
pub struct BotValidationSummary {
    pub total_bots: usize,
    pub valid_bots: usize,
    pub invalid_bots: usize,
    pub total_issues: usize,
    pub average_health: f32,
}

impl Default for BotValidationThresholds {
    fn default() -> Self {
        Self {
            max_stuck_duration: Duration::from_secs(10),
            max_pathfinding_failures: 5,
            min_actions_per_minute: 30,
            min_position_variance: 100.0,
        }
    }
}
```



---

## 7. Automated Test Suite

### 7.1 Unit Tests

```rust
// server/src/validation/tests/unit.rs

#[cfg(test)]
mod integrity_tests {
    use super::*;
    use crate::validation::integrity::position::*;
    use crate::core::types::*;

    #[test]
    fn test_position_validation_normal_movement() {
        let mut validator = PositionValidator::new();
        let player_id = PlayerId(1);
        
        // Simulate normal movement at base speed
        let pos1 = Position { x: 0.0, y: 0.0 };
        let pos2 = Position { x: 2.5, y: 0.0 }; // ~150 units/sec at 60Hz
        let velocity = Velocity { x: 150.0, y: 0.0 };

        validator.validate_position(player_id, pos1, velocity, 1);
        let result = validator.validate_position(player_id, pos2, velocity, 2);
        
        assert!(matches!(result, PositionValidationResult::Valid));
    }

    #[test]
    fn test_position_validation_speed_hack() {
        let mut validator = PositionValidator::new();
        let player_id = PlayerId(1);
        
        // Simulate speed hack (3x normal speed)
        let pos1 = Position { x: 0.0, y: 0.0 };
        let pos2 = Position { x: 7.5, y: 0.0 }; // ~450 units/sec
        let velocity = Velocity { x: 450.0, y: 0.0 };

        validator.validate_position(player_id, pos1, velocity, 1);
        
        // Need 3 consecutive violations
        for tick in 2..=4 {
            let _ = validator.validate_position(player_id, pos2, velocity, tick);
        }
        
        let result = validator.validate_position(player_id, pos2, velocity, 5);
        assert!(matches!(result, PositionValidationResult::SpeedViolation { .. }));
    }

    #[test]
    fn test_position_validation_teleport() {
        let mut validator = PositionValidator::new();
        let player_id = PlayerId(1);
        
        // Instant teleport across map
        let pos1 = Position { x: 0.0, y: 0.0 };
        let pos2 = Position { x: 1000.0, y: 1000.0 };
        let velocity = Velocity { x: 0.0, y: 0.0 };

        validator.validate_position(player_id, pos1, velocity, 1);
        let result = validator.validate_position(player_id, pos2, velocity, 2);
        
        assert!(matches!(result, PositionValidationResult::TeleportDetected { .. }));
    }

    #[test]
    fn test_position_validation_out_of_bounds() {
        let mut validator = PositionValidator::new();
        let player_id = PlayerId(1);
        
        let pos = Position { x: 9999.0, y: 9999.0 };
        let velocity = Velocity { x: 0.0, y: 0.0 };

        let result = validator.validate_position(player_id, pos, velocity, 1);
        
        assert!(matches!(result, PositionValidationResult::OutOfBounds { .. }));
    }
}

#[cfg(test)]
mod performance_tests {
    use super::*;
    use crate::validation::performance::tick_rate::*;
    use std::time::{Duration, Instant};

    #[test]
    fn test_tick_rate_normal() {
        let mut monitor = TickRateMonitor::new(55.0, 3);
        let base = Instant::now();
        
        // Simulate 60Hz
        for i in 0..300 {
            let alert = monitor.record_tick(base + Duration::from_millis(i * 1000 / 60));
            assert!(matches!(alert, TickRateAlert::Normal));
        }
    }

    #[test]
    fn test_tick_rate_degradation() {
        let mut monitor = TickRateMonitor::new(55.0, 3);
        let base = Instant::now();
        
        // Start normal
        for i in 0..60 {
            let _ = monitor.record_tick(base + Duration::from_millis(i * 1000 / 60));
        }
        
        // Degrade to 30Hz
        for i in 60..120 {
            let alert = monitor.record_tick(base + Duration::from_millis(i * 1000 / 30));
            
            if i >= 63 {
                assert!(matches!(alert, TickRateAlert::Warning { .. }));
            }
        }
    }

    #[test]
    fn test_tick_rate_recovery() {
        let mut monitor = TickRateMonitor::new(55.0, 3);
        let base = Instant::now();
        
        // Degrade
        for i in 0..10 {
            let _ = monitor.record_tick(base + Duration::from_millis(i * 1000 / 30));
        }
        
        // Recover
        for i in 10..20 {
            let alert = monitor.record_tick(base + Duration::from_millis(i * 1000 / 60));
            
            if i == 10 {
                assert!(matches!(alert, TickRateAlert::Recovered { .. }));
            }
        }
    }
}

#[cfg(test)]
mod sync_tests {
    use super::*;
    use crate::validation::sync::state_sync::*;
    use crate::core::types::*;

    #[test]
    fn test_state_sync_normal() {
        let mut validator = StateSyncValidator::new(1.0, 100);
        let player_id = PlayerId(1);
        let entity_id = EntityId(1);
        
        let server_pos = Position { x: 100.0, y: 100.0 };
        let client_pos = Position { x: 100.2, y: 100.1 }; // Small divergence
        
        validator.record_server_state(entity_id, server_pos, (0.0, 0.0), 100.0, 0);
        validator.record_client_state(player_id, client_pos, 1);
        
        // Note: validate_sync only runs at intervals, so we may not get immediate result
    }

    #[test]
    fn test_state_sync_major_divergence() {
        let mut validator = StateSyncValidator::new(1.0, 0); // No interval delay
        let player_id = PlayerId(1);
        let entity_id = EntityId(1);
        
        let server_pos = Position { x: 100.0, y: 100.0 };
        let client_pos = Position { x: 200.0, y: 200.0 }; // Major divergence
        
        validator.record_server_state(entity_id, server_pos, (0.0, 0.0), 100.0, 0);
        validator.record_client_state(player_id, client_pos, 1);
        
        let alert = validator.validate_sync(player_id);
        assert!(matches!(alert, Some(SyncAlert::DesyncDetected { .. })));
    }
}
```

### 7.2 Integration Tests

```rust
// server/tests/integration/validation.rs

use massive_game_server::validation::*;
use massive_game_server::core::types::*;
use massive_game_server::world::GameWorld;
use tokio::time::{sleep, Duration};

/// Integration test: Full validation pipeline
#[tokio::test]
async fn test_full_validation_pipeline() {
    // Setup
    let config = ValidationConfig::default();
    let validation = ValidationSystem::new(config);
    
    // Simulate game state
    let player_id = PlayerId(1);
    let pos1 = Position { x: 0.0, y: 0.0 };
    let pos2 = Position { x: 2.5, y: 0.0 };
    let velocity = Velocity { x: 150.0, y: 0.0 };
    
    // Run position validation
    {
        let mut integrity = validation.integrity.write().await;
        integrity.position.validate_position(player_id, pos1, velocity, 1);
        let result = integrity.position.validate_position(player_id, pos2, velocity, 2);
        
        assert!(matches!(result, PositionValidationResult::Valid));
    }
    
    // Run performance check
    {
        let mut perf = validation.performance.write().await;
        let now = std::time::Instant::now();
        
        for i in 0..60 {
            let alert = perf.tick_rate.record_tick(now + Duration::from_millis(i * 1000 / 60));
            assert!(matches!(alert, TickRateAlert::Normal));
        }
    }
}

/// Integration test: Cheat detection scenario
#[tokio::test]
async fn test_cheat_detection_scenario() {
    let config = ValidationConfig::default();
    let validation = ValidationSystem::new(config);
    let player_id = PlayerId(1);
    
    // Simulate speed hack
    {
        let mut integrity = validation.integrity.write().await;
        let pos1 = Position { x: 0.0, y: 0.0 };
        let pos2 = Position { x: 50.0, y: 0.0 }; // Way too far
        let velocity = Velocity { x: 3000.0, y: 0.0 };
        
        integrity.position.validate_position(player_id, pos1, velocity, 1);
        
        // Trigger 3 violations
        for tick in 2..=5 {
            let _ = integrity.position.validate_position(player_id, pos2, velocity, tick);
        }
        
        let result = integrity.position.validate_position(player_id, pos2, velocity, 6);
        
        assert!(matches!(result, PositionValidationResult::SpeedViolation { .. }));
    }
}

/// Integration test: Memory leak detection
#[tokio::test]
async fn test_memory_leak_detection() {
    let mut monitor = MemoryMonitor::new(1.0); // 1 MB/min threshold
    
    // Normal operation
    for _ in 0..10 {
        let alert = monitor.record_sample();
        assert!(matches!(alert, MemoryAlert::Normal));
    }
    
    // Note: Actual memory leak testing requires simulating memory growth
    // which is difficult in a unit test environment
}

/// Integration test: Network latency monitoring
#[tokio::test]
async fn test_network_latency_monitoring() {
    let mut monitor = NetworkLatencyMonitor::new(100, 3);
    let player_id = PlayerId(1);
    
    // Normal latency
    for _ in 0..10 {
        let alert = monitor.record_rtt(player_id, Duration::from_millis(30));
        assert!(alert.is_none());
    }
    
    // High latency
    for _ in 0..5 {
        let alert = monitor.record_rtt(player_id, Duration::from_millis(150));
        // May trigger warning after threshold
    }
}
```

### 7.3 Load Tests

```rust
// server/tests/performance/load_test.rs

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use massive_game_server::validation::*;
use massive_game_server::core::types::*;
use std::time::Duration;

/// Benchmark: Position validation throughput
fn bench_position_validation(c: &mut Criterion) {
    let mut group = c.benchmark_group("position_validation");
    
    for player_count in [10, 100, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("players", player_count),
            player_count,
            |b, &player_count| {
                let mut validator = PositionValidator::new();
                let positions: Vec<_> = (0..player_count)
                    .map(|i| Position { 
                        x: (i as f32) * 10.0, 
                        y: (i as f32) * 10.0 
                    })
                    .collect();
                let velocity = Velocity { x: 150.0, y: 0.0 };
                
                b.iter(|| {
                    for (i, pos) in positions.iter().enumerate() {
                        let player_id = PlayerId(i as u32);
                        black_box(validator.validate_position(
                            player_id, 
                            *pos, 
                            velocity, 
                            i as u64
                        ));
                    }
                });
            },
        );
    }
    
    group.finish();
}

/// Benchmark: Tick rate monitoring
fn bench_tick_rate_monitor(c: &mut Criterion) {
    c.bench_function("tick_rate_1000_samples", |b| {
        let mut monitor = TickRateMonitor::new(55.0, 3);
        let base = std::time::Instant::now();
        
        b.iter(|| {
            for i in 0..1000 {
                black_box(monitor.record_tick(base + Duration::from_millis(i * 1000 / 60)));
            }
        });
    });
}

/// Benchmark: Full validation pipeline
fn bench_full_validation_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_validation");
    
    for player_count in [50, 100, 200].iter() {
        group.bench_with_input(
            BenchmarkId::new("players", player_count),
            player_count,
            |b, &player_count| {
                let config = ValidationConfig::default();
                let validation = ValidationSystem::new(config);
                
                b.iter(|| {
                    // Simulate validation for all players
                    for i in 0..player_count {
                        let player_id = PlayerId(i as u32);
                        let pos = Position { 
                            x: (i as f32) * 5.0, 
                            y: (i as f32) * 5.0 
                        };
                        let velocity = Velocity { x: 150.0, y: 0.0 };
                        
                        black_box(pos);
                        black_box(velocity);
                        black_box(player_id);
                    }
                });
            },
        );
    }
    
    group.finish();
}

criterion_group!(
    benches,
    bench_position_validation,
    bench_tick_rate_monitor,
    bench_full_validation_pipeline
);
criterion_main!(benches);
```

### 7.4 Chaos Tests

```rust
// server/tests/chaos/chaos_test.rs

use massive_game_server::validation::*;
use massive_game_server::core::types::*;
use massive_game_server::world::GameWorld;
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use tokio::time::{sleep, Duration};

/// Chaos test: Random player movement patterns
#[tokio::test]
async fn test_chaos_random_movement() {
    let mut rng = StdRng::seed_from_u64(42);
    let mut validator = PositionValidator::new();
    
    let player_count = 100;
    let tick_count = 1000;
    
    let mut positions: Vec<Position> = (0..player_count)
        .map(|_| Position { 
            x: rng.gen_range(-800.0..800.0), 
            y: rng.gen_range(-600.0..600.0) 
        })
        .collect();
    
    let mut violations = 0;
    let mut teleportations = 0;
    
    for tick in 0..tick_count {
        for (i, pos) in positions.iter_mut().enumerate() {
            // Random movement with occasional teleport
            let is_teleport = rng.gen_bool(0.01); // 1% chance
            
            let new_pos = if is_teleport {
                Position { 
                    x: rng.gen_range(-800.0..800.0), 
                    y: rng.gen_range(-600.0..600.0) 
                }
            } else {
                let dx = rng.gen_range(-5.0..5.0);
                let dy = rng.gen_range(-5.0..5.0);
                Position { 
                    x: (pos.x + dx).clamp(-800.0, 800.0), 
                    y: (pos.y + dy).clamp(-600.0, 600.0) 
                }
            };
            
            let velocity = Velocity { 
                x: (new_pos.x - pos.x) * 60.0, 
                y: (new_pos.y - pos.y) * 60.0 
            };
            
            let result = validator.validate_position(
                PlayerId(i as u32), 
                new_pos, 
                velocity, 
                tick
            );
            
            match result {
                PositionValidationResult::SpeedViolation { .. } => violations += 1,
                PositionValidationResult::TeleportDetected { .. } => teleportations += 1,
                _ => {}
            }
            
            *pos = new_pos;
        }
    }
    
    // We expect some violations due to random teleportation
    assert!(teleportations > 0, "Should detect teleportations");
    println!("Violations: {}, Teleportations: {}", violations, teleportations);
}

/// Chaos test: Sudden server load spike
#[tokio::test]
async fn test_chaos_load_spike() {
    let mut monitor = TickRateMonitor::new(55.0, 3);
    let base = std::time::Instant::now();
    
    // Normal operation
    for i in 0..100 {
        let _ = monitor.record_tick(base + Duration::from_millis(i * 1000 / 60));
    }
    
    // Simulate load spike (slower ticks)
    let mut warnings = 0;
    for i in 100..200 {
        let alert = monitor.record_tick(base + Duration::from_millis(i * 1000 / 30));
        if matches!(alert, TickRateAlert::Warning { .. }) {
            warnings += 1;
        }
    }
    
    assert!(warnings > 0, "Should detect tick rate degradation");
}

/// Chaos test: Rapid connect/disconnect cycles
#[tokio::test]
async fn test_chaos_connect_disconnect() {
    let mut latency_monitor = NetworkLatencyMonitor::new(100, 3);
    let mut rng = StdRng::seed_from_u64(42);
    
    // Simulate players connecting and disconnecting
    for round in 0..100 {
        let player_count = rng.gen_range(10..100);
        
        // Connect players
        for i in 0..player_count {
            let player_id = PlayerId((round * 1000 + i) as u32);
            let rtt = Duration::from_millis(rng.gen_range(20..80));
            latency_monitor.record_rtt(player_id, rtt);
        }
        
        // Check for stale players (simulating disconnects)
        if round % 10 == 0 {
            let alerts = latency_monitor.check_stale_players(Duration::from_secs(1));
            // Some players may be flagged as disconnected
        }
        
        // Update global stats
        latency_monitor.update_global_stats();
    }
    
    let stats = latency_monitor.get_global_stats();
    println!("Player count: {}, Avg latency: {:.1}ms", 
        stats.player_count, stats.average_ms);
}

/// Chaos test: Mixed anomaly patterns
#[tokio::test]
async fn test_chaos_mixed_anomalies() {
    let mut detector = StatisticalAnomalyDetector::new(2.0, 100);
    let mut rng = StdRng::seed_from_u64(42);
    
    // Simulate normal players
    for player_id in 0..50 {
        for _ in 0..100 {
            let accuracy = rng.gen_range(0.3..0.6);
            let reaction = rng.gen_range(150.0..300.0);
            
            detector.record_accuracy(PlayerId(player_id), accuracy);
            detector.record_reaction_time(PlayerId(player_id), reaction);
        }
    }
    
    // Simulate anomalous player (cheater)
    let cheater_id = PlayerId(999);
    for _ in 0..100 {
        let accuracy = rng.gen_range(0.9..0.99); // Suspiciously high
        let reaction = rng.gen_range(50.0..80.0); // Suspiciously fast
        
        detector.record_accuracy(cheater_id, accuracy);
        detector.record_reaction_time(cheater_id, reaction);
    }
    
    // Update baselines
    detector.update_baselines();
    
    // Check for anomalies
    let normal_anomalies = detector.detect_anomalies(PlayerId(1));
    let cheater_anomalies = detector.detect_anomalies(cheater_id);
    
    assert!(normal_anomalies.is_empty() || normal_anomalies.len() < 2,
        "Normal player should have few anomalies");
    assert!(!cheater_anomalies.is_empty(),
        "Cheater should be detected");
    
    println!("Normal player anomalies: {}", normal_anomalies.len());
    println!("Cheater anomalies: {}", cheater_anomalies.len());
}
```

### 7.5 Test Configuration for CI/CD

```yaml
# .github/workflows/validation-tests.yml
name: Validation Tests

on:
  push:
    branches: [main, develop]
  pull_request:
    branches: [main]

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1

jobs:
  unit-tests:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      
      - name: Install Rust
        uses: dtolnay/rust-action@stable
      
      - name: Cache dependencies
        uses: Swatinem/rust-cache@v2
      
      - name: Run unit tests
        run: cargo test --lib validation::tests::unit
      
      - name: Run integrity tests
        run: cargo test --lib validation::integrity
      
      - name: Run performance tests
        run: cargo test --lib validation::performance

  integration-tests:
    runs-on: ubuntu-latest
    needs: unit-tests
    steps:
      - uses: actions/checkout@v4
      
      - name: Install Rust
        uses: dtolnay/rust-action@stable
      
      - name: Cache dependencies
        uses: Swatinem/rust-cache@v2
      
      - name: Run integration tests
        run: cargo test --test integration validation
        timeout-minutes: 10

  chaos-tests:
    runs-on: ubuntu-latest
    needs: unit-tests
    steps:
      - uses: actions/checkout@v4
      
      - name: Install Rust
        uses: dtolnay/rust-action@stable
      
      - name: Run chaos tests
        run: cargo test --test chaos
        timeout-minutes: 15

  benchmark-tests:
    runs-on: ubuntu-latest
    needs: unit-tests
    steps:
      - uses: actions/checkout@v4
      
      - name: Install Rust
        uses: dtolnay/rust-action@stable
      
      - name: Run benchmarks
        run: cargo bench -- validation
        timeout-minutes: 20
      
      - name: Upload benchmark results
        uses: actions/upload-artifact@v4
        with:
          name: benchmark-results
          path: target/criterion/

  coverage:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      
      - name: Install Rust
        uses: dtolnay/rust-action@stable
      
      - name: Install tarpaulin
        run: cargo install cargo-tarpaulin
      
      - name: Generate coverage
        run: cargo tarpaulin --lib --out Xml
      
      - name: Upload coverage
        uses: codecov/codecov-action@v4
        with:
          files: ./cobertura.xml
          fail_ci_if_error: true
```



---

## 8. Alerting System

### 8.1 Alert Manager

```rust
// server/src/validation/alerting/manager.rs

use crate::core::types::PlayerId;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use serde::{Serialize, Deserialize};

/// Central alert management system
pub struct AlertManager {
    /// Active alerts
    active_alerts: HashMap<AlertId, Alert>,
    /// Alert history
    alert_history: Vec<Alert>,
    /// Alert configurations per type
    configs: HashMap<AlertType, AlertConfig>,
    /// Notification channels
    channels: Vec<Box<dyn NotificationChannel>>,
    /// Rate limiting per alert type
    last_alert_time: HashMap<AlertType, Instant>,
}

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct AlertId(String);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Alert {
    pub id: AlertId,
    pub alert_type: AlertType,
    pub severity: AlertSeverity,
    pub title: String,
    pub message: String,
    pub timestamp: Instant,
    pub player_id: Option<PlayerId>,
    pub metadata: HashMap<String, String>,
    pub acknowledged: bool,
    pub resolved: bool,
}

#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum AlertType {
    PositionViolation,
    SpeedHack,
    Teleport,
    HealthMismatch,
    ScoreAnomaly,
    TickRateDrop,
    MemoryLeak,
    HighLatency,
    StateDesync,
    AOIError,
    InvalidHit,
    StatisticalAnomaly,
    ImpossibleAction,
    BotIssue,
    SystemError,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AlertSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Clone, Debug)]
pub struct AlertConfig {
    pub alert_type: AlertType,
    pub enabled: bool,
    pub min_severity: AlertSeverity,
    pub cooldown: Duration,
    pub max_alerts_per_hour: u32,
    pub auto_resolve_after: Option<Duration>,
}

impl AlertManager {
    pub fn new() -> Self {
        let mut configs = HashMap::new();
        
        // Default configurations
        configs.insert(AlertType::PositionViolation, AlertConfig {
            alert_type: AlertType::PositionViolation,
            enabled: true,
            min_severity: AlertSeverity::Warning,
            cooldown: Duration::from_secs(60),
            max_alerts_per_hour: 100,
            auto_resolve_after: Some(Duration::from_secs(300)),
        });
        
        configs.insert(AlertType::SpeedHack, AlertConfig {
            alert_type: AlertType::SpeedHack,
            enabled: true,
            min_severity: AlertSeverity::Error,
            cooldown: Duration::from_secs(30),
            max_alerts_per_hour: 50,
            auto_resolve_after: None,
        });
        
        configs.insert(AlertType::TickRateDrop, AlertConfig {
            alert_type: AlertType::TickRateDrop,
            enabled: true,
            min_severity: AlertSeverity::Warning,
            cooldown: Duration::from_secs(120),
            max_alerts_per_hour: 10,
            auto_resolve_after: Some(Duration::from_secs(600)),
        });
        
        configs.insert(AlertType::MemoryLeak, AlertConfig {
            alert_type: AlertType::MemoryLeak,
            enabled: true,
            min_severity: AlertSeverity::Error,
            cooldown: Duration::from_secs(300),
            max_alerts_per_hour: 5,
            auto_resolve_after: None,
        });
        
        Self {
            active_alerts: HashMap::new(),
            alert_history: Vec::new(),
            configs,
            channels: Vec::new(),
            last_alert_time: HashMap::new(),
        }
    }

    /// Adds a notification channel
    pub fn add_channel(&mut self, channel: Box<dyn NotificationChannel>) {
        self.channels.push(channel);
    }

    /// Creates and dispatches an alert
    pub async fn create_alert(
        &mut self,
        alert_type: AlertType,
        severity: AlertSeverity,
        title: &str,
        message: &str,
        player_id: Option<PlayerId>,
        metadata: HashMap<String, String>,
    ) -> Option<AlertId> {
        // Check if alert type is enabled
        let config = self.configs.get(&alert_type)?;
        
        if !config.enabled {
            return None;
        }
        
        // Check minimum severity
        if !self.meets_severity_threshold(&severity, &config.min_severity) {
            return None;
        }
        
        // Check cooldown
        if let Some(last_time) = self.last_alert_time.get(&alert_type) {
            if Instant::now() - *last_time < config.cooldown {
                return None;
            }
        }
        
        // Check rate limit
        let recent_count = self.count_recent_alerts(&alert_type, Duration::from_secs(3600));
        if recent_count >= config.max_alerts_per_hour {
            return None;
        }
        
        // Create alert
        let alert_id = AlertId(format!("{}-{}", 
            alert_type_to_string(&alert_type),
            chrono::Utc::now().timestamp_millis()
        ));
        
        let alert = Alert {
            id: alert_id.clone(),
            alert_type: alert_type.clone(),
            severity: severity.clone(),
            title: title.to_string(),
            message: message.to_string(),
            timestamp: Instant::now(),
            player_id,
            metadata,
            acknowledged: false,
            resolved: false,
        };
        
        // Store alert
        self.active_alerts.insert(alert_id.clone(), alert.clone());
        self.alert_history.push(alert.clone());
        self.last_alert_time.insert(alert_type, Instant::now());
        
        // Dispatch to channels
        for channel in &self.channels {
            if channel.should_send(&alert) {
                if let Err(e) = channel.send(&alert).await {
                    eprintln!("Failed to send alert: {}", e);
                }
            }
        }
        
        Some(alert_id)
    }

    /// Acknowledges an alert
    pub fn acknowledge_alert(&mut self, alert_id: &AlertId) -> bool {
        if let Some(alert) = self.active_alerts.get_mut(alert_id) {
            alert.acknowledged = true;
            true
        } else {
            false
        }
    }

    /// Resolves an alert
    pub fn resolve_alert(&mut self, alert_id: &AlertId) -> bool {
        if let Some(alert) = self.active_alerts.get_mut(alert_id) {
            alert.resolved = true;
            true
        } else {
            false
        }
    }

    /// Gets all active alerts
    pub fn get_active_alerts(&self) -> Vec<&Alert> {
        self.active_alerts.values().filter(|a| !a.resolved).collect()
    }

    /// Gets alerts by type
    pub fn get_alerts_by_type(&self, alert_type: &AlertType) -> Vec<&Alert> {
        self.active_alerts.values()
            .filter(|a| a.alert_type == *alert_type && !a.resolved)
            .collect()
    }

    /// Gets alerts by severity
    pub fn get_alerts_by_severity(&self, min_severity: &AlertSeverity) -> Vec<&Alert> {
        self.active_alerts.values()
            .filter(|a| !a.resolved && self.meets_severity_threshold(&a.severity, min_severity))
            .collect()
    }

    /// Auto-resolves expired alerts
    pub fn auto_resolve_expired(&mut self) {
        let now = Instant::now();
        let to_resolve: Vec<_> = self.active_alerts.iter()
            .filter(|(_, alert)| {
                if alert.resolved {
                    return false;
                }
                
                if let Some(config) = self.configs.get(&alert.alert_type) {
                    if let Some(auto_resolve_after) = config.auto_resolve_after {
                        return now - alert.timestamp > auto_resolve_after;
                    }
                }
                false
            })
            .map(|(id, _)| id.clone())
            .collect();
        
        for id in to_resolve {
            self.resolve_alert(&id);
        }
    }

    fn meets_severity_threshold(&self, actual: &AlertSeverity, min: &AlertSeverity) -> bool {
        use AlertSeverity::*;
        match (actual, min) {
            (Critical, _) => true,
            (Error, Error) | (Error, Warning) | (Error, Info) => true,
            (Warning, Warning) | (Warning, Info) => true,
            (Info, Info) => true,
            _ => false,
        }
    }

    fn count_recent_alerts(&self, alert_type: &AlertType, window: Duration) -> u32 {
        let cutoff = Instant::now() - window;
        
        self.alert_history.iter()
            .filter(|a| a.alert_type == *alert_type && a.timestamp > cutoff)
            .count() as u32
    }
}

fn alert_type_to_string(alert_type: &AlertType) -> String {
    format!("{:?}", alert_type).to_lowercase()
}

#[async_trait::async_trait]
pub trait NotificationChannel: Send + Sync {
    fn should_send(&self, alert: &Alert) -> bool;
    async fn send(&self, alert: &Alert) -> Result<(), Box<dyn std::error::Error>>;
}
```

### 8.2 Notification Channels

```rust
// server/src/validation/alerting/channels.rs

use super::manager::{NotificationChannel, Alert, AlertSeverity};
use reqwest::Client;
use serde_json::json;

/// Webhook notification channel (Discord, Slack, etc.)
pub struct WebhookChannel {
    client: Client,
    url: String,
    min_severity: AlertSeverity,
}

impl WebhookChannel {
    pub fn new(url: String, min_severity: AlertSeverity) -> Self {
        Self {
            client: Client::new(),
            url,
            min_severity,
        }
    }

    fn format_discord_embed(&self, alert: &Alert) -> serde_json::Value {
        let color = match alert.severity {
            AlertSeverity::Info => 0x3498db,    // Blue
            AlertSeverity::Warning => 0xf1c40f, // Yellow
            AlertSeverity::Error => 0xe74c3c,   // Red
            AlertSeverity::Critical => 0x8e44ad, // Purple
        };

        json!({
            "embeds": [{
                "title": &alert.title,
                "description": &alert.message,
                "color": color,
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "fields": [
                    {
                        "name": "Type",
                        "value": format!("{:?}", alert.alert_type),
                        "inline": true
                    },
                    {
                        "name": "Severity",
                        "value": format!("{:?}", alert.severity),
                        "inline": true
                    },
                    {
                        "name": "Player",
                        "value": alert.player_id.map(|p| format!("{:?}", p)).unwrap_or_else(|| "N/A".to_string()),
                        "inline": true
                    }
                ]
            }]
        })
    }
}

#[async_trait::async_trait]
impl NotificationChannel for WebhookChannel {
    fn should_send(&self, alert: &Alert) -> bool {
        use AlertSeverity::*;
        match (&alert.severity, &self.min_severity) {
            (Critical, _) => true,
            (Error, Error) | (Error, Warning) | (Error, Info) => true,
            (Warning, Warning) | (Warning, Info) => true,
            (Info, Info) => true,
            _ => false,
        }
    }

    async fn send(&self, alert: &Alert) -> Result<(), Box<dyn std::error::Error>> {
        let payload = self.format_discord_embed(alert);
        
        self.client
            .post(&self.url)
            .json(&payload)
            .send()
            .await?;
        
        Ok(())
    }
}

/// Email notification channel
pub struct EmailChannel {
    smtp_server: String,
    from_address: String,
    to_addresses: Vec<String>,
    min_severity: AlertSeverity,
}

impl EmailChannel {
    pub fn new(
        smtp_server: String,
        from_address: String,
        to_addresses: Vec<String>,
        min_severity: AlertSeverity,
    ) -> Self {
        Self {
            smtp_server,
            from_address,
            to_addresses,
            min_severity,
        }
    }
}

#[async_trait::async_trait]
impl NotificationChannel for EmailChannel {
    fn should_send(&self, alert: &Alert) -> bool {
        matches!(alert.severity, AlertSeverity::Error | AlertSeverity::Critical)
    }

    async fn send(&self, alert: &Alert) -> Result<(), Box<dyn std::error::Error>> {
        // Implementation would use lettre or similar SMTP library
        // For now, just log
        println!("[EMAIL] Would send alert to {:?}: {}", 
            self.to_addresses, alert.title);
        Ok(())
    }
}

/// In-game notification channel
pub struct InGameChannel {
    min_severity: AlertSeverity,
}

impl InGameChannel {
    pub fn new(min_severity: AlertSeverity) -> Self {
        Self { min_severity }
    }
}

#[async_trait::async_trait]
impl NotificationChannel for InGameChannel {
    fn should_send(&self, alert: &Alert) -> bool {
        // Only send critical alerts in-game
        matches!(alert.severity, AlertSeverity::Critical)
    }

    async fn send(&self, alert: &Alert) -> Result<(), Box<dyn std::error::Error>> {
        // Would integrate with game's messaging system
        println!("[IN-GAME] Critical alert: {}", alert.message);
        Ok(())
    }
}

/// Metrics/logging channel for observability
pub struct MetricsChannel;

#[async_trait::async_trait]
impl NotificationChannel for MetricsChannel {
    fn should_send(&self, _alert: &Alert) -> bool {
        true // Log all alerts
    }

    async fn send(&self, alert: &Alert) -> Result<(), Box<dyn std::error::Error>> {
        // Log to structured logging system
        tracing::info!(
            alert_id = %alert.id.0,
            alert_type = ?alert.alert_type,
            severity = ?alert.severity,
            title = %alert.title,
            player_id = ?alert.player_id,
            "Validation alert triggered"
        );
        
        // Could also send to metrics system (Prometheus, etc.)
        // metrics::counter!("validation_alerts_total", 
        //     "type" => format!("{:?}", alert.alert_type),
        //     "severity" => format!("{:?}", alert.severity)
        // ).increment();
        
        Ok(())
    }
}
```

### 8.3 Alert Dashboard Integration

```rust
// server/src/validation/alerting/dashboard.rs

use super::manager::{AlertManager, Alert, AlertSeverity, AlertType};
use axum::{
    extract::State,
    response::Json,
    routing::get,
    Router,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::Serialize;

/// Dashboard API for alert visualization
pub fn dashboard_routes(alert_manager: Arc<RwLock<AlertManager>>) -> Router {
    Router::new()
        .route("/api/alerts/active", get(get_active_alerts))
        .route("/api/alerts/summary", get(get_alert_summary))
        .route("/api/alerts/history", get(get_alert_history))
        .route("/api/alerts/stats", get(get_alert_stats))
        .with_state(alert_manager)
}

#[derive(Serialize)]
struct AlertSummary {
    total_active: usize,
    by_severity: SeverityCounts,
    by_type: TypeCounts,
    recent_count_1h: u32,
    recent_count_24h: u32,
}

#[derive(Serialize)]
struct SeverityCounts {
    info: usize,
    warning: usize,
    error: usize,
    critical: usize,
}

#[derive(Serialize)]
struct TypeCounts {
    position_violation: usize,
    speed_hack: usize,
    tick_rate_drop: usize,
    memory_leak: usize,
    high_latency: usize,
    state_desync: usize,
    other: usize,
}

#[derive(Serialize)]
struct AlertStats {
    total_alerts_all_time: usize,
    average_resolution_time_seconds: f32,
    most_common_type: String,
    peak_alerts_per_hour: u32,
}

async fn get_active_alerts(
    State(manager): State<Arc<RwLock<AlertManager>>>,
) -> Json<Vec<AlertDto>> {
    let manager = manager.read().await;
    let alerts: Vec<AlertDto> = manager.get_active_alerts()
        .into_iter()
        .map(AlertDto::from)
        .collect();
    
    Json(alerts)
}

async fn get_alert_summary(
    State(manager): State<Arc<RwLock<AlertManager>>>,
) -> Json<AlertSummary> {
    let manager = manager.read().await;
    let active = manager.get_active_alerts();
    
    let summary = AlertSummary {
        total_active: active.len(),
        by_severity: count_by_severity(&active),
        by_type: count_by_type(&active),
        recent_count_1h: 0, // Would calculate from history
        recent_count_24h: 0,
    };
    
    Json(summary)
}

async fn get_alert_history(
    State(_manager): State<Arc<RwLock<AlertManager>>>,
) -> Json<Vec<AlertDto>> {
    // Would return paginated history
    Json(vec![])
}

async fn get_alert_stats(
    State(_manager): State<Arc<RwLock<AlertManager>>>,
) -> Json<AlertStats> {
    Json(AlertStats {
        total_alerts_all_time: 0,
        average_resolution_time_seconds: 0.0,
        most_common_type: "N/A".to_string(),
        peak_alerts_per_hour: 0,
    })
}

#[derive(Serialize)]
struct AlertDto {
    id: String,
    alert_type: String,
    severity: String,
    title: String,
    message: String,
    timestamp: String,
    player_id: Option<u32>,
    acknowledged: bool,
}

impl From<&Alert> for AlertDto {
    fn from(alert: &Alert) -> Self {
        Self {
            id: alert.id.0.clone(),
            alert_type: format!("{:?}", alert.alert_type),
            severity: format!("{:?}", alert.severity),
            title: alert.title.clone(),
            message: alert.message.clone(),
            timestamp: format!("{:?}", alert.timestamp),
            player_id: alert.player_id.map(|p| p.0),
            acknowledged: alert.acknowledged,
        }
    }
}

fn count_by_severity(alerts: &[&Alert]) -> SeverityCounts {
    let mut counts = SeverityCounts {
        info: 0,
        warning: 0,
        error: 0,
        critical: 0,
    };
    
    for alert in alerts {
        match alert.severity {
            AlertSeverity::Info => counts.info += 1,
            AlertSeverity::Warning => counts.warning += 1,
            AlertSeverity::Error => counts.error += 1,
            AlertSeverity::Critical => counts.critical += 1,
        }
    }
    
    counts
}

fn count_by_type(alerts: &[&Alert]) -> TypeCounts {
    let mut counts = TypeCounts {
        position_violation: 0,
        speed_hack: 0,
        tick_rate_drop: 0,
        memory_leak: 0,
        high_latency: 0,
        state_desync: 0,
        other: 0,
    };
    
    for alert in alerts {
        match alert.alert_type {
            AlertType::PositionViolation => counts.position_violation += 1,
            AlertType::SpeedHack => counts.speed_hack += 1,
            AlertType::TickRateDrop => counts.tick_rate_drop += 1,
            AlertType::MemoryLeak => counts.memory_leak += 1,
            AlertType::HighLatency => counts.high_latency += 1,
            AlertType::StateDesync => counts.state_desync += 1,
            _ => counts.other += 1,
        }
    }
    
    counts
}
```



---

## 9. Implementation Guide

### 9.1 Integration with Game Server

```rust
// server/src/server/game_server.rs

use crate::validation::*;
use crate::world::GameWorld;
use crate::network::NetworkManager;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};

pub struct GameServer {
    world: Arc<RwLock<GameWorld>>,
    network: Arc<RwLock<NetworkManager>>,
    validation: Arc<RwLock<ValidationSystem>>,
    config: ServerConfig,
}

impl GameServer {
    pub async fn new(config: ServerConfig) -> Self {
        let validation_config = ValidationConfig {
            max_position_delta: 3.0,
            max_speed_multiplier: 1.08,
            violation_threshold: 3,
            min_tick_rate: 55,
            max_memory_growth_mb_per_min: 1.0,
            max_latency_ms: 100,
            max_sync_latency_ms: 50,
            max_state_divergence: 0.01,
            anomaly_z_score_threshold: 3.0,
            anomaly_window_size: 100,
            alert_cooldown_seconds: 60,
            webhook_url: config.discord_webhook_url.clone(),
        };

        let validation = Arc::new(RwLock::new(ValidationSystem::new(validation_config)));

        // Setup alert channels
        {
            let mut val = validation.write().await;
            
            if let Some(webhook_url) = &config.discord_webhook_url {
                val.alerting.add_channel(Box::new(
                    alerting::channels::WebhookChannel::new(
                        webhook_url.clone(),
                        AlertSeverity::Warning
                    )
                ));
            }
            
            val.alerting.add_channel(Box::new(
                alerting::channels::MetricsChannel
            ));
        }

        Self {
            world: Arc::new(RwLock::new(GameWorld::new())),
            network: Arc::new(RwLock::new(NetworkManager::new())),
            validation,
            config,
        }
    }

    /// Main game loop with validation
    pub async fn run(&self) {
        let mut tick_interval = interval(Duration::from_millis(1000 / 60));
        let mut validation_interval = interval(Duration::from_millis(100));
        let mut metrics_interval = interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                _ = tick_interval.tick() => {
                    self.game_tick().await;
                }
                _ = validation_interval.tick() => {
                    self.run_validation().await;
                }
                _ = metrics_interval.tick() => {
                    self.collect_metrics().await;
                }
            }
        }
    }

    async fn game_tick(&self) {
        let mut world = self.world.write().await;
        
        // Update game state
        world.update().await;
        
        // Record tick for performance monitoring
        let mut validation = self.validation.write().await;
        let alert = validation.performance.tick_rate.record_tick(std::time::Instant::now());
        
        if let TickRateAlert::Warning { current, expected } = alert {
            let _ = validation.alerting.create_alert(
                AlertType::TickRateDrop,
                AlertSeverity::Warning,
                "Tick Rate Degradation",
                &format!("Current: {:.1}Hz, Expected: {:.1}Hz", current, expected),
                None,
                HashMap::new(),
            ).await;
        }
    }

    async fn run_validation(&self) {
        let world = self.world.read().await;
        let mut validation = self.validation.write().await;

        // Validate all player positions
        for (player_id, player) in world.get_players() {
            let result = validation.integrity.position.validate_position(
                player_id,
                player.position,
                player.velocity,
                world.get_tick(),
            );

            match result {
                PositionValidationResult::SpeedViolation { expected, actual } => {
                    let _ = validation.alerting.create_alert(
                        AlertType::SpeedHack,
                        AlertSeverity::Error,
                        "Speed Hack Detected",
                        &format!("Player {:?} exceeded speed limit: {:.1} / {:.1}", 
                            player_id, actual, expected),
                        Some(player_id),
                        HashMap::from([
                            ("expected".to_string(), expected.to_string()),
                            ("actual".to_string(), actual.to_string()),
                        ]),
                    ).await;
                }
                PositionValidationResult::TeleportDetected { from, to, delta } => {
                    let _ = validation.alerting.create_alert(
                        AlertType::PositionViolation,
                        AlertSeverity::Error,
                        "Teleport Detected",
                        &format!("Player {:?} teleported {:.1} units", player_id, delta),
                        Some(player_id),
                        HashMap::from([
                            ("from".to_string(), format!("{:?}", from)),
                            ("to".to_string(), format!("{:?}", to)),
                        ]),
                    ).await;
                }
                _ => {}
            }
        }

        // Validate bot behavior
        for (bot_id, bot) in world.get_bots() {
            validation.anomaly.bot.record_position(bot_id, bot.position);
            
            let report = validation.anomaly.bot.validate_bot(bot_id);
            if !report.is_valid {
                for issue in &report.issues {
                    let _ = validation.alerting.create_alert(
                        AlertType::BotIssue,
                        match issue.severity {
                            BotIssueSeverity::Info => AlertSeverity::Info,
                            BotIssueSeverity::Warning => AlertSeverity::Warning,
                            BotIssueSeverity::Error => AlertSeverity::Error,
                            BotIssueSeverity::Critical => AlertSeverity::Critical,
                        },
                        &format!("Bot Issue: {:?}", issue.issue_type),
                        &issue.description,
                        Some(bot_id),
                        HashMap::from([
                            ("recommendation".to_string(), issue.recommendation.clone()),
                        ]),
                    ).await;
                }
            }
        }
    }

    async fn collect_metrics(&self) {
        let mut validation = self.validation.write().await;
        
        // Record memory sample
        let alert = validation.performance.memory.record_sample();
        
        if let MemoryAlert::Growing { rate, threshold } = alert {
            let _ = validation.alerting.create_alert(
                AlertType::MemoryLeak,
                AlertSeverity::Warning,
                "Memory Growth Detected",
                &format!("Growth rate: {:.2} MB/min (threshold: {:.2})", rate, threshold),
                None,
                HashMap::new(),
            ).await;
        }

        // Update global latency stats
        validation.performance.network.update_global_stats();
        
        // Auto-resolve expired alerts
        validation.alerting.auto_resolve_expired();
    }
}
```

### 9.2 Client-Side Validation Reporter

```rust
// static_client/src/validation_reporter.ts

interface ValidationReport {
    playerId: number;
    fps: number;
    latency: number;
    position: { x: number; y: number };
    timestamp: number;
}

interface ValidationConfig {
    reportIntervalMs: number;
    minFps: number;
    maxLatency: number;
}

export class ClientValidationReporter {
    private config: ValidationConfig;
    private lastReportTime: number = 0;
    private fpsHistory: number[] = [];
    private latencyHistory: number[] = [];

    constructor(config: ValidationConfig = {
        reportIntervalMs: 1000,
        minFps: 30,
        maxLatency: 100
    }) {
        this.config = config;
    }

    /**
     * Records FPS sample
     */
    recordFps(fps: number): void {
        this.fpsHistory.push(fps);
        if (this.fpsHistory.length > 60) {
            this.fpsHistory.shift();
        }

        // Check for low FPS
        if (fps < this.config.minFps) {
            console.warn(`[Validation] Low FPS detected: ${fps}`);
        }
    }

    /**
     * Records latency sample
     */
    recordLatency(latencyMs: number): void {
        this.latencyHistory.push(latencyMs);
        if (this.latencyHistory.length > 60) {
            this.latencyHistory.shift();
        }

        // Check for high latency
        if (latencyMs > this.config.maxLatency) {
            console.warn(`[Validation] High latency detected: ${latencyMs}ms`);
        }
    }

    /**
     * Generates and sends validation report to server
     */
    sendReport(playerId: number, position: { x: number; y: number }): void {
        const now = Date.now();
        
        if (now - this.lastReportTime < this.config.reportIntervalMs) {
            return;
        }

        this.lastReportTime = now;

        const avgFps = this.fpsHistory.length > 0
            ? this.fpsHistory.reduce((a, b) => a + b, 0) / this.fpsHistory.length
            : 60;

        const avgLatency = this.latencyHistory.length > 0
            ? this.latencyHistory.reduce((a, b) => a + b, 0) / this.latencyHistory.length
            : 0;

        const report: ValidationReport = {
            playerId,
            fps: avgFps,
            latency: avgLatency,
            position,
            timestamp: now
        };

        // Send via WebRTC data channel
        this.sendToServer(report);
    }

    private sendToServer(report: ValidationReport): void {
        // Implementation depends on your WebRTC setup
        const message = JSON.stringify({
            type: 'validation_report',
            data: report
        });

        // Send through data channel
        // dataChannel.send(message);
    }

    /**
     * Gets current validation statistics
     */
    getStats(): {
        avgFps: number;
        minFps: number;
        avgLatency: number;
        maxLatency: number;
    } {
        return {
            avgFps: this.fpsHistory.length > 0
                ? this.fpsHistory.reduce((a, b) => a + b, 0) / this.fpsHistory.length
                : 60,
            minFps: this.fpsHistory.length > 0
                ? Math.min(...this.fpsHistory)
                : 60,
            avgLatency: this.latencyHistory.length > 0
                ? this.latencyHistory.reduce((a, b) => a + b, 0) / this.latencyHistory.length
                : 0,
            maxLatency: this.latencyHistory.length > 0
                ? Math.max(...this.latencyHistory)
                : 0
        };
    }
}

// Usage in game loop
const reporter = new ClientValidationReporter();

function gameLoop() {
    const fps = calculateFps();
    reporter.recordFps(fps);
    
    // Send report periodically
    reporter.sendReport(playerId, playerPosition);
    
    requestAnimationFrame(gameLoop);
}
```

### 9.3 Configuration File

```toml
# config/validation.toml

[validation]
enabled = true
log_level = "info"

[validation.integrity]
max_position_delta = 3.0
max_speed_multiplier = 1.08
violation_threshold = 3
max_acceleration_per_tick = 525.0
acceleration_violation_threshold = 3

[validation.performance]
min_tick_rate = 55
max_memory_growth_mb_per_min = 1.0
max_latency_ms = 100
fps_report_interval_ms = 1000

[validation.synchronization]
max_sync_latency_ms = 50
max_state_divergence = 0.01
aoi_check_interval_ms = 100
state_sync_interval_ms = 50

[validation.anomaly]
z_score_threshold = 3.0
window_size = 100
min_samples_for_detection = 20

[validation.anomaly.impossible_actions]
max_shots_per_second = 10
max_headshot_percent = 80.0
max_wallbang_percent = 30.0
max_turn_speed = 360.0

[validation.bot]
max_stuck_duration_seconds = 10
max_pathfinding_failures = 5
min_actions_per_minute = 30
min_position_variance = 100.0

[alerting]
enabled = true
default_cooldown_seconds = 60

[alerting.channels.discord]
enabled = true
webhook_url = "${DISCORD_WEBHOOK_URL}"
min_severity = "warning"

[alerting.channels.email]
enabled = false
smtp_server = "smtp.example.com"
from_address = "alerts@game-server.com"
to_addresses = ["admin@example.com"]
min_severity = "error"

[alerting.channels.in_game]
enabled = true
min_severity = "critical"

[alerting.types.position_violation]
enabled = true
min_severity = "warning"
cooldown_seconds = 60
max_per_hour = 100

[alerting.types.speed_hack]
enabled = true
min_severity = "error"
cooldown_seconds = 30
max_per_hour = 50

[alerting.types.tick_rate_drop]
enabled = true
min_severity = "warning"
cooldown_seconds = 120
max_per_hour = 10
auto_resolve_after_seconds = 600

[alerting.types.memory_leak]
enabled = true
min_severity = "error"
cooldown_seconds = 300
max_per_hour = 5
```

### 9.4 Docker Compose for Validation Stack

```yaml
# docker/validation-stack.yml
version: '3.8'

services:
  game-server:
    build: 
      context: ..
      dockerfile: docker/server.Dockerfile
    environment:
      - RUST_LOG=info
      - VALIDATION_ENABLED=true
      - DISCORD_WEBHOOK_URL=${DISCORD_WEBHOOK_URL}
    volumes:
      - ../config/validation.toml:/app/config/validation.toml:ro
    ports:
      - "8080:8080"
    networks:
      - validation-network

  prometheus:
    image: prom/prometheus:latest
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml:ro
      - prometheus-data:/prometheus
    ports:
      - "9090:9090"
    networks:
      - validation-network

  grafana:
    image: grafana/grafana:latest
    environment:
      - GF_SECURITY_ADMIN_PASSWORD=${GRAFANA_PASSWORD:-admin}
    volumes:
      - grafana-data:/var/lib/grafana
      - ./grafana/dashboards:/etc/grafana/provisioning/dashboards:ro
      - ./grafana/datasources:/etc/grafana/provisioning/datasources:ro
    ports:
      - "3000:3000"
    networks:
      - validation-network
    depends_on:
      - prometheus

  alertmanager:
    image: prom/alertmanager:latest
    volumes:
      - ./alertmanager.yml:/etc/alertmanager/alertmanager.yml:ro
    ports:
      - "9093:9093"
    networks:
      - validation-network

  loki:
    image: grafana/loki:latest
    ports:
      - "3100:3100"
    volumes:
      - ./loki-config.yml:/etc/loki/local-config.yaml:ro
    networks:
      - validation-network

  promtail:
    image: grafana/promtail:latest
    volumes:
      - /var/log:/var/log:ro
      - ./promtail-config.yml:/etc/promtail/config.yml:ro
    networks:
      - validation-network

networks:
  validation-network:
    driver: bridge

volumes:
  prometheus-data:
  grafana-data:
```

### 9.5 Prometheus Configuration

```yaml
# docker/prometheus.yml
global:
  scrape_interval: 15s
  evaluation_interval: 15s

rule_files:
  - "validation_alerts.yml"

alerting:
  alertmanagers:
    - static_configs:
        - targets: ['alertmanager:9093']

scrape_configs:
  - job_name: 'game-server'
    static_configs:
      - targets: ['game-server:8080']
    metrics_path: /metrics
    scrape_interval: 5s

  - job_name: 'prometheus'
    static_configs:
      - targets: ['localhost:9090']

  - job_name: 'node-exporter'
    static_configs:
      - targets: ['node-exporter:9100']
```

```yaml
# docker/validation_alerts.yml
groups:
  - name: validation_alerts
    rules:
      - alert: HighPositionViolationRate
        expr: rate(validation_position_violations_total[5m]) > 0.1
        for: 1m
        labels:
          severity: warning
        annotations:
          summary: "High position violation rate detected"
          description: "{{ $value }} position violations per second"

      - alert: TickRateDrop
        expr: game_server_tick_rate < 55
        for: 30s
        labels:
          severity: critical
        annotations:
          summary: "Server tick rate dropped"
          description: "Current tick rate: {{ $value }} Hz"

      - alert: MemoryLeak
        expr: rate(process_resident_memory_bytes[5m]) > 1048576
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Potential memory leak detected"
          description: "Memory growing at {{ $value }} bytes/second"

      - alert: HighLatency
        expr: histogram_quantile(0.95, rate(player_latency_bucket[5m])) > 100
        for: 2m
        labels:
          severity: warning
        annotations:
          summary: "High player latency detected"
          description: "95th percentile latency: {{ $value }} ms"

      - alert: StateDesync
        expr: rate(validation_state_desync_total[5m]) > 0.05
        for: 1m
        labels:
          severity: warning
        annotations:
          summary: "State desynchronization detected"
          description: "{{ $value }} desyncs per second"

      - alert: BotHealthLow
        expr: bot_health_score < 50
        for: 2m
        labels:
          severity: warning
        annotations:
          summary: "Bot health score low"
          description: "Average bot health: {{ $value }}"

      - alert: AnomalyDetected
        expr: increase(validation_anomalies_total[5m]) > 5
        for: 1m
        labels:
          severity: info
        annotations:
          summary: "Statistical anomalies detected"
          description: "{{ $value }} anomalies in last 5 minutes"
```



---

## 10. Appendix: Threshold Reference

### 10.1 Default Thresholds Summary

| Category | Metric | Default Value | Critical Value | Notes |
|----------|--------|---------------|----------------|-------|
| **Position** | Max Speed Multiplier | 1.08x | 1.15x | Based on PLAYER_BASE_SPEED |
| **Position** | Max Position Delta | 3.0 units | 10.0 units | Slack for network jitter |
| **Position** | Violation Threshold | 3 | 1 | Consecutive violations before alert |
| **Position** | Max Acceleration/Tick | 525 units/s | 700 units/s | PLAYER_BASE_SPEED * 3.5 |
| **Health** | Max Heal per Tick | 0 | 10 | No healing without source |
| **Ammo** | Max Regen per Minute | 30 | 60 | Based on pickup spawn rate |
| **Score** | Max Score Delta | 100 | 200 | Per kill with bonuses |
| **Tick Rate** | Minimum | 55 Hz | 50 Hz | Below 60 Hz is degraded |
| **Tick Rate** | Violation Threshold | 3 | 1 | Consecutive slow ticks |
| **Memory** | Max Growth | 1 MB/min | 5 MB/min | Sustained growth indicates leak |
| **Network** | Max Latency | 100 ms | 200 ms | RTT threshold |
| **Network** | Jitter Threshold | 20 ms | 50 ms | Standard deviation |
| **Sync** | Max Divergence | 0.01 | 0.05 | Position difference ratio |
| **Sync** | Max Sync Latency | 50 ms | 100 ms | Server-to-client delay |
| **AOI** | Min Correctness | 99% | 95% | Visible entity accuracy |
| **Anomaly** | Z-Score Threshold | 3.0 | 4.0 | Statistical outlier detection |
| **Anomaly** | Window Size | 100 | 50 | Sample history size |
| **Actions** | Max Fire Rate | 10/s | 15/s | Weapon-dependent |
| **Actions** | Max Headshot % | 80% | 90% | Even pros rarely exceed 70% |
| **Actions** | Max Turn Speed | 360°/s | 720°/s | Human limit |
| **Bot** | Max Stuck Time | 10s | 30s | Before warning |
| **Bot** | Min Actions/Min | 30 | 15 | Activity threshold |

### 10.2 Environment Variable Overrides

| Variable | Default | Description |
|----------|---------|-------------|
| `MGS_VALIDATION_ENABLED` | `true` | Master switch for validation |
| `MGS_SPEED_HACK_TOLERANCE` | `1.08` | Speed multiplier tolerance |
| `MGS_POSITION_SLACK` | `3.0` | Position delta slack |
| `MGS_MIN_TICK_RATE` | `55` | Minimum acceptable tick rate |
| `MGS_MAX_LATENCY_MS` | `100` | Network latency threshold |
| `MGS_MEMORY_GROWTH_THRESHOLD` | `1.0` | MB per minute |
| `MGS_ANOMALY_Z_SCORE` | `3.0` | Statistical threshold |
| `MGS_ALERT_COOLDOWN_SECONDS` | `60` | Between same-type alerts |
| `MGS_DISCORD_WEBHOOK_URL` | - | Discord notifications |
| `MGS_LOG_VALIDATION` | `false` | Verbose validation logging |

### 10.3 Alert Severity Matrix

| Issue Type | Info | Warning | Error | Critical |
|------------|------|---------|-------|----------|
| Position Delta > 3.0 | | ✓ | | |
| Position Delta > 10.0 | | | ✓ | |
| Speed > 1.08x | | | ✓ | |
| Speed > 1.15x | | | | ✓ |
| Teleport Detected | | | ✓ | |
| Tick Rate 50-55 Hz | | ✓ | | |
| Tick Rate < 50 Hz | | | ✓ | |
| Memory Growth 1-5 MB/min | | ✓ | | |
| Memory Growth > 5 MB/min | | | ✓ | |
| Latency 100-200 ms | | ✓ | | |
| Latency > 200 ms | | | ✓ | |
| State Divergence 1-5% | | ✓ | | |
| State Divergence > 5% | | | ✓ | |
| AOI Error Rate 1-5% | | ✓ | | |
| AOI Error Rate > 5% | | | ✓ | |
| Z-Score 2-3 | ✓ | | | |
| Z-Score 3-4 | | ✓ | | |
| Z-Score 4-5 | | | ✓ | |
| Z-Score > 5 | | | | ✓ |
| Bot Stuck > 10s | | ✓ | | |
| Bot Stuck > 30s | | | ✓ | |

### 10.4 Performance Budget

| Component | Target CPU | Target Memory | Notes |
|-----------|------------|---------------|-------|
| Position Validation | < 1% | < 10 MB | Per 100 players |
| Tick Rate Monitor | < 0.1% | < 1 MB | Fixed cost |
| Memory Monitor | < 0.1% | < 5 MB | Sampling-based |
| Network Monitor | < 0.5% | < 20 MB | Per 100 players |
| State Sync Validation | < 2% | < 50 MB | Per 100 players |
| AOI Validation | < 1% | < 30 MB | Spatial indexing |
| Anomaly Detection | < 1% | < 20 MB | Statistical window |
| Alerting | < 0.5% | < 10 MB | Rate-limited |
| **Total** | **< 6%** | **< 150 MB** | **Per 100 players** |

### 10.5 Test Coverage Requirements

| Module | Unit Tests | Integration | Chaos | Load |
|--------|------------|-------------|-------|------|
| Position Validation | ✓ | ✓ | ✓ | ✓ |
| Health Validation | ✓ | ✓ | | |
| Score Validation | ✓ | ✓ | | |
| Round Validation | ✓ | ✓ | | |
| Tick Rate Monitor | ✓ | ✓ | ✓ | |
| Memory Monitor | ✓ | ✓ | | |
| Network Monitor | ✓ | ✓ | ✓ | ✓ |
| FPS Monitor | ✓ | | | |
| State Sync | ✓ | ✓ | ✓ | ✓ |
| AOI Validation | ✓ | ✓ | ✓ | |
| Projectile Validation | ✓ | ✓ | ✓ | |
| Statistical Anomaly | ✓ | ✓ | ✓ | |
| Impossible Actions | ✓ | ✓ | ✓ | |
| Bot Validation | ✓ | ✓ | ✓ | |
| Alert Manager | ✓ | ✓ | | |
| **Total Coverage** | **100%** | **85%** | **60%** | **50%** |

### 10.6 File Structure

```
server/src/validation/
├── mod.rs                    # Main validation system coordinator
├── config.rs                 # Configuration structures
├── metrics.rs                # Metrics collection
│
├── integrity/                # Game state integrity
│   ├── mod.rs
│   ├── position.rs           # Position/speed validation
│   ├── resources.rs          # Health/ammo validation
│   ├── score.rs              # Score validation
│   └── round.rs              # Round state validation
│
├── performance/              # Performance monitoring
│   ├── mod.rs
│   ├── tick_rate.rs          # Tick rate monitoring
│   ├── memory.rs             # Memory leak detection
│   ├── network.rs            # Network latency monitoring
│   └── client_fps.rs         # Client FPS tracking
│
├── synchronization/          # Sync validation
│   ├── mod.rs
│   ├── state_sync.rs         # Server-client state sync
│   ├── aoi.rs                # AOI correctness
│   └── projectile.rs         # Projectile hit validation
│
├── anomaly/                  # Anomaly detection
│   ├── mod.rs
│   ├── statistical.rs        # Statistical outlier detection
│   ├── impossible_actions.rs # Impossible action detection
│   └── bot_validation.rs     # Bot behavior validation
│
└── alerting/                 # Alerting system
    ├── mod.rs
    ├── manager.rs            # Alert management
    ├── channels.rs           # Notification channels
    └── dashboard.rs          # Dashboard API

server/tests/
├── integration/
│   └── validation.rs         # Integration tests
├── chaos/
│   └── chaos_test.rs         # Chaos tests
└── performance/
    └── load_test.rs          # Load tests

scripts/
├── validation/
│   ├── run_tests.sh          # Test runner
│   ├── chaos_test.sh         # Chaos test runner
│   └── benchmark.sh          # Benchmark runner
└── monitoring/
    ├── setup_prometheus.sh   # Monitoring setup
    └── alert_rules.yml       # Alert rules

docker/
├── validation-stack.yml      # Docker Compose
├── prometheus.yml            # Prometheus config
├── validation_alerts.yml     # Alert rules
└── grafana/
    └── dashboards/
        └── validation.json   # Grafana dashboard
```

### 10.7 API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/alerts/active` | GET | List active alerts |
| `/api/alerts/summary` | GET | Alert summary statistics |
| `/api/alerts/history` | GET | Historical alerts (paginated) |
| `/api/alerts/stats` | GET | Alert statistics |
| `/api/alerts/{id}/acknowledge` | POST | Acknowledge alert |
| `/api/alerts/{id}/resolve` | POST | Resolve alert |
| `/api/validation/integrity` | GET | Integrity check status |
| `/api/validation/performance` | GET | Performance metrics |
| `/api/validation/sync` | GET | Sync validation status |
| `/api/validation/anomaly` | GET | Anomaly detection status |
| `/api/validation/bots` | GET | Bot validation status |
| `/api/metrics` | GET | Prometheus metrics |
| `/health` | GET | Health check endpoint |

### 10.8 Troubleshooting Guide

#### High False Positive Rate

**Symptoms:** Many legitimate players being flagged

**Solutions:**
1. Increase `MGS_SPEED_HACK_TOLERANCE` to 1.10 or 1.15
2. Increase `MGS_POSITION_SLACK` for high-latency regions
3. Adjust violation thresholds higher
4. Review network infrastructure for packet loss

#### Tick Rate Drops

**Symptoms:** Server running below 60 Hz

**Solutions:**
1. Check CPU usage - may need more cores
2. Review validation overhead - disable non-critical checks
3. Reduce player count per shard
4. Optimize hot paths in game logic
5. Enable SIMD optimizations

#### Memory Leak Alerts

**Symptoms:** Continuous memory growth

**Solutions:**
1. Check for unclosed connections
2. Review entity cleanup on player disconnect
3. Verify projectile cleanup
4. Check for circular references
5. Use heap profiling tools

#### State Desync

**Symptoms:** Clients showing different positions than server

**Solutions:**
1. Check network packet loss
2. Increase state sync frequency
3. Review client-side prediction
4. Verify AOI correctness
5. Check for clock drift

#### Bot Issues

**Symptoms:** Bots stuck or behaving incorrectly

**Solutions:**
1. Check navigation mesh integrity
2. Verify pathfinder configuration
3. Review bot spawn points
4. Check for obstacle collisions
5. Increase bot action rate

---

## Conclusion

This comprehensive validation system provides:

- **Real-time cheat detection** with configurable thresholds
- **Performance monitoring** to catch issues before they impact players
- **Synchronization validation** ensuring fair gameplay
- **Anomaly detection** for statistical outliers
- **Automated testing** for continuous quality assurance
- **Integrated alerting** for immediate issue notification

### Next Steps

1. **Phase 1:** Implement core validation modules (position, tick rate, memory)
2. **Phase 2:** Add synchronization and anomaly detection
3. **Phase 3:** Integrate alerting and dashboard
4. **Phase 4:** Expand test coverage and chaos testing
5. **Phase 5:** Production deployment with monitoring

### Support

For issues or questions:
- GitHub Issues: https://github.com/TrebuchetNetwork/massive_game_server/issues
- Documentation: https://docs.trebuchet.network/validation
- Discord: #validation-system channel

---

*Document Version: 1.0*  
*Last Updated: 2026-02-27*  
*Maintained by: Game Server Team*
