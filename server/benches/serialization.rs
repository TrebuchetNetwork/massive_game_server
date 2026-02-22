use criterion::{black_box, criterion_group, criterion_main, Criterion};
use massive_game_server_core::flatbuffers_generated::game_protocol as fb;

const BENCH_PROTOCOL_VERSION: u32 = 1;

fn build_welcome_message_bytes(player_id: &str, server_tick_rate: u16) -> Vec<u8> {
    let mut builder = flatbuffers::FlatBufferBuilder::with_capacity(256);
    let player_id_fb = builder.create_string(player_id);
    let message_fb = builder.create_string("Welcome to MassiveGameServer!");
    let payload = fb::WelcomeMessage::create(
        &mut builder,
        &fb::WelcomeMessageArgs {
            player_id: Some(player_id_fb),
            message: Some(message_fb),
            server_tick_rate,
        },
    );
    let root = fb::GameMessage::create(
        &mut builder,
        &fb::GameMessageArgs {
            msg_type: fb::MessageType::Welcome,
            actual_message_type: fb::MessagePayload::WelcomeMessage,
            actual_message: Some(payload.as_union_value()),
            protocol_version: BENCH_PROTOCOL_VERSION,
        },
    );
    builder.finish(root, None);
    let (buffer, root_index) = builder.collapse();
    buffer[root_index..].to_vec()
}

fn bench_build_welcome_message(c: &mut Criterion) {
    c.bench_function("serialization/build_welcome_message", |b| {
        b.iter(|| {
            let bytes = build_welcome_message_bytes(black_box("bench-player"), black_box(60));
            black_box(bytes.len())
        })
    });
}

fn bench_parse_welcome_message(c: &mut Criterion) {
    let bytes = build_welcome_message_bytes("bench-player", 60);
    c.bench_function("serialization/parse_welcome_message", |b| {
        b.iter(|| {
            let message = fb::root_as_game_message(black_box(bytes.as_slice()))
                .expect("valid flatbuffer payload in benchmark fixture");
            black_box(message.msg_type())
        })
    });
}

criterion_group!(
    serialization_benches,
    bench_build_welcome_message,
    bench_parse_welcome_message
);
criterion_main!(serialization_benches);
