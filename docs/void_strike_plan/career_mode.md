# Space Shooter Career Mode Design
## CS2 Premier-Style Competitive Progression System

---

## 1. RANK SYSTEM

### 1.1 Tier Structure (CS2 Premier-Style)

The rank system uses a tiered structure with 18 ranks across 6 major divisions. Each rank has 3 tiers (I, II, III) except the top rank.

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           RANK HIERARCHY                                │
├─────────────────────────────────────────────────────────────────────────┤
│  RANK NAME          │ MMR RANGE    │ TIER ICON    │ COLOR CODE          │
├─────────────────────┼──────────────┼──────────────┼─────────────────────┤
│  ★ Cosmic Legend    │ 30,000+      │ Crown        │ #FFD700 (Gold)      │
│  ★ Galactic Elite   │ 25,000-29,999│ Diamond+Star │ #B9F2FF (Diamond)   │
│  ★ Void Master      │ 22,000-24,999│ Diamond      │ #00CED1 (Cyan)      │
│  ★ Nebula Champion  │ 19,000-21,999│ Platinum+    │ #E5E4E2 (Platinum)  │
│  ★ Star Commander   │ 16,000-18,999│ Platinum     │ #A0A0A0 (Silver)    │
│  ★ Solar Captain    │ 13,000-15,999│ Gold+        │ #FFD700 (Gold)      │
│  ★ Meteor Hunter    │ 10,000-12,999│ Gold         │ #DAA520 (Goldenrod) │
│  ★ Comet Striker    │ 7,500-9,999  │ Silver+      │ #C0C0C0 (Silver)    │
│  ★ Asteroid Pilot   │ 5,000-7,499  │ Silver       │ #A9A9A9 (DarkSilver)│
│  ★ Space Cadet      │ 3,000-4,999  │ Bronze+      │ #CD7F32 (Bronze)    │
│  ★ Rookie Flyer     │ 1,500-2,999  │ Bronze       │ #8B4513 (SaddleBrown│
│  ★ Trainee          │ 0-1,499      │ Iron         │ #708090 (SlateGray) │
└─────────────────────────────────────────────────────────────────────────┘
```

### 1.2 Detailed Rank Breakdown

| Division | Rank | Tier | MMR Min | MMR Max | Promotion Buffer | Demotion Buffer |
|----------|------|------|---------|---------|------------------|-----------------|
| **Cosmic** | Legend | - | 30,000 | ∞ | N/A | 500 |
| **Galactic** | Elite | I | 28,000 | 29,999 | 500 | 500 |
| | | II | 26,500 | 27,999 | 500 | 500 |
| | | III | 25,000 | 26,499 | 500 | 500 |
| **Void** | Master | I | 23,500 | 24,999 | 400 | 400 |
| | | II | 22,750 | 23,499 | 400 | 400 |
| | | III | 22,000 | 22,749 | 400 | 400 |
| **Nebula** | Champion | I | 20,500 | 21,999 | 400 | 400 |
| | | II | 19,750 | 20,499 | 400 | 400 |
| | | III | 19,000 | 19,749 | 400 | 400 |
| **Star** | Commander | I | 17,500 | 18,999 | 300 | 300 |
| | | II | 16,750 | 17,499 | 300 | 300 |
| | | III | 16,000 | 16,749 | 300 | 300 |
| **Solar** | Captain | I | 14,500 | 15,999 | 300 | 300 |
| | | II | 13,750 | 14,499 | 300 | 300 |
| | | III | 13,000 | 13,749 | 300 | 300 |
| **Meteor** | Hunter | I | 12,000 | 12,999 | 250 | 250 |
| | | II | 11,000 | 11,999 | 250 | 250 |
| | | III | 10,000 | 10,999 | 250 | 250 |
| **Comet** | Striker | I | 9,000 | 9,999 | 250 | 250 |
| | | II | 8,250 | 8,999 | 250 | 250 |
| | | III | 7,500 | 8,249 | 250 | 250 |
| **Asteroid** | Pilot | I | 6,500 | 7,499 | 200 | 200 |
| | | II | 5,750 | 6,499 | 200 | 200 |
| | | III | 5,000 | 5,749 | 200 | 200 |
| **Space** | Cadet | I | 4,000 | 4,999 | 200 | 200 |
| | | II | 3,500 | 3,999 | 200 | 200 |
| | | III | 3,000 | 3,499 | 200 | 200 |
| **Rookie** | Flyer | I | 2,250 | 2,999 | 150 | 150 |
| | | II | 1,875 | 2,249 | 150 | 150 |
| | | III | 1,500 | 1,874 | 150 | 150 |
| **Trainee** | - | I | 750 | 1,499 | 100 | 0 |
| | | II | 375 | 749 | 100 | 0 |
| | | III | 0 | 374 | 100 | 0 |

### 1.3 Promotion/Demotion Mechanics

```rust
// server/src/ranking/rank_system.rs

pub struct RankSystem {
    promotion_buffer: HashMap<Rank, i32>,
    demotion_buffer: HashMap<Rank, i32>,
}

impl RankSystem {
    /// Check if player qualifies for promotion
    pub fn check_promotion(&self, player: &PlayerStats) -> Option<Rank> {
        let current_rank = player.current_rank;
        let current_mmr = player.mmr;
        let buffer = self.promotion_buffer.get(&current_rank).copied().unwrap_or(0);
        
        // Must have minimum games played in current rank
        if player.games_in_current_rank < 5 {
            return None;
        }
        
        // Must have positive win rate in current rank (≥50%)
        if player.win_rate_in_rank < 0.50 {
            return None;
        }
        
        // Check if MMR exceeds next rank threshold + buffer
        if let Some(next_rank) = current_rank.next() {
            let required_mmr = next_rank.mmr_threshold() + buffer;
            if current_mmr >= required_mmr {
                return Some(next_rank);
            }
        }
        
        None
    }
    
