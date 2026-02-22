use criterion::{black_box, criterion_group, criterion_main, Criterion};
use massive_game_server_core::core::simd;

fn bench_filter_indices_within_radius(c: &mut Criterion) {
    let count = 8_192usize;
    let xs: Vec<f32> = (0..count).map(|idx| ((idx % 256) as f32) - 128.0).collect();
    let ys: Vec<f32> = (0..count).map(|idx| ((idx / 256) as f32) - 16.0).collect();
    let mut out = Vec::with_capacity(count);

    c.bench_function("physics/filter_indices_within_radius_8k", |b| {
        b.iter(|| {
            simd::filter_indices_within_radius(
                black_box(&xs),
                black_box(&ys),
                black_box(0.0),
                black_box(0.0),
                black_box(70.0 * 70.0),
                &mut out,
            );
            black_box(out.len())
        })
    });
}

criterion_group!(physics_benches, bench_filter_indices_within_radius);
criterion_main!(physics_benches);
