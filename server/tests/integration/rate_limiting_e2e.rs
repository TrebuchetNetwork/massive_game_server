#[path = "../common/helpers.rs"]
mod helpers;

use std::sync::Arc;

use helpers::spawn_server;
use tokio::sync::Barrier;
use tokio_tungstenite::tungstenite::Error as WsError;

fn signaling_url(base_ws_url: &str, username: &str) -> String {
    format!("{base_ws_url}?username={username}")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_connection_cap_rejects_second_upgrade() {
    let proc = spawn_server(
        "mgs_rate_limiting_e2e",
        &[("MGS_MAX_CONCURRENT_CONNECTIONS", "1".to_owned())],
        false,
    )
    .await;

    let (first_ws, _) =
        tokio_tungstenite::connect_async(signaling_url(&proc.ws_url, "rate-limit-a"))
            .await
            .expect("first websocket upgrade should succeed");

    let second_attempt =
        tokio_tungstenite::connect_async(signaling_url(&proc.ws_url, "rate-limit-b"))
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn websocket_connection_cap_never_oversubscribes_under_concurrent_upgrades() {
    let proc = spawn_server(
        "mgs_rate_limiting_e2e",
        &[("MGS_MAX_CONCURRENT_CONNECTIONS", "2".to_owned())],
        false,
    )
    .await;

    let attempt_count = 8usize;
    let barrier = Arc::new(Barrier::new(attempt_count));
    let mut tasks = Vec::with_capacity(attempt_count);

    for idx in 0..attempt_count {
        let barrier = barrier.clone();
        let ws_url = signaling_url(&proc.ws_url, &format!("rate-limit-burst-{idx}"));
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            tokio_tungstenite::connect_async(ws_url).await
        }));
    }

    let mut accepted = Vec::new();
    let mut rejected = 0usize;

    for task in tasks {
        match task.await.expect("join concurrent upgrade task") {
            Ok((stream, _response)) => accepted.push(stream),
            Err(WsError::Http(response)) => {
                assert_eq!(
                    response.status(),
                    reqwest::StatusCode::SERVICE_UNAVAILABLE,
                    "unexpected concurrent rejection status"
                );
                rejected += 1;
            }
            Err(other) => panic!("unexpected concurrent websocket result: {other}"),
        }
    }

    assert_eq!(
        accepted.len(),
        2,
        "connection cap should admit only two upgrades"
    );
    assert_eq!(
        rejected,
        attempt_count - accepted.len(),
        "all remaining upgrades should be rejected by the cap"
    );

    for mut stream in accepted {
        let _ = stream.close(None).await;
    }
}
