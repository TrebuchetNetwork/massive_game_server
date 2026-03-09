# Bot Stuck Detection and Recovery System

## Overview
This document describes the enhanced bot stuck detection and recovery system implemented to prevent bots from getting permanently stuck against walls or in corners.

## Implementation Details

### 1. Stuck Detection Fields in BotController
```rust
pub struct BotController {
    // ... other fields ...
    // Stuck detection fields
    pub last_position: Vec2,           // Bot's position from last check
    pub stuck_timer: f32,              // Time accumulator for stuck detection
    pub stuck_check_position: Vec2,    // Position at last stuck check
}
```

### 2. Stuck Detection Algorithm (in optimized_bot_ai.rs)
The `check_stuck_status` method implements the following logic:

```rust
const BOT_STUCK_THRESHOLD: f32 = 10.0;        // Min distance to move to not be considered stuck
const BOT_STUCK_TIME_THRESHOLD: f32 = 2.0;    // Seconds before considering bot stuck
const BOT_STUCK_CHECK_INTERVAL: f32 = 0.5;    // Check every half second
```

#### Detection Process:
1. **Timer Update**: Accumulates time since last check
2. **Position Check**: Every 0.5 seconds, checks if bot has moved more than 10 units
3. **Stuck Confirmation**: If bot hasn't moved enough for 2 seconds, it's considered stuck
4. **Recovery Action**: Generates a new random target position to escape

### 3. Recovery Strategy
When a bot is detected as stuck:

```rust
// Generate escape direction
let escape_angle = rng.gen_range(0.0..2.0 * std::f32::consts::PI);
let escape_distance = rng.gen_range(100.0..300.0);

// Calculate new target position
let new_x = (current_pos.x + escape_distance * escape_angle.cos())
    .clamp(WORLD_MIN_X + 100.0, WORLD_MAX_X - 100.0);
let new_y = (current_pos.y + escape_distance * escape_angle.sin())
    .clamp(WORLD_MIN_Y + 100.0, WORLD_MAX_Y - 100.0);

// Update bot state
bot_controller.target_position = Some(Vec2::new(new_x, new_y));
bot_controller.behavior_state = BotBehaviorState::MovingToPosition;
```

### 4. Prevention Measures
To reduce the likelihood of bots getting stuck:

1. **Spatial Index Collision Detection**: Uses wall spatial index for efficient collision checks
2. **Ray Casting**: Projectiles use ray casting to detect walls between frames
3. **Line of Sight Checks**: Bots check if they have clear line of sight before targeting enemies
4. **Movement Tolerance**: Bots consider themselves "at target" within 50 units to avoid wall hugging

### 5. Integration Points

#### Bot Spawning (instance.rs)
When bots are spawned, stuck detection fields are initialized:
```rust
let bot_controller = BotController {
    // ... other fields ...
    last_position: Vec2::new(spawn_pos.x, spawn_pos.y),
    stuck_timer: 0.0,
    stuck_check_position: Vec2::new(spawn_pos.x, spawn_pos.y),
};
```

#### Bot AI Update Loop
The stuck detection runs every frame:
```rust
// In update_bots_batch
for bot_id in &bot_ids {
    // ... get bot state ...
    
    // Check if bot is stuck before generating input
    Self::check_stuck_status(bot_controller, &bot_state, delta_time);
    
    // Generate input based on current objective
    let input = Self::generate_combat_input(&bot_state, bot_controller, server_instance, game_mode);
}
```

### 6. Debug Logging
The system includes comprehensive logging for debugging:

```rust
// When stuck is detected
warn!("Bot {} is stuck at ({:.0}, {:.0}), generating new target", 
    bot_state.username, current_pos.x, current_pos.y);

// When unstuck action is taken
debug!("Bot {} unstuck - new target: ({:.0}, {:.0})", 
    bot_state.username, new_x, new_y);
```

### 7. Performance Considerations

1. **Check Interval**: Only checks position every 0.5 seconds to reduce overhead
2. **Spatial Queries**: Uses efficient wall spatial index for collision detection
3. **Early Exit**: Skips stuck detection for dead bots

### 8. Configuration Tweaking

You can adjust these constants to fine-tune the stuck detection:

```rust
// More sensitive detection (catches stuck bots faster)
const BOT_STUCK_THRESHOLD: f32 = 5.0;         // Reduced from 10.0
const BOT_STUCK_TIME_THRESHOLD: f32 = 1.0;    // Reduced from 2.0

// Less sensitive detection (fewer false positives)
const BOT_STUCK_THRESHOLD: f32 = 20.0;        // Increased from 10.0
const BOT_STUCK_TIME_THRESHOLD: f32 = 3.0;    // Increased from 2.0
```

## Testing and Monitoring

To monitor stuck detection effectiveness:

1. Watch for "Bot X is stuck" warnings in logs
2. Check if bots successfully unstuck themselves
3. Monitor bot movement patterns in dense wall areas
4. Verify bots don't get stuck in spawn areas

## Future Improvements

1. **Path Finding**: Implement A* or similar algorithm for better navigation
2. **Wall Avoidance**: Add predictive collision avoidance
3. **Team Coordination**: Prevent bots from blocking each other
4. **Stuck History**: Track locations where bots frequently get stuck for map improvements
