use crate::operational::config::env_registry::AdminAuthEnv;
use ipnet::IpNet;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, OnceLock};
use subtle::ConstantTimeEq;
use tracing::{info, warn};
use warp::http::{HeaderMap, Method, StatusCode};
use warp::Filter;

#[derive(Clone, Default)]
pub struct AdminAuthConfig {
    bearer_token: Option<Arc<String>>,
    ip_allowlist: Arc<Vec<IpNet>>,
}

impl AdminAuthConfig {
    pub fn from_env() -> Self {
        let mut ip_allowlist = parse_list_env("MGS_ADMIN_IP_ALLOWLIST");
        ip_allowlist.extend(parse_list_env("MGS_ADMIN_ALLOWED_IPS"));
        let env = AdminAuthEnv {
            bearer_token: std::env::var("MGS_ADMIN_BEARER_TOKEN")
                .or_else(|_| std::env::var("MGS_ADMIN_TOKEN"))
                .ok(),
            ip_allowlist,
        };
        Self::from_env_config(&env)
    }

    pub fn from_env_config(env: &AdminAuthEnv) -> Self {
        let bearer_token = env
            .bearer_token
            .as_deref()
            .map(str::trim)
            .filter(|raw| !raw.is_empty())
            .map(str::to_owned)
            .map(Arc::new);

        if bearer_token.is_some() {
            info!("Admin bearer auth enabled for /api/ops/* and /api/arena/* routes.");
        } else {
            warn!(
                "Admin bearer auth token is not configured. Protected routes will reject requests \
                (set MGS_ADMIN_BEARER_TOKEN)."
            );
        }

        let ip_allowlist = parse_admin_ip_allowlist_entries(&env.ip_allowlist);
        if ip_allowlist.is_empty() {
            info!(
                "Admin IP allowlist is disabled (set MGS_ADMIN_IP_ALLOWLIST to enforce source IP restrictions)."
            );
        } else {
            info!(
                "Admin IP allowlist enabled with {} CIDR entries.",
                ip_allowlist.len()
            );
        }

        Self {
            bearer_token,
            ip_allowlist: Arc::new(ip_allowlist),
        }
    }
}

#[derive(Debug)]
pub struct AdminAuthRejection {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl AdminAuthRejection {
    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "admin_auth_required",
            message: message.into(),
        }
    }

    fn service_unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "admin_auth_unconfigured",
            message: message.into(),
        }
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code: "admin_ip_blocked",
            message: message.into(),
        }
    }
}

impl warp::reject::Reject for AdminAuthRejection {}

pub fn parse_bearer_token(authorization_header: Option<&str>) -> Option<String> {
    let raw = authorization_header?.trim();
    if raw.is_empty() {
        return None;
    }
    let token = raw
        .strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))?
        .trim();
    if token.is_empty() {
        return None;
    }
    Some(token.to_owned())
}

pub fn constant_time_eq(left: &str, right: &str) -> bool {
    let left_bytes = left.as_bytes();
    let right_bytes = right.as_bytes();
    if left_bytes.len() != right_bytes.len() {
        return false;
    }
    left_bytes.ct_eq(right_bytes).into()
}

pub fn is_admin_protected_path(path: &str) -> bool {
    let normalized = path.trim_end_matches('/');
    normalized == "/api/ops"
        || normalized.starts_with("/api/ops/")
        || normalized == "/api/arena"
        || normalized.starts_with("/api/arena/")
}

pub fn parse_forwarded_for_ip(raw: &str) -> Option<IpAddr> {
    raw.split(',')
        .map(str::trim)
        .filter(|candidate| !candidate.is_empty())
        .find_map(|candidate| candidate.parse::<IpAddr>().ok())
}

pub fn resolve_admin_source_ip(socket_ip: Option<IpAddr>, headers: &HeaderMap) -> Option<IpAddr> {
    if socket_ip.is_some_and(is_trusted_proxy) {
        let forwarded_ip = headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
            .and_then(parse_forwarded_for_ip);
        let real_ip = headers
            .get("x-real-ip")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<IpAddr>().ok());
        forwarded_ip.or(real_ip).or(socket_ip)
    } else {
        socket_ip
    }
}

