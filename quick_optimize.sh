#!/bin/bash
# Quick optimizations based on flamegraph analysis

echo "Applying quick optimizations..."

# Update thread configuration
sed -i 's/ai_threads: 30/ai_threads: 80/' server/src/core/config.rs

# Add AI frame skipping
echo '
// AI Optimization: Skip frames
pub const AI_UPDATE_INTERVAL: u32 = 3;  // Update AI every 3 frames
pub const MAX_BOTS_PER_FRAME: usize = 200;  // Limit bots processed per frame
' >> server/src/core/config.rs

echo "Rebuilding with optimizations..."
cd server && cargo build --release

echo "Done! Restart your server to apply changes."
