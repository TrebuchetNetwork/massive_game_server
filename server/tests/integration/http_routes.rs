use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use reqwest::redirect::Policy;
use serde_json::Value;
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
    for _ in 0..80 {
        if let Ok(resp) = client.get(format!("{base_url}/readyz")).send().await {
            if resp.status().is_success() {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(125)).await;
    }
    panic!("server did not become ready at {base_url}");
}

async fn spawn_server(admin_token: Option<&str>) -> ServerProcess {
    spawn_server_with_env(admin_token, &[]).await
}

async fn spawn_server_with_env(
    admin_token: Option<&str>,
    extra_env: &[(&str, &str)],
) -> ServerProcess {
    let port = reserve_free_port();
    let base_url = format!("http://127.0.0.1:{port}");
    let mut command = Command::new(env!("CARGO_BIN_EXE_massive_game_server_core"));
    command
        .env("MGS_HOST", "127.0.0.1")
        .env("MGS_PORT", port.to_string())
        .env("MGS_DISABLE_STUN", "1")
        .env("MGS_TARGET_BOT_COUNT", "0")
        .env("MGS_DIAGNOSTICS_ENABLED", "0")
        .env("MGS_QUIC_PRIMARY", "0")
        .env("RUST_LOG", "warn")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(token) = admin_token {
        command.env("MGS_ADMIN_BEARER_TOKEN", token);
    }
    for (key, value) in extra_env {
        command.env(key, value);
    }
    let child = command.spawn().expect("spawn game server binary");
    let proc = ServerProcess { child, base_url };
    wait_until_ready(&proc.base_url).await;
    proc
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn health_ready_and_root_redirect_work() {
    let proc = spawn_server(None).await;
    let client = reqwest::Client::builder()
        .redirect(Policy::none())
        .build()
        .expect("build client");

    let root = client
        .get(format!("{}/", proc.base_url))
        .send()
        .await
        .expect("GET /");
    assert!(root.status().is_redirection());
    let location = root
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|value| value.to_str().ok());
    assert_eq!(location, Some("/index.html"));

    let health = client
        .get(format!("{}/healthz", proc.base_url))
        .send()
        .await
        .expect("GET /healthz");
    assert!(health.status().is_success());
    let health_json: Value = health.json().await.expect("health json");
    assert_eq!(health_json["ok"], Value::Bool(true));

    let ready = client
        .get(format!("{}/readyz", proc.base_url))
        .send()
        .await
        .expect("GET /readyz");
    assert!(ready.status().is_success());
    let ready_json: Value = ready.json().await.expect("ready json");
    assert_eq!(ready_json["ok"], Value::Bool(true));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_routes_require_bearer_token() {
    let proc = spawn_server(Some("integration-admin-token")).await;
    let client = reqwest::Client::new();

    let unauthorized = client
        .get(format!("{}/api/ops/match-type", proc.base_url))
        .send()
        .await
        .expect("GET protected route without token");
    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);
    let unauthorized_json: Value = unauthorized.json().await.expect("unauthorized json");
    assert_eq!(unauthorized_json["ok"], Value::Bool(false));
    assert_eq!(
        unauthorized_json["error"]["code"],
        Value::String("admin_auth_required".to_owned())
    );

    let authorized = client
        .get(format!("{}/api/ops/match-type", proc.base_url))
        .bearer_auth("integration-admin-token")
        .send()
        .await
        .expect("GET protected route with token");
    assert!(authorized.status().is_success());
    let authorized_json: Value = authorized.json().await.expect("authorized json");
    assert_eq!(authorized_json["ok"], Value::Bool(true));
    assert!(authorized_json["match_type"].is_string());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auth_leaderboard_requires_authenticated_session() {
    let proc = spawn_server(None).await;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("{}/auth/leaderboard?limit=999999", proc.base_url))
        .send()
        .await
        .expect("GET /auth/leaderboard without session");
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    let body: Value = response
        .json()
        .await
        .expect("leaderboard unauthorized json");
    assert_eq!(body["ok"], Value::Bool(false));
    assert_eq!(
        body["error"]["code"],
        Value::String("session_invalid".to_owned())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inline_admin_checks_apply_to_feature_flags_and_codegen() {
    let admin_token = "integration-inline-admin-token";
    let proc = spawn_server(Some(admin_token)).await;
    let client = reqwest::Client::new();

    let feature_flags_unauthorized = client
        .post(format!("{}/api/ops/feature-flags/evaluate", proc.base_url))
        .header(CONTENT_TYPE, "application/json")
        .body(r#"{"key":"test","subject":"user-1"}"#)
        .send()
        .await
        .expect("POST /api/ops/feature-flags/evaluate without token");
    let feature_flags_unauthorized_json: Value = feature_flags_unauthorized
        .json()
        .await
        .expect("feature flags unauthorized json");
    assert_eq!(feature_flags_unauthorized_json["ok"], Value::Bool(false));
    assert_eq!(
        feature_flags_unauthorized_json["error"]["code"],
        Value::String("admin_auth_required".to_owned())
    );

    let feature_flags_authorized = client
        .post(format!("{}/api/ops/feature-flags/evaluate", proc.base_url))
        .header(CONTENT_TYPE, "application/json")
        .header(AUTHORIZATION, format!("Bearer {admin_token}"))
        .body(r#"{"key":"test","subject":"user-1"}"#)
        .send()
        .await
        .expect("POST /api/ops/feature-flags/evaluate with token");
    let feature_flags_authorized_json: Value = feature_flags_authorized
        .json()
        .await
        .expect("feature flags authorized json");
    assert_eq!(feature_flags_authorized_json["ok"], Value::Bool(false));
    assert_eq!(
        feature_flags_authorized_json["error"]["code"],
        Value::String("flag_not_found".to_owned())
    );

    let codegen_unauthorized = client
        .post(format!("{}/api/arena/code/validate", proc.base_url))
        .header(CONTENT_TYPE, "application/json")
        .body(r#"{"source_code":"fn bot_tick() -> i32 { 0 }","language":"rust"}"#)
        .send()
        .await
        .expect("POST /api/arena/code/validate without token");
    let codegen_unauthorized_json: Value = codegen_unauthorized
        .json()
        .await
        .expect("codegen unauthorized json");
    assert_eq!(codegen_unauthorized_json["ok"], Value::Bool(false));
    assert_eq!(
        codegen_unauthorized_json["error"]["code"],
        Value::String("admin_auth_required".to_owned())
    );

    let codegen_authorized = client
        .post(format!("{}/api/arena/code/validate", proc.base_url))
        .header(CONTENT_TYPE, "application/json")
        .header(AUTHORIZATION, format!("Bearer {admin_token}"))
        .body(
            r##"{"source_code":"#[no_mangle] pub extern \"C\" fn bot_tick(a:i32,b:i32,c:i32,d:i32)->i32{a+b+c+d}","language":"rust"}"##,
        )
        .send()
        .await
        .expect("POST /api/arena/code/validate with token");
    let codegen_authorized_json: Value = codegen_authorized
        .json()
        .await
        .expect("codegen authorized json");
    assert_eq!(codegen_authorized_json["ok"], Value::Bool(true));
    assert!(codegen_authorized_json["data"]["valid"].is_boolean());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn admin_ip_allowlist_rejects_spoofed_forwarded_ip_from_untrusted_proxy() {
    let admin_token = "integration-admin-ip-token";
    let client = reqwest::Client::new();

    let untrusted_proxy_proc = spawn_server_with_env(
        Some(admin_token),
        &[
            ("MGS_ADMIN_IP_ALLOWLIST", "198.51.100.10/32"),
            ("MGS_TRUSTED_PROXY_CIDRS", "10.0.0.0/8"),
        ],
    )
    .await;

    let untrusted_response = client
        .get(format!(
            "{}/api/ops/match-type",
            untrusted_proxy_proc.base_url
        ))
        .header(AUTHORIZATION, format!("Bearer {admin_token}"))
        .header("x-forwarded-for", "198.51.100.10")
        .send()
        .await
        .expect("GET /api/ops/match-type from untrusted proxy");
    assert_eq!(untrusted_response.status(), reqwest::StatusCode::FORBIDDEN);
    let untrusted_json: Value = untrusted_response
        .json()
        .await
        .expect("untrusted proxy response json");
    assert_eq!(untrusted_json["ok"], Value::Bool(false));
    assert_eq!(
        untrusted_json["error"]["code"],
        Value::String("admin_ip_blocked".to_owned())
    );

    drop(untrusted_proxy_proc);

    let trusted_proxy_proc = spawn_server_with_env(
        Some(admin_token),
        &[
            ("MGS_ADMIN_IP_ALLOWLIST", "198.51.100.10/32"),
            ("MGS_TRUSTED_PROXY_CIDRS", "127.0.0.1/32"),
        ],
    )
    .await;

    let trusted_response = client
        .get(format!(
            "{}/api/ops/match-type",
            trusted_proxy_proc.base_url
        ))
        .header(AUTHORIZATION, format!("Bearer {admin_token}"))
        .header("x-forwarded-for", "198.51.100.10")
        .send()
        .await
        .expect("GET /api/ops/match-type from trusted proxy");
    assert!(
        trusted_response.status().is_success(),
        "trusted forwarded header should be accepted when proxy CIDR is trusted"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn auth_me_applies_token_validation_rate_limits() {
    let proc = spawn_server_with_env(
        None,
        &[
            ("MGS_AUTH_TOKEN_RATE_LIMIT_PER_SEC", "1"),
            ("MGS_AUTH_TOKEN_RATE_LIMIT_BURST", "2"),
        ],
    )
    .await;
    let client = reqwest::Client::new();

    for attempt in 0..2 {
        let response = client
            .get(format!("{}/auth/me", proc.base_url))
            .header(AUTHORIZATION, "Bearer invalid-session-token")
            .send()
            .await
            .expect("GET /auth/me before token rate limit");
        assert_eq!(
            response.status(),
            reqwest::StatusCode::UNAUTHORIZED,
            "attempt {attempt} should still be an auth failure before rate-limit is exhausted"
        );
    }

    let rate_limited = client
        .get(format!("{}/auth/me", proc.base_url))
        .header(AUTHORIZATION, "Bearer invalid-session-token")
        .send()
        .await
        .expect("GET /auth/me after token rate limit exhaustion");
    assert_eq!(
        rate_limited.status(),
        reqwest::StatusCode::TOO_MANY_REQUESTS
    );
    let rate_limited_json: Value = rate_limited
        .json()
        .await
        .expect("token rate-limit response json");
    assert_eq!(rate_limited_json["ok"], Value::Bool(false));
    assert_eq!(
        rate_limited_json["error"]["code"],
        Value::String("token_rate_limited".to_owned())
    );
}