pub fn requires_admin_auth(
    config: AdminAuthConfig,
) -> impl Filter<Extract = ((),), Error = warp::Rejection> + Clone {
    warp::method()
        .and(warp::path::full())
        .and(warp::header::optional::<String>("authorization"))
        .and(warp::header::headers_cloned())
        .and(warp::addr::remote())
        .and_then(
            move |method: Method,
                  full_path: warp::path::FullPath,
                  authorization: Option<String>,
                  headers: HeaderMap,
                  remote_addr: Option<SocketAddr>| {
                let config = config.clone();
                async move {
                    let path = full_path.as_str();
                    if !is_admin_protected_path(path) {
                        return Err(warp::reject::not_found());
                    }
                    if method == Method::OPTIONS {
                        return Ok(());
                    }

                    let Some(expected_token) = config.bearer_token.as_ref() else {
                        return Err(warp::reject::custom(
                            AdminAuthRejection::service_unavailable(
                                "Admin routes are currently unavailable.",
                            ),
                        ));
                    };

                    if !config.ip_allowlist.is_empty() {
                        let socket_ip = remote_addr.map(|addr| addr.ip());
                        let source_ip = resolve_admin_source_ip(socket_ip, &headers);

                        let Some(source_ip) = source_ip else {
                            return Err(warp::reject::custom(AdminAuthRejection::forbidden(
                                "Admin request source IP could not be determined.",
                            )));
                        };

                        if !admin_ip_allowed(config.ip_allowlist.as_slice(), source_ip) {
                            return Err(warp::reject::custom(AdminAuthRejection::forbidden(
                                format!(
                                    "Admin access denied for source IP {} (not in allowlist).",
                                    source_ip
                                ),
                            )));
                        }
                    }

                    let Some(provided_token) = parse_bearer_token(authorization.as_deref()) else {
                        return Err(warp::reject::custom(AdminAuthRejection::unauthorized(
                            "Missing Authorization bearer token.",
                        )));
                    };

                    if !constant_time_eq(expected_token.as_str(), provided_token.as_str()) {
                        return Err(warp::reject::custom(AdminAuthRejection::unauthorized(
                            "Invalid admin bearer token.",
                        )));
                    }

                    Ok(())
                }
            },
        )
}

#[derive(Clone)]
struct TrustedProxyConfig {
    cidrs: Vec<IpNet>,
}

fn trusted_proxy_config() -> &'static TrustedProxyConfig {
    static CONFIG: OnceLock<TrustedProxyConfig> = OnceLock::new();
    CONFIG.get_or_init(|| {
        let configured = parse_list_env("MGS_TRUSTED_PROXY_CIDRS");
        if configured.is_empty() {
            let mut defaults = vec!["127.0.0.1/32", "::1/128"];
            if env_flag("MGS_DEV_MODE") && env_flag("MGS_DEV_TRUST_PRIVATE_PROXIES") {
                defaults.extend(["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16"]);
                warn!(
                    "MGS_DEV_TRUST_PRIVATE_PROXIES enabled: trusting RFC1918 proxy ranges in dev mode."
                );
            }
            let cidrs = defaults
                .iter()
                .filter_map(|entry| entry.parse::<IpNet>().ok())
                .collect();
            return TrustedProxyConfig { cidrs };
        }

        let mut cidrs = Vec::new();
        for entry in configured {
            if let Ok(cidr) = entry.parse::<IpNet>() {
                cidrs.push(cidr);
                continue;
            }
            if let Ok(ip) = entry.parse::<IpAddr>() {
                cidrs.push(IpNet::from(ip));
                continue;
            }
            warn!(
                "Skipping invalid trusted proxy entry '{}'. Expected IP or CIDR.",
                entry
            );
        }
        cidrs.sort_by_key(|a| a.to_string());
        cidrs.dedup_by(|a, b| a == b);
        TrustedProxyConfig { cidrs }
    })
}

fn is_trusted_proxy(ip: IpAddr) -> bool {
    trusted_proxy_config()
        .cidrs
        .iter()
        .any(|cidr| cidr.contains(&ip))
}

fn parse_admin_ip_allowlist_entries(entries: &[String]) -> Vec<IpNet> {
    let mut allowlist = Vec::new();
    for entry in entries {
        let entry = entry.as_str();
        if let Ok(cidr) = entry.parse::<IpNet>() {
            allowlist.push(cidr);
            continue;
        }
        if let Ok(ip) = entry.parse::<IpAddr>() {
            allowlist.push(IpNet::from(ip));
            continue;
        }
        warn!(
            "Skipping invalid admin allowlist entry '{}'. Expected IP or CIDR.",
            entry
        );
    }
    allowlist.sort_by_key(|a| a.to_string());
    allowlist.dedup_by(|a, b| a == b);
    allowlist
}

fn admin_ip_allowed(ip_allowlist: &[IpNet], source_ip: IpAddr) -> bool {
    if ip_allowlist.is_empty() {
        return true;
    }
    ip_allowlist.iter().any(|cidr| cidr.contains(&source_ip))
}

fn parse_list_env(var_name: &str) -> Vec<String> {
    std::env::var(var_name)
        .ok()
        .into_iter()
        .flat_map(|raw| {
            raw.split(',')
                .map(str::trim)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|item| !item.is_empty())
        .collect()
}

fn env_flag(var_name: &str) -> bool {
    std::env::var(var_name)
        .ok()
        .map(|raw| {
            let normalized = raw.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "yes" || normalized == "on"
        })
        .unwrap_or(false)
}
