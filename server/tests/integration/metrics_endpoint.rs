#[path = "../common/helpers.rs"]
mod helpers;

use helpers::{scrape_metrics, spawn_server};
use tokio_tungstenite::connect_async;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn metrics_endpoint_exposes_expected_prometheus_metrics() {
    let process = spawn_server("mgs_metrics_endpoint", &[], true).await;
    let metrics_url = process
        .metrics_url
        .as_deref()
        .expect("metrics url should be configured for metrics test");

    let (_ws_stream, _) = connect_async(format!("{}?username=metrics-check", process.ws_url))
        .await
        .expect("open websocket signaling connection");
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let metrics = scrape_metrics(metrics_url).await;
    assert!(
        metrics.contains("game_frames_total"),
        "expected game_frames_total in metrics scrape"
    );
    assert!(
        metrics.contains("game_frame_time_seconds"),
        "expected game_frame_time_seconds in metrics scrape"
    );
    assert!(
        metrics.contains("game_players_connected"),
        "expected game_players_connected in metrics scrape"
    );
    assert!(
        metrics.contains("game_match_degraded"),
        "expected game_match_degraded in metrics scrape"
    );
    assert!(
        metrics.contains("game_ws_connections_active"),
        "expected game_ws_connections_active in metrics scrape"
    );

    for line in metrics.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let fields = trimmed.split_whitespace().collect::<Vec<_>>();
        assert!(
            fields.len() >= 2,
            "invalid Prometheus exposition line: {trimmed}"
        );
    }
}
