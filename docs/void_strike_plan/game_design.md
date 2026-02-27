# VOID STRIKE: CS2-Inspired Space Shooter Game Design Document

## Executive Summary

**VOID STRIKE** is a competitive round-based space shooter that captures Counter-Strike 2's addictive tension through permadeath rounds, economy management, and clutch moments - translated into 2D space combat. Built for the existing Rust server architecture supporting 200v200 players.

---

## 1. CORE GAME LOOP

### 1.1 Round-Based Structure

```
MATCH STRUCTURE (Best of 24 Rounds)
├── First Half: 12 Rounds (Teams swap sides at round 13)
├── Second Half: Up to 12 Rounds
├── Overtime: First to win by 2 rounds (max 6 rounds)
└── Round Win Condition: First to 13 rounds wins
```

### 1.2 The CS2 Tension Translation

| CS2 Element | VOID STRIKE Equivalent |
|-------------|------------------------|
| One-life per round | Ship destruction = round death |
| Bomb plant/defuse | Core Hack/Defense mechanic |
| Economy for guns/shields | Credits for ships/modules |
| Weapon variety | Ship classes with loadouts |
| Clutch 1vX moments | Last ship vs squad scenarios |
| Team coordination | Formation tactics, ability combos |

### 1.3 Core Loop Flow

```
[LOBBY] → [WARMUP: 60s] → [ROUND 1] → [BUY PHASE: 20s] → [COMBAT: 120s] → [END ROUND]
                                              ↓
[POST-ROUND: 10s] ← [ECONOMY UPDATE] ← [WINNER DETERMINED] ← [CORE HACKED/DEFENDED]
       ↓
[REPEAT until match end] → [VICTORY/DEFEAT] → [RANK UPDATE] → [REMATCH VOTE]
```

---

## 2. TENSION MECHANICS (The "CLUTCH" Factor)

### 2.1 Permadeath Round System

**Rule**: When your ship is destroyed, you spectate until the next round. No respawns.

**Why it works**: Creates high stakes for every decision. One mistake ends your round.

### 2.2 The Clutch Multiplier System

```rust
// Server-side clutch detection
struct ClutchSituation {
    alive_allies: u8,      // Your team alive
    alive_enemies: u8,     // Enemy team alive
    clutch_multiplier: f32 // Credit bonus multiplier
}

impl ClutchSituation {
    fn get_multiplier(&self) -> f32 {
        match (self.alive_allies, self.alive_enemies) {
            (1, 2) => 1.25,  // 1v2 = 25% bonus
            (1, 3) => 1.50,  // 1v3 = 50% bonus
            (1, 4) => 2.00,  // 1v4 = 100% bonus
            (1, 5) => 3.00,  // 1v5 = 200% bonus (ACE CLUTCH)
            _ => 1.00
        }
    }
}
```

### 2.3 Audio-Visual Clutch Indicators

| Situation | Audio Cue | Visual Effect |
|-----------|-----------|---------------|
| 1v2 | "It's down to you..." | Subtle heartbeat pulse |
| 1v3 | "Clutch situation!" | Screen edge glow |
| 1v4 | "All eyes on you!" | Slow-motion kill replay |
| 1v5 | "LEGENDARY CLUTCH!" | Full cinematic slowdown |

### 2.4 The "Core" Objective (Bomb Equivalent)

**Core Hacking Mechanics**:
- **Hack Time**: 8 seconds (4 seconds with Hack Module)
- **Defuse Time**: 10 seconds (5 seconds with Defuse Module)
- **Beep Interval**: Every 2 seconds when hacking (audio cue for defenders)
- **Visual**: Core pulses red when being hacked, blue when being defended

### 2.5 Information Asymmetry

```
VISION SYSTEM:
├── Ship Radar: 800 unit radius (shows enemy positions)
├── Line of Sight: Required for targeting
├── Stealth Ships: Invisible on radar until firing
├── Sensor Drones: Deployable vision (300 radius, 15s duration)
└── Communication: Voice/text only - no automated callouts
```

---

## 3. SPACE SHOOTER SPECIFICS

### 3.1 Movement System

```rust
struct ShipMovement {
    max_speed: f32,           // Base max velocity
    acceleration: f32,        // How fast to reach max speed
    rotation_speed: f32,      // Degrees per second
    boost_speed: f32,         // Temporary speed increase
    boost_duration: f32,      // Seconds boost lasts
    boost_cooldown: f32,      // Seconds before boost recharges
    inertia_dampening: f32,   // 0.0 = full drift, 1.0 = instant stop
}
```

