use super::sanitization::parse_csv;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::Sha256;
use std::{
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tracing::warn;
use webrtc::ice_transport::ice_server::RTCIceServer;

/// HMAC-SHA1 type alias for legacy TURN credential generation.
type HmacSha1 = Hmac<Sha1>;
/// HMAC-SHA256 type alias for TURN credential generation.
type HmacSha256 = Hmac<Sha256>;

/// Default TURN credential TTL: 24 hours (in seconds).
pub(super) const TURN_CREDENTIAL_TTL_SECS: u64 = 86400;

/// TURN credential type parsed from `MGS_TURN_CREDENTIAL_TYPE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TurnCredentialType {
    /// Static password -- credential is sent as-is.
    Password,
    /// HMAC time-limited credentials -- credential is HMAC-SHA256(secret, username)
    /// where username = "expiry_timestamp:random_suffix".
    HmacSha256,
    /// Legacy HMAC-SHA1 mode for transitional TURN deployments.
    HmacSha1Legacy,
}

impl TurnCredentialType {
    pub(super) fn from_raw(raw: Option<&str>) -> Self {
        match raw.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("hmac-sha1") | Some("sha1") => Self::HmacSha1Legacy,
            Some("hmac") | Some("hmac-sha256") | Some("sha256") => Self::HmacSha256,
            _ => Self::Password,
        }
    }

    #[cfg(test)]
    pub(super) fn from_env() -> Self {
        Self::from_raw(std::env::var("MGS_TURN_CREDENTIAL_TYPE").ok().as_deref())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TurnHmacAlgorithm {
    Sha1,
    Sha256,
}

/// Generate time-limited TURN credentials using HMAC.
pub(super) fn generate_turn_hmac_credentials_with_algorithm(
    shared_secret: &str,
    suffix: &str,
    algorithm: TurnHmacAlgorithm,
) -> (String, String) {
    let algorithm_label = match algorithm {
        TurnHmacAlgorithm::Sha1 => "SHA-1",
        TurnHmacAlgorithm::Sha256 => "SHA-256",
    };

    let expiry = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs()
        + TURN_CREDENTIAL_TTL_SECS;
    let username = format!("{expiry}:{suffix}");

    let credential = match algorithm {
        TurnHmacAlgorithm::Sha1 => {
            let mut mac = match HmacSha1::new_from_slice(shared_secret.as_bytes()) {
                Ok(mac) => mac,
                Err(err) => {
                    warn!(
                        "Failed to initialize TURN HMAC ({}) generator (secret length={}): {}",
                        algorithm_label,
                        shared_secret.len(),
                        err
                    );
                    return (username, String::new());
                }
            };
            mac.update(username.as_bytes());
            base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
        }
        TurnHmacAlgorithm::Sha256 => {
            let mut mac = match HmacSha256::new_from_slice(shared_secret.as_bytes()) {
                Ok(mac) => mac,
                Err(err) => {
                    warn!(
                        "Failed to initialize TURN HMAC ({}) generator (secret length={}): {}",
                        algorithm_label,
                        shared_secret.len(),
                        err
                    );
                    return (username, String::new());
                }
            };
            mac.update(username.as_bytes());
            base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes())
        }
    };

    (username, credential)
}

/// Generate time-limited TURN credentials using HMAC-SHA256.
///
/// The username is `expiry_timestamp:suffix` and the credential is
/// `Base64(HMAC-SHA256(shared_secret, username))`.
///
/// This follows the ephemeral credential mechanism described in
/// [RFC draft: A REST API For Access To TURN Services](https://datatracker.ietf.org/doc/html/draft-uberti-behave-turn-rest-00)
/// and used by coturn, Twilio, Xirsys, and other TURN providers.
pub fn generate_turn_hmac_credentials(shared_secret: &str, suffix: &str) -> (String, String) {
    generate_turn_hmac_credentials_with_algorithm(shared_secret, suffix, TurnHmacAlgorithm::Sha256)
}

/// A serializable ICE server entry sent to the client during signaling.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClientIceServer {
    pub urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct CachedIceConfig {
    pub(super) disable_stun: bool,
    pub(super) stun_urls: Vec<String>,
    pub(super) turn_urls: Vec<String>,
    pub(super) turn_credential_type: TurnCredentialType,
    pub(super) turn_username: Option<String>,
    pub(super) turn_credential: Option<String>,
    pub(super) extra_ice_servers: Vec<RTCIceServer>,
}

fn load_cached_ice_config() -> CachedIceConfig {
    let runtime = super::signaling_env_config();
    let disable_stun = runtime.disable_stun;
    let stun_urls = if runtime.stun_urls.is_empty() {
        vec!["stun:stun.l.google.com:19302".to_owned()]
    } else {
        runtime.stun_urls.clone()
    };
    let turn_urls = runtime.turn_urls.clone();
    let turn_credential_type =
        TurnCredentialType::from_raw(runtime.turn_credential_type.as_deref());
    let turn_username = runtime.turn_username.clone();
    let turn_credential = runtime.turn_credential.clone();
    let extra_ice_servers = runtime
        .extra_ice_servers
        .as_ref()
        .map(|raw| parse_ice_servers_env(raw))
        .unwrap_or_default();

    CachedIceConfig {
        disable_stun,
        stun_urls,
        turn_urls,
        turn_credential_type,
        turn_username,
        turn_credential,
        extra_ice_servers,
    }
}

