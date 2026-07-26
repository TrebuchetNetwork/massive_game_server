use massive_game_server_core::operational::runtime_utils::{parse_list_env, parse_u64_env};
use std::net::{IpAddr, Ipv4Addr};
use std::path::Path;

fn with_env_var<T>(key: &str, value: Option<&str>, f: impl FnOnce() -> T) -> T {
    temp_env::with_var(key, value, f)
}

#[test]
fn test_parse_bearer_token_accepts_standard_forms() {
    assert_eq!(
        massive_game_server_core::operational::admin_auth::parse_bearer_token(Some(
            "Bearer abc123",
        )),
        Some("abc123".to_owned())
    );
    assert_eq!(
        massive_game_server_core::operational::admin_auth::parse_bearer_token(
            Some("bearer  xyz ",)
        ),
        Some("xyz".to_owned())
    );
}

#[test]
fn test_parse_bearer_token_rejects_invalid_values() {
    assert_eq!(
        massive_game_server_core::operational::admin_auth::parse_bearer_token(None),
        None
    );
    assert_eq!(
        massive_game_server_core::operational::admin_auth::parse_bearer_token(Some("")),
        None
    );
    assert_eq!(
        massive_game_server_core::operational::admin_auth::parse_bearer_token(Some("Token abc",)),
        None
    );
    assert_eq!(
        massive_game_server_core::operational::admin_auth::parse_bearer_token(Some("Bearer    ",)),
        None
    );
}

#[test]
fn test_constant_time_eq_behaves_as_expected() {
    assert!(
        massive_game_server_core::operational::admin_auth::constant_time_eq(
            "same-value",
            "same-value"
        )
    );
    assert!(
        !massive_game_server_core::operational::admin_auth::constant_time_eq(
            "same-value",
            "different"
        )
    );
    assert!(
        !massive_game_server_core::operational::admin_auth::constant_time_eq("short", "shorter")
    );
    let left = "a".repeat(256);
    let right = "a".repeat(512);
    assert!(
        !massive_game_server_core::operational::admin_auth::constant_time_eq(&left, &right),
        "length mismatches must not alias through truncation"
    );
}

#[test]
fn test_effective_ws_auth_requirement_respects_dev_override() {
    assert!(
        massive_game_server_core::routes::ws_signaling::effective_ws_auth_requirement(true, false)
    );
    assert!(
        !massive_game_server_core::routes::ws_signaling::effective_ws_auth_requirement(true, true)
    );
    assert!(
        !massive_game_server_core::routes::ws_signaling::effective_ws_auth_requirement(
            false, false
        )
    );
}

#[test]
fn test_is_admin_protected_path() {
    assert!(massive_game_server_core::operational::admin_auth::is_admin_protected_path("/api/ops"));
    assert!(
        massive_game_server_core::operational::admin_auth::is_admin_protected_path(
            "/api/ops/match-type"
        )
    );
    assert!(
        massive_game_server_core::operational::admin_auth::is_admin_protected_path(
            "/api/arena/matches"
        )
    );
    assert!(
        !massive_game_server_core::operational::admin_auth::is_admin_protected_path("/healthz")
    );
    assert!(
        !massive_game_server_core::operational::admin_auth::is_admin_protected_path("/api/public")
    );
}

#[test]
fn test_parse_list_env_splits_and_trims_values() {
    let key = "MGS_TEST_PARSE_LIST_ENV";
    with_env_var(key, Some("alpha, beta ,, gamma "), || {
        let parsed = parse_list_env(key);
        assert_eq!(parsed, vec!["alpha", "beta", "gamma"]);
    });
}

#[test]
fn test_is_allowed_ws_origin_accepts_same_origin_and_allowlist() {
    let allow = vec!["https://play.example.com".to_owned()];
    assert!(
        massive_game_server_core::routes::ws_signaling::is_allowed_ws_origin(
            Some("https://api.example.com"),
            Some("api.example.com"),
            &allow,
            false
        )
    );
    assert!(
        massive_game_server_core::routes::ws_signaling::is_allowed_ws_origin(
            Some("https://play.example.com"),
            Some("api.example.com"),
            &allow,
            false
        )
    );
}

#[test]
fn test_is_allowed_ws_origin_dev_mode_localhost_rules() {
    assert!(
        massive_game_server_core::routes::ws_signaling::is_allowed_ws_origin(
            Some("http://localhost:5173"),
            Some("api.example.com"),
            &[],
            true
        )
    );
    assert!(
        !massive_game_server_core::routes::ws_signaling::is_allowed_ws_origin(
            Some("http://localhost:5173"),
            Some("api.example.com"),
            &[],
            false
        )
    );
}

#[test]
fn test_static_cache_control_for_path() {
    assert_eq!(
        massive_game_server_core::routes::static_files::static_cache_control_for_path(Path::new(
            "index.html"
        )),
        "no-cache, no-store, must-revalidate"
    );
    assert_eq!(
        massive_game_server_core::routes::static_files::static_cache_control_for_path(Path::new(
            "bundle.js"
        )),
        "public, max-age=300, must-revalidate"
    );
    assert_eq!(
        massive_game_server_core::routes::static_files::static_cache_control_for_path(Path::new(
            "runtime.unknown"
        )),
        "public, max-age=3600"
    );
}