**Movement Controls**:
- **W/S**: Thrust forward/backward
- **A/D**: Rotate left/right
- **Shift**: Boost (consumes energy)
- **Space**: Brake/Inertia dampener

### 3.2 Combat Fundamentals

```rust
struct WeaponStats {
    damage: f32,              // Damage per shot
    fire_rate: f32,           // Shots per second
    projectile_speed: f32,    // Units per second
    range: f32,               // Max effective range
    spread: f32,              // Accuracy (0 = perfect)
    ammo_capacity: u32,       // Magazine size
    reload_time: f32,         // Seconds to reload
    energy_cost: f32,         // Energy consumed per shot
}
```

### 3.3 Energy System

```
ENERGY MECHANICS:
├── Max Energy: 100
├── Regeneration: 10/second (when not firing/boosting)
├── Weapons consume energy per shot
├── Boost consumes 25 energy
├── Abilities have varying costs
└── Energy Management = Skill Expression
```

---

## 4. GAME MODES

### 4.1 CORE ASSAULT (Primary Mode - CS2 Bomb Equivalent)

**Objective**: Attackers hack the Core. Defenders prevent hacking.

```
ROUND STRUCTURE:
├── Buy Phase: 20 seconds
├── Round Timer: 120 seconds
├── Core Hack Time: 8 seconds
├── Core Defuse Time: 10 seconds
└── Win Conditions:
    ├── Attackers: Hack Core OR eliminate all defenders
    └── Defenders: Prevent hack for 120s OR eliminate all attackers
```

**Map Layout**: Symmetrical with 3 lanes to Core
- **Alpha Lane**: Wide, open space (sniper/fighter territory)
- **Beta Lane**: Asteroid field with cover (brawler/assassin territory)  
- **Gamma Lane**: Narrow corridor (tank/support territory)

### 4.2 SECTOR CONTROL (CS2 Retake Equivalent)

**Objective**: Teams fight for control of 3 sectors. First to hold all 3 wins.

```
SECTOR CONTROL RULES:
├── 3 Sectors on map (A, B, C)
├── Capture Time: 5 seconds (must stay in sector)
├── Sector Size: 400 unit radius
├── Points: 1 point per second per controlled sector
├── Win: First to 300 points OR eliminate enemy team
└── Respawns: Every 15 seconds at base (different from permadeath)
```

### 4.3 ELIMINATION (Pure Combat Mode)

**Objective**: Last team standing wins. No objectives, pure combat.

```
ELIMINATION RULES:
├── No respawns
├── No buy phase - pre-selected loadouts
├── 5 rounds, first to 3 wins
├── Sudden Death: Sector shrinks over time (battle royale style)
└── Perfect for warm-up and competitive tournaments
```

### 4.4 VOID WARFARE (Large-Scale Mode)

**Objective**: 200v200 territory warfare with multiple objectives.

```
VOID WARFARE RULES:
├── 5 Capture Points across massive map (5000x5000 units)
├── Respawn at controlled points
├── Ticket System: Each team starts with 1000 tickets
├── Ticket Loss: -1 per death, -50 per point lost
├── Win: Deplete enemy tickets OR hold majority for 10 minutes
└── Designed for maximum player count (200v200)
```

---

## 5. ECONOMY SYSTEM

### 5.1 Credit Economy

```
STARTING CREDITS:
├── Pistol Round: 800 credits (everyone)
├── Subsequent Loss: Previous round credits + round income
└── Maximum Credits: 12,000

ROUND INCOME:
├── Round Loss: +2,400 (increases by +500 per consecutive loss, max +3,900)
├── Round Win: +3,200
├── Core Hack: +800 bonus
├── Core Defense: +600 bonus
├── Kill: +300
├── Assist: +150
└── Clutch Bonus: Multiplier applied to all round earnings
```

### 5.2 Ship Pricing

| Ship Class | Cost | Role |
|------------|------|------|
| Scout | 0 (Free) | Fast, weak, good for eco rounds |
| Interceptor | 1,500 | Balanced fighter |
| Gunship | 2,800 | Heavy damage dealer |
| Destroyer | 3,500 | Tank/frontline |
| Stealth | 4,200 | Assassin/infiltrator |
| Carrier | 5,000 | Support/utility |

