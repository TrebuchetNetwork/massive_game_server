use reqwest::header::CONTENT_TYPE;
use serde_json::Value;
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct ServerProcess {
    child: Child,
    base_url: String,
    sms_capture_path: PathBuf,
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
        let health_ready = client
            .get(format!("{base_url}/healthz"))
            .send()
            .await
            .ok()
            .map(|resp| resp.status().is_success())
            .unwrap_or(false);
        let gameplay_ready = client
            .get(format!("{base_url}/readyz"))
            .send()
            .await
            .ok()
            .map(|resp| resp.status().is_success())
            .unwrap_or(false);
        if health_ready && gameplay_ready {
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("server did not become ready at {base_url}");
}

fn sms_capture_script_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../scripts/test_support/capture_sms.sh")
        .canonicalize()
        .expect("canonicalize SMS capture script")
}

async fn spawn_server_with_store(data_root: &Path) -> ServerProcess {
    let port = reserve_free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let auth_store_path = data_root.join("auth_store.json");
    let sms_capture_path = data_root.join("otp_code.txt");
    let arena_wasm_dir = data_root.join("arena_wasm");
    let arena_source_dir = data_root.join("arena_sources");
    let _ = fs::create_dir_all(&arena_wasm_dir);
    let _ = fs::create_dir_all(&arena_source_dir);
    let _ = fs::remove_file(&sms_capture_path);

    let mut command = Command::new(env!("CARGO_BIN_EXE_massive_game_server_core"));
    command
        .env("MGS_HOST", "127.0.0.1")
        .env("MGS_PORT", port.to_string())
        .env("MGS_DISABLE_STUN", "1")
        .env("MGS_TARGET_BOT_COUNT", "0")
        .env("MGS_DIAGNOSTICS_ENABLED", "0")
        .env("MGS_QUIC_PRIMARY", "0")
        .env("MGS_SMS_COMMAND", sms_capture_script_path())
        .env("MGS_TEST_SMS_CAPTURE_PATH", &sms_capture_path)
        .env("MGS_AUTH_STORE_PATH", &auth_store_path)
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
    let proc = ServerProcess {
        child,
        base_url,
        sms_capture_path,
    };
    wait_until_ready(&proc.base_url).await;
    proc
}

async fn request_code(client: &reqwest::Client, base_url: &str, phone: &str) {
    let response = client
        .post(format!("{base_url}/auth/phone/request-code"))
        .header(CONTENT_TYPE, "application/json")
        .body(format!(r#"{{"phone_number":"{phone}"}}"#))
        .send()
        .await
        .expect("POST /auth/phone/request-code");
    assert!(
        response.status().is_success(),
        "request-code failed with status {}",
        response.status()
    );
}

async fn wait_for_sms_code(path: &Path) -> String {
    for _ in 0..120 {
        if let Ok(contents) = fs::read_to_string(path) {
            if let Some(code) = contents
                .split(|ch: char| !ch.is_ascii_digit())
                .find(|part| part.len() == 6)
            {
                return code.to_owned();
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("timed out waiting for OTP capture at {}", path.display());
}

async fn verify_code(client: &reqwest::Client, base_url: &str, phone: &str, code: &str) -> Value {
    let response = client
        .post(format!("{base_url}/auth/phone/verify-code"))
        .header(CONTENT_TYPE, "application/json")
        .body(format!(r#"{{"phone_number":"{phone}","code":"{code}"}}"#))
        .send()
        .await
        .expect("POST /auth/phone/verify-code");
    let status = response.status();
    let payload: Value = response.json().await.expect("verify-code json");
    assert!(status.is_success(), "verify-code failed: {payload}");
    payload
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auth_store_survives_server_restart() {
    let phone = "+15555550131";
    let data_root =
        std::env::temp_dir().join(format!("mgs_persistence_{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir_all(&data_root).expect("create persistence temp dir");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("build reqwest client");

    let proc = spawn_server_with_store(&data_root).await;
    request_code(&client, &proc.base_url, phone).await;
    let otp_code = wait_for_sms_code(&proc.sms_capture_path).await;
    let verify_payload = verify_code(&client, &proc.base_url, phone, &otp_code).await;
    let first_user_id = verify_payload["data"]["profile"]["user_id"]
        .as_str()
        .expect("first user_id")
        .to_owned();

    drop(proc);

    let proc = spawn_server_with_store(&data_root).await;
    request_code(&client, &proc.base_url, phone).await;
    let otp_code = wait_for_sms_code(&proc.sms_capture_path).await;
    let verify_payload = verify_code(&client, &proc.base_url, phone, &otp_code).await;
    let second_user_id = verify_payload["data"]["profile"]["user_id"]
        .as_str()
        .expect("second user_id")
        .to_owned();

    assert_eq!(
        second_user_id, first_user_id,
        "auth store should keep the same user record across restarts"
    );
}