    /// Check if player should be demoted
    pub fn check_demotion(&self, player: &PlayerStats) -> Option<Rank> {
        let current_rank = player.current_rank;
        let current_mmr = player.mmr;
        let buffer = self.demotion_buffer.get(&current_rank).copied().unwrap_or(0);
        
        // Check if MMR falls below current rank threshold - buffer
        let min_mmr = current_rank.mmr_threshold() - buffer;
        if current_mmr < min_mmr {
            return current_rank.previous();
        }
        
        None
    }
}
```

### 1.4 MMR Calculation Formula

```rust
// Base MMR change calculation
pub fn calculate_mmr_change(
    player_mmr: i32,
    opponent_avg_mmr: i32,
    match_result: MatchResult,
    performance_score: f32,  // 0.0 - 2.0 multiplier
) -> i32 {
    // Expected win probability (Elo-based)
    let expected = 1.0 / (1.0 + 10f32.powf((opponent_avg_mmr - player_mmr) as f32 / 400.0));
    
    // Actual result (1.0 = win, 0.5 = draw, 0.0 = loss)
    let actual = match match_result {
        MatchResult::Win => 1.0,
        MatchResult::Draw => 0.5,
        MatchResult::Loss => 0.0,
    };
    
    // K-factor decreases at higher ranks (stabilizes top players)
    let k_factor = match player_mmr {
        0..=4999 => 40,      // Lower ranks: faster progression
        5000..=14999 => 30,  // Mid ranks: standard
        15000..=24999 => 25, // High ranks: slower
        _ => 20,             // Top ranks: very stable
    };
    
    // Calculate base change
    let base_change = k_factor as f32 * (actual - expected);
    
    // Apply performance multiplier (caps at 2x)
    let final_change = base_change * performance_score.clamp(0.5, 2.0);
    
    final_change.round() as i32
}
```

### 1.5 Performance Score Components

| Metric | Weight | Calculation |
|--------|--------|-------------|
| Kills | 25% | (player_kills / team_avg_kills) |
| Damage Dealt | 20% | (player_damage / team_avg_damage) |
| Objective Score | 25% | (player_objective / max_possible) |
| Survival Time | 15% | (player_survival / match_duration) |
| Accuracy | 10% | (shots_hit / shots_fired) |
| Support Actions | 5% | (assists + heals + buffs) |

---

## 2. PROGRESSION PATH

### 2.1 Experience Points (XP) System

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        XP EARN BREAKDOWN                                │
├─────────────────────────────────────────────────────────────────────────┤
│  ACTIVITY                              │ BASE XP    │ BONUS CONDITIONS │
├────────────────────────────────────────┼────────────┼──────────────────┤
│  Match Completion                      │ 100 XP     │ +50 for victory  │
│  Per Kill                              │ 25 XP      │ +10 for headshot │
│  Per Assist                            │ 15 XP      │ -                │
│  Objective Capture                     │ 75 XP      │ +25 for solo cap │
│  Match MVP                             │ 200 XP     │ -                │
│  Win Streak (2x)                       │ +50%       │ Max 5x stack     │
│  Daily First Win                       │ +100 XP    │ -                │
│  Friend Bonus (per friend in match)    │ +10%       │ Max +30%         │
│  Premium Bonus                         │ +50%       │ -                │
└─────────────────────────────────────────────────────────────────────────┘
```

### 2.2 Level Requirements (1-100)

```rust
// XP required for each level follows exponential curve
pub fn xp_required_for_level(level: u32) -> u64 {
    if level == 1 {
        return 0;
    }
    if level > 100 {
        return xp_required_for_level(100);
    }
    
    // Formula: Base + (Level^1.8 * Multiplier)
    let base_xp: u64 = 1000;
    let multiplier: f64 = 150.0;
    let exponent: f64 = 1.8;
    
    (base_xp + (level as f64).powf(exponent) * multiplier) as u64
}

// Cumulative XP to reach level
pub fn cumulative_xp_for_level(target_level: u32) -> u64 {
    (1..=target_level).map(|l| xp_required_for_level(l)).sum()
}
```

### 2.3 Level Milestones Summary

| Level Range | XP Required (per level) | Cumulative XP | Title Unlocked |
|-------------|------------------------|---------------|----------------|
| 1-10 | 1,000 - 2,500 | 17,500 | Space Recruit |
| 11-20 | 2,700 - 5,500 | 72,500 | Combat Pilot |
| 21-30 | 5,800 - 9,500 | 185,000 | Squadron Leader |
| 31-40 | 9,900 - 14,500 | 375,000 | Wing Commander |
| 41-50 | 15,000 - 20,500 | 670,000 | Fleet Admiral |
| 51-60 | 21,000 - 27,500 | 1,100,000 | Star Marshal |
| 61-70 | 28,000 - 35,000 | 1,700,000 | Void Walker |
| 71-80 | 35,500 - 43,500 | 2,500,000 | Cosmic Entity |
| 81-90 | 44,000 - 53,000 | 3,550,000 | Galaxy Guardian |
| 91-100 | 53,500 - 63,500 | 5,000,000 | Universal Legend |

**Total XP to reach Level 100: ~5,000,000 XP**

### 2.4 Level Rewards Table

| Level | Reward Type | Reward | Description |
|-------|-------------|--------|-------------|
| 2 | Ship | Scout MK-II | Faster starter ship |
| 5 | Weapon | Plasma Blaster | Energy weapon unlock |
| 8 | Title | "Rookie" | Display title |
| 10 | Ship | Interceptor X1 | Medium fighter |
| 12 | Skin | Carbon Fiber | Ship skin |
| 15 | Weapon | Homing Missiles | Lock-on missiles |
| 18 | Badge | 10-Kill Streak | Achievement badge |
| 20 | Ship | Vanguard Class | Heavy assault ship |
| 25 | Skin | Neon Cyber | Animated skin |
| 30 | Ship | Phantom Stealth | Cloaking ability |
| 35 | Weapon | Railgun Sniper | Long-range precision |
| 40 | Ship | Titan Destroyer | Boss-class vessel |
| 45 | Title | "Elite" | Prestige title |
| 50 | Ship | Omega Prototype | Ultimate ship |
| 60 | Skin | Golden Legend | Exclusive gold skin |
| 70 | Badge | Master Pilot | Prestige badge |
| 80 | Title | "Legend" | Ultimate title |
| 90 | Skin | Cosmic Aura | Particle effect skin |
| 100 | Ship | Celestial Being | Mythic ship class |

---

## 3. STATS TRACKING

### 3.1 Core Statistics