### 5.3 Module/Weapon Pricing

| Module | Cost | Effect |
|--------|------|--------|
| Basic Laser | 0 (Free) | Standard weapon |
| Plasma Cannon | 800 | Higher damage, slower fire rate |
| Railgun | 2,000 | Sniper weapon, high damage |
| Missile Pod | 1,500 | Homing missiles |
| Shield Generator | 1,200 | +50 shield HP |
| Repair Drone | 900 | Slow health regen |
| Hack Module | 600 | Faster hacking |
| Sensor Array | 700 | Extended radar range |
| Afterburner | 1,000 | Better boost |

### 5.4 Economy Strategy Examples

```
FULL BUY (Round 3+): 8,000+ credits
├── Ship: Gunship (2,800)
├── Primary: Plasma Cannon (800)
├── Secondary: Missile Pod (1,500)
├── Utility: Shield Generator (1,200)
└── Total: 6,300 (save 1,700 for next round)

ECO ROUND (After loss): < 2,000 credits
├── Ship: Scout (0)
├── Primary: Basic Laser (0)
├── Utility: Save all credits
└── Goal: Get kills, plant core, survive

FORCE BUY (Must win round): 3,000-5,000 credits
├── Ship: Interceptor (1,500)
├── Primary: Plasma Cannon (800)
├── Utility: Shield Generator (1,200)
└── Total: 3,500 (all-in)
```

---

## 6. ROUND STRUCTURE

### 6.1 Phase Breakdown

```
COMPLETE ROUND (155 seconds total):

[FREEZE PHASE: 5s]
├── Players frozen, can plan
├── Show enemy ship classes (but not loadouts)
└── Strategic positioning discussion

[BUY PHASE: 20s]
├── Access shop
├── Purchase ships and modules
├── Position on map (within spawn zone)
└── Cannot leave spawn until phase ends

[COMBAT PHASE: 120s]
├── Core objective active
├── Full combat enabled
├── No respawns
└── Timer visible to all

[OVERTIME: +30s if core is being hacked at 0:00]
├── Only if hack in progress
├── Hack must complete or be stopped
└── Combat continues

[END PHASE: 10s]
├── Winner announced
├── MVP shown
├── Economy updated
└── Next round countdown
```

### 6.2 Time Pressure Mechanics

```rust
// Time-based tension modifiers
struct RoundTimer {
    total_time: f32,      // 120 seconds
    
    fn get_tension_modifier(&self, time_remaining: f32) -> f32 {
        match time_remaining {
            t if t > 60.0 => 1.0,    // Normal
            t if t > 30.0 => 1.2,    // +20% credit bonus for actions
            t if t > 10.0 => 1.5,    // +50% credit bonus
            _ => 2.0,                 // +100% credit bonus (final 10s)
        }
    }
}
```

### 6.3 Win Conditions Matrix

| Scenario | Attacker Win | Defender Win |
|----------|--------------|--------------|
| Core Hacked | ✓ | |
| Core Defended (time) | | ✓ |
| All Defenders Dead | ✓ | |
| All Attackers Dead | | ✓ |
| Core Explodes (timer) | ✓ | |

---

## 7. SHIP CLASSES

### 7.1 Class Overview

```
SHIP CLASS TRIANGLE:
        TANK
       (Destroyer)
       /         \
      /           \
     /             \
FIGHTER -------- ASSASSIN
(Interceptor)   (Stealth)
     \             /
      \           /
       \         /
        SUPPORT
        (Carrier)
```

### 7.2 Detailed Ship Specifications

#### SCOUT (Eco Round Ship)
```rust
ShipClass::Scout {
    cost: 0,
    health: 60,
    shield: 20,
    speed: 280,
    acceleration: 180,
    rotation: 220,
    size: 0.7,        // Hitbox multiplier
    energy: 80,
    energy_regen: 12,
    abilities: vec!["Boost", "Evasive Maneuver"],
}
```
**Role**: Eco rounds, information gathering, fast flanks
**Playstyle**: Hit-and-run, don't engage directly

#### INTERCEPTOR (All-Rounder)
```rust
ShipClass::Interceptor {
    cost: 1500,
    health: 100,
    shield: 50,
    speed: 240,
    acceleration: 150,
    rotation: 180,
    size: 1.0,
    energy: 100,
    energy_regen: 10,
    abilities: vec!["Boost", "Barrel Roll"],
}
```
**Role**: Entry fragger, versatile fighter
**Playstyle**: Balanced approach, can adapt to any situation

