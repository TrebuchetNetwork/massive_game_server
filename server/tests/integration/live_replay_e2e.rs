use reqwest::header::AUTHORIZATION;
use serde_json::Value;
use std::fs;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct ServerProcess {
    child: Child,
    base_url: String,
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

async fn spawn_server(admin_token: &str) -> ServerProcess {
    let port = reserve_free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let data_root = std::env::temp_dir().join(format!("mgs_live_replay_e2e_{port}"));
    let arena_wasm_dir = data_root.join("arena_wasm");
    let arena_source_dir = data_root.join("arena_sources");
    let _ = fs::create_dir_all(&arena_wasm_dir);
    let _ = fs::create_dir_all(&arena_source_dir);

    let mut command = Command::new(env!("CARGO_BIN_EXE_massive_game_server_core"));
    command
        .env("MGS_HOST", "127.0.0.1")
        .env("MGS_PORT", port.to_string())
        .env("MGS_DISABLE_STUN", "1")
        .env("MGS_TARGET_BOT_COUNT", "4")
        .env("MGS_DIAGNOSTICS_ENABLED", "0")
        .env("MGS_QUIC_PRIMARY", "0")
        .env("MGS_LIVE_REPLAY_ENABLED", "1")
        .env("MGS_LIVE_REPLAY_DISPUTE_PERSIST", "1")
        .env(
            "MGS_LIVE_REPLAY_DISPUTE_SIGNING_KEY",
            "live-replay-e2e-secret",
        )
        .env("MGS_ADMIN_BEARER_TOKEN", admin_token)
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
    let proc = ServerProcess { child, base_url };
    wait_until_ready(&proc.base_url).await;
    proc
}

async fn wait_for_recent_frames(
    client: &reqwest::Client,
    base_url: &str,
    admin_token: &str,
) -> Value {
    for _ in 0..80 {
        let response = client
            .get(format!("{base_url}/api/ops/live-replay/recent?limit=4"))
            .header(AUTHORIZATION, format!("Bearer {admin_token}"))
            .send()
            .await
            .expect("GET live replay recent");
        if response.status().is_success() {
            let payload: Value = response.json().await.expect("live replay recent json");
            let frames = payload["frames"].as_array().cloned().unwrap_or_default();
            if frames
                .iter()
                .any(|frame| frame["players"].as_u64().unwrap_or(0) >= 2)
            {
                return payload;
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("timed out waiting for populated live replay frames");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dispute_reports_form_a_signed_chain_over_live_frames() {
    let admin_token = "live-replay-e2e-admin";
    let proc = spawn_server(admin_token).await;
    let client = reqwest::Client::new();

    let recent_payload = wait_for_recent_frames(&client, &proc.base_url, admin_token).await;
    let recent_frames = recent_payload["frames"].as_array().expect("frames array");
    assert!(!recent_frames.is_empty());
    assert!(recent_frames
        .iter()
        .any(|frame| frame["players"].as_u64().unwrap_or(0) >= 2));

    let first_response = client
        .post(format!("{}/api/ops/live-replay/dispute", proc.base_url))
        .header(AUTHORIZATION, format!("Bearer {admin_token}"))
        .json(&serde_json::json!({"limit": 2}))
        .send()
        .await
        .expect("POST first dispute");
    assert!(first_response.status().is_success());
    let first_report: Value = first_response.json().await.expect("first dispute json");
    let first_audit = &first_report["audit"];
    let first_chain = first_audit["chain_hash_sha256"]
        .as_str()
        .expect("first chain hash")
        .to_owned();
    let first_signature = first_audit["signature_hmac_sha256"]
        .as_str()
        .expect("first signature")
        .to_owned();
    assert_eq!(first_chain.len(), 64);
    assert_eq!(first_signature.len(), 64);

    tokio::time::sleep(Duration::from_millis(250)).await;

    let second_response = client
        .post(format!("{}/api/ops/live-replay/dispute", proc.base_url))
        .header(AUTHORIZATION, format!("Bearer {admin_token}"))
        .json(&serde_json::json!({"limit": 3}))
        .send()
        .await
        .expect("POST second dispute");
    assert!(second_response.status().is_success());
    let second_report: Value = second_response.json().await.expect("second dispute json");
    let second_audit = &second_report["audit"];
    assert_eq!(
        second_audit["chain_prev_hash_sha256"].as_str(),
        Some(first_chain.as_str())
    );
    assert_eq!(
        second_audit["signature_hmac_sha256"].as_str().map(str::len),
        Some(64)
    );

    let audits_response = client
        .get(format!(
            "{}/api/ops/live-replay/disputes/recent?limit=2",
            proc.base_url
        ))
        .header(AUTHORIZATION, format!("Bearer {admin_token}"))
        .send()
        .await
        .expect("GET disputes recent");
    assert!(audits_response.status().is_success());
    let audits_payload: Value = audits_response.json().await.expect("recent disputes json");
    let audits = audits_payload["audits"].as_array().expect("audits array");
    assert!(audits.len() >= 2);
    assert_eq!(
        audits[0]["chain_prev_hash_sha256"].as_str(),
        Some(first_chain.as_str())
    );
}
