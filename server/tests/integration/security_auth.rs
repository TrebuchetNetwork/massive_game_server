use reqwest::header::{CONTENT_TYPE, COOKIE, SET_COOKIE};
use serde_json::Value;
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

struct ServerProcess {
    child: Child,
    base_url: String,
    ws_url: String,
    sms_capture_path: PathBuf,
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn auth_test_mutex() -> &'static tokio::sync::Mutex<()> {
    static AUTH_TEST_MUTEX: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    AUTH_TEST_MUTEX.get_or_init(|| tokio::sync::Mutex::new(()))
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

fn sms_capture_script_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../scripts/test_support/capture_sms.sh")
        .canonicalize()
        .expect("canonicalize SMS capture script")
}

async fn spawn_server_with_sms_capture(
    data_root: PathBuf,
    cookie_mode: bool,
    require_ws_auth: bool,
    behind_tls_proxy: bool,
) -> ServerProcess {
    let port = reserve_free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let ws_url = format!("ws://127.0.0.1:{port}/ws");
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
        .env("MGS_AUTH_USE_COOKIES", if cookie_mode { "1" } else { "0" })
        .env("MGS_REQUIRE_AUTH", if require_ws_auth { "1" } else { "0" })
        .env(
            "MGS_BEHIND_TLS_PROXY",
            if behind_tls_proxy { "1" } else { "0" },
        )
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

    let proc = ServerProcess {
        child,
        base_url,
        ws_url,
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

async fn request_code(client: &reqwest::Client, base_url: &str, phone: &str) -> reqwest::Response {
    client
        .post(format!("{base_url}/auth/phone/request-code"))
        .header(CONTENT_TYPE, "application/json")
        .body(format!(r#"{{"phone_number":"{phone}"}}"#))
        .send()
        .await
        .expect("POST /auth/phone/request-code")
}

async fn verify_code(
    client: &reqwest::Client,
    base_url: &str,
    phone: &str,
    code: &str,
) -> reqwest::Response {
    client
        .post(format!("{base_url}/auth/phone/verify-code"))
        .header(CONTENT_TYPE, "application/json")
        .body(format!(r#"{{"phone_number":"{phone}","code":"{code}"}}"#))
        .send()
        .await
        .expect("POST /auth/phone/verify-code")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cookie_mode_auth_flow_supports_http_and_ws_without_bearer_token() {
    let _guard = auth_test_mutex().lock().await;
    let data_root =
        std::env::temp_dir().join(format!("mgs_security_auth_cookie_{}", reserve_free_port()));
    let proc = spawn_server_with_sms_capture(data_root, true, true, false).await;
    let client = reqwest::Client::new();
    let phone = "+15551239876";

    let request_response = request_code(&client, &proc.base_url, phone).await;
    assert!(
        request_response.status().is_success(),
        "request-code failed with status {}",
        request_response.status()
    );

    let otp_code = wait_for_sms_code(&proc.sms_capture_path).await;
    let verify_response = verify_code(&client, &proc.base_url, phone, &otp_code).await;
    assert!(
        verify_response.status().is_success(),
        "verify-code failed with status {}",
        verify_response.status()
    );

    let set_cookie = verify_response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("expected Set-Cookie header")
        .to_owned();
    assert!(
        set_cookie.contains("mgs_session=") && set_cookie.contains("HttpOnly"),
        "expected secure session cookie, got {set_cookie}"
    );
    assert!(
        !set_cookie.contains("Secure"),
        "plain-http cookie mode should omit Secure so localhost/dev works"
    );
    let cookie_pair = set_cookie
        .split(';')
        .next()
        .expect("cookie pair")
        .to_owned();

    let verify_payload: Value = verify_response.json().await.expect("verify-code json");
    assert_eq!(verify_payload["ok"], Value::Bool(true));
    assert!(
        verify_payload["data"].get("token").is_none(),
        "cookie mode must not expose bearer token in JSON"
    );

    let me = client
        .get(format!("{}/auth/me", proc.base_url))
        .header(COOKIE, &cookie_pair)
        .send()
        .await
        .expect("GET /auth/me with cookie");
    assert!(
        me.status().is_success(),
        "/auth/me should accept cookie-only auth"
    );

    let leaderboard = client
        .get(format!("{}/auth/leaderboard?limit=5", proc.base_url))
        .header(COOKIE, &cookie_pair)
        .send()
        .await
        .expect("GET /auth/leaderboard with cookie");
    assert!(
        leaderboard.status().is_success(),
        "/auth/leaderboard should accept cookie-only auth"
    );

    let no_auth_ws = tokio_tungstenite::connect_async(proc.ws_url.clone())
        .await
        .expect_err("unauthenticated websocket should be rejected");
    match no_auth_ws {
        tokio_tungstenite::tungstenite::Error::Http(response) => {
            assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
        }
        other => panic!("unexpected websocket error for unauthenticated request: {other}"),
    }

    let mut authed_request = proc
        .ws_url
        .clone()
        .into_client_request()
        .expect("websocket client request");
    authed_request
        .headers_mut()
        .insert(COOKIE, cookie_pair.parse().expect("cookie header value"));
    let (mut ws_stream, _) = tokio_tungstenite::connect_async(authed_request)
        .await
        .expect("cookie-authenticated websocket should upgrade");
    let _ = ws_stream.close(None).await;

    let logout = client
        .post(format!("{}/auth/logout", proc.base_url))
        .header(COOKIE, &cookie_pair)
        .send()
        .await
        .expect("POST /auth/logout with cookie");
    assert!(logout.status().is_success());
    let logout_cookie = logout
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("logout should clear session cookie");
    assert!(
        logout_cookie.contains("mgs_session=") && logout_cookie.contains("Max-Age=0"),
        "logout should expire the browser cookie, got {logout_cookie}"
    );
    let logout_payload: Value = logout.json().await.expect("logout json");
    assert_eq!(logout_payload["data"]["revoked"], Value::Bool(true));

    let me_after_logout = client
        .get(format!("{}/auth/me", proc.base_url))
        .header(COOKIE, &cookie_pair)
        .send()
        .await
        .expect("GET /auth/me after logout");
    assert_eq!(me_after_logout.status(), reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn otp_ip_rate_limit_blocks_excessive_request_code_attempts() {
    let _guard = auth_test_mutex().lock().await;
    let data_root = std::env::temp_dir().join(format!(
        "mgs_security_auth_rate_limit_{}",
        reserve_free_port()
    ));
    let proc = spawn_server_with_sms_capture(data_root, false, false, false).await;
    let client = reqwest::Client::new();

    for idx in 0..5 {
        let phone = format!("+1555000{:05}", idx);
        let response = request_code(&client, &proc.base_url, &phone).await;
        assert!(
            response.status().is_success(),
            "request {} should succeed, got {}",
            idx + 1,
            response.status()
        );
    }

    let blocked = request_code(&client, &proc.base_url, "+155500099999").await;
    assert_eq!(blocked.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
    let payload: Value = blocked.json().await.expect("rate limit json");
    assert_eq!(payload["ok"], Value::Bool(false));
    assert_eq!(
        payload["error"]["code"],
        Value::String("ip_rate_limited".to_owned())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cookie_mode_sets_secure_cookie_when_tls_proxy_mode_is_enabled() {
    let _guard = auth_test_mutex().lock().await;
    let data_root = std::env::temp_dir().join(format!(
        "mgs_security_auth_cookie_secure_{}",
        reserve_free_port()
    ));
    let proc = spawn_server_with_sms_capture(data_root, true, false, true).await;
    let client = reqwest::Client::new();
    let phone = "+15551239877";

    let request_response = request_code(&client, &proc.base_url, phone).await;
    assert!(
        request_response.status().is_success(),
        "request-code failed with status {}",
        request_response.status()
    );

    let otp_code = wait_for_sms_code(&proc.sms_capture_path).await;
    let verify_response = verify_code(&client, &proc.base_url, phone, &otp_code).await;
    assert!(
        verify_response.status().is_success(),
        "verify-code failed with status {}",
        verify_response.status()
    );

    let set_cookie = verify_response
        .headers()
        .get(SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("expected Set-Cookie header");
    assert!(
        set_cookie.contains("Secure"),
        "TLS proxy mode should emit Secure cookies, got {set_cookie}"
    );
}