```rust
// server/src/stats/player_stats.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerStats {
    // Identity
    pub player_id: Uuid,
    pub username: String,
    
    // Rank Data
    pub current_rank: Rank,
    pub current_mmr: i32,
    pub peak_mmr: i32,
    pub games_in_current_rank: u32,
    pub win_rate_in_rank: f32,
    
    // Level Data
    pub level: u32,
    pub current_xp: u64,
    pub total_xp_earned: u64,
    
    // Combat Stats (Lifetime)
    pub total_kills: u64,
    pub total_deaths: u64,
    pub total_assists: u64,
    pub total_damage_dealt: u64,
    pub total_damage_taken: u64,
    pub total_shots_fired: u64,
    pub total_shots_hit: u64,
    pub headshot_kills: u64,
    pub critical_hits: u64,
    
    // Match Stats (Lifetime)
    pub total_matches: u64,
    pub wins: u64,
    pub losses: u64,
    pub draws: u64,
    pub mvps: u64,
    pub top_3_finishes: u64,
    
    // Objective Stats
    pub objectives_captured: u64,
    pub objectives_defended: u64,
    pub payload_pushes: u64,
    pub zone_captures: u64,
    
    // Performance Stats
    pub longest_kill_streak: u32,
    pub longest_life: Duration,
    pub fastest_match_win: Duration,
    pub highest_score: u32,
    
    // Clutch Stats
    pub one_vs_three_clutches: u64,
    pub one_vs_two_clutches: u64,
    pub comeback_wins: u64,  // Won when behind by 50%+
    
    // Weapon Stats (per weapon)
    pub weapon_stats: HashMap<WeaponId, WeaponStats>,
    
    // Ship Stats (per ship)
    pub ship_stats: HashMap<ShipId, ShipStats>,
    
    // Season Stats
    pub current_season_stats: SeasonStats,
    pub previous_seasons: Vec<SeasonStats>,
}
```

### 3.2 Derived Statistics (Calculated)

| Statistic | Formula | Target Range |
|-----------|---------|--------------|
| **K/D Ratio** | Kills / Deaths | 1.0 - 3.0+ |
| **KDA Ratio** | (Kills + Assists) / Deaths | 1.5 - 4.0+ |
| **Win Rate** | Wins / Total Matches | 45% - 70% |
| **Accuracy** | Shots Hit / Shots Fired | 15% - 45% |
| **Headshot %** | Headshot Kills / Total Kills | 10% - 35% |
| **Damage Per Match** | Total Damage / Total Matches | 2,000 - 8,000 |
| **Kills Per Match** | Total Kills / Total Matches | 5 - 20 |
| **Survival Rate** | Matches Survived / Total Matches | 30% - 70% |
| **Objective Score Rate** | Objectives / Matches | 2 - 8 per match |
| **Clutch Success Rate** | Clutches Won / Clutch Situations | 20% - 60% |

### 3.3 Weapon Statistics

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WeaponStats {
    pub weapon_id: WeaponId,
    pub kills: u64,
    pub shots_fired: u64,
    pub shots_hit: u64,
    pub headshots: u64,
    pub damage_dealt: u64,
    pub time_used: Duration,
    pub accuracy: f32,           // calculated: hits / fired
    pub headshot_rate: f32,      // calculated: headshots / kills
    pub kills_per_minute: f32,   // calculated: kills / (time / 60)
}
```

### 3.4 Ship Statistics

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShipStats {
    pub ship_id: ShipId,
    pub matches_played: u64,
    pub wins: u64,
    pub kills: u64,
    pub deaths: u64,
    pub damage_dealt: u64,
    pub time_played: Duration,
    pub win_rate: f32,           // calculated: wins / matches
    pub kd_ratio: f32,           // calculated: kills / deaths
    pub favorite_map: Option<MapId>, // Most played map with this ship
}
```

### 3.5 Season Statistics

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeasonStats {
    pub season_id: String,
    pub season_number: u32,
    pub starting_rank: Rank,
    pub ending_rank: Option<Rank>,
    pub peak_rank: Rank,
    pub peak_mmr: i32,
    pub matches_played: u32,
    pub wins: u32,
    pub losses: u32,
    pub win_streak_best: u32,
    pub placement_matches: Vec<MatchResult>,
    pub final_placement: Option<u32>, // Leaderboard position
}
```

### 3.6 Match History

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchRecord {
    pub match_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub game_mode: GameMode,
    pub map: MapId,
    pub duration: Duration,
    pub result: MatchResult,
    pub score: u32,
    pub team_score: u32,
    pub enemy_score: u32,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub damage_dealt: u32,
    pub damage_taken: u32,
    pub accuracy: f32,
    pub ship_used: ShipId,
    pub mmr_change: i32,
    pub xp_earned: u64,
    pub teammates: Vec<Uuid>,
    pub opponents: Vec<Uuid>,
    pub highlights: Vec<Highlight>, // MVP, Clutch, etc.
}
```

---

## 4. UNLOCK SYSTEM

### 4.1 Ship Unlock Progression

| Ship Name | Unlock Level | Class | Special Ability | Cost (Credits) |
|-----------|--------------|-------|-----------------|----------------|
| **Scout MK-I** | Starter | Light | Speed Boost | Free |
| **Scout MK-II** | Level 2 | Light | Afterburner | 500 |
| **Interceptor X1** | Level 10 | Medium | Barrel Roll | 2,500 |
| **Vanguard** | Level 20 | Heavy | Shield Overcharge | 5,000 |
| **Phantom** | Level 30 | Stealth | Cloak | 10,000 |
| **Titan** | Level 40 | Boss | Ram Attack | 25,000 |
| **Omega** | Level 50 | Hybrid | Phase Shift | 50,000 |
| **Celestial** | Level 100 | Mythic | All Abilities | 100,000 |

### 4.2 Weapon Unlock Progression

| Weapon | Unlock Level | Type | Damage | Fire Rate | Unlock Cost |
|--------|--------------|------|--------|-----------|-------------|
| **Pulse Laser** | Starter | Energy | Medium | High | Free |
| **Plasma Blaster** | Level 5 | Energy | High | Medium | 1,000 |
| **Homing Missiles** | Level 15 | Explosive | Very High | Low | 3,000 |
| **Railgun** | Level 35 | Kinetic | Extreme | Very Low | 8,000 |
| **Scatter Cannon** | Level 25 | Shotgun | High (close) | Medium | 5,000 |
| **Ion Beam** | Level 45 | Continuous | High | Beam | 15,000 |
| **Quantum Torpedo** | Level 60 | Heavy | Massive | Very Low | 30,000 |
| **Singularity Gun** | Level 80 | Ultimate | Extreme | Single | 75,000 |

