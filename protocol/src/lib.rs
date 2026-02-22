pub const GAME_PROTOCOL_VERSION: u32 = 1;

#[allow(dead_code)]
#[allow(clippy::all)]
#[allow(warnings)]
pub mod flatbuffers_generated {
    include!(concat!(
        env!("OUT_DIR"),
        "/flatbuffers_generated/game_generated.rs"
    ));
}

pub use flatbuffers_generated::game_protocol;