pub(super) fn cached_ice_config() -> &'static CachedIceConfig {
    static CONFIG: OnceLock<CachedIceConfig> = OnceLock::new();
    CONFIG.get_or_init(load_cached_ice_config)
}

pub(super) fn build_ice_servers_from_config(cfg: &CachedIceConfig) -> Vec<RTCIceServer> {
    let mut ice_servers: Vec<RTCIceServer> = Vec::new();

    if !cfg.disable_stun {
        ice_servers.push(RTCIceServer {
            urls: cfg.stun_urls.clone(),
            ..Default::default()
        });
    }

    if !cfg.turn_urls.is_empty() {
        let mut turn_server = RTCIceServer {
            urls: cfg.turn_urls.clone(),
            ..Default::default()
        };
        match cfg.turn_credential_type {
            TurnCredentialType::Password => {
                if let Some(username) = cfg.turn_username.as_ref() {
                    turn_server.username = username.clone();
                }
                if let Some(credential) = cfg.turn_credential.as_ref() {
                    turn_server.credential = credential.clone();
                }
            }
            TurnCredentialType::HmacSha256 => {
                if let Some(secret) = cfg.turn_credential.as_ref() {
                    let suffix = cfg.turn_username.as_deref().unwrap_or("server");
                    let (username, credential) = generate_turn_hmac_credentials(secret, suffix);
                    turn_server.username = username;
                    turn_server.credential = credential;
                }
            }
            TurnCredentialType::HmacSha1Legacy => {
                if let Some(secret) = cfg.turn_credential.as_ref() {
                    let suffix = cfg.turn_username.as_deref().unwrap_or("server");
                    let (username, credential) = generate_turn_hmac_credentials_with_algorithm(
                        secret,
                        suffix,
                        TurnHmacAlgorithm::Sha1,
                    );
                    turn_server.username = username;
                    turn_server.credential = credential;
                }
            }
        }
        ice_servers.push(turn_server);
    }

    if !cfg.extra_ice_servers.is_empty() {
        ice_servers.extend(cfg.extra_ice_servers.clone());
    }

    ice_servers
}

pub(super) fn build_ice_servers() -> Vec<RTCIceServer> {
    build_ice_servers_from_config(cached_ice_config())
}

/// Build the ICE server configuration to send to a connecting client.
///
/// When HMAC credential mode is active, this generates fresh per-session
/// credentials so each client gets a unique short-lived TURN token.
pub(super) fn build_client_ice_config_from_config(
    cfg: &CachedIceConfig,
    session_id: &str,
) -> Vec<ClientIceServer> {
    let mut servers: Vec<ClientIceServer> = Vec::new();

    if !cfg.disable_stun {
        servers.push(ClientIceServer {
            urls: cfg.stun_urls.clone(),
            username: None,
            credential: None,
        });
    }

    if !cfg.turn_urls.is_empty() {
        let mut turn_entry = ClientIceServer {
            urls: cfg.turn_urls.clone(),
            username: None,
            credential: None,
        };
        match cfg.turn_credential_type {
            TurnCredentialType::Password => {
                if let Some(username) = cfg.turn_username.as_ref() {
                    turn_entry.username = Some(username.clone());
                }
                if let Some(credential) = cfg.turn_credential.as_ref() {
                    turn_entry.credential = Some(credential.clone());
                }
            }
            TurnCredentialType::HmacSha256 => {
                if let Some(secret) = cfg.turn_credential.as_ref() {
                    let (username, credential) = generate_turn_hmac_credentials(secret, session_id);
                    turn_entry.username = Some(username);
                    turn_entry.credential = Some(credential);
                }
            }
            TurnCredentialType::HmacSha1Legacy => {
                if let Some(secret) = cfg.turn_credential.as_ref() {
                    let (username, credential) = generate_turn_hmac_credentials_with_algorithm(
                        secret,
                        session_id,
                        TurnHmacAlgorithm::Sha1,
                    );
                    turn_entry.username = Some(username);
                    turn_entry.credential = Some(credential);
                }
            }
        }
        servers.push(turn_entry);
    }

    servers
}

pub(super) fn build_client_ice_config(session_id: &str) -> Vec<ClientIceServer> {
    build_client_ice_config_from_config(cached_ice_config(), session_id)
}

pub(super) fn parse_ice_servers_env(raw: &str) -> Vec<RTCIceServer> {
    raw.split(';')
        .filter_map(|entry| {
            let mut parts = entry.split('|').map(|segment| segment.trim());
            let urls_raw = parts.next().unwrap_or_default();
            let urls = parse_csv(urls_raw);
            if urls.is_empty() {
                return None;
            }

            let username = parts.next().unwrap_or_default().to_owned();
            let credential = parts.next().unwrap_or_default().to_owned();

            let mut server = RTCIceServer {
                urls,
                ..Default::default()
            };
            if !username.is_empty() {
                server.username = username;
            }
            if !credential.is_empty() {
                server.credential = credential;
            }
            Some(server)
        })
        .collect()
}