#### GUNSHIP (Damage Dealer)
```rust
ShipClass::Gunship {
    cost: 2800,
    health: 120,
    shield: 40,
    speed: 180,
    acceleration: 100,
    rotation: 120,
    size: 1.3,
    energy: 120,
    energy_regen: 8,
    abilities: vec!["Overcharge", "Weapon Stabilizer"],
}
```
**Role**: DPS, area denial, holding angles
**Playstyle**: Position carefully, melt enemies from range

#### DESTROYER (Tank)
```rust
ShipClass::Destroyer {
    cost: 3500,
    health: 200,
    shield: 100,
    speed: 140,
    acceleration: 80,
    rotation: 90,
    size: 1.5,
    energy: 80,
    energy_regen: 6,
    abilities: vec!["Shield Wall", "Taunt", "Fortress Mode"],
}
```
**Role**: Frontline, creating space for team
**Playstyle**: Lead pushes, absorb damage, protect carries

#### STEALTH (Assassin)
```rust
ShipClass::Stealth {
    cost: 4200,
    health: 80,
    shield: 30,
    speed: 260,
    acceleration: 160,
    rotation: 200,
    size: 0.8,
    energy: 100,
    energy_regen: 10,
    abilities: vec!["Cloak", "Backstab", "Silent Running"],
    special: "Invisible on radar when not firing",
}
```
**Role**: Flanker, core hacker, pick off isolated enemies
**Playstyle**: Sneak behind lines, hack core, escape

#### CARRIER (Support)
```rust
ShipClass::Carrier {
    cost: 5000,
    health: 90,
    shield: 60,
    speed: 160,
    acceleration: 90,
    rotation: 100,
    size: 1.2,
    energy: 150,
    energy_regen: 15,
    abilities: vec!["Deploy Drone", "Repair Beam", "Shield Bubble", "Resupply"],
}
```
**Role**: Team support, utility, force multiplier
**Playstyle**: Stay alive, enable teammates, control space

### 7.3 Class Counters

```
COUNTER RELATIONSHIPS:
├── Stealth → Gunship (sneak up on stationary target)
├── Gunship → Interceptor (out-damage at range)
├── Interceptor → Stealth (chase down, reveal)
├── Destroyer → All (absorbs damage, creates space)
├── Carrier → Destroyer (sustain the tank)
└── Scout → Carrier (fast enough to hunt support)
```

---

## 8. WEAPON & ABILITY BALANCE

### 8.1 Primary Weapons

| Weapon | Damage | Fire Rate | Range | Speed | Cost | Best For |
|--------|--------|-----------|-------|-------|------|----------|
| Basic Laser | 15 | 8.0/s | 600 | 800 | 0 | All-round |
| Plasma Cannon | 35 | 3.5/s | 500 | 600 | 800 | Burst damage |
| Railgun | 80 | 1.0/s | 1200 | Instant | 2000 | Sniping |
| Scatter Cannon | 12x5 | 2.0/s | 300 | 500 | 1200 | Close range |
| Photon Stream | 8 | 15.0/s | 400 | 700 | 1500 | Sustained DPS |
| Missile Pod | 45 | 1.5/s | 800 | 350 | 1500 | Homing |

### 8.2 Secondary Weapons

| Weapon | Damage | Fire Rate | Special | Cost |
|--------|--------|-----------|---------|------|
| Micro Missiles | 25 | 2.0/s | Small AOE | 600 |
| Ion Cannon | 30 | 1.5/s | Disables shields | 800 |
| Mines | 60 | - | Deployable trap | 500 |
| Drone Swarm | 5x6 | - | Homing drones | 1000 |

### 8.3 Ship Abilities

#### Universal Abilities (All Ships)
```rust
Ability::Boost {
    energy_cost: 25,
    cooldown: 8.0,
    duration: 2.0,
    effect: "+50% speed, leaves trail",
}

Ability::EmergencyRepair {
    energy_cost: 50,
    cooldown: 30.0,
    effect: "Restore 40 HP over 5 seconds",
    restriction: "Only below 30% HP",
}
```

#### Class-Specific Abilities

