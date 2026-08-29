use crate::core::config::ServerConfig;
use crate::core::types::PlayerAoI;
use crate::network::signaling::{
    handle_signaling_connection, try_acquire_ip_connection_slot, ChatMessagesQueue,
    ClientStatesMap, DataChannelsMap, IpConnectionGuard, PlayerManagerRef, ServerInstanceRef,
    SignalingPeers, WorldPartitionManagerRef,
};
use crate::operational::admin_auth::resolve_admin_source_ip;
use crate::operational::auth::AuthService;
use crate::operational::config::env_registry::WsSecurityEnv;
use crate::operational::monitoring::tracing as monitoring_tracing;
use crate::scaling::HorizontalScalingCoordinator;
use crate::server::instance::MatchType;
use dashmap::DashMap;
use serde::Deserialize;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::{info, warn, Instrument};
use tracing_opentelemetry::OpenTelemetrySpanExt;
use uuid::Uuid;
use warp::http::uri::Authority;
use warp::http::{HeaderMap, StatusCode, Uri};
use warp::{Filter, Reply};

/// Rejection returned when a WebSocket upgrade request has a disallowed Origin header.
#[derive(Debug)]
pub struct OriginRejection;

impl warp::reject::Reject for OriginRejection {}

/// Rejection returned when WebSocket upgrades arrive over an insecure transport in production mode.
#[derive(Debug)]
pub struct TransportSecurityRejection;

impl warp::reject::Reject for TransportSecurityRejection {}

/// Rejection returned when the server has reached its WebSocket signaling connection capacity.
#[derive(Debug)]
pub struct ConnectionLimitRejection;

impl warp::reject::Reject for ConnectionLimitRejection {}

pub type WsConnectionPermit = OwnedSemaphorePermit;

#[derive(Clone)]
pub struct WsSecurityFilters {
    pub behind_tls_proxy: bool,
    pub ws_require_auth: bool,
    pub origin_check_filter: warp::filters::BoxedFilter<()>,
    pub ws_connection_cap_filter: warp::filters::BoxedFilter<(WsConnectionPermit,)>,
}

pub fn effective_ws_auth_requirement(require_auth_env: bool, ws_dev_mode: bool) -> bool {
    require_auth_env && !ws_dev_mode
}

pub fn build_ws_security_filters(
    signaling_peers: SignalingPeers,
    default_max_ws_connections: u64,
    ws_env: &WsSecurityEnv,
) -> WsSecurityFilters {
    let behind_tls_proxy = ws_env.behind_tls_proxy;
    let ws_dev_mode = ws_env.dev_mode;
    let ws_require_auth_env = ws_env.require_auth_env;
    let ws_require_auth = effective_ws_auth_requirement(ws_require_auth_env, ws_dev_mode);
    let ws_allowed_origins: Arc<Vec<String>> = Arc::new(ws_env.allowed_origins.clone());
    let enforce_secure_ws_transport =
        behind_tls_proxy && !ws_dev_mode && !ws_env.allow_insecure_ws_proxy_proto;
    let configured_proxy_cidrs = ws_env.trusted_proxy_cidrs.clone();

    if behind_tls_proxy {
        info!("MGS_BEHIND_TLS_PROXY enabled: HSTS headers will be added to HTTP responses.");
    }
    if enforce_secure_ws_transport {
        info!("WebSocket transport enforcement enabled: X-Forwarded-Proto must be 'https'.");
    }
    if ws_dev_mode {
        info!("MGS_DEV_MODE enabled: localhost/127.0.0.1 WebSocket origins are permitted.");
        if ws_require_auth_env {
            warn!(
                "MGS_REQUIRE_AUTH is set but MGS_DEV_MODE is enabled; WebSocket auth enforcement is disabled in dev mode."
            );
        }
    }
    if ws_require_auth {
        info!("WebSocket auth enforcement enabled: valid auth token required for /ws.");
    } else if !ws_dev_mode {
        warn!("WebSocket auth enforcement is disabled. Set MGS_REQUIRE_AUTH=1 for production.");
    }
    if !configured_proxy_cidrs.is_empty() {
        info!(
            "Trusted proxy CIDR allowlist configured explicitly ({} entries).",
            configured_proxy_cidrs.len()
        );
    } else if ws_dev_mode {
        warn!(
            "Using development trusted proxy defaults (loopback + RFC1918). Set \
             MGS_TRUSTED_PROXY_CIDRS for explicit proxy trust."
        );
    } else {
        warn!(
            "No explicit trusted proxy CIDRs configured; only loopback is trusted. \
             Set MGS_TRUSTED_PROXY_CIDRS in production when running behind a proxy."
        );
    }
    if !ws_allowed_origins.is_empty() {
        info!(
            "WebSocket Origin allowlist ({} entries): {:?}",
            ws_allowed_origins.len(),
            *ws_allowed_origins
        );
    }

    let origin_check_filter = build_origin_check_filter(
        ws_allowed_origins.clone(),
        ws_dev_mode,
        enforce_secure_ws_transport,
    )
    .untuple_one()
    .boxed();

    let max_ws_connections = ws_env
        .max_concurrent_connections
        .unwrap_or(default_max_ws_connections)
        .max(1);
    info!("WebSocket signaling connection cap: {}", max_ws_connections);
    let ws_connection_cap_filter =
        build_connection_cap_filter(signaling_peers, max_ws_connections).boxed();

    WsSecurityFilters {
        behind_tls_proxy,
        ws_require_auth,
        origin_check_filter,
        ws_connection_cap_filter,
    }
}