### 4.3 Skin Rarity Tiers

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         SKIN RARITY SYSTEM                              │
├─────────────────────────────────────────────────────────────────────────┤
│  RARITY      │ DROP RATE    │ UNLOCK METHOD          │ EXAMPLE         │
├──────────────┼──────────────┼────────────────────────┼─────────────────┤
│  Common      │ 60%          │ Level up, Credits      │ Standard Paint  │
│  Uncommon    │ 25%          │ Level up, Credits      │ Camo Patterns   │
│  Rare        │ 10%          │ Achievements, Crates   │ Metallic Finish │
│  Epic        │ 4%           │ Season Rewards, Crates │ Animated Glow   │
│  Legendary   │ 0.9%         │ Season Top 100, Events │ Particle Effects│
│  Mythic      │ 0.1%         | Level 100, Events      │ Transforming    │
└─────────────────────────────────────────────────────────────────────────┘
```

### 4.4 Title System

| Title | Requirement | Rarity | Prefix/Suffix |
|-------|-------------|--------|---------------|
| "Recruit" | Reach Level 5 | Common | Prefix |
| "Pilot" | Reach Level 15 | Common | Prefix |
| "Ace" | 1000 Kills | Uncommon | Prefix |
| "Veteran" | 100 Matches | Uncommon | Prefix |
| "Elite" | Reach Level 45 | Rare | Prefix |
| "Unstoppable" | 20 Kill Streak | Rare | Prefix |
| "Legend" | Reach Level 80 | Epic | Prefix |
| "The Untouchable" | 50-0 Match | Legendary | Suffix |
| "Cosmic" | Reach Level 100 | Mythic | Prefix |
| "of the Void" | Void Master Rank | Epic | Suffix |
| "the Galaxy Slayer" | 10,000 Kills | Legendary | Suffix |

### 4.5 Badge System

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Badge {
    pub id: BadgeId,
    pub name: String,
    pub description: String,
    pub rarity: Rarity,
    pub category: BadgeCategory,
    pub unlock_condition: UnlockCondition,
    pub icon_url: String,
}

pub enum BadgeCategory {
    Combat,      // Kills, damage, accuracy
    Survival,    // Lives saved, clutch plays
    Objective,   // Caps, defends, pushes
    Social,      // Team play, friend bonuses
    Collection,  // Ships owned, skins owned
    Mastery,     // Weapon/ship mastery
    Seasonal,    // Season-specific
}
```

**Example Badges:**

| Badge | Requirement | Rarity | Category |
|-------|-------------|--------|----------|
| First Blood | Get first kill in 100 matches | Common | Combat |
| Sharpshooter | 40%+ Accuracy over 50 matches | Uncommon | Combat |
| Immortal | 50 Match win streak | Legendary | Survival |
| Team Player | 1000 Assists | Rare | Social |
| Collector | Own 10 ships | Uncommon | Collection |
| Weapon Master | 1000 kills with each weapon | Epic | Mastery |
| Season Champion | Top 100 in season | Legendary | Seasonal |

---

## 5. SEASONAL CONTENT

### 5.1 Season Structure

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         SEASON TIMELINE                                 │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  Week 1-2    Week 3-4    Week 5-8      Week 9-10   Week 11-12         │
│     │           │           │             │           │                │
│     ▼           ▼           ▼             ▼           ▼                │
│  ┌─────┐    ┌─────┐    ┌─────────┐    ┌─────┐    ┌─────────┐          │
│  │START│───▶│EVENT│───▶│  MID    │───▶│FINAL│───▶│ REWARDS │          │
│  │     │    │WEEK │    │ SEASON  │    │PUSH │    │ & RESET │          │
│  └─────┘    └─────┘    └─────────┘    └─────┘    └─────────┘          │
│                                                                         │
│  • Placement    • Special     • Double XP    • Rank      • Rewards    │
│    matches      event         weekends       boost      distributed   │
│  • New content  • Limited     • Mid-season   • Final     • Soft MMR   │
│    released     modes         tournament     standings    reset        │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 5.2 Season Details

| Attribute | Value |
|-----------|-------|
| **Duration** | 12 weeks (84 days) |
| **Placement Matches** | 10 matches required |
| **Season Start** | First Monday of each quarter |
| **Season End** | Sunday before next season |
| **Rank Reset** | Soft reset (MMR compressed toward 10,000) |

### 5.3 Placement System

```rust
// server/src/seasons/placement.rs

pub struct PlacementSystem;

impl PlacementSystem {
    /// Calculate initial rank after placement matches
    pub fn calculate_placement_rank(
        &self,
        previous_season_rank: Option<Rank>,
        placement_results: &[MatchResult],
        performance_scores: &[f32],
    ) -> (Rank, i32) {
        // Base MMR from previous season (if exists)
        let base_mmr = previous_season_rank
            .map(|r| r.mmr_threshold())
            .unwrap_or(5000); // Start at Asteroid Pilot if new
        
        // Calculate win rate in placements
        let wins = placement_results.iter().filter(|r| **r == MatchResult::Win).count();
        let win_rate = wins as f32 / placement_results.len() as f32;
        
        // Average performance score
        let avg_performance = performance_scores.iter().sum::<f32>() / performance_scores.len() as f32;
        
        // MMR adjustment based on placement performance
        let adjustment = match (win_rate, avg_performance) {
            (w, p) if w >= 0.8 && p >= 1.5 => 3000,  // Exceptional
            (w, p) if w >= 0.7 && p >= 1.3 => 2000,  // Excellent
            (w, p) if w >= 0.6 && p >= 1.1 => 1000,  // Good
            (w, p) if w >= 0.5 && p >= 0.9 => 0,     // Average
            (w, p) if w >= 0.4 && p >= 0.7 => -500,  // Below Average
            _ => -1000,                              // Poor
        };
        
        let final_mmr = (base_mmr + adjustment).clamp(0, 35000);
        let rank = Rank::from_mmr(final_mmr);
        
        (rank, final_mmr)
    }
}
```

### 5.4 Season Rewards

