# Bot Movement Fix Summary

## Current Situation
1. Bots spawn correctly at frame 10
2. Game loop runs normally until frame 6
3. When a player connects via WebRTC, the game loop freezes
4. No more frames are processed after the freeze
5. Bots can't move because the entire simulation has stopped

## Root Cause
The game loop is being blocked by the broadcast system when a player connects. The issue occurs in `process_game_tick` -> `broadcast_world_updates_optimized`.

## Immediate Fix

### Option 1: Add Broadcast Timeout
In `instance.rs`, modify the broadcast timeout handling:

```rust
// In process_game_tick, around the broadcast call
let broadcast_future = tokio::time::timeout(
    Duration::from_millis(50), // 50ms max for broadcast
    server_for_broadcast_call.broadcast_world_updates_optimized()
);

match broadcast_future.await {
    Ok(_) => trace!("[Frame {}] Broadcast completed", frame),
    Err(_) => {
        warn!("[Frame {}] Broadcast timed out - skipping to keep game running", frame);
    }
}
```

### Option 2: Make Broadcast Non-blocking
Spawn the broadcast in a separate task:

```rust
// In process_game_tick
let broadcast_server = Arc::clone(&self);
tokio::spawn(async move {
    broadcast_server.broadcast_world_updates_optimized().await;
});
// Don't await - let it run in background
```

### Option 3: Quick Bot Movement Test
To verify bots work when the loop runs, temporarily disable broadcasts:

```rust
// In process_game_tick, comment out:
// self.broadcast_world_updates_optimized().await;
```

## Long-term Solution

1. **Decouple Network from Game Loop**
   - Move all network operations to separate tasks
   - Use channels for communication
   - Never block the game loop on network I/O

2. **Implement Broadcast Queue**
   - Queue state updates instead of immediate broadcast
   - Process queue in separate task
   - Drop old updates if queue backs up

3. **Add Game Loop Monitoring**
   ```rust
   // In run_game_loop
   let mut last_frame_time = Instant::now();
   
   // After each frame
   if last_frame_time.elapsed() > Duration::from_millis(100) {
       error!("Game loop stalled! Last frame took {:?}", last_frame_time.elapsed());
   }
   last_frame_time = Instant::now();
   ```

## Testing Bot Movement

Once the game loop is running again:

1. **Check Bot AI is Called**
   - AI updates every 3 frames (AI_UPDATE_STRIDE)
   - Look for "Updating bot batch" logs

2. **Verify Bot Decisions**
   - Bots should pick random targets
   - Movement inputs should be generated

3. **Monitor Physics Updates**
   - Bot positions should change each frame
   - Velocities should be applied

## Quick Verification

Add this logging to see if bots are trying to move:

```rust
// In OptimizedBotAI::update_bots_batch
info!("Bot {} at ({:.1}, {:.1}) moving to ({:.1}, {:.1})", 
      bot_id, bot_state.x, bot_state.y, 
      target_pos.x, target_pos.y);
```

## Emergency Bot Test

If you need to see bots move RIGHT NOW:

1. Comment out the broadcast call in process_game_tick
2. Restart the server
3. Don't connect any clients
4. Watch the logs - bots should move every frame

The key issue is not the bot AI - it's that the game loop is frozen by network operations. Fix the blocking broadcast and the bots will move.