**DESTROYER**
```rust
Ability::ShieldWall {
    energy_cost: 40,
    cooldown: 20.0,
    duration: 5.0,
    effect: "Frontal shield blocks 90% damage",
}

Ability::FortressMode {
    energy_cost: 60,
    cooldown: 45.0,
    duration: 8.0,
    effect: "-50% speed, +100% damage resistance, immobile",
}
```

**STEALTH**
```rust
Ability::Cloak {
    energy_cost: 35,
    cooldown: 25.0,
    duration: 6.0,
    effect: "Invisible, broken by firing or taking damage",
}

Ability::Backstab {
    energy_cost: 30,
    cooldown: 15.0,
    effect: "Next attack from behind deals 3x damage",
}
```

**CARRIER**
```rust
Ability::DeployDrone {
    energy_cost: 40,
    cooldown: 20.0,
    duration: 30.0,
    effect: "Drone attacks nearest enemy (20 DPS)",
}

Ability::ShieldBubble {
    energy_cost: 50,
    cooldown: 35.0,
    duration: 6.0,
    effect: "AOE shield for allies (radius 200)",
}

Ability::RepairBeam {
    energy_cost: 20,
    cooldown: 0.5,
    effect: "Heal ally for 15 HP/s while channeled",
}
```

### 8.4 Damage Calculation

```rust
fn calculate_damage(
    base_damage: f32,
    attacker_weapon: &Weapon,
    target_ship: &Ship,
    hit_location: HitLocation,
    distance: f32
) -> f32 {
    let mut damage = base_damage;
    
    // Distance falloff
    let effective_range = attacker_weapon.range;
    if distance > effective_range {
        damage *= 0.5; // Beyond effective range
    }
    
    // Hit location modifier
    damage *= match hit_location {
        HitLocation::Front => 1.0,
        HitLocation::Side => 1.1,
        HitLocation::Rear => 1.5,  // Backshots hurt more
    };
    
    // Apply to shield first
    if target_ship.shield > 0 {
        let shield_damage = damage.min(target_ship.shield);
        target_ship.shield -= shield_damage;
        damage -= shield_damage;
    }
    
    // Remaining damage to hull
    damage
}
```

---

## 9. MAP DESIGN PRINCIPLES

### 9.1 Core Assault Map Layout

```
TYPICAL CORE ASSAULT MAP (3000x2000 units):

                    [DEFENDER SPAWN]
                           |
                    [CORE CHAMBER]
                          /|\
                         / | \
                        /  |  \
              [ALPHA]--/   |   \--[GAMMA]
               LANE        |        LANE
                  \        |       /
                   \    [BETA]    /
                    \    LANE    /
                     \     |     /
                      \    |    /
                       \   |   /
                        \  |  /
                     [ATTACKER SPAWN]

LANE CHARACTERISTICS:
├── Alpha: Open space, long sightlines (sniper friendly)
├── Beta: Central, balanced (all ship types viable)
└── Gamma: Tight corridors, lots of cover (close combat)
```

### 9.2 Map Elements

```
ENVIRONMENTAL FEATURES:
├── Asteroids: Breakable cover (200 HP)
├── Nebula Clouds: Reduces visibility, hides from radar
├── Debris Fields: Slow movement by 30%
├── Solar Flares: Periodic damage zones
├── Repair Stations: Restore 50 HP (one-time use)
└── Jump Gates: Teleport between fixed points
```

### 9.3 Spawn Design

```
SPAWN PROTECTION:
├── 5-second invulnerability after spawn
├── Cannot fire during invulnerability
├── Visual indicator (glowing outline)
└── Ends early if you move >200 units from spawn

SPAWN TIMING:
├── Attackers: 3 seconds closer to objectives
├── Defenders: Better defensive positions
└── Balanced for 50% win rate on pistol round
```

### 9.4 Map Pool (Launch)

| Map Name | Theme | Size | Special Feature |
|----------|-------|------|-----------------|
| Asteroid Base | Mining facility | Medium | Destructible asteroids |
| Nebula Station | Space station | Large | Radar interference zones |
| Void Rift | Cosmic anomaly | Small | Gravity wells |
| Derelict Fleet | Ship graveyard | Medium | Cover-rich, flanking routes |
| Solar Forge | Factory near star | Large | Environmental hazards |

---

## 10. ADDICTION LOOP (Retention Mechanics)

### 10.1 The CS2 Addiction Formula

