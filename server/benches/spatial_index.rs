use criterion::{black_box, criterion_group, criterion_main, Criterion};
use massive_game_server_core::concurrent::spatial_index::ImprovedSpatialIndex;
use std::sync::Arc;

fn bench_nearby_player_query(c: &mut Criterion) {
    let index = ImprovedSpatialIndex::new(1600.0, 1200.0, -800.0, -600.0, 64.0);
    let player_count = 5_000usize;
    for idx in 0..player_count {
        let x = -760.0 + ((idx % 100) as f32) * 15.0;
        let y = -560.0 + ((idx / 100) as f32) * 20.0;
        index.update_player_position(Arc::<str>::from(format!("bench_player_{idx}")), x, y);
    }

    c.bench_function("spatial_index/query_nearby_players_5k", |b| {
        let mut tick = 0usize;
        b.iter(|| {
            tick = tick.wrapping_add(1);
            let x = -800.0 + ((tick % 1600) as f32);
            let y = -600.0 + ((tick % 1200) as f32);
            let nearby = index.query_nearby_players(black_box(x), black_box(y), black_box(220.0));
            black_box(nearby.len())
        })
    });
}

fn bench_batch_projectile_updates(c: &mut Criterion) {
    let index = ImprovedSpatialIndex::new(1600.0, 1200.0, -800.0, -600.0, 64.0);
    let mut updates = Vec::with_capacity(2_000);
    for idx in 0..2_000usize {
        let x = -700.0 + ((idx % 120) as f32) * 11.0;
        let y = -500.0 + ((idx / 120) as f32) * 19.0;
        updates.push((idx as u64, x, y));
    }

    c.bench_function("spatial_index/batch_update_projectiles_2k", |b| {
        b.iter(|| {
            index.batch_update_projectiles(black_box(&updates));
            black_box(index.query_nearby_projectiles(0.0, 0.0, 300.0).len())
        })
    });
}

criterion_group!(
    spatial_index_benches,
    bench_nearby_player_query,
    bench_batch_projectile_updates
);
criterion_main!(spatial_index_benches);