pub fn build_origin_check_filter(
    allowed_origins: Arc<Vec<String>>,
    ws_dev_mode: bool,
    enforce_secure_ws_transport: bool,
) -> impl Filter<Extract = ((),), Error = warp::Rejection> + Clone {
    warp::header::headers_cloned().and_then(move |headers: HeaderMap| {
        let allowed = allowed_origins.clone();
        async move {
            if enforce_secure_ws_transport {
                let forwarded_proto = headers
                    .get("x-forwarded-proto")
                    .and_then(|v| v.to_str().ok())
                    .map(|v| v.trim().to_ascii_lowercase());
                if forwarded_proto.as_deref() != Some("https") {
                    warn!(
                        "WebSocket upgrade rejected due to insecure forwarded proto: {:?}",
                        forwarded_proto
                    );
                    return Err(warp::reject::custom(TransportSecurityRejection));
                }
            }
            let origin = headers.get("origin").and_then(|v| v.to_str().ok());
            let host = headers.get("host").and_then(|v| v.to_str().ok());
            if is_allowed_ws_origin(origin, host, &allowed, ws_dev_mode) {
                Ok(())
            } else {
                warn!(
                    "WebSocket upgrade rejected due to Origin mismatch. origin={:?} host={:?}",
                    origin, host
                );
                Err(warp::reject::custom(OriginRejection))
            }
        }
    })
}

pub fn build_connection_cap_filter(
    _peers: SignalingPeers,
    max_ws_connections: u64,
) -> impl Filter<Extract = (WsConnectionPermit,), Error = warp::Rejection> + Clone {
    let connection_slots = Arc::new(Semaphore::new(max_ws_connections as usize));
    warp::any()
        .and(warp::any().map(move || connection_slots.clone()))
        .and_then(move |connection_slots: Arc<Semaphore>| async move {
            match connection_slots.clone().try_acquire_owned() {
                Ok(permit) => Ok(permit),
                Err(_) => {
                    let active_connections = max_ws_connections
                        .saturating_sub(connection_slots.available_permits() as u64);
                    warn!(
                        "WebSocket signaling connection limit reached: {} active peers (limit {}).",
                        active_connections, max_ws_connections
                    );
                    Err(warp::reject::custom(ConnectionLimitRejection))
                }
            }
        })
}