```
CS2 ADDICTION TRANSLATED:

CS2:                    VOID STRIKE:
├── Short rounds        → 2.5 minute rounds
├── Permadeath tension  → One-life rounds
├── Economy decisions   → Ship/module buying
├── Clutch moments      → 1vX multipliers
├── Team coordination   → Formation tactics
├── Skill expression    → Movement + aim
├── Rank progression    → Competitive ladder
└── Skin economy        → Ship cosmetics
```

### 10.2 Progression Systems

#### Ranked Mode
```
RANK TIERS (8 divisions each):
├── Iron (I-IV)      → New players
├── Bronze (I-IV)    → Learning basics
├── Silver (I-IV)    → Understanding economy
├── Gold (I-IV)      → Good mechanics
├── Platinum (I-IV)  → Strong game sense
├── Diamond (I-IV)   → Excellent players
├── Master (I-IV)    → Top 1%
└── Void Legend      → Top 100 players

RANK PROGRESSION:
├── Win = +15-25 RR (Rank Rating)
├── Loss = -10-20 RR
├── Performance bonus = +5 RR (top fragger)
├── Streak bonus = +3 RR per consecutive win (max +9)
└── Demotion protection = 3 loss buffer
```

#### Battle Pass (Seasonal)
```
SEASON STRUCTURE (3 months):
├── Free Track: Ships, modules, basic cosmetics
├── Premium Track ($10): Exclusive skins, effects, titles
├── 100 levels, XP from matches
├── Weekly challenges for bonus XP
└── Prestige levels after 100 (cosmetic only)
```

### 10.3 Daily/Weekly Engagement

```
DAILY SYSTEMS:
├── First Win Bonus: +50% credits
├── Daily Challenges (3):
│   ├── Easy: Play 2 matches (500 XP)
│   ├── Medium: Get 10 kills (1000 XP)
│   └── Hard: Win a clutch round (2000 XP)
└── Login Streak: Bonus increases daily (resets at 7)

WEEKLY SYSTEMS:
├── Weekly Missions (5):
│   ├── Play 20 matches
│   ├── Hack/defuse 10 cores
│   ├── Get 100 kills
│   ├── Win 5 rounds as Stealth
│   └── Deal 50,000 damage
├── Weekly Ranked Games: Required for rank
└── Weekend Events: Double XP, special modes
```

### 10.4 Social Systems

```
SOCIAL FEATURES:
├── Friends List: See online status, invite
├── Parties: Up to 5 players
├── Guilds/Clans: Up to 50 members
│   ├── Guild XP from member games
│   ├── Guild-only leaderboards
│   └── Guild cosmetics unlocks
├── Voice Chat: Team only (no all-chat voice)
├── Replay System: Save and share clips
└── Spectate Friends: Watch live games
```

### 10.5 The "One More Round" Effect

```
PSYCHOLOGICAL TRIGGERS:

1. NEAR-MISS EFFECT
   ├── "We almost had that!"
   ├── Close rounds create desire for redemption
   └── Show round score difference prominently

2. CLUTCH ADDICTION
   ├── Clutch wins release dopamine
   ├── Highlight reel moments
   └── Shareable clips

3. ECONOMY ANTICIPATION
   ├── "Next round I can buy Gunship"
   ├── Saving for big purchase
   └── Risk/reward decisions

4. RANK ANXIETY
   ├── Visible rank progress
   ├── "One more win to promote"
   └── Loss aversion

5. SOCIAL PRESSURE
   ├── Team reliance
   ├── "Don't let teammates down"
   └── Post-game lobby stays together
```

### 10.6 Match Flow Optimization

```
MINIMIZE DOWNTIME:
├── Queue Time Target: < 30 seconds
├── Load Time Target: < 15 seconds
├── Between Rounds: 10 seconds
├── Match End to Next: 15 seconds
└── Total downtime per match: ~3 minutes

MAXIMIZE ENGAGEMENT:
├── Always show progress (XP, challenges)
├── Post-match stats breakdown
├── Highlight personal bests
├── Suggest improvements ("Your aim was 15% better!")
└── Quick rematch option
```

---

## 11. IMPLEMENTATION NOTES

### 11.1 Server Architecture Integration

