# Running Cargo with Debug/Trace Messages

## Prerequisites
First, ensure Rust and Cargo are installed. If not, install from https://rustup.rs/

## Setting Log Levels

There are several ways to run your Rust application with debug/trace messages:

### 1. Using RUST_LOG Environment Variable

#### Windows (PowerShell)
```powershell
# Set for current session
$env:RUST_LOG="trace"
cargo run

# Or inline
$env:RUST_LOG="trace"; cargo run

# For specific modules
$env:RUST_LOG="your_crate_name=trace"
cargo run

# Multiple modules with different levels
$env:RUST_LOG="your_crate=debug,hyper=info,tokio=trace"
cargo run
```

#### Windows (Command Prompt)
```cmd
# Set for current session
set RUST_LOG=trace
cargo run

# Or inline (requires && operator)
set RUST_LOG=trace && cargo run
```

#### Linux/Mac
```bash
# Inline
RUST_LOG=trace cargo run

# Or export for session
export RUST_LOG=trace
cargo run
```

### 2. Log Levels Available

- `error` - Only errors
- `warn` - Warnings and errors
- `info` - Info, warnings, and errors
- `debug` - Debug info and all above
- `trace` - Most verbose, includes all logging

### 3. Module-Specific Logging

You can target specific modules:
```powershell
# Only your application's trace logs
$env:RUST_LOG="massive_game_server=trace"

# Mix different levels for different modules
$env:RUST_LOG="massive_game_server=trace,actix_web=info,tokio=debug"

# Target specific module paths
$env:RUST_LOG="massive_game_server::server=trace,massive_game_server::network=debug"
```

### 4. Using with Release Builds

Debug/trace logs are typically only available in debug builds. For release builds:
```powershell
# Run in release mode with logs
$env:RUST_LOG="trace"
cargo run --release
```

### 5. Pretty Printing with env_logger

If your project uses `env_logger`, you can enable pretty printing:
```powershell
$env:RUST_LOG="trace"
$env:RUST_LOG_STYLE="always"  # For colored output
cargo run
```

### 6. Filtering by Target

You can filter logs by file/module:
```powershell
# Only logs from specific file
$env:RUST_LOG="massive_game_server/server/src/main.rs=trace"
```

### 7. Common Debugging Commands

```powershell
# Most common for debugging
$env:RUST_LOG="debug"; cargo run

# Full trace (very verbose)
$env:RUST_LOG="trace"; cargo run

# Your app trace, dependencies info
$env:RUST_LOG="massive_game_server=trace,info"; cargo run

# Specific subsystem debugging
$env:RUST_LOG="massive_game_server::concurrent=trace"; cargo run
```

## Implementing Logging in Your Code

Make sure your Rust code has logging set up:

```rust
// In main.rs or lib.rs
use log::{debug, error, info, trace, warn};

fn main() {
    // Initialize logger (using env_logger)
    env_logger::init();
    
    // Now you can use logging macros
    trace!("This is a trace message");
    debug!("This is a debug message");
    info!("This is an info message");
    warn!("This is a warning");
    error!("This is an error");
}
```

Add to Cargo.toml:
```toml
[dependencies]
log = "0.4"
env_logger = "0.10"
```

## Quick Start Examples

1. **See all trace messages from your app:**
   ```powershell
   cd massive_game_server/server
   $env:RUST_LOG="trace"; cargo run
   ```

2. **Debug specific module:**
   ```powershell
   $env:RUST_LOG="massive_game_server::concurrent::wall_spatial_index=trace"; cargo run
   ```

3. **Mixed verbosity:**
   ```powershell
   $env:RUST_LOG="massive_game_server=debug,tokio=warn,hyper=info"; cargo run
