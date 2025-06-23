# Async Bot AI Integration Guide

## Overview
The async bot AI system runs independently from the main game loop, making bot decisions in a separate async task. This prevents bot AI from blocking the game loop and allows for more complex bot behaviors.

## Architecture

```
Main Game Loop (60 Hz)
    │
    ├─> Poll Bot Decisions (non-blocking)
    │     └─> Apply inputs to bot players
    │
    └─> Continue with physics/game logic

Async Bot AI Task (10 Hz)
    │
    ├─> Analyze game state for each bot
    ├─> Make intelligent decisions
    └─> Send decisions via channel
```

## Integration Steps

### 1. Modify server/src/server/mod.rs
Add the module:
```rust
pub mod instance_bot_integration;
```

### 2. Update MassiveGameServer struct in instance.rs
Add these fields:
```rust
pub struct MassiveGameServer {
    // ... existing fields ...
    
    // Async bot AI system
    pub bot_ai_system: Option<Arc<Mutex<AsyncBotAI>>>,
    pub bot_ai_handle: Option<JoinHandle<()>>,
}
```

### 3. Initialize in MassiveGameServer::new()
```rust
bot_ai_system: None,
bot_ai_handle: None,
```

### 4. Start async bot AI after server creation
In the game server startup (after creating MassiveGameServer):
```rust
// Start async bot AI
let (bot_ai, ai_handle) = server.start_async_bot_ai();
server.bot_ai_system = Some(Arc::new(Mutex::new(bot_ai)));
server.bot_ai_handle = Some(ai_handle);
```

### 5. Update run_ai_update in instance.rs
Replace the current bot AI call with:
```rust
pub async fn run_ai_update(&self) {
    // Process async bot decisions instead of OptimizedBotAI
    if let Some(bot_ai) = &self.bot_ai_system {
        if let Ok(mut bot_ai_guard) = bot_ai.try_lock() {
            self.process_async_bot_decisions(&mut bot_ai_guard);
        }
    }
}
```

### 6. Clean shutdown
Add to server shutdown:
```rust
// Stop bot AI task
if let Some(handle) = self.bot_ai_handle.take() {
    handle.abort();
}
```

## Benefits

1. **Non-blocking**: Bot AI runs independently, doesn't slow down game loop
2. **Intelligent behavior**: More complex decision making without performance impact
3. **Scalable**: Can adjust bot update frequency independently
4. **Modular**: Easy to swap AI implementations

## Configuration

Adjust these constants in async_bot_ai.rs:
- Bot update interval: `Duration::from_millis(100)` (10 Hz)
- Think time per bot: `Duration::from_millis(500)` (2 decisions/sec)
- Detection ranges for enemies, pickups, projectiles

## Debugging

Enable trace logs to see bot decisions:
```
RUST_LOG=massive_game_server_core::systems::ai=trace
```

Monitor bot behavior:
- Check if bots are receiving inputs
- Verify movement patterns
- Watch for stuck bots
