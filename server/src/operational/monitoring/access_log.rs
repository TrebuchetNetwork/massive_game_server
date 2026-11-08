// server/src/operational/monitoring/access_log.rs
//! Per-request HTTP access log with the real client IP for security auditing.
//!
//! The server sits behind ngrok, so the socket peer is always the local tunnel.
//! The real client address arrives in `X-Forwarded-For` (first entry wins);
//! direct local traffic (health checks, loopback) falls back to the socket peer.
//!
//! Output: one JSON object per line at `MGS_ACCESS_LOG_PATH`
//! (default `logs/access.jsonl`, relative to the process working directory).

use std::fs::OpenOptions;
use std::io::Write;
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use warp::http::HeaderMap;
use warp::log::Info;

const DEFAULT_LOG_PATH: &str = "logs/access.jsonl";

/// warp `log::custom` handler — call once per completed request.
pub fn log_request(info: Info<'_>) {
    let mut line = json!({
        "ts": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0),
        "ip": client_ip(info.request_headers(), info.remote_addr()),
        "method": info.method().as_str(),
        "path": info.path(),
        "status": info.status().as_u16(),
        "elapsed_ms": info.elapsed().as_secs_f64() * 1000.0,
        "ua": info.user_agent().unwrap_or("-"),
        "referer": info.referer().unwrap_or("-"),
    })
    .to_string();
    // Single write of the whole line: concurrent requests on multi-threaded
    // runtimes otherwise interleave (writeln! issues separate writes for the
    // payload and the newline), corrupting the JSONL stream.
    line.push('\n');
    append_line(&line);
}

fn client_ip(headers: &HeaderMap, remote: Option<SocketAddr>) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(String::from)
        .or_else(|| remote.map(|addr| addr.ip().to_string()))
        .unwrap_or_else(|| "-".to_string())
}

fn log_path() -> String {
    std::env::var("MGS_ACCESS_LOG_PATH").unwrap_or_else(|_| DEFAULT_LOG_PATH.to_string())
}

fn append_line(line: &str) {
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log_path()) {
        // Logging must never break request handling; ignore write errors.
        let _ = file.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use warp::http::HeaderValue;

    #[test]
    fn client_ip_prefers_first_x_forwarded_for_entry() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.7, 10.0.0.1"),
        );
        let remote: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        assert_eq!(client_ip(&headers, Some(remote)), "203.0.113.7");
    }

    #[test]
    fn client_ip_falls_back_to_socket_peer() {
        let headers = HeaderMap::new();
        let remote: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        assert_eq!(client_ip(&headers, Some(remote)), "127.0.0.1");
    }

    #[test]
    fn client_ip_dash_when_nothing_known() {
        let headers = HeaderMap::new();
        assert_eq!(client_ip(&headers, None), "-");
    }

    #[test]
    fn client_ip_ignores_empty_x_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("  "));
        let remote: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        assert_eq!(client_ip(&headers, Some(remote)), "127.0.0.1");
    }
}