/// Pre-upgrade per-IP concurrent-connection cap check for /ws. Returns the
/// slot guard to hold for the connection's lifetime, or a 429 response that
/// rejects the upgrade before the WebSocket handshake completes. (Previously
/// the cap was enforced after the 101 upgrade, so over-cap clients still
/// fired the browser `open` event before the server closed the socket.)
pub fn check_ws_ip_connection_cap(
    client_ip: Option<IpAddr>,
    peer_id: &str,
) -> Result<Option<IpConnectionGuard>, warp::reply::Response> {
    let Some(ip) = client_ip else {
        return Ok(None);
    };
    match try_acquire_ip_connection_slot(&ip) {
        Some(guard) => Ok(Some(guard)),
        None => {
            warn!(
                "[{}]: WebSocket upgrade rejected, per-IP concurrent connection cap reached (ip={}).",
                peer_id, ip
            );
            Err(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "ip_connection_limit",
                    "detail": "Too many simultaneous connections from this IP.",
                })),
                StatusCode::TOO_MANY_REQUESTS,
            )
            .into_response())
        }
    }
}

fn normalize_ws_host(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('.');
    if trimmed.is_empty() {
        return None;
    }

    let ip_candidate = trimmed
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(trimmed);
    if let Ok(ip) = ip_candidate.parse::<std::net::IpAddr>() {
        return Some(ip.to_string());
    }

    idna::domain_to_ascii(trimmed)
        .ok()
        .map(|host| host.to_ascii_lowercase())
}

fn parse_authority_host_port(raw: &str) -> Option<(String, Option<u16>)> {
    let authority = raw.trim().parse::<Authority>().ok()?;
    if authority.as_str().contains('@') {
        return None;
    }
    let host = normalize_ws_host(authority.host())?;
    Some((host, authority.port_u16()))
}

fn parse_ws_origin_host_port(raw_origin: &str) -> Option<(String, u16)> {
    let origin_uri = raw_origin.trim().parse::<Uri>().ok()?;
    let scheme = origin_uri.scheme_str()?.to_ascii_lowercase();
    let default_port = match scheme.as_str() {
        "https" => 443,
        "http" => 80,
        _ => return None,
    };
    if origin_uri.query().is_some() {
        return None;
    }
    let origin_path = origin_uri.path();
    if !origin_path.is_empty() && origin_path != "/" {
        return None;
    }
    let authority = origin_uri.authority()?;
    let (host, explicit_port) = parse_authority_host_port(authority.as_str())?;
    Some((host, explicit_port.unwrap_or(default_port)))
}

fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or_else(|_| {
            host.strip_prefix('[')
                .and_then(|value| value.strip_suffix(']'))
                .and_then(|value| value.parse::<std::net::IpAddr>().ok())
                .map(|ip| ip.is_loopback())
                .unwrap_or(false)
        })
}

pub fn is_allowed_ws_origin(
    origin: Option<&str>,
    host: Option<&str>,
    allowed_origins: &[String],
    dev_mode: bool,
) -> bool {
    let origin = match origin {
        Some(o) if !o.is_empty() => o,
        // No Origin header (non-browser client) -- allow
        _ => return true,
    };
    let (origin_host, origin_port) = match parse_ws_origin_host_port(origin) {
        Some(parsed) => parsed,
        None => return false,
    };

    // Same-origin check against Host header.
    if let Some(host_value) = host {
        if let Some((host_name, host_port)) = parse_authority_host_port(host_value) {
            if host_name == origin_host && (host_port.is_none() || host_port == Some(origin_port)) {
                return true;
            }
        }
    }

    // Explicit allowlist from MGS_ALLOWED_ORIGINS.
    for allowed in allowed_origins {
        if let Some((allowed_host, allowed_port)) = parse_ws_origin_host_port(allowed) {
            if origin_host == allowed_host && origin_port == allowed_port {
                return true;
            }
            continue;
        }

        // Backward-compatible support for host[:port]-only entries.
        if let Some((allowed_host, allowed_port)) = parse_authority_host_port(allowed) {
            if origin_host == allowed_host
                && (allowed_port.is_none() || allowed_port == Some(origin_port))
            {
                return true;
            }
        }
    }

    // In dev mode, accept localhost / 127.0.0.1 origins.
    if dev_mode && is_loopback_host(&origin_host) {
        return true;
    }

    false
}

