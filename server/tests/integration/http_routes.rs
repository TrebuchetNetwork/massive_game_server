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
