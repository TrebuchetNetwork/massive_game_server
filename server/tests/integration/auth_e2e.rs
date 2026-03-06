use massive_game_server_core::operational::auth::AuthService;
use massive_game_server_core::operational::config::env_registry::AuthEnv;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::Value;
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct ServerProcess {
    child: Child,
    base_url: String,
    auth_store_path: PathBuf,
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

async fn spawn_server_with_sms_capture(data_root: PathBuf) -> ServerProcess {
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
        auth_store_path,
        sms_capture_path,
    };
    wait_until_ready(&proc.base_url).await;
    proc
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

fn auth_env_for_store(store_path: &Path) -> AuthEnv {
    AuthEnv {
        store_path: store_path.display().to_string(),
        otp_ttl_seconds: 300,
        session_ttl_seconds: 86_400,
        resend_interval_seconds: 30,
        max_verify_attempts: 5,
        token_validation_rate_limit_per_sec: 24,
        token_validation_rate_limit_burst: 48,
        sms_command: None,
        sms_dev_mode: false,
        use_auth_cookies: false,
        deletion_grace_period_hours: 1,
        redis_url: None,
        redis_store_key: "mgs_auth_store".to_owned(),
        gdpr_hash_salt: Some("auth-e2e-salt".to_owned()),
    }
}

fn force_pending_deletion_expired(store_path: &Path, user_id: &str) {
    let raw = fs::read_to_string(store_path).expect("read auth store");
    let mut json: Value = serde_json::from_str(&raw).expect("parse auth store json");
    let scheduled = json
        .get_mut("pending_deletions")
        .and_then(|value| value.get_mut(user_id))
        .and_then(|value| value.as_object_mut())
        .expect("pending deletion record");
    scheduled.insert("scheduled_deletion_time".to_owned(), Value::from(0_u64));
    fs::write(
        store_path,
        serde_json::to_vec_pretty(&json).expect("serialize auth store"),
    )
    .expect("write auth store");
}

async fn wait_for_pending_deletion_store_state(store_path: &Path, user_id: &str) {
    for _ in 0..80 {
        if let Ok(raw) = fs::read_to_string(store_path) {
            if let Ok(json) = serde_json::from_str::<Value>(&raw) {
                let present = json
                    .get("pending_deletions")
                    .and_then(|value| value.get(user_id))
                    .is_some();
                if present {
                    return;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!(
        "timed out waiting for pending deletion state in {}",
        store_path.display()
    );
}

async fn wait_for_deleted_store_state(store_path: &Path, user_id: &str) -> Value {
    for _ in 0..80 {
        if let Ok(raw) = fs::read_to_string(store_path) {
            if let Ok(json) = serde_json::from_str::<Value>(&raw) {
                let deleted = json
                    .get("users")
                    .and_then(|users| users.get(user_id))
                    .and_then(|user| user.get("deleted"))
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if deleted {
                    return json;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!(
        "timed out waiting for deleted user state in {}",
        store_path.display()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn otp_session_logout_and_gdpr_lifecycle_hold_end_to_end() {
    let phone = "+15555550111";
    let data_root =
        std::env::temp_dir().join(format!("mgs_auth_e2e_{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir_all(&data_root).expect("create auth e2e temp dir");

    let proc = spawn_server_with_sms_capture(data_root.clone()).await;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("build reqwest client");

    request_code(&client, &proc.base_url, phone).await;
    let otp_code = wait_for_sms_code(&proc.sms_capture_path).await;
    let verify_payload = verify_code(&client, &proc.base_url, phone, &otp_code).await;
    let token = verify_payload["data"]["token"]
        .as_str()
        .expect("session token")
        .to_owned();
    let user_id = verify_payload["data"]["profile"]["user_id"]
        .as_str()
        .expect("user_id")
        .to_owned();

    let auth_me = client
        .get(format!("{}/auth/me", proc.base_url))
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .send()
        .await
        .expect("GET /auth/me");
    assert!(auth_me.status().is_success());

    let logout = client
        .post(format!("{}/auth/logout", proc.base_url))
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .send()
        .await
        .expect("POST /auth/logout");
    assert!(logout.status().is_success());
    let logout_json: Value = logout.json().await.expect("logout json");
    assert_eq!(logout_json["ok"], Value::Bool(true));
    assert_eq!(logout_json["data"]["revoked"], Value::Bool(true));

    let me_after_logout = client
        .get(format!("{}/auth/me", proc.base_url))
        .header(AUTHORIZATION, format!("Bearer {token}"))
        .send()
        .await
        .expect("GET /auth/me after logout");
    assert_eq!(me_after_logout.status(), reqwest::StatusCode::UNAUTHORIZED);

    let _ = fs::remove_file(&proc.sms_capture_path);
    request_code(&client, &proc.base_url, phone).await;
    let otp_code = wait_for_sms_code(&proc.sms_capture_path).await;
    let verify_payload = verify_code(&client, &proc.base_url, phone, &otp_code).await;
    let fresh_token = verify_payload["data"]["token"]
        .as_str()
        .expect("fresh session token")
        .to_owned();

    let delete_account = client
        .post(format!("{}/auth/delete-account", proc.base_url))
        .header(AUTHORIZATION, format!("Bearer {fresh_token}"))
        .send()
        .await
        .expect("POST /auth/delete-account");
    assert!(delete_account.status().is_success());

    let cancel_delete = client
        .post(format!("{}/auth/cancel-deletion", proc.base_url))
        .header(AUTHORIZATION, format!("Bearer {fresh_token}"))
        .send()
        .await
        .expect("POST /auth/cancel-deletion");
    assert!(cancel_delete.status().is_success());

    let me_after_cancel = client
        .get(format!("{}/auth/me", proc.base_url))
        .header(AUTHORIZATION, format!("Bearer {fresh_token}"))
        .send()
        .await
        .expect("GET /auth/me after cancel");
    assert!(me_after_cancel.status().is_success());

    let delete_account_again = client
        .post(format!("{}/auth/delete-account", proc.base_url))
        .header(AUTHORIZATION, format!("Bearer {fresh_token}"))
        .send()
        .await
        .expect("POST /auth/delete-account again");
    assert!(delete_account_again.status().is_success());

    wait_for_pending_deletion_store_state(&proc.auth_store_path, &user_id).await;
    let store_path = proc.auth_store_path.clone();
    drop(proc);

    force_pending_deletion_expired(&store_path, &user_id);
    let auth_service = AuthService::new_from_env_config(&auth_env_for_store(&store_path));
    assert_eq!(auth_service.process_pending_deletions(), 1);

    let store_json = wait_for_deleted_store_state(&store_path, &user_id).await;
    let user = &store_json["users"][&user_id];
    assert_eq!(user["deleted"], Value::Bool(true));
    let phone_number = user["phone_number"].as_str().expect("deleted phone hash");
    assert!(phone_number.starts_with("deleted:"));

    let proc = spawn_server_with_sms_capture(data_root).await;
    let _ = fs::remove_file(&proc.sms_capture_path);
    request_code(&client, &proc.base_url, phone).await;
    let otp_code = wait_for_sms_code(&proc.sms_capture_path).await;
    let deleted_login = client
        .post(format!("{}/auth/phone/verify-code", proc.base_url))
        .header(CONTENT_TYPE, "application/json")
        .body(format!(
            r#"{{"phone_number":"{phone}","code":"{otp_code}"}}"#
        ))
        .send()
        .await
        .expect("verify-code after deletion");
    assert_eq!(deleted_login.status(), reqwest::StatusCode::GONE);
    let deleted_login_json: Value = deleted_login.json().await.expect("deleted login json");
    assert_eq!(
        deleted_login_json["error"]["code"],
        Value::String("account_deleted".to_owned())
    );
}
