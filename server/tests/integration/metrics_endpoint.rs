use reqwest::StatusCode;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::Duration;
use tokio_tungstenite::connect_async;

struct ServerProcess {
    child: Child,
    base_url: String,
    ws_url: String,
    metrics_url: String,
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn reserve_free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral port");
    let port = listener.local_addr().expect("local addr").port();
    drop(listener);
    port
}

async fn wait_until_ready(base_url: &str) {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .expect("build client");
    for _ in 0..120 {
        if let Ok(resp) = client.get(format!("{base_url}/readyz")).send().await {
            if resp.status().is_success() {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("server did not become ready at {base_url}");
}

async fn spawn_server_with_metrics() -> ServerProcess {
    let app_port = reserve_free_port();
    let metrics_port = reserve_free_port();
    let base_url = format!("http://127.0.0.1:{app_port}");
    let ws_url = format!("ws://127.0.0.1:{app_port}/ws");
    let metrics_url = format!("http://127.0.0.1:{metrics_port}/metrics");
    let data_root = std::env::temp_dir().join(format!("mgs_metrics_endpoint_{app_port}"));
    let arena_wasm_dir = data_root.join("arena_wasm");
    let arena_source_dir = data_root.join("arena_sources");
    let _ = std::fs::create_dir_all(&arena_wasm_dir);
    let _ = std::fs::create_dir_all(&arena_source_dir);

    let mut command = Command::new(env!("CARGO_BIN_EXE_massive_game_server_core"));
    command
        .env("MGS_HOST", "127.0.0.1")
        .env("MGS_PORT", app_port.to_string())
        .env("MGS_METRICS_ENABLED", "1")
        .env("MGS_METRICS_BIND_ADDR", format!("127.0.0.1:{metrics_port}"))
        .env("MGS_DISABLE_STUN", "1")
        .env("MGS_TARGET_BOT_COUNT", "0")
        .env("MGS_DIAGNOSTICS_ENABLED", "0")
        .env("MGS_QUIC_PRIMARY", "0")
        .env("MGS_AUTH_STORE_PATH", data_root.join("auth_store.json"))
        .env(
            "MGS_FEATURE_FLAG_STORE_PATH",
            data_root.join("feature_flags_store.json"),
        )
        .env("MGS_ARENA_STORE_PATH", data_root.join("arena_store.json"))
        .env("MGS_ARENA_WASM_DIR", &arena_wasm_dir)
        .env("MGS_ARENA_SOURCE_DIR", &arena_source_dir)
        .env("RUST_LOG", "warn")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let child = command.spawn().expect("spawn game server binary");
    let process = ServerProcess {
        child,
        base_url,
        ws_url,
        metrics_url,
    };
    wait_until_ready(&process.base_url).await;
    process
}

async fn scrape_metrics(url: &str) -> String {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("build metrics client");
    for _ in 0..40 {
        if let Ok(response) = client.get(url).send().await {
            if response.status() == StatusCode::OK {
                let body = response.text().await.expect("metrics body");
                if !body.trim().is_empty() {
                    return body;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("metrics endpoint did not return data at {url}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn metrics_endpoint_exposes_expected_prometheus_metrics() {
    let process = spawn_server_with_metrics().await;

    let (_ws_stream, _) = connect_async(process.ws_url.clone())
        .await
        .expect("open websocket signaling connection");
    tokio::time::sleep(Duration::from_millis(500)).await;

    let metrics = scrape_metrics(&process.metrics_url).await;
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