| Rank Achieved | End-of-Season Rewards |
|---------------|----------------------|
| Trainee | 500 Credits, Common Crate |
| Rookie | 1,000 Credits, Uncommon Crate |
| Space Cadet | 2,000 Credits, Rare Crate |
| Asteroid Pilot | 3,500 Credits, Rare Crate + Title |
| Comet Striker | 5,000 Credits, Epic Crate |
| Meteor Hunter | 7,500 Credits, Epic Crate + Animated Badge |
| Solar Captain | 10,000 Credits, Legendary Crate |
| Star Commander | 15,000 Credits, Legendary Crate + Exclusive Skin |
| Nebula Champion | 25,000 Credits, Legendary Crate + Title |
| Void Master | 40,000 Credits, Mythic Crate |
| Galactic Elite | 60,000 Credits, Mythic Crate + Particle Effect |
| Cosmic Legend | 100,000 Credits, Ultimate Crate + Season Badge |

### 5.5 Leaderboard System

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderboardEntry {
    pub rank: u32,
    pub player_id: Uuid,
    pub username: String,
    pub mmr: i32,
    pub current_rank: Rank,
    pub wins: u32,
    pub matches: u32,
    pub win_rate: f32,
    pub region: Region,
}

pub struct Leaderboard {
    pub season_id: String,
    pub last_updated: DateTime<Utc>,
    pub global_top_100: Vec<LeaderboardEntry>,
    pub regional_leaderboards: HashMap<Region, Vec<LeaderboardEntry>>,
    pub friends_leaderboard: Vec<LeaderboardEntry>, // For specific player
}
```

**Leaderboard Categories:**
- Global Top 100 (All players)
- Regional (NA, EU, ASIA, SA, OCE)
- Friends (Social comparison)
- Ship-Specific (Best players per ship)
- Weapon-Specific (Best players per weapon)

### 5.6 Rank Reset Formula

```rust
/// Soft reset MMR at season end
pub fn calculate_season_reset_mmr(current_mmr: i32) -> i32 {
    // Compress MMR toward 10,000 (Comet Striker baseline)
    let target_mmr = 10000;
    let compression_factor = 0.6; // Keep 60% of deviation
    
    let deviation = current_mmr - target_mmr;
    let new_deviation = (deviation as f32 * compression_factor) as i32;
    
    (target_mmr + new_deviation).clamp(0, 35000)
}

// Example resets:
// 30,000 (Legend) → 22,000 (Void Master III)
// 20,000 (Champion) → 16,000 (Star Commander III)
// 10,000 (Striker) → 10,000 (Striker - no change)
// 5,000 (Pilot) → 7,000 (Comet Striker III)
```

---

## 6. PRACTICE MODE

### 6.1 Bot Difficulty Levels

| Difficulty | Bot Skill | Reaction Time | Accuracy | Behavior |
|------------|-----------|---------------|----------|----------|
| **Recruit** | 20% | 800ms | 15% | Predictable patterns, slow movement |
| **Rookie** | 40% | 600ms | 25% | Basic dodging, occasional cover |
| **Pilot** | 60% | 400ms | 35% | Tactical positioning, team coordination |
| **Veteran** | 75% | 300ms | 45% | Advanced maneuvers, ability usage |
| **Ace** | 90% | 200ms | 55% | Human-like unpredictability |
| **Elite** | 100% | 150ms | 65% | Near-perfect play, optimal decisions |
| **Legend** | 110% | 100ms | 75% | Superhuman reflexes, perfect aim |

### 6.2 Bot AI Configuration

```rust
// server/src/bots/bot_config.rs

#[derive(Debug, Clone)]
pub struct BotConfig {
    pub difficulty: BotDifficulty,
    pub aim_assist: f32,           // 0.0 - 1.0
    pub reaction_time_ms: u32,     // Delay before reacting
    pub accuracy_multiplier: f32,  // Multiplies base accuracy
    pub movement_skill: f32,       // 0.0 - 1.0
    pub decision_speed: f32,       // 0.0 - 1.0
    pub teamwork_rating: f32,      // 0.0 - 1.0
    pub ability_usage: f32,        // 0.0 - 1.0
}

impl BotDifficulty {
    pub fn config(&self) -> BotConfig {
        match self {
            BotDifficulty::Recruit => BotConfig {
                difficulty: *self,
                aim_assist: 0.1,
                reaction_time_ms: 800,
                accuracy_multiplier: 0.3,
                movement_skill: 0.2,
                decision_speed: 0.3,
                teamwork_rating: 0.1,
                ability_usage: 0.1,
            },
            BotDifficulty::Legend => BotConfig {
                difficulty: *self,
                aim_assist: 1.0,
                reaction_time_ms: 100,
                accuracy_multiplier: 1.2,
                movement_skill: 1.0,
                decision_speed: 1.0,
                teamwork_rating: 1.0,
                ability_usage: 1.0,
            },
            // ... other difficulties
        }
    }
}
```

### 6.3 Practice Mode Types

| Mode | Description | XP Reward | Unlock Requirement |
|------|-------------|-----------|-------------------|
| **Target Practice** | Shoot static/moving targets | 25% normal XP | Level 1 |
| **Bot Match (1v1)** | Single bot duel | 50% normal XP | Level 1 |
| **Bot Match (Team)** | 5v5 bot match | 50% normal XP | Level 5 |
| **Survival** | Endless waves of bots | 40% normal XP | Level 10 |
| **Time Trial** | Complete objectives fastest | 30% normal XP | Level 15 |
| **Training Course** | Guided skill tutorials | 20% normal XP | Level 1 |

### 6.4 Training Scenarios

```rust
#[derive(Debug, Clone)]
pub struct TrainingScenario {
    pub id: ScenarioId,
    pub name: String,
    pub description: String,
    pub objectives: Vec<ScenarioObjective>,
    pub difficulty: BotDifficulty,
    pub time_limit: Option<Duration>,
    pub xp_reward: u64,
    pub completion_rewards: Vec<Reward>,
}

