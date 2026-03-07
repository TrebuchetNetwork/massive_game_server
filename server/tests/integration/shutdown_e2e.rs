use serde_json::Value;
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct ServerProcess {
    child: Child,
    shutdown_state_path: PathBuf,
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

async fn spawn_server() -> ServerProcess {
    let port = reserve_free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let data_root = std::env::temp_dir().join(format!("mgs_shutdown_e2e_{port}"));
    let shutdown_state_path = data_root.join("shutdown_state.json");
    let arena_wasm_dir = data_root.join("arena_wasm");
    let arena_source_dir = data_root.join("arena_sources");
    let _ = fs::create_dir_all(&arena_wasm_dir);
    let _ = fs::create_dir_all(&arena_source_dir);

    let mut command = Command::new(env!("CARGO_BIN_EXE_massive_game_server_core"));
    command
        .env("MGS_HOST", "127.0.0.1")
        .env("MGS_PORT", port.to_string())
        .env("MGS_DISABLE_STUN", "1")
        .env("MGS_TARGET_BOT_COUNT", "2")
        .env("MGS_DIAGNOSTICS_ENABLED", "0")
        .env("MGS_QUIC_PRIMARY", "0")
        .env("MGS_SHUTDOWN_STATE_PATH", &shutdown_state_path)
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
    wait_until_ready(&base_url).await;
    ServerProcess {
        child,
        shutdown_state_path,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sigint_persists_shutdown_state_snapshot() {
    let mut proc = spawn_server().await;

    let signal_result = unsafe { libc::kill(proc.child.id() as i32, libc::SIGINT) };
    assert_eq!(signal_result, 0, "SIGINT delivery should succeed");

    for _ in 0..80 {
        if let Some(status) = proc.child.try_wait().expect("poll child exit") {
            assert!(
                status.success(),
                "server should exit cleanly after SIGINT: {status}"
            );
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    assert!(
        proc.shutdown_state_path.exists(),
        "shutdown snapshot should be written to {}",
        proc.shutdown_state_path.display()
    );
    let snapshot: Value = serde_json::from_slice(
        &fs::read(&proc.shutdown_state_path).expect("read shutdown state json"),
    )
    .expect("parse shutdown state json");
    assert!(snapshot["match_summary"].is_object());
    assert!(snapshot["population"].is_object());
    assert!(snapshot["entities"].is_object());
    assert!(snapshot["players"].is_array());
    assert!(
        snapshot["population"]["total_players"]
            .as_u64()
            .unwrap_or(0)
            >= 1
    );
}
