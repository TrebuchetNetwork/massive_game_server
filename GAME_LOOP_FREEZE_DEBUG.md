# Game Loop Freeze Debugging Guide

## Symptoms
- Game processes frames 1-6 normally
- After a player connects (frame 6), no more frames are processed
- Bots stop moving because the game loop has stopped

## Likely Causes

### 1. Synchronous WebRTC Operations
The issue appears when `[3baeae89-3680-4b70-b9f2-4eb92a2b86ba]: DataChannel 'gameDataChannel' OPENED` happens. This suggests the game loop might be blocked by synchronous WebRTC operations.

### 2. Deadlock in State Synchronization
The `synchronize_state()` function in the game loop might be deadlocking when trying to access player state while the signaling system is also accessing it.

### 3. Blocking Network Operations
The broadcast system might be doing blocking network I/O when sending to connected clients.

## Quick Fixes

### Fix 1: Add Timeout to Broadcasts
In `instance.rs`, modify `broadcast_world_updates_optimized`:
```rust
// Add timeout to client broadcasts
const BROADCAST_TIMEOUT: Duration = Duration::from_millis(50);

match timeout(BROADCAST_TIMEOUT, 
    Self::process_client_broadcast(&peer_id_str, &client_info, &shared_broadcast_data, &self)
).await {
    Ok(Ok(_)) => {},
    Ok(Err(e)) => error!("Broadcast error: {:?}", e),
    Err(_) => warn!("Broadcast timeout for client {}", peer_id_str),
}
```

### Fix 2: Check for Blocking Operations
Add more detailed logging to identify where the freeze happens:
```rust
// In process_game_tick
info!("[Frame {}] Starting physics update", frame);
self.run_physics_update(dt).await;
info!("[Frame {}] Physics complete, starting game logic", frame);
self.run_game_logic_update(dt).await;
info!("[Frame {}] Game logic complete, starting sync", frame);
self.synchronize_state().await;
info!("[Frame {}] Sync complete, starting broadcast", frame);
self.broadcast_world_updates_optimized().await;
info!("[Frame {}] Broadcast complete", frame);
```

### Fix 3: Async Channel Operations
Ensure all channel operations use try_send instead of blocking send:
```rust
// Instead of channel.send(&data).await
if let Err(e) = channel.try_send(&data) {
    warn!("Channel send failed: {:?}", e);
}
```

## Immediate Workaround

To get bots moving immediately without fixing the root cause:

1. **Increase AI update frequency** in `constants.rs`:
```rust
pub const AI_UPDATE_STRIDE: u64 = 1; // Update every frame instead of every 3
```

2. **Use simpler bot AI** that doesn't depend on complex state:
```rust
// In OptimizedBotAI::update_bots_batch
// Add simple movement even if no targets found
if bot_controller.target_position.is_none() {
    // Random patrol
    bot_controller.target_position = Some(Vec2::new(
        rng.gen_range(-500.0..500.0),
        rng.gen_range(-500.0..500.0)
    ));
}
```

3. **Add game loop monitoring**:
```rust
// At the start of game_loop::run
let mut last_tick_logged = Instant::now();

// In the main loop
if last_tick_logged.elapsed() > Duration::from_secs(5) {
    error!("Game loop may be frozen! Last frame: {}", 
           frame_counter.load(Ordering::Relaxed));
    last_tick_logged = Instant::now();
}
```

## Root Cause Analysis

To find the actual cause:

1. **Enable trace logging**:
```bash
RUST_LOG=massive_game_server_core=trace cargo run --release
```

2. **Add frame timestamps**:
```rust
// Log exact time for each frame
info!("[Frame {} @ {:?}] Starting", frame, Instant::now());
```

3. **Check for lock contention**:
```rust
// Use try_lock to detect contention
match some_mutex.try_lock() {
    Ok(guard) => { /* use guard */ },
    Err(_) => warn!("Lock contention detected!"),
}
```

## Emergency Bot Movement

If you need bots to move NOW without fixing the freeze:

1. Create a separate bot movement task that runs independently
2. Use atomic operations instead of locks where possible
3. Implement a "heartbeat" system that detects and recovers from freezes

The key issue is that something in the network/broadcast system is blocking the main game loop after a player connects. Focus debugging efforts there.