// Example scenarios
pub const SCENARIOS: &[TrainingScenario] = &[
    TrainingScenario {
        id: ScenarioId("aim_training_1"),
        name: "Basic Targeting".to_string(),
        description: "Destroy 50 targets within 2 minutes".to_string(),
        objectives: vec![
            ScenarioObjective::DestroyTargets { count: 50 },
        ],
        difficulty: BotDifficulty::Recruit,
        time_limit: Some(Duration::from_secs(120)),
        xp_reward: 500,
        completion_rewards: vec![Reward::Title("Sharpshooter".to_string())],
    },
    TrainingScenario {
        id: ScenarioId("clutch_training_1"),
        name: "1v3 Clutch".to_string(),
        description: "Win a 1v3 situation against Veteran bots".to_string(),
        objectives: vec![
            ScenarioObjective::EliminateBots { count: 3 },
            ScenarioObjective::Survive,
        ],
        difficulty: BotDifficulty::Veteran,
        time_limit: None,
        xp_reward: 2000,
        completion_rewards: vec![Reward::Badge(BadgeId("clutch_master"))],
    },
];
```

### 6.5 Tutorial System

| Tutorial | Content | XP Reward | Unlocks |
|----------|---------|-----------|---------|
| **Basic Flight** | Movement, boosting, braking | 200 XP | Advanced tutorials |
| **Combat Basics** | Shooting, aiming, leading | 300 XP | Weapon tutorials |
| **Abilities** | Ship abilities, cooldowns | 250 XP | Ship-specific tutorials |
| **Objectives** | Capture, defend, payload | 400 XP | Ranked mode |
| **Advanced Combat** | Dodging, positioning | 500 XP | All game modes |
| **Team Play** | Communication, roles | 350 XP | Squad features |

---

## 7. CHALLENGES & MISSIONS

### 7.1 Mission Types

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MissionType {
    // Daily Missions (3 per day)
    Daily {
        description: String,
        requirement: MissionRequirement,
        reward: MissionReward,
        expires_at: DateTime<Utc>,
    },
    
    // Weekly Missions (5 per week)
    Weekly {
        description: String,
        requirement: MissionRequirement,
        reward: MissionReward,
        expires_at: DateTime<Utc>,
    },
    
    // Season Missions (10 per season)
    Seasonal {
        description: String,
        requirement: MissionRequirement,
        reward: MissionReward,
        season_id: String,
    },
    
    // Achievement Missions (One-time)
    Achievement {
        description: String,
        requirement: MissionRequirement,
        reward: MissionReward,
        badge_reward: Option<BadgeId>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MissionRequirement {
    WinMatches { count: u32, mode: Option<GameMode> },
    GetKills { count: u32, weapon: Option<WeaponId> },
    GetAssists { count: u32 },
    DealDamage { amount: u64 },
    CaptureObjectives { count: u32 },
    PlayMatches { count: u32, mode: Option<GameMode> },
    GetKillStreak { count: u32 },
    SurviveMatches { count: u32 },
    UseAbility { ability: AbilityId, count: u32 },
    PlayWithFriends { count: u32 },
}
```

### 7.2 Daily Mission Pool

| Mission | Requirement | Base Reward |
|---------|-------------|-------------|
| Victory Lap | Win 3 matches | 300 XP, 100 Credits |
| Sharpshooter | Get 20 kills | 250 XP, 75 Credits |
| Team Player | Get 15 assists | 200 XP, 50 Credits |
| Objective Focus | Capture 5 objectives | 350 XP, 125 Credits |
| Damage Dealer | Deal 10,000 damage | 300 XP, 100 Credits |
| Survivalist | Survive 5 matches | 250 XP, 75 Credits |
| Weapon Mastery | Get 10 kills with [random weapon] | 400 XP, 150 Credits |
| Social Flyer | Play 3 matches with friends | 200 XP, 100 Credits |
| Clutch King | Win 2 matches by less than 10% | 500 XP, 200 Credits |
| Consistency | Get 5+ kills in 3 consecutive matches | 450 XP, 175 Credits |

### 7.3 Weekly Mission Pool

| Mission | Requirement | Base Reward |
|---------|-------------|-------------|
| Ranked Grinder | Win 15 ranked matches | 2000 XP, 1000 Credits, Rare Crate |
| Killing Spree | Get 150 total kills | 1500 XP, 750 Credits |
| Objective Master | Capture 30 objectives | 1800 XP, 900 Credits |
| Damage Lord | Deal 100,000 damage | 2000 XP, 1000 Credits |
| Streak Hunter | Get a 15+ kill streak | 2500 XP, 1500 Credits, Epic Crate |
| Weapon Expert | Get 50 kills with 3 different weapons | 2200 XP, 1100 Credits |
| Team Champion | Win 10 matches with full squad | 3000 XP, 2000 Credits |
| Unstoppable | Win 10 consecutive matches | 5000 XP, 3000 Credits, Legendary Crate |

### 7.4 Streak Bonus System

```rust
// Daily login streak rewards
pub fn get_daily_streak_reward(streak_days: u32) -> StreakReward {
    match streak_days {
        1 => StreakReward {
            xp_multiplier: 1.0,
            credit_bonus: 0,
            special_reward: None,
        },
        2 => StreakReward {
            xp_multiplier: 1.1,
            credit_bonus: 50,
            special_reward: None,
        },
        3 => StreakReward {
            xp_multiplier: 1.2,
            credit_bonus: 100,
            special_reward: None,
        },
        4 => StreakReward {
            xp_multiplier: 1.3,
            credit_bonus: 150,
            special_reward: None,
        },
        5 => StreakReward {
            xp_multiplier: 1.5,
            credit_bonus: 250,
            special_reward: Some(Reward::Crate(CrateType::Uncommon)),
        },
        6 => StreakReward {
            xp_multiplier: 1.7,
            credit_bonus: 350,
            special_reward: None,
        },
        7 => StreakReward {
            xp_multiplier: 2.0,
            credit_bonus: 500,
            special_reward: Some(Reward::Crate(CrateType::Rare)),
        },
        _ if streak_days > 7 => StreakReward {
            xp_multiplier: 2.0,
            credit_bonus: 500 + (streak_days - 7) * 25,
            special_reward: Some(Reward::Crate(CrateType::Rare)),
        },
        _ => StreakReward::default(),
    }
}

// Mission completion streak (weekly)
pub fn get_mission_streak_bonus(completed_this_week: u32) -> f32 {
    match completed_this_week {
        0..=2 => 1.0,
        3..=4 => 1.1,
        5 => 1.25,
        _ => 1.25,
    }
}
```

### 7.5 Mission Reward Structure

| Mission Type | XP Range | Credits Range | Bonus Rewards |
|--------------|----------|---------------|---------------|
| Daily (Easy) | 200-300 | 50-100 | - |
| Daily (Medium) | 300-450 | 100-175 | - |
| Daily (Hard) | 450-600 | 175-250 | Common Crate |
| Weekly (Easy) | 1000-1500 | 500-750 | - |
| Weekly (Medium) | 1500-2500 | 750-1500 | Uncommon Crate |
| Weekly (Hard) | 2500-4000 | 1500-2500 | Rare Crate |
| Weekly (Elite) | 4000-6000 | 2500-4000 | Epic Crate |

