use std::sync::Arc;
use std::time::Instant;
use metrics::histogram;
use massive_game_server_core::server::instance::MassiveGameServer;

#[tokio::test]
async fn stress_test_game_tick() {
    let server = Arc::new(setup_test_server());
    let delta_time = 1.0 / 60.0;
    for _ in 0..1000 {
        let start = Instant::now();
        server.clone().process_game_tick(delta_time).await.unwrap();
        histogram!("game_tick_duration_ms").record(start.elapsed().as_secs_f64() * 1000.0);
    }
}