#[derive(Clone, Default, Deserialize)]
struct WsAuthQuery {
    team_id: Option<u8>,
    team: Option<String>,
    spectator: Option<String>,
    mode: Option<String>,
    username: Option<String>,
    is_mobile: Option<bool>,
    match_type: Option<String>,
}

impl WsAuthQuery {
    fn requested_team_id(&self) -> Option<u8> {
        if let Some(team_id) = self.team_id {
            return Some(team_id);
        }

        let team_hint = self
            .team
            .as_deref()
            .or(self.mode.as_deref())
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if team_hint == "spectator" || team_hint == "spec" {
            return Some(0);
        }
        if team_hint == "1" || team_hint == "team1" || team_hint == "red" {
            return Some(1);
        }
        if team_hint == "2" || team_hint == "team2" || team_hint == "blue" {
            return Some(2);
        }

        if self
            .spectator
            .as_deref()
            .and_then(parse_boolish_query)
            .unwrap_or(false)
        {
            return Some(0);
        }
        None
    }
}

fn parse_boolish_query(raw: &str) -> Option<bool> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_signaling_route(
    config: Arc<ServerConfig>,
    signaling_peers: SignalingPeers,
    player_manager: PlayerManagerRef,
    world_partition_manager: WorldPartitionManagerRef,
    data_channels: DataChannelsMap,
    client_states: ClientStatesMap,
    chat_messages: ChatMessagesQueue,
    player_aois: Arc<DashMap<String, PlayerAoI>>,
    server_instance: ServerInstanceRef,
    auth_service: AuthService,
    scaling_coordinator: Arc<HorizontalScalingCoordinator>,
    ws_require_auth: bool,
    quic_primary_only: bool,
    origin_check_filter: warp::filters::BoxedFilter<()>,
    ws_connection_cap_filter: warp::filters::BoxedFilter<(WsConnectionPermit,)>,
) -> warp::filters::BoxedFilter<(warp::reply::Response,)> {
    let signaling_route_ws = warp::path("ws")
        .and(origin_check_filter)
        .and(ws_connection_cap_filter)
        .and(warp::ws())
        .and(
            warp::query::<WsAuthQuery>()
                .or(warp::any().map(WsAuthQuery::default))
                .unify(),
        )
        .and(warp::header::headers_cloned())
        .and(warp::addr::remote())
        .and(warp::any().map(move || signaling_peers.clone()))
        .and(warp::any().map(move || player_manager.clone()))
        .and(warp::any().map(move || world_partition_manager.clone()))
        .and(warp::any().map(move || data_channels.clone()))
        .and(warp::any().map(move || client_states.clone()))
        .and(warp::any().map(move || chat_messages.clone()))
        .and(warp::any().map(move || config.clone()))
        .and(warp::any().map(move || player_aois.clone()))
        .and(warp::any().map(move || server_instance.clone()))
        .and(warp::any().map(move || auth_service.clone()))
        .and(warp::any().map(move || scaling_coordinator.clone()))
        .map(
            move |ws_connection_permit: WsConnectionPermit,
                  ws: warp::ws::Ws,
                  ws_auth_query: WsAuthQuery,
                  request_headers: HeaderMap,
                  remote_addr: Option<SocketAddr>,
                  s_peers: SignalingPeers,
                  p_manager: PlayerManagerRef,
                  w_p_manager: WorldPartitionManagerRef,
                  d_channels: DataChannelsMap,
                  c_states: ClientStatesMap,
                  chats: ChatMessagesQueue,
                  conf: Arc<ServerConfig>,
                  p_aois: Arc<DashMap<String, PlayerAoI>>,
                  server_inst: ServerInstanceRef,
                  auth_service: AuthService,
                  scaling_coordinator: Arc<HorizontalScalingCoordinator>| {
                let peer_id = Uuid::new_v4().to_string();
                let requested_team_id = ws_auth_query.requested_team_id();
                let requested_username = ws_auth_query.username.clone();
                let auth_token = request_headers
                    .get("authorization")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|auth_hdr| auth_hdr.strip_prefix("Bearer ").map(str::trim))
                    .map(str::to_owned)
                    .or_else(|| {
                        request_headers
                            .get("cookie")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|cookie_hdr| {
                                cookie_hdr.split(';').find_map(|pair| {
                                    let pair = pair.trim();
                                    pair.strip_prefix("mgs_session=")
                                        .map(|v| v.trim().to_owned())
                                        .filter(|v| !v.is_empty())
                                })
                            })
                    })
                    .unwrap_or_default();
                let auth_user_id = auth_service.resolve_user_id_from_token(&auth_token);
                if ws_require_auth && auth_user_id.is_none() {
                    warn!(
                        "Rejecting unauthenticated WebSocket signaling upgrade for peer={}",
                        peer_id
                    );
                    return warp::reply::with_status(
                        warp::reply::json(&serde_json::json!({
                            "error": "auth_required",
                            "detail": "Authentication required for signaling.",
                        })),
                        StatusCode::UNAUTHORIZED,
                    )
                    .into_response();
                }

                if let Some(bound_user_id) = auth_user_id.as_deref() {
                    if let Some(profile) = auth_service.profile_by_user_id(bound_user_id) {
                        let routing_key = format!("user:{}", bound_user_id);
                        let assignment = scaling_coordinator
                            .assignment_for_match_with_mmr(&routing_key, profile.mmr);
                        info!(
                            "MMR shard hint for {} (band={}, mmr={:.1}): primary={}, replicas={:?}",
                            bound_user_id,
                            profile.mmr_band,
                            profile.mmr,
                            assignment.primary_shard,
                            assignment.replica_shards
                        );
                    }
                }
                let socket_ip = remote_addr.map(|addr| addr.ip());
                let client_ip = resolve_admin_source_ip(socket_ip, &request_headers);
                // Enforce the per-IP concurrent-connection cap before the
                // upgrade so over-cap clients get a clean 429 at the HTTP
                // handshake instead of a successful 101 followed by a close.
                let ip_connection_guard = match check_ws_ip_connection_cap(client_ip, &peer_id) {
                    Ok(guard) => guard,
                    Err(response) => return response,
                };
                let remote_context = monitoring_tracing::extract_remote_context(
                    request_headers
                        .get("traceparent")
                        .and_then(|value| value.to_str().ok()),
                    request_headers
                        .get("tracestate")
                        .and_then(|value| value.to_str().ok()),
                );
                let ws_upgrade_span = tracing::info_span!(
                    "ws_signaling_connection",
                    peer_id = %peer_id,
                    transport = "webrtc",
                    auth_user_id = auth_user_id.as_deref().unwrap_or("anonymous")
                );
                ws_upgrade_span.set_parent(remote_context);

                let is_mobile = ws_auth_query.is_mobile.unwrap_or(false);
                let requested_match_type = ws_auth_query.match_type.as_deref().unwrap_or("full");
                let match_type = MatchType::from_query_str(requested_match_type);
                info!(
                    "WS connection: peer={}, match_type={}, is_mobile={}",
                    peer_id, match_type, is_mobile
                );
                if match_type == MatchType::QuickMatch {
                    server_inst.note_human_queue_arrival();
                }
                ws.on_upgrade(move |socket| {
                    handle_signaling_connection(
                        socket,
                        peer_id,
                        s_peers,
                        p_manager,
                        w_p_manager,
                        d_channels,
                        c_states,
                        chats,
                        conf,
                        p_aois,
                        server_inst,
                        auth_service,
                        auth_user_id,
                        requested_team_id,
                        requested_username,
                        client_ip,
                        ip_connection_guard,
                        is_mobile,
                        ws_connection_permit,
                    )
                    .instrument(ws_upgrade_span)
                })
                .into_response()
            },
        )
        .boxed();

    if quic_primary_only {
        warp::path("ws")
            .and(warp::path::end())
            .and(warp::get())
            .map(|| {
                warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "error": "quic_primary_only",
                        "detail": "WebSocket signaling is disabled. Use QUIC primary transport."
                    })),
                    StatusCode::UPGRADE_REQUIRED,
                )
                .into_response()
            })
            .boxed()
    } else {
        signaling_route_ws.boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::signaling::DEFAULT_MAX_WS_CONNECTIONS_PER_IP;
    use std::net::Ipv4Addr;

    // The cap's OnceLock reads MGS_MAX_WS_CONNECTIONS_PER_IP once per process;
    // reading the same env var here keeps the test correct under overrides.
    fn effective_per_ip_cap() -> u32 {
        std::env::var("MGS_MAX_WS_CONNECTIONS_PER_IP")
            .ok()
            .and_then(|value| value.trim().parse::<u32>().ok())
            .unwrap_or(DEFAULT_MAX_WS_CONNECTIONS_PER_IP)
    }

    #[test]
    fn cap_check_passes_without_client_ip() {
        assert!(check_ws_ip_connection_cap(None, "test-peer")
            .expect("missing client IP must not be capped")
            .is_none());
    }

    #[test]
    fn cap_check_keys_on_forwarded_ip_from_trusted_proxy() {
        let max = effective_per_ip_cap();
        if max == 0 {
            return; // Cap disabled in this environment.
        }
        // Simulate ngrok: the direct peer is loopback (trusted proxy) and the
        // real client IP arrives via X-Forwarded-For.
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            warp::http::HeaderValue::from_static("198.51.100.77"),
        );
        let socket_ip = Some(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        let client_ip = resolve_admin_source_ip(socket_ip, &headers);
        assert_eq!(
            client_ip,
            Some(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 77))),
            "trusted proxy must resolve the forwarded client IP"
        );

        // Exhaust the forwarded IP's slots through the pre-upgrade check.
        let mut guards = Vec::new();
        for _ in 0..max {
            guards.push(
                check_ws_ip_connection_cap(client_ip, "test-peer")
                    .expect("acquisition within the cap must succeed"),
            );
        }

        // The next upgrade from the same forwarded IP is rejected pre-handshake
        // with HTTP 429.
        let rejection = check_ws_ip_connection_cap(client_ip, "test-peer")
            .expect_err("connection beyond the cap must be rejected");
        assert_eq!(rejection.status(), StatusCode::TOO_MANY_REQUESTS);

        // A different forwarded IP (different real client) is unaffected.
        headers.insert(
            "x-forwarded-for",
            warp::http::HeaderValue::from_static("198.51.100.78"),
        );
        let other_client_ip = resolve_admin_source_ip(socket_ip, &headers);
        assert!(
            check_ws_ip_connection_cap(other_client_ip, "test-peer").is_ok(),
            "a different client IP must have its own slot pool"
        );

        // Releasing a connection (guard drop) frees a slot for the capped IP.
        drop(guards.pop());
        assert!(
            check_ws_ip_connection_cap(client_ip, "test-peer").is_ok(),
            "dropping a guard must release its slot"
        );
        drop(guards);
    }

    #[test]
    fn cap_check_ignores_forwarded_header_from_untrusted_peer() {
        // A direct (untrusted) peer must not be able to steer the cap key via
        // spoofed forwarding headers.
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            warp::http::HeaderValue::from_static("198.51.100.99"),
        );
        let socket_ip = Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 20)));
        let client_ip = resolve_admin_source_ip(socket_ip, &headers);
        assert_eq!(
            client_ip, socket_ip,
            "untrusted peer's forwarding headers must be ignored"
        );
    }
}