---

## 8. IMPLEMENTATION GUIDANCE

### 8.1 Database Schema

```sql
-- Player progression table
CREATE TABLE player_progression (
    player_id UUID PRIMARY KEY,
    level INTEGER NOT NULL DEFAULT 1,
    current_xp BIGINT NOT NULL DEFAULT 0,
    total_xp_earned BIGINT NOT NULL DEFAULT 0,
    current_rank VARCHAR(50) NOT NULL DEFAULT 'Trainee',
    current_mmr INTEGER NOT NULL DEFAULT 0,
    peak_mmr INTEGER NOT NULL DEFAULT 0,
    games_in_current_rank INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

-- Player stats table
CREATE TABLE player_stats (
    player_id UUID PRIMARY KEY,
    total_kills BIGINT NOT NULL DEFAULT 0,
    total_deaths BIGINT NOT NULL DEFAULT 0,
    total_assists BIGINT NOT NULL DEFAULT 0,
    total_damage_dealt BIGINT NOT NULL DEFAULT 0,
    total_shots_fired BIGINT NOT NULL DEFAULT 0,
    total_shots_hit BIGINT NOT NULL DEFAULT 0,
    total_matches BIGINT NOT NULL DEFAULT 0,
    wins BIGINT NOT NULL DEFAULT 0,
    losses BIGINT NOT NULL DEFAULT 0,
    mvps BIGINT NOT NULL DEFAULT 0,
    longest_kill_streak INTEGER NOT NULL DEFAULT 0,
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

-- Match history table
CREATE TABLE match_history (
    match_id UUID PRIMARY KEY,
    player_id UUID NOT NULL,
    game_mode VARCHAR(50) NOT NULL,
    map_id VARCHAR(50) NOT NULL,
    result VARCHAR(20) NOT NULL,
    kills INTEGER NOT NULL DEFAULT 0,
    deaths INTEGER NOT NULL DEFAULT 0,
    assists INTEGER NOT NULL DEFAULT 0,
    damage_dealt INTEGER NOT NULL DEFAULT 0,
    accuracy FLOAT NOT NULL DEFAULT 0,
    mmr_change INTEGER NOT NULL DEFAULT 0,
    xp_earned BIGINT NOT NULL DEFAULT 0,
    played_at TIMESTAMP NOT NULL DEFAULT NOW(),
    INDEX idx_player_matches (player_id, played_at)
);

-- Player unlocks table
CREATE TABLE player_unlocks (
    player_id UUID NOT NULL,
    unlock_type VARCHAR(50) NOT NULL, -- 'ship', 'weapon', 'skin', 'title', 'badge'
    unlock_id VARCHAR(100) NOT NULL,
    unlocked_at TIMESTAMP NOT NULL DEFAULT NOW,
    PRIMARY KEY (player_id, unlock_type, unlock_id)
);

-- Active missions table
CREATE TABLE active_missions (
    mission_id UUID PRIMARY KEY,
    player_id UUID NOT NULL,
    mission_type VARCHAR(50) NOT NULL, -- 'daily', 'weekly', 'seasonal'
    description TEXT NOT NULL,
    requirement JSON NOT NULL,
    progress JSON NOT NULL,
    reward JSON NOT NULL,
    expires_at TIMESTAMP NOT NULL,
    completed_at TIMESTAMP,
    INDEX idx_player_missions (player_id, expires_at)
);

-- Season data table
CREATE TABLE season_data (
    season_id VARCHAR(50) PRIMARY KEY,
    season_number INTEGER NOT NULL,
    start_date TIMESTAMP NOT NULL,
    end_date TIMESTAMP NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT FALSE
);

-- Season player stats table
CREATE TABLE season_player_stats (
    season_id VARCHAR(50) NOT NULL,
    player_id UUID NOT NULL,
    starting_rank VARCHAR(50) NOT NULL,
    ending_rank VARCHAR(50),
    peak_rank VARCHAR(50),
    peak_mmr INTEGER NOT NULL DEFAULT 0,
    matches_played INTEGER NOT NULL DEFAULT 0,
    wins INTEGER NOT NULL DEFAULT 0,
    final_placement INTEGER,
    rewards_claimed BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (season_id, player_id)
);
```

### 8.2 API Endpoints

```rust
// server/src/api/progression.rs

// GET /api/player/{id}/progression
pub async fn get_player_progression(
    Path(player_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<PlayerProgression>, ApiError> {
    let progression = state.db.get_player_progression(player_id).await?;
    Ok(Json(progression))
}

// GET /api/player/{id}/stats
pub async fn get_player_stats(
    Path(player_id): Path<Uuid>,
    Query(params): Query<StatsQuery>,
    State(state): State<AppState>,
) -> Result<Json<PlayerStats>, ApiError> {
    let stats = match params.period {
        Some(period) => state.db.get_stats_for_period(player_id, period).await?,
        None => state.db.get_lifetime_stats(player_id).await?,
    };
    Ok(Json(stats))
}

// GET /api/player/{id}/match-history
pub async fn get_match_history(
    Path(player_id): Path<Uuid>,
    Query(pagination): Query<Pagination>,
    State(state): State<AppState>,
) -> Result<Json<Vec<MatchRecord>>, ApiError> {
    let history = state.db.get_match_history(player_id, pagination).await?;
    Ok(Json(history))
}

// GET /api/player/{id}/missions
pub async fn get_active_missions(
    Path(player_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<Vec<Mission>>, ApiError> {
    let missions = state.mission_system.get_active_missions(player_id).await?;
    Ok(Json(missions))
}

// POST /api/player/{id}/missions/{mission_id}/claim
pub async fn claim_mission_reward(
    Path((player_id, mission_id)): Path<(Uuid, Uuid)>,
    State(state): State<AppState>,
) -> Result<Json<Reward>, ApiError> {
    let reward = state.mission_system.claim_reward(player_id, mission_id).await?;
    Ok(Json(reward))
}

// GET /api/leaderboard/{season_id}
pub async fn get_leaderboard(
    Path(season_id): Path<String>,
    Query(params): Query<LeaderboardQuery>,
    State(state): State<AppState>,
) -> Result<Json<Leaderboard>, ApiError> {
    let leaderboard = state.leaderboard.get_leaderboard(&season_id, params).await?;
    Ok(Json(leaderboard))
}

// GET /api/unlocks/{player_id}
pub async fn get_player_unlocks(
    Path(player_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<Json<PlayerUnlocks>, ApiError> {
    let unlocks = state.db.get_player_unlocks(player_id).await?;
    Ok(Json(unlocks))
}
```

