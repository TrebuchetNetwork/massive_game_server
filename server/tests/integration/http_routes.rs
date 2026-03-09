use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use reqwest::redirect::Policy;
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
    let data_root = std::env::temp_dir().join(format!("mgs_http_routes_{port}"));
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

async fn assert_admin_auth_required(response: reqwest::Response) {
    let body: Value = response.json().await.expect("admin unauthorized json body");
    assert_eq!(body["ok"], Value::Bool(false));
    assert_eq!(
        body["error"]["code"],
        Value::String("admin_auth_required".to_owned())
    );
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
async fn inline_admin_checks_apply_to_feature_flags_set_and_list() {
    let admin_token = "integration-feature-flags-admin-token";
    let proc = spawn_server(Some(admin_token)).await;
    let client = reqwest::Client::new();

    let list_unauthorized = client
        .get(format!("{}/api/ops/feature-flags", proc.base_url))
        .send()
        .await
        .expect("GET /api/ops/feature-flags without token");
    assert_admin_auth_required(list_unauthorized).await;

    let list_authorized = client
        .get(format!("{}/api/ops/feature-flags", proc.base_url))
        .header(AUTHORIZATION, format!("Bearer {admin_token}"))
        .send()
        .await
        .expect("GET /api/ops/feature-flags with token");
    let list_authorized_json: Value = list_authorized
        .json()
        .await
        .expect("feature-flags list json");
    assert_eq!(list_authorized_json["ok"], Value::Bool(true));
    assert!(list_authorized_json["data"].is_array());

    let set_unauthorized = client
        .post(format!("{}/api/ops/feature-flags/set", proc.base_url))
        .header(CONTENT_TYPE, "application/json")
        .body(
            r#"{"key":"integration.new_ui","enabled":true,"rollout_percentage":25,"description":"integration test flag"}"#,
        )
        .send()
        .await
        .expect("POST /api/ops/feature-flags/set without token");
    assert_admin_auth_required(set_unauthorized).await;

    let set_authorized = client
        .post(format!("{}/api/ops/feature-flags/set", proc.base_url))
        .header(CONTENT_TYPE, "application/json")
        .header(AUTHORIZATION, format!("Bearer {admin_token}"))
        .body(
            r#"{"key":"integration.new_ui","enabled":true,"rollout_percentage":25,"description":"integration test flag"}"#,
        )
        .send()
        .await
        .expect("POST /api/ops/feature-flags/set with token");
    let set_authorized_json: Value = set_authorized.json().await.expect("feature-flags set json");
    assert_eq!(set_authorized_json["ok"], Value::Bool(true));
    assert_eq!(
        set_authorized_json["data"]["key"],
        Value::String("integration.new_ui".to_owned())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inline_admin_checks_apply_to_arena_mutation_routes() {
    let admin_token = "integration-arena-admin-token";
    let proc = spawn_server(Some(admin_token)).await;
    let client = reqwest::Client::new();

    let register_unauthorized = client
        .post(format!("{}/api/arena/models/register", proc.base_url))
        .header(CONTENT_TYPE, "application/json")
        .body(r#"{"model_name":"bot-alpha"}"#)
        .send()
        .await
        .expect("POST /api/arena/models/register without token");
    assert_admin_auth_required(register_unauthorized).await;

    let register_alpha = client
        .post(format!("{}/api/arena/models/register", proc.base_url))
        .header(CONTENT_TYPE, "application/json")
        .header(AUTHORIZATION, format!("Bearer {admin_token}"))
        .body(r#"{"model_name":"bot-alpha"}"#)
        .send()
        .await
        .expect("POST /api/arena/models/register alpha with token");
    let register_alpha_json: Value = register_alpha
        .json()
        .await
        .expect("register alpha response json");
    assert_eq!(register_alpha_json["ok"], Value::Bool(true));
    let model_alpha_id = register_alpha_json["data"]["model_id"]
        .as_str()
        .expect("model alpha id")
        .to_owned();

    let register_beta = client
        .post(format!("{}/api/arena/models/register", proc.base_url))
        .header(CONTENT_TYPE, "application/json")
        .header(AUTHORIZATION, format!("Bearer {admin_token}"))
        .body(r#"{"model_name":"bot-beta"}"#)
        .send()
        .await
        .expect("POST /api/arena/models/register beta with token");
    let register_beta_json: Value = register_beta
        .json()
        .await
        .expect("register beta response json");
    assert_eq!(register_beta_json["ok"], Value::Bool(true));
    let model_beta_id = register_beta_json["data"]["model_id"]
        .as_str()
        .expect("model beta id")
        .to_owned();

    let queue_unauthorized = client
        .post(format!("{}/api/arena/matches/queue", proc.base_url))
        .header(CONTENT_TYPE, "application/json")
        .body(format!(
            r#"{{"model_a_id":"{model_alpha_id}","model_b_id":"{model_beta_id}"}}"#
        ))
        .send()
        .await
        .expect("POST /api/arena/matches/queue without token");
    assert_admin_auth_required(queue_unauthorized).await;

    let queue_authorized = client
        .post(format!("{}/api/arena/matches/queue", proc.base_url))
        .header(CONTENT_TYPE, "application/json")
        .header(AUTHORIZATION, format!("Bearer {admin_token}"))
        .body(format!(
            r#"{{"model_a_id":"{model_alpha_id}","model_b_id":"{model_beta_id}"}}"#
        ))
        .send()
        .await
        .expect("POST /api/arena/matches/queue with token");
    let queue_authorized_json: Value = queue_authorized.json().await.expect("queue match json");
    assert_eq!(queue_authorized_json["ok"], Value::Bool(true));
    let queued_match_id = queue_authorized_json["data"]["queued_matches"][0]["match_id"]
        .as_str()
        .expect("queued match id")
        .to_owned();

    let claim_unauthorized = client
        .post(format!("{}/api/arena/matches/claim_next", proc.base_url))
        .send()
        .await
        .expect("POST /api/arena/matches/claim_next without token");
    assert_admin_auth_required(claim_unauthorized).await;

    let claim_authorized = client
        .post(format!("{}/api/arena/matches/claim_next", proc.base_url))
        .header(AUTHORIZATION, format!("Bearer {admin_token}"))
        .send()
        .await
        .expect("POST /api/arena/matches/claim_next with token");
    let claim_authorized_json: Value = claim_authorized.json().await.expect("claim next json");
    assert_eq!(claim_authorized_json["ok"], Value::Bool(true));
    assert_eq!(
        claim_authorized_json["data"]["claimed"]["match_id"],
        Value::String(queued_match_id.clone())
    );

    let report_unauthorized = client
        .post(format!("{}/api/arena/matches/report", proc.base_url))
        .header(CONTENT_TYPE, "application/json")
        .body(format!(r#"{{"match_id":"{queued_match_id}","draw":true}}"#))
        .send()
        .await
        .expect("POST /api/arena/matches/report without token");
    assert_admin_auth_required(report_unauthorized).await;
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inline_admin_checks_apply_to_ops_report_routes() {
    let admin_token = "integration-ops-admin-token";
    let proc = spawn_server(Some(admin_token)).await;
    let client = reqwest::Client::new();

    let join_stages_unauthorized = client
        .get(format!("{}/api/ops/join-stages", proc.base_url))
        .send()
        .await
        .expect("GET /api/ops/join-stages without token");
    assert_admin_auth_required(join_stages_unauthorized).await;

    let join_stages_authorized = client
        .get(format!("{}/api/ops/join-stages", proc.base_url))
        .header(AUTHORIZATION, format!("Bearer {admin_token}"))
        .send()
        .await
        .expect("GET /api/ops/join-stages with token");
    let join_stages_json: Value = join_stages_authorized
        .json()
        .await
        .expect("join stages json");
    assert_eq!(join_stages_json["total_tracked_clients"], Value::from(0));
    assert!(join_stages_json["waves"].is_object());

    let join_stages_reset_unauthorized = client
        .post(format!("{}/api/ops/join-stages/reset", proc.base_url))
        .send()
        .await
        .expect("POST /api/ops/join-stages/reset without token");
    assert_admin_auth_required(join_stages_reset_unauthorized).await;

    let join_stages_reset_authorized = client
        .post(format!("{}/api/ops/join-stages/reset", proc.base_url))
        .header(AUTHORIZATION, format!("Bearer {admin_token}"))
        .send()
        .await
        .expect("POST /api/ops/join-stages/reset with token");
    let join_stages_reset_json: Value = join_stages_reset_authorized
        .json()
        .await
        .expect("join stages reset json");
    assert_eq!(join_stages_reset_json["ok"], Value::Bool(true));

    let live_replay_recent_unauthorized = client
        .get(format!(
            "{}/api/ops/live-replay/recent?limit=5",
            proc.base_url
        ))
        .send()
        .await
        .expect("GET /api/ops/live-replay/recent without token");
    assert_admin_auth_required(live_replay_recent_unauthorized).await;

    let live_replay_recent_authorized = client
        .get(format!(
            "{}/api/ops/live-replay/recent?limit=5",
            proc.base_url
        ))
        .header(AUTHORIZATION, format!("Bearer {admin_token}"))
        .send()
        .await
        .expect("GET /api/ops/live-replay/recent with token");
    let live_replay_recent_json: Value = live_replay_recent_authorized
        .json()
        .await
        .expect("live replay recent json");
    assert!(live_replay_recent_json["enabled"].is_boolean());
    assert!(live_replay_recent_json["frames"].is_array());
    assert_eq!(live_replay_recent_json["limit"], Value::from(5));

    let live_replay_dispute_unauthorized = client
        .post(format!("{}/api/ops/live-replay/dispute", proc.base_url))
        .header(CONTENT_TYPE, "application/json")
        .body(r#"{"limit":4}"#)
        .send()
        .await
        .expect("POST /api/ops/live-replay/dispute without token");
    assert_admin_auth_required(live_replay_dispute_unauthorized).await;

    let live_replay_dispute_authorized = client
        .post(format!("{}/api/ops/live-replay/dispute", proc.base_url))
        .header(CONTENT_TYPE, "application/json")
        .header(AUTHORIZATION, format!("Bearer {admin_token}"))
        .body(r#"{"limit":4}"#)
        .send()
        .await
        .expect("POST /api/ops/live-replay/dispute with token");
    let live_replay_dispute_json: Value = live_replay_dispute_authorized
        .json()
        .await
        .expect("live replay dispute json");
    assert!(live_replay_dispute_json["generated_at_ms"].is_number());
    assert!(live_replay_dispute_json["selected_frames"].is_array());
    assert!(live_replay_dispute_json["relevant_kill_feed"].is_array());
    assert!(live_replay_dispute_json["audit"].is_object());

    let live_replay_disputes_recent_unauthorized = client
        .get(format!(
            "{}/api/ops/live-replay/disputes/recent?limit=3",
            proc.base_url
        ))
        .send()
        .await
        .expect("GET /api/ops/live-replay/disputes/recent without token");
    assert_admin_auth_required(live_replay_disputes_recent_unauthorized).await;

    let live_replay_disputes_recent_authorized = client
        .get(format!(
            "{}/api/ops/live-replay/disputes/recent?limit=3",
            proc.base_url
        ))
        .header(AUTHORIZATION, format!("Bearer {admin_token}"))
        .send()
        .await
        .expect("GET /api/ops/live-replay/disputes/recent with token");
    let live_replay_disputes_recent_json: Value = live_replay_disputes_recent_authorized
        .json()
        .await
        .expect("live replay disputes recent json");
    assert_eq!(live_replay_disputes_recent_json["ok"], Value::Bool(true));
    assert!(live_replay_disputes_recent_json["audits"].is_array());
    assert_eq!(live_replay_disputes_recent_json["limit"], Value::from(3));

    let match_summary_unauthorized = client
        .get(format!("{}/api/ops/match-summary/latest", proc.base_url))
        .send()
        .await
        .expect("GET /api/ops/match-summary/latest without token");
    assert_admin_auth_required(match_summary_unauthorized).await;

    let match_summary_authorized = client
        .get(format!("{}/api/ops/match-summary/latest", proc.base_url))
        .header(AUTHORIZATION, format!("Bearer {admin_token}"))
        .send()
        .await
        .expect("GET /api/ops/match-summary/latest with token");
    let match_summary_json: Value = match_summary_authorized
        .json()
        .await
        .expect("match summary json");
    assert_eq!(match_summary_json["ok"], Value::Bool(true));
    assert!(match_summary_json["summary"].is_null() || match_summary_json["summary"].is_object());

    let backup_latest_unauthorized = client
        .get(format!("{}/api/ops/backup/latest", proc.base_url))
        .send()
        .await
        .expect("GET /api/ops/backup/latest without token");
    assert_admin_auth_required(backup_latest_unauthorized).await;

    let backup_latest_authorized = client
        .get(format!("{}/api/ops/backup/latest", proc.base_url))
        .header(AUTHORIZATION, format!("Bearer {admin_token}"))
        .send()
        .await
        .expect("GET /api/ops/backup/latest with token");
    let backup_latest_json: Value = backup_latest_authorized
        .json()
        .await
        .expect("backup latest json");
    assert_eq!(backup_latest_json["ok"], Value::Bool(true));
    assert!(backup_latest_json["backup"].is_null() || backup_latest_json["backup"].is_object());

    let killcam_unauthorized = client
        .get(format!("{}/api/ops/killcam/test-player", proc.base_url))
        .send()
        .await
        .expect("GET /api/ops/killcam/:player without token");
    assert_admin_auth_required(killcam_unauthorized).await;

    let killcam_authorized = client
        .get(format!("{}/api/ops/killcam/test-player", proc.base_url))
        .header(AUTHORIZATION, format!("Bearer {admin_token}"))
        .send()
        .await
        .expect("GET /api/ops/killcam/:player with token");
    let killcam_json: Value = killcam_authorized.json().await.expect("killcam json");
    assert_eq!(killcam_json["ok"], Value::Bool(true));
    assert_eq!(
        killcam_json["player_id"],
        Value::String("test-player".to_owned())
    );
    assert!(killcam_json["killcam"].is_null() || killcam_json["killcam"].is_object());
}
