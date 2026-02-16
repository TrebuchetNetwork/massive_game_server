// massive_game_server/server/src/lib.rs

// Make flatbuffers generated code accessible throughout the crate
pub mod flatbuffers_generated {
    // The include! macro will paste the contents of game_generated.rs here during compilation.
    // The path is constructed relative to the OUT_DIR environment variable.
    include!(concat!(
        env!("OUT_DIR"),
        "/flatbuffers_generated/game_generated.rs"
    ));
}

// Re-export or declare other public modules of your library here
pub mod concurrent;
pub mod core;
pub mod entities;
pub mod memory;
pub mod network;
pub mod operational;
pub mod server;
pub mod state_sync;
pub mod systems;
pub mod world; // Assuming signaling.rs is in here

// Example re-exports if you want to shorten paths for users of this library:
// pub use crate::core::types::PlayerId;
// pub use crate::server::instance::MassiveGameServer;
