use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use bytes::Bytes;
use massive_game_server_core::network::quic::{
    send_quic_packet_batch, start_quic_runtime, QuicEndpointConfig, QuicRequestHandler,
};
use serde_json::json;

fn write_test_quic_identity() -> Result<(PathBuf, PathBuf, PathBuf, Vec<u8>)> {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_owned()])
        .context("failed to generate self-signed test certificate")?;
    let cert_der = cert.cert.der().to_vec();
    let key_der = cert.key_pair.serialize_der();

    let suffix = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let temp_dir = std::env::temp_dir().join(format!("mgs-quic-integration-{suffix}"));
    std::fs::create_dir_all(&temp_dir).context("failed to create temp identity directory")?;

    let cert_path = temp_dir.join("cert.der");
    let key_path = temp_dir.join("key.der");
    std::fs::write(&cert_path, &cert_der).context("failed to write temp cert")?;
    std::fs::write(&key_path, &key_der).context("failed to write temp key")?;

    Ok((temp_dir, cert_path, key_path, cert_der))
}

async fn send_control_message(
    conn: &quinn::Connection,
    payload: serde_json::Value,
) -> Result<serde_json::Value> {
    let (mut send, mut recv) = conn
        .open_bi()
        .await
        .context("failed to open client bidi stream")?;
    let raw = serde_json::to_vec(&payload).context("failed serializing control payload")?;
    send.write_all(&raw)
        .await
        .context("failed writing control payload")?;
    send.finish().context("failed finishing control stream")?;
    let response = recv
        .read_to_end(16 * 1024)
        .await
        .context("failed reading control response")?;
    serde_json::from_slice(&response).context("failed parsing control response")
}

#[tokio::test(flavor = "multi_thread")]
async fn quic_transport_handles_bidi_and_outbound_paths() -> Result<()> {
    let (temp_dir, cert_path, key_path, cert_der) = write_test_quic_identity()?;
    let cert_path_raw = cert_path
        .to_str()
        .context("cert path is not valid UTF-8")?
        .to_owned();
    let key_path_raw = key_path
        .to_str()
        .context("key path is not valid UTF-8")?
        .to_owned();

    let test_result = temp_env::async_with_vars(
        [
            ("MGS_QUIC_CERT_PATH", Some(cert_path_raw.as_str())),
            ("MGS_QUIC_KEY_PATH", Some(key_path_raw.as_str())),
            ("MGS_QUIC_OUTBOUND_MODE", Some("legacy")),
        ],
        async move {
            let request_handler: QuicRequestHandler = Arc::new(|payload, bound_peer_id| {
                let envelope: serde_json::Value = match serde_json::from_slice(payload) {
                    Ok(value) => value,
                    Err(_) => {
                        return Some(
                            json!({"ok": false, "error": "invalid_json"})
                                .to_string()
                                .into_bytes(),
                        )
                    }
                };
                let op = envelope
                    .get("op")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown");

                match (op, bound_peer_id) {
                    ("join", None) => {
                        if envelope.get("auth_token").and_then(|value| value.as_str())
                            == Some("allow")
                        {
                            Some(
                                json!({
                                    "ok": true,
                                    "op": "join",
                                    "_bound_peer_id": "integration-peer",
                                    "detail": "joined",
                                })
                                .to_string()
                                .into_bytes(),
                            )
                        } else {
                            Some(
                                json!({"ok": false, "op": "join", "error": "auth_required"})
                                    .to_string()
                                    .into_bytes(),
                            )
                        }
                    }
                    ("ping", Some("integration-peer")) => Some(
                        json!({"ok": true, "op": "ping", "detail": "pong"})
                            .to_string()
                            .into_bytes(),
                    ),
                    _ => Some(
                        json!({"ok": false, "op": op, "error": "unauthorized"})
                            .to_string()
                            .into_bytes(),
                    ),
                }
            });

            let config = QuicEndpointConfig {
                bind_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
                max_concurrent_bidi_streams: 32,
                max_stream_payload_bytes: 16 * 1024,
            };
            let runtime = start_quic_runtime(&config, Some(request_handler))
                .context("failed to start QUIC runtime for integration test")?;

            let mut roots = quinn::rustls::RootCertStore::empty();
            roots
                .add(quinn::rustls::pki_types::CertificateDer::from(cert_der))
                .context("failed to trust generated QUIC certificate")?;
            let client_config = quinn::ClientConfig::with_root_certificates(Arc::new(roots))
                .context("failed to build QUIC client config")?;
            let mut endpoint =
                quinn::Endpoint::client(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
                    .context("failed to create QUIC client endpoint")?;
            endpoint.set_default_client_config(client_config);

            let conn = endpoint
                .connect(runtime.local_addr(), "localhost")
                .context("failed to begin QUIC connection")?
                .await
                .context("failed to establish QUIC connection")?;

            let join_response = send_control_message(
                &conn,
                json!({
                    "op": "join",
                    "peer_id": "forged-client-peer",
                    "auth_token": "allow",
                }),
            )
            .await?;
            assert_eq!(
                join_response
                    .get("_bound_peer_id")
                    .and_then(|value| value.as_str()),
                Some("integration-peer"),
                "join response should include connection-bound peer ID"
            );

            let ping_response = send_control_message(
                &conn,
                json!({
                    "op": "ping",
                    "peer_id": "forged-after-bind",
                }),
            )
            .await?;
            assert_eq!(
                ping_response.get("detail").and_then(|value| value.as_str()),
                Some("pong"),
                "bound peer ID should authorize follow-up operations"
            );

            let mut sent_packets = 0usize;
            for _ in 0..20 {
                sent_packets = send_quic_packet_batch(
                    "integration-peer",
                    &[Bytes::from_static(b"server-to-client-payload")],
                );
                if sent_packets == 1 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            assert_eq!(
                sent_packets, 1,
                "expected outbound QUIC packet to be enqueued for bound peer"
            );

            let mut outbound_stream =
                tokio::time::timeout(Duration::from_secs(2), conn.accept_uni())
                    .await
                    .context("timed out waiting for outbound uni stream")?
                    .context("failed accepting outbound uni stream")?;
            let outbound_payload =
                tokio::time::timeout(Duration::from_secs(2), outbound_stream.read_to_end(4096))
                    .await
                    .context("timed out reading outbound payload")?
                    .context("failed reading outbound payload")?;
            assert_eq!(outbound_payload, b"server-to-client-payload");

            conn.close(0u32.into(), b"done");
            endpoint.wait_idle().await;
            drop(runtime);
            Ok(())
        },
    )
    .await;

    let _ = std::fs::remove_dir_all(temp_dir);
    test_result
}
