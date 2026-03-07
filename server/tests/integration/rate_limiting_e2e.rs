use std::fs;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use tokio_tungstenite::tungstenite::Error as WsError;

struct ServerProcess {
    child: Child,
    ws_url: String,
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

async fn spawn_server(extra_env: &[(&str, &str)]) -> ServerProcess {
    let port = reserve_free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let ws_url = format!("ws://127.0.0.1:{port}/ws?username=rate-limit-a");
    let data_root = std::env::temp_dir().join(format!("mgs_rate_limiting_e2e_{port}"));
    let arena_wasm_dir = data_root.join("arena_wasm");
    let arena_source_dir = data_root.join("arena_sources");
    let _ = fs::create_dir_all(&arena_wasm_dir);
    let _ = fs::create_dir_all(&arena_source_dir);

    let mut command = Command::new(env!("CARGO_BIN_EXE_massive_game_server_core"));
    command
        .env("MGS_HOST", "127.0.0.1")
        .env("MGS_PORT", port.to_string())
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
    for (key, value) in extra_env {
        command.env(key, value);
    }

    let child = command.spawn().expect("spawn game server binary");
    wait_until_ready(&base_url).await;
    ServerProcess { child, ws_url }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_connection_cap_rejects_second_upgrade() {
    let proc = spawn_server(&[("MGS_MAX_CONCURRENT_CONNECTIONS", "1")]).await;

    let (first_ws, _) = tokio_tungstenite::connect_async(proc.ws_url.clone())
        .await
        .expect("first websocket upgrade should succeed");

    let second_ws_url = proc.ws_url.replace("rate-limit-a", "rate-limit-b");
    let second_attempt = tokio_tungstenite::connect_async(second_ws_url)
        .await
        .expect_err("second websocket upgrade should be rejected by connection cap");

    match second_attempt {
        WsError::Http(response) => {
            assert_eq!(response.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
        }
        other => panic!("unexpected websocket rejection error: {other}"),
    }

    let mut first_ws = first_ws;
    let _ = first_ws.close(None).await;
}