### 8.3 Key Constants

```rust
// server/src/constants.rs

pub const MAX_LEVEL: u32 = 100;
pub const PLACEMENT_MATCHES_REQUIRED: u32 = 10;
pub const SEASON_DURATION_WEEKS: u32 = 12;
pub const DAILY_MISSIONS_COUNT: usize = 3;
pub const WEEKLY_MISSIONS_COUNT: usize = 5;
pub const SEASONAL_MISSIONS_COUNT: usize = 10;
pub const MAX_DAILY_STREAK: u32 = 7;
pub const MATCH_HISTORY_LIMIT: usize = 100;
pub const LEADERBOARD_UPDATE_INTERVAL_SECS: u64 = 300; // 5 minutes

// XP Multipliers
pub const WIN_XP_MULTIPLIER: f32 = 1.5;
pub const MVP_XP_MULTIPLIER: f32 = 1.25;
pub const PREMIUM_XP_MULTIPLIER: f32 = 1.5;
pub const PRACTICE_XP_MULTIPLIER: f32 = 0.5;

// MMR Constants
pub const BASE_K_FACTOR: i32 = 40;
pub const HIGH_RANK_K_FACTOR: i32 = 20;
pub const MMR_FLOOR: i32 = 0;
pub const MMR_CEILING: i32 = 50000;

// Rank thresholds
pub const RANK_TRAINEE_MAX: i32 = 1499;
pub const RANK_ROOKIE_MAX: i32 = 2999;
pub const RANK_CADET_MAX: i32 = 4999;
pub const RANK_PILOT_MAX: i32 = 7499;
pub const RANK_STRIKER_MAX: i32 = 9999;
pub const RANK_HUNTER_MAX: i32 = 12999;
pub const RANK_CAPTAIN_MAX: i32 = 15999;
pub const RANK_COMMANDER_MAX: i32 = 18999;
pub const RANK_CHAMPION_MAX: i32 = 21999;
pub const RANK_MASTER_MAX: i32 = 24999;
pub const RANK_ELITE_MAX: i32 = 29999;
```

---

## 9. PLAYER JOURNEY MAP

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                              PLAYER JOURNEY                                     │
├─────────────────────────────────────────────────────────────────────────────────┤
│                                                                                 │
│  NEW PLAYER                    CASUAL PLAYER              COMPETITIVE PLAYER   │
│      │                              │                           │              │
│      ▼                              ▼                           ▼              │
│  ┌─────────┐    ┌─────────┐    ┌─────────┐    ┌─────────┐    ┌─────────┐      │
│  │TUTORIAL │───▶│PRACTICE │───▶│UNRANKED │───▶│PLACEMENT│───▶│ RANKED  │      │
│  │  MODE   │    │ MATCHES │    │ MATCHES │    │ MATCHES │    │  MODE   │      │
│  └─────────┘    └─────────┘    └─────────┘    └─────────┘    └─────────┘      │
│       │              │              │              │              │             │
│       ▼              ▼              ▼              ▼              ▼             │
│   Level 1-5      Level 5-15     Level 10+      Level 15+      Level 20+       │
│   Unlock:        Unlock:        Unlock:        Unlock:        Unlock:         │
│   - Scout MK-II  - Interceptor  - Ranked Mode  - Season Play  - Tournaments   │
│   - Basic Skins  - More Ships   - Weekly Missions - Leaderboard - Clans       │
│                                                                                 │
│  ═══════════════════════════════════════════════════════════════════════════   │
│                                                                                 │
│                              PROGRESSION PATH                                   │
│                                                                                 │
│  Trainee ──▶ Rookie ──▶ Cadet ──▶ Pilot ──▶ Striker ──▶ Hunter ──▶ Captain   │
│    (0)       (1500)    (3000)    (5000)     (7500)      (10000)    (13000)     │
│                                                                                 │
│  ──▶ Commander ──▶ Champion ──▶ Master ──▶ Elite ──▶ Legend                   │
│      (16000)      (19000)      (22000)    (25000)    (30000)                   │
│                                                                                 │
│  ═══════════════════════════════════════════════════════════════════════════   │
│                                                                                 │
│                              ENGAGEMENT LOOPS                                   │
│                                                                                 │
│     ┌──────────┐         ┌──────────┐         ┌──────────┐                     │
│     │  PLAY    │────────▶│  EARN    │────────▶│  UNLOCK  │                     │
│     │  MATCH   │         │  XP/CR   │         │  REWARD  │                     │
│     └──────────┘         └──────────┘         └────┬─────┘                     │
│          ▲                                         │                            │
│          └─────────────────────────────────────────┘                            │
│                                                                                 │
│     ┌──────────┐         ┌──────────┐         ┌──────────┐                     │
│     │  COMPLETE│────────▶│  CLAIM   │────────▶│  PROGRESS│                     │
│     │  MISSION │         │  REWARD  │         │  FURTHER │                     │
│     └──────────┘         └──────────┘         └──────────┘                     │
│                                                                                 │
└─────────────────────────────────────────────────────────────────────────────────┘
```

---

## 10. SUMMARY

This career mode design provides a comprehensive progression system that rivals CS2's Premier mode with:

| Feature | Implementation Status |
|---------|----------------------|
| **18 Ranks** | 6 divisions, 3 tiers each |
| **30,000+ MMR Range** | Granular skill-based ranking |
| **100 Levels** | ~5M XP total progression |
| **8 Ships** | Unlockable by level |
| **8 Weapons** | Progressive unlock system |
| **6 Skin Rarities** | From Common to Mythic |
| **50+ Titles** | Achievement-based |
| **40+ Badges** | Collection & mastery |
| **12-Week Seasons** | Regular content refresh |
| **Daily/Weekly Missions** | Continuous engagement |
| **7 Bot Difficulties** | Scalable practice |
| **Training Scenarios** | Skill development |
| **Global Leaderboards** | Competitive recognition |

**Total Estimated Development Time: 6-8 weeks**
- Core ranking system: 1 week
- XP/leveling system: 1 week
- Stats tracking: 1 week
- Unlock system: 1 week
- Season framework: 1 week
- Practice mode: 1 week
- Missions system: 1 week
- Polish & testing: 1-2 weeks