#[test]
fn test_client_html_uses_hash_based_csp_without_unsafe_inline() {
    let client_html_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../static_client/client.html");
    let csp =
        massive_game_server_core::routes::static_files::static_content_security_policy_for_path(
            &client_html_path,
        )
        .expect("client.html CSP should be generated");
    assert!(
        csp.contains("script-src 'self' 'unsafe-eval' blob:"),
        "expected client CSP to allow self, blob, and unsafe-eval for pixi shader compilation"
    );
    assert!(
        csp.contains("'sha256-"),
        "expected client CSP to contain inline script hashes"
    );
    assert!(
        !csp.contains("script-src 'self' 'unsafe-inline'"),
        "client.html should no longer require unsafe-inline in script-src"
    );
}

#[test]
fn test_client_html_csp_tracks_live_file_updates() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be after Unix epoch")
        .as_nanos();
    let temp_dir =
        std::env::temp_dir().join(format!("mgs-csp-refresh-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&temp_dir).expect("create CSP test directory");
    let client_path = temp_dir.join("client.html");

    std::fs::write(&client_path, "<script>window.version = 1;</script>")
        .expect("write initial client HTML");
    let initial =
        massive_game_server_core::routes::static_files::static_content_security_policy_for_path(
            &client_path,
        )
        .expect("initial client CSP should be generated");

    std::fs::write(&client_path, "<script>window.version = 2;</script>")
        .expect("write updated client HTML");
    let updated =
        massive_game_server_core::routes::static_files::static_content_security_policy_for_path(
            &client_path,
        )
        .expect("updated client CSP should be generated");

    std::fs::remove_dir_all(&temp_dir).expect("remove CSP test directory");
    assert_ne!(initial, updated, "CSP hashes must follow the served HTML");
}

#[test]
fn test_index_html_uses_strict_self_hosted_csp() {
    let index_html_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../static_client/index.html");
    let csp =
        massive_game_server_core::routes::static_files::static_content_security_policy_for_path(
            &index_html_path,
        )
        .expect("index.html CSP should be generated");
    assert!(csp.contains("script-src 'self'"));
    assert!(csp.contains("style-src 'self'"));
    assert!(!csp.contains("'unsafe-inline'"));
    assert!(!csp.contains("'unsafe-eval'"));
}

#[test]
fn test_index_html_does_not_depend_on_external_cdns() {
    let index_html_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../static_client/index.html");
    let html = std::fs::read_to_string(index_html_path).expect("read index.html");
    assert!(!html.contains("cdn.tailwindcss.com"));
    assert!(!html.contains("fonts.googleapis.com"));
    assert!(!html.contains("fonts.gstatic.com"));
    assert!(!html.contains("cdnjs.cloudflare.com"));
}

#[test]
fn test_parse_u64_env_defaults_and_filters_zero() {
    let key = "MGS_TEST_PARSE_U64_ENV";
    with_env_var(key, Some("256"), || {
        assert_eq!(parse_u64_env(key, 10), 256);
    });
    with_env_var(key, Some("0"), || {
        assert_eq!(parse_u64_env(key, 10), 10);
    });
    with_env_var(key, Some("invalid"), || {
        assert_eq!(parse_u64_env(key, 10), 10);
    });
}

#[test]
fn test_parse_forwarded_for_ip_single() {
    assert_eq!(
        massive_game_server_core::operational::admin_auth::parse_forwarded_for_ip("192.168.1.1",),
        Some(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)))
    );
}

#[test]
fn test_parse_forwarded_for_ip_multiple() {
    assert_eq!(
        massive_game_server_core::operational::admin_auth::parse_forwarded_for_ip(
            "10.0.0.1, 192.168.1.1",
        ),
        Some(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)))
    );
}

#[test]
fn test_parse_forwarded_for_ip_invalid() {
    assert_eq!(
        massive_game_server_core::operational::admin_auth::parse_forwarded_for_ip("invalid-ip",),
        None
    );
    assert_eq!(
        massive_game_server_core::operational::admin_auth::parse_forwarded_for_ip(""),
        None
    );
}

#[test]
fn test_resolve_admin_source_ip_trusts_forwarded_headers_only_for_trusted_proxy() {
    let mut headers = warp::http::HeaderMap::new();
    headers.insert(
        "x-forwarded-for",
        warp::http::HeaderValue::from_static("198.51.100.10"),
    );
    headers.insert(
        "x-real-ip",
        warp::http::HeaderValue::from_static("198.51.100.11"),
    );

    // Default trusted list includes loopback.
    let trusted_socket = Some(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
    assert_eq!(
        massive_game_server_core::operational::admin_auth::resolve_admin_source_ip(
            trusted_socket,
            &headers
        ),
        Some(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 10)))
    );

    // Public/untrusted socket should ignore spoofable forwarding headers.
    let untrusted_socket = Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 20)));
    assert_eq!(
        massive_game_server_core::operational::admin_auth::resolve_admin_source_ip(
            untrusted_socket,
            &headers
        ),
        Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 20)))
    );
}