```rust
// Existing Rust server compatibility

// FlatBuffers message types to add
enum MessageType {
    // Existing...
    
    // New for VOID STRIKE
    BuyRequest,
    BuyResponse,
    RoundStart,
    RoundEnd,
    CoreHackStart,
    CoreHackProgress,
    CoreHackComplete,
    EconomyUpdate,
    ClutchAnnouncement,
}

// AOI considerations for 200v200
const CORE_ASSAULT_MAX_PLAYERS: u32 = 10;  // 5v5
const VOID_WARFARE_MAX_PLAYERS: u32 = 400; // 200v200

// Tick rate recommendations
const ROUND_BASED_TICK_RATE: u32 = 60;  // Precision for competitive
const WARFARE_TICK_RATE: u32 = 30;      // Acceptable for large scale
```

### 11.2 Network Optimization

```
BANDWIDTH OPTIMIZATION:
├── Ship states: Delta compression
├── Weapon fire: Event-based (not continuous)
├── Position updates: 20Hz for distant, 60Hz for nearby
├── Economy: Update only on change
└── Core hack: Reliable delivery

PREDICTION:
├── Client-side movement prediction
├── Server reconciliation
├── Weapon fire client-authoritative (validated server-side)
└── 100ms input buffer for fairness
```

### 11.3 Anti-Cheat Considerations

```
SERVER-SIDE VALIDATION:
├── Position speed checks
├── Damage validation
├── Cooldown enforcement
├── Economy sanity checks
└── Replay system for manual review

DETECTION:
├── Statistical analysis (impossible reaction times)
├── Pattern detection (perfect tracking)
├── Report system with priority queue
└── Automatic flagging for review
```

---

## 12. BALANCE PHILOSOPHY

### 12.1 Core Principles

1. **Skill Over Gear**: A skilled Scout can beat an average Gunship
2. **Rock-Paper-Scissors**: No ship dominates all situations
3. **Economy Matters**: Smart buying beats expensive buying
4. **Teamwork Wins**: Coordinated team beats individual skill
5. **Comebacks Possible**: Never feel completely out of the game

### 12.2 Balance Metrics

```
TARGET METRICS:
├── Round Win Rate (Attackers): 48-52%
├── Pistol Round Win Rate: 50%
├── Eco Round Win Rate: 25-30%
├── Force Buy Win Rate: 35-40%
├── Full Buy Win Rate: 55-60%
├── Average Match Time: 35-45 minutes
├── Average Rounds: 22-26
└── Overtime Frequency: 15-20%
```

### 12.3 Tuning Process

```
BALANCE ITERATION:
1. Collect data (win rates, pick rates, kill rates)
2. Identify outliers (>5% from target)
3. Make small adjustments (5-10% changes)
4. Test for 1 week minimum
5. Evaluate and repeat
```

---

## 13. QUICK REFERENCE

### 13.1 Timing Summary

| Phase | Duration | Notes |
|-------|----------|-------|
| Freeze | 5s | Planning |
| Buy | 20s | Shopping |
| Combat | 120s | Main action |
| Overtime | +30s | If hacking |
| End | 10s | Results |
| **Total/Round** | **~155s** | ~2.5 min |
| **Full Match** | **~40 min** | 24 rounds |

### 13.2 Economy Summary

| Action | Credits |
|--------|---------|
| Starting | 800 |
| Loss Bonus | 2400-3900 |
| Win | 3200 |
| Kill | 300 |
| Assist | 150 |
| Core Hack | +800 |
| Core Defend | +600 |
| Max | 12000 |

### 13.3 Ship Cost Summary

| Ship | Cost | HP+Shield | Speed |
|------|------|-----------|-------|
| Scout | 0 | 80 | 280 |
| Interceptor | 1500 | 150 | 240 |
| Gunship | 2800 | 160 | 180 |
| Destroyer | 3500 | 300 | 140 |
| Stealth | 4200 | 110 | 260 |
| Carrier | 5000 | 150 | 160 |

---

## 14. CONCLUSION

VOID STRIKE captures Counter-Strike 2's addictive tension through:

1. **Permadeath rounds** creating high-stakes moments
2. **Economy system** with meaningful buy decisions
3. **Clutch mechanics** rewarding individual skill
4. **Team coordination** essential for victory
5. **Progression systems** keeping players engaged
6. **Balanced ship classes** offering diverse playstyles

The design leverages the existing Rust server architecture while delivering a competitive experience that keeps players coming back for "just one more round."

---

*Document Version: 1.0*
*Last Updated: 2024*
*Designed for: Trebuchet Network Massive Game Server*
