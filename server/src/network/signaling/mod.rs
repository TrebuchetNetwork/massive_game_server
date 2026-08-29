// massive_game_server/server/src/network/signaling/mod.rs
//
// Directory-module refactoring of the signaling subsystem.
// All public items are re-exported from this module to preserve
// the original `crate::network::signaling::*` API surface.

mod chat;
mod cleanup;
mod client_state;
mod connection;
mod ice_config;
mod rate_limiting;
mod sanitization;
mod webrtc_state;

// ── Type aliases (formerly at the top of signaling.rs) ─────────────────────
use crate::core::types::RTCDataChannel as CoreRTCDataChannel;
use crate::entities::player::ImprovedPlayerManager;
use crate::server::instance::MassiveGameServer;
use crate::world::partition::WorldPartitionManager;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use warp::ws::Message;

pub type SignalingPeers = Arc<DashMap<String, mpsc::Sender<Result<Message, warp::Error>>>>;
pub type PlayerManagerRef = Arc<ImprovedPlayerManager>;
pub type DataChannelsMap = Arc<DashMap<String, Arc<CoreRTCDataChannel>>>;
pub type WorldPartitionManagerRef = Arc<WorldPartitionManager>;
pub type ServerInstanceRef = Arc<MassiveGameServer>; // Type alias for server instance

// ── Static config ──────────────────────────────────────────────────────────
use crate::operational::config::env_registry::SignalingEnv;
use std::sync::OnceLock;

use rate_limiting::DEFAULT_SDP_ADMISSION_CONCURRENCY;

static SIGNALING_RUNTIME_CONFIG: OnceLock<SignalingEnv> = OnceLock::new();

fn default_signaling_env_config() -> SignalingEnv {
    SignalingEnv {
        chat_cooldown_ms: 450,
        chat_burst_capacity: 5,
        chat_burst_window_ms: 5_000,
        disable_stun: false,
        stun_urls: vec!["stun:stun.l.google.com:19302".to_owned()],
        turn_urls: Vec::new(),
        turn_credential_type: None,
        turn_username: None,
        turn_credential: None,
        extra_ice_servers: None,
        sdp_concurrency: DEFAULT_SDP_ADMISSION_CONCURRENCY,
        webrtc_nat_1to1_ips: Vec::new(),
        webrtc_nat_1to1_candidate_type: None,
        webrtc_udp_port_min: None,
        webrtc_udp_port_max: None,
    }
}

fn signaling_env_config() -> &'static SignalingEnv {
    SIGNALING_RUNTIME_CONFIG.get_or_init(default_signaling_env_config)
}

pub fn configure_signaling_runtime(config: &SignalingEnv) {
    let _ = SIGNALING_RUNTIME_CONFIG.set(config.clone());
}

// ── Public re-exports ──────────────────────────────────────────────────────
pub use chat::{
    next_chat_message_seq, BoundedChatQueue, ChatMessage, ChatMessagesQueue, MAX_CHAT_QUEUE_SIZE,
};
pub use cleanup::{cleanup_connection, handle_dc_send_error};
pub use client_state::{ClientState, ClientStatesMap, PickupState};
pub use connection::handle_signaling_connection;
pub use ice_config::{generate_turn_hmac_credentials, ClientIceServer};
pub use rate_limiting::IpConnectionGuard;
pub(crate) use rate_limiting::try_acquire_ip_connection_slot;
#[cfg(test)]
pub(crate) use rate_limiting::DEFAULT_MAX_WS_CONNECTIONS_PER_IP;
pub use webrtc_state::current_webrtc_peer_state_label;

// ── Tests ──────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::chat::try_consume_chat_rate_limit_with_map;
    use super::ice_config::parse_ice_servers_env;
    use super::ice_config::{
        build_client_ice_config_from_config, build_ice_servers_from_config, CachedIceConfig,
        TurnCredentialType, TURN_CREDENTIAL_TTL_SECS,
    };
    use super::rate_limiting::{
        env_u32, normalize_ws_keepalive_interval_secs, validate_signaling_payload,
        InputRateLimiter, JoinRateLimiter, RTCIceCandidateInitSerde, SignalingMessageJson,
        MAX_DATACHANNEL_MESSAGE_BYTES, MAX_SIGNALING_ICE_CANDIDATE_BYTES,
        MAX_SIGNALING_ICE_SDP_MID_BYTES, MAX_SIGNALING_ICE_USERNAME_FRAGMENT_BYTES,
        MAX_SIGNALING_SDP_BYTES, MAX_SIGNALING_TEXT_BYTES,
    };
    use super::sanitization::{
        build_welcome_message_bytes, env_bool, is_bidi_or_directional_control, parse_csv,
        sanitize_chat_field, sanitize_text_field, sanitize_username_field,
        signaling_protocol_version,
    };
    use super::webrtc_state::{webrtc_state_label, PeerConnectionDropGuard};
    use super::*;

    use crate::core::constants::*;
    use crate::flatbuffers_generated::game_protocol as fb;
    use base64::Engine as _;
    use dashmap::DashMap;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use std::time::{SystemTime, UNIX_EPOCH};
    use webrtc::{
        ice_transport::ice_credential_type::RTCIceCredentialType,
        peer_connection::{
            peer_connection_state::RTCPeerConnectionState,
            sdp::session_description::RTCSessionDescription,
        },
    };

    type HmacSha256 = Hmac<Sha256>;

    // JoinRateLimiter and InputRateLimiter need to be accessible for tests
    // They are pub(super) in rate_limiting.rs so accessible here.

    #[test]
    fn datachannel_message_limit_is_reasonable() {
        const {
            assert!(
                MAX_DATACHANNEL_MESSAGE_BYTES >= 64 * 1024,
                "limit too small for game messages"
            )
        };
        const {
            assert!(
                MAX_DATACHANNEL_MESSAGE_BYTES <= 8 * 1024 * 1024,
                "limit too large to be protective"
            )
        };
    }

    #[test]
    fn signaling_text_limit_is_below_datachannel_limit() {
        const { assert!(MAX_SIGNALING_TEXT_BYTES <= MAX_DATACHANNEL_MESSAGE_BYTES) };
    }

    #[test]
    fn drop_guard_defuse_prevents_close() {
        let mut guard = PeerConnectionDropGuard {
            peer_connection: None,
            peer_id: "test_peer".to_owned(),
        };
        guard.defuse();
        assert!(guard.peer_connection.is_none());
    }

    #[test]
    fn chat_cooldown_blocks_burst_for_same_peer() {
        let rate_limits = DashMap::new();
        let peer_id = "peer-1";
        let cooldown_ms = 450;

        assert!(try_consume_chat_rate_limit_with_map(
            peer_id,
            1_000,
            cooldown_ms,
            0,
            0,
            &rate_limits
        ));
        assert!(!try_consume_chat_rate_limit_with_map(
            peer_id,
            1_200,
            cooldown_ms,
            0,
            0,
            &rate_limits
        ));
        assert!(try_consume_chat_rate_limit_with_map(
            peer_id,
            1_451,
            cooldown_ms,
            0,
            0,
            &rate_limits
        ));
    }

    #[test]
    fn chat_cooldown_is_per_peer() {
        let rate_limits = DashMap::new();
        let cooldown_ms = 450;

        assert!(try_consume_chat_rate_limit_with_map(
            "peer-a",
            2_000,
            cooldown_ms,
            0,
            0,
            &rate_limits
        ));
        assert!(try_consume_chat_rate_limit_with_map(
            "peer-b",
            2_050,
            cooldown_ms,
            0,
            0,
            &rate_limits
        ));
        assert!(!try_consume_chat_rate_limit_with_map(
            "peer-a",
            2_200,
            cooldown_ms,
            0,
            0,
            &rate_limits
        ));
    }

    #[test]
    fn chat_burst_budget_blocks_spam_until_tokens_refill() {
        let rate_limits = DashMap::new();
        let peer_id = "peer-burst";

        for _ in 0..5 {
            assert!(try_consume_chat_rate_limit_with_map(
                peer_id,
                10_000,
                0,
                5,
                5_000,
                &rate_limits
            ));
        }

        assert!(!try_consume_chat_rate_limit_with_map(
            peer_id,
            10_000,
            0,
            5,
            5_000,
            &rate_limits
        ));

        assert!(try_consume_chat_rate_limit_with_map(
            peer_id,
            11_000,
            0,
            5,
            5_000,
            &rate_limits
        ));
        assert!(!try_consume_chat_rate_limit_with_map(
            peer_id,
            11_000,
            0,
            5,
            5_000,
            &rate_limits
        ));
    }

    #[test]
    fn chat_burst_budget_is_isolated_per_peer() {
        let rate_limits = DashMap::new();

        assert!(try_consume_chat_rate_limit_with_map(
            "peer-a",
            2_000,
            0,
            1,
            5_000,
            &rate_limits
        ));
        assert!(try_consume_chat_rate_limit_with_map(
            "peer-b",
            2_000,
            0,
            1,
            5_000,
            &rate_limits
        ));
        assert!(!try_consume_chat_rate_limit_with_map(
            "peer-a",
            2_100,
            0,
            1,
            5_000,
            &rate_limits
        ));
    }

    // ── sanitize_text_field tests ────────────────────────────────────

    #[test]
    fn sanitize_text_field_strips_control_chars() {
        let input = "hello\x00\x01\x02world";
        let result = sanitize_text_field(input, 100, false);
        assert_eq!(result, Some("helloworld".to_owned()));
    }

    #[test]
    fn sanitize_text_field_strips_bidi_control_chars() {
        // LRM (\u{200E}), RLM (\u{200F}), LRO (\u{202D})
        let input = "hello\u{200E}\u{200F}\u{202D}world";
        let result = sanitize_text_field(input, 100, false);
        assert_eq!(result, Some("helloworld".to_owned()));
    }

    #[test]
    fn sanitize_text_field_strips_html_special_chars() {
        let input = "hello<script>alert('xss')</script>";
        let result = sanitize_text_field(input, 200, false);
        // <, >, ', / are stripped
        assert!(result.is_some());
        let cleaned = result.unwrap();
        assert!(!cleaned.contains('<'));
        assert!(!cleaned.contains('>'));
        assert!(!cleaned.contains('\''));
        assert!(!cleaned.contains('/'));
    }

    #[test]
    fn sanitize_text_field_truncates_at_max_chars() {
        let input = "abcdefghijklmnop";
        let result = sanitize_text_field(input, 5, false);
        assert_eq!(result, Some("abcde".to_owned()));
    }

    #[test]
    fn sanitize_text_field_returns_none_for_empty_input() {
        let result = sanitize_text_field("", 100, false);
        assert_eq!(result, None);
    }

    #[test]
    fn sanitize_text_field_returns_none_for_zero_max_chars() {
        let result = sanitize_text_field("hello", 0, false);
        assert_eq!(result, None);
    }

    #[test]
    fn sanitize_text_field_collapses_whitespace() {
        let input = "hello    world";
        let result = sanitize_text_field(input, 100, false);
        assert_eq!(result, Some("hello world".to_owned()));
    }

    #[test]
    fn sanitize_text_field_trims_leading_trailing_spaces() {
        let input = "   hello   ";
        let result = sanitize_text_field(input, 100, false);
        assert_eq!(result, Some("hello".to_owned()));
    }

    #[test]
    fn sanitize_text_field_returns_none_for_all_whitespace() {
        let result = sanitize_text_field("     ", 100, false);
        assert_eq!(result, None);
    }

    #[test]
    fn sanitize_text_field_username_mode_allows_alphanumeric_dash_underscore_dot() {
        let input = "Player_123-test.name";
        let result = sanitize_text_field(input, 100, true);
        assert_eq!(result, Some("Player_123-test.name".to_owned()));
    }

    #[test]
    fn sanitize_text_field_username_mode_strips_special_chars() {
        let input = "Player!@#$%^&*()name";
        let result = sanitize_text_field(input, 100, true);
        assert_eq!(result, Some("Playername".to_owned()));
    }

    // ── sanitize_chat_field / sanitize_username_field wrappers ────────

    #[test]
    fn sanitize_chat_field_delegates_non_username_mode() {
        // Chat mode should allow special characters that username mode strips
        let input = "hello! how are you?";
        let result = sanitize_chat_field(input, 100);
        assert!(result.is_some());
        let cleaned = result.unwrap();
        assert!(cleaned.contains('!'));
        assert!(cleaned.contains('?'));
    }

    #[test]
    fn sanitize_username_field_uses_username_mode() {
        let input = "Player!Name";
        let result = sanitize_username_field(input, 100);
        assert_eq!(result, Some("PlayerName".to_owned()));
    }

    // ── parse_csv tests ─────────────────────────────────────────────

    #[test]
    fn parse_csv_splits_comma_separated_values() {
        let result = parse_csv("a,b,c");
        assert_eq!(result, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_csv_trims_whitespace() {
        let result = parse_csv(" a , b , c ");
        assert_eq!(result, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_csv_filters_empty_entries() {
        let result = parse_csv("a,,b,,c");
        assert_eq!(result, vec!["a", "b", "c"]);
    }

    #[test]
    fn parse_csv_handles_empty_string() {
        let result = parse_csv("");
        assert!(result.is_empty());
    }

    #[test]
    fn parse_csv_handles_single_value() {
        let result = parse_csv("stun:example.com:3478");
        assert_eq!(result, vec!["stun:example.com:3478"]);
    }

    // ── is_bidi_or_directional_control tests ─────────────────────────

    #[test]
    fn bidi_control_chars_detected() {
        assert!(is_bidi_or_directional_control('\u{200E}')); // LRM
        assert!(is_bidi_or_directional_control('\u{200F}')); // RLM
        assert!(is_bidi_or_directional_control('\u{202A}')); // LRE
        assert!(is_bidi_or_directional_control('\u{202B}')); // RLE
        assert!(is_bidi_or_directional_control('\u{2066}')); // LRI
        assert!(is_bidi_or_directional_control('\u{2069}')); // PDI
    }

    #[test]
    fn normal_chars_not_flagged_as_bidi() {
        assert!(!is_bidi_or_directional_control('a'));
        assert!(!is_bidi_or_directional_control(' '));
        assert!(!is_bidi_or_directional_control('0'));
        assert!(!is_bidi_or_directional_control('\n'));
    }

    // ── parse_ice_servers_env tests ──────────────────────────────────

    #[test]
    fn parse_ice_servers_env_single_stun() {
        let result = parse_ice_servers_env("stun:stun.example.com:3478");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].urls, vec!["stun:stun.example.com:3478"]);
        assert!(result[0].username.is_empty());
        assert!(result[0].credential.is_empty());
    }

    #[test]
    fn parse_ice_servers_env_with_credentials() {
        let result = parse_ice_servers_env("turn:turn.example.com:3478|myuser|mypass");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].urls, vec!["turn:turn.example.com:3478"]);
        assert_eq!(result[0].username, "myuser");
        assert_eq!(result[0].credential, "mypass");
    }

    #[test]
    fn parse_ice_servers_env_multiple_servers() {
        let result = parse_ice_servers_env(
            "stun:stun1.example.com:3478;turn:turn1.example.com:3478|user1|pass1",
        );
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].urls, vec!["stun:stun1.example.com:3478"]);
        assert_eq!(result[1].urls, vec!["turn:turn1.example.com:3478"]);
        assert_eq!(result[1].username, "user1");
    }

    #[test]
    fn parse_ice_servers_env_empty_string() {
        let result = parse_ice_servers_env("");
        assert!(result.is_empty());
    }

    #[test]
    fn parse_ice_servers_env_multiple_urls_per_server() {
        let result =
            parse_ice_servers_env("stun:stun1.example.com:3478,stun:stun2.example.com:3478");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].urls.len(), 2);
    }

    // ── validate_signaling_payload tests ─────────────────────────────

    #[test]
    fn validate_payload_rejects_empty() {
        let payload = SignalingMessageJson {
            protocol_version: Some(signaling_protocol_version()),
            sdp: None,
            ice: None,
        };
        assert_eq!(
            validate_signaling_payload(&payload),
            Err("empty signaling payload")
        );
    }

    #[test]
    fn validate_payload_rejects_oversized_sdp() {
        let large_sdp = "x".repeat(MAX_SIGNALING_SDP_BYTES + 1);
        let mut sdp = RTCSessionDescription::default();
        sdp.sdp = large_sdp;
        let payload = SignalingMessageJson {
            protocol_version: Some(signaling_protocol_version()),
            sdp: Some(sdp),
            ice: None,
        };
        assert_eq!(
            validate_signaling_payload(&payload),
            Err("SDP payload too large")
        );
    }

    #[test]
    fn validate_payload_accepts_valid_sdp() {
        let mut sdp = RTCSessionDescription::default();
        sdp.sdp = "v=0\r\n".to_owned();
        let payload = SignalingMessageJson {
            protocol_version: Some(signaling_protocol_version()),
            sdp: Some(sdp),
            ice: None,
        };
        assert!(validate_signaling_payload(&payload).is_ok());
    }

    #[test]
    fn validate_payload_rejects_oversized_ice_candidate() {
        let large_candidate = "x".repeat(MAX_SIGNALING_ICE_CANDIDATE_BYTES + 1);
        let ice = RTCIceCandidateInitSerde {
            candidate: large_candidate,
            sdp_mid: None,
            sdp_m_line_index: None,
            username_fragment: None,
        };
        let payload = SignalingMessageJson {
            protocol_version: Some(signaling_protocol_version()),
            sdp: None,
            ice: Some(ice),
        };
        assert_eq!(
            validate_signaling_payload(&payload),
            Err("ICE candidate payload too large")
        );
    }

    #[test]
    fn validate_payload_rejects_oversized_sdp_mid() {
        let large_mid = "x".repeat(MAX_SIGNALING_ICE_SDP_MID_BYTES + 1);
        let ice = RTCIceCandidateInitSerde {
            candidate: "candidate:1".to_owned(),
            sdp_mid: Some(large_mid),
            sdp_m_line_index: None,
            username_fragment: None,
        };
        let payload = SignalingMessageJson {
            protocol_version: Some(signaling_protocol_version()),
            sdp: None,
            ice: Some(ice),
        };
        assert_eq!(
            validate_signaling_payload(&payload),
            Err("ICE sdpMid payload too large")
        );
    }

    #[test]
    fn validate_payload_rejects_oversized_username_fragment() {
        let large_frag = "x".repeat(MAX_SIGNALING_ICE_USERNAME_FRAGMENT_BYTES + 1);
        let ice = RTCIceCandidateInitSerde {
            candidate: "candidate:1".to_owned(),
            sdp_mid: None,
            sdp_m_line_index: None,
            username_fragment: Some(large_frag),
        };
        let payload = SignalingMessageJson {
            protocol_version: Some(signaling_protocol_version()),
            sdp: None,
            ice: Some(ice),
        };
        assert_eq!(
            validate_signaling_payload(&payload),
            Err("ICE usernameFragment payload too large")
        );
    }

    #[test]
    fn validate_payload_accepts_valid_ice() {
        let ice = RTCIceCandidateInitSerde {
            candidate: "candidate:1 1 udp 2130706431 192.168.1.1 1234 typ host".to_owned(),
            sdp_mid: Some("0".to_owned()),
            sdp_m_line_index: Some(0),
            username_fragment: Some("abc".to_owned()),
        };
        let payload = SignalingMessageJson {
            protocol_version: Some(signaling_protocol_version()),
            sdp: None,
            ice: Some(ice),
        };
        assert!(validate_signaling_payload(&payload).is_ok());
    }

    #[test]
    fn validate_payload_rejects_missing_protocol_version() {
        let payload = SignalingMessageJson {
            protocol_version: None,
            sdp: None,
            ice: Some(RTCIceCandidateInitSerde {
                candidate: "candidate:1".to_owned(),
                sdp_mid: Some("0".to_owned()),
                sdp_m_line_index: Some(0),
                username_fragment: None,
            }),
        };
        assert_eq!(
            validate_signaling_payload(&payload),
            Err("missing protocol_version")
        );
    }

    #[test]
    fn validate_payload_rejects_protocol_version_mismatch() {
        let payload = SignalingMessageJson {
            protocol_version: Some(signaling_protocol_version() + 1),
            sdp: None,
            ice: Some(RTCIceCandidateInitSerde {
                candidate: "candidate:1".to_owned(),
                sdp_mid: Some("0".to_owned()),
                sdp_m_line_index: Some(0),
                username_fragment: None,
            }),
        };
        assert_eq!(
            validate_signaling_payload(&payload),
            Err("protocol_version mismatch")
        );
    }

    // ── JoinRateLimiter tests ────────────────────────────────────────

    #[test]
    fn join_rate_limiter_starts_full() {
        let mut limiter = JoinRateLimiter::new(10, 10);
        for _ in 0..10 {
            assert!(limiter.try_acquire());
        }
        assert!(!limiter.try_acquire());
    }

    #[test]
    fn join_rate_limiter_clamps_minimum_values() {
        let mut limiter = JoinRateLimiter::new(0, 0);
        assert!(limiter.try_acquire());
        assert!(!limiter.try_acquire());
    }

    #[test]
    fn join_rate_limiter_consumes_tokens() {
        let mut limiter = JoinRateLimiter::new(10, 5);
        for _ in 0..5 {
            assert!(limiter.try_acquire());
        }
        // Should be exhausted now
        assert!(!limiter.try_acquire());
    }

    #[test]
    fn join_rate_limiter_does_not_exceed_capacity() {
        let mut limiter = JoinRateLimiter::new(100, 3);
        // Immediately all 3 tokens should be available
        assert!(limiter.try_acquire());
        assert!(limiter.try_acquire());
        assert!(limiter.try_acquire());
        assert!(!limiter.try_acquire());
    }

    // ── Per-IP concurrent connection cap tests ───────────────────────

    // The cap's OnceLock reads MGS_MAX_WS_CONNECTIONS_PER_IP once per process;
    // reading the same env var here keeps the test correct under overrides.
    fn effective_per_ip_cap() -> u32 {
        super::rate_limiting::env_u32(
            "MGS_MAX_WS_CONNECTIONS_PER_IP",
            super::rate_limiting::DEFAULT_MAX_WS_CONNECTIONS_PER_IP,
        )
    }

    #[test]
    fn per_ip_connection_cap_enforces_boundary() {
        let max = effective_per_ip_cap();
        if max == 0 {
            return; // Cap disabled in this environment.
        }
        let ip: std::net::IpAddr = "203.0.113.101".parse().unwrap();
        let mut guards = Vec::new();
        for _ in 0..max {
            guards.push(
                super::rate_limiting::try_acquire_ip_connection_slot(&ip)
                    .expect("acquisition within the cap must succeed"),
            );
        }
        assert!(
            super::rate_limiting::try_acquire_ip_connection_slot(&ip).is_none(),
            "connection beyond the per-IP cap must be rejected"
        );
        drop(guards);
    }

    #[test]
    fn per_ip_connection_cap_is_scoped_per_ip() {
        let max = effective_per_ip_cap();
        if max == 0 {
            return;
        }
        let ip_a: std::net::IpAddr = "203.0.113.102".parse().unwrap();
        let ip_b: std::net::IpAddr = "203.0.113.103".parse().unwrap();
        let mut guards = Vec::new();
        for _ in 0..max {
            guards.push(super::rate_limiting::try_acquire_ip_connection_slot(&ip_a).unwrap());
        }
        assert!(super::rate_limiting::try_acquire_ip_connection_slot(&ip_a).is_none());
        assert!(
            super::rate_limiting::try_acquire_ip_connection_slot(&ip_b).is_some(),
            "a different IP must have its own independent slot pool"
        );
        drop(guards);
    }

    #[test]
    fn per_ip_connection_cap_releases_slot_on_guard_drop() {
        let max = effective_per_ip_cap();
        if max == 0 {
            return;
        }
        let ip: std::net::IpAddr = "203.0.113.104".parse().unwrap();
        let mut guards = Vec::new();
        for _ in 0..max {
            guards.push(super::rate_limiting::try_acquire_ip_connection_slot(&ip).unwrap());
        }
        assert!(super::rate_limiting::try_acquire_ip_connection_slot(&ip).is_none());
        drop(guards.pop());
        assert!(
            super::rate_limiting::try_acquire_ip_connection_slot(&ip).is_some(),
            "dropping a guard must release its slot"
        );
        drop(guards);
    }

    // ── InputRateLimiter tests ───────────────────────────────────────

    #[test]
    fn input_rate_limiter_starts_full() {
        let mut limiter = InputRateLimiter::new(240, 360);
        for _ in 0..360 {
            assert!(limiter.try_acquire());
        }
        assert!(!limiter.try_acquire());
    }

    #[test]
    fn input_rate_limiter_clamps_minimum_values() {
        let mut limiter = InputRateLimiter::new(0, 0);
        assert!(limiter.try_acquire());
        assert!(!limiter.try_acquire());
    }

    #[test]
    fn input_rate_limiter_consumes_tokens() {
        let mut limiter = InputRateLimiter::new(240, 5);
        for _ in 0..5 {
            assert!(limiter.try_acquire());
        }
        assert!(!limiter.try_acquire());
    }

    #[test]
    fn input_rate_limiter_should_log_throttle_initially_true() {
        // The limiter initializes last_drop_log_at to a time in the past
        // (offset by INPUT_RATE_LIMIT_THROTTLE_LOG_INTERVAL_SECS), so the
        // first call to should_log_throttle() should return true.
        let mut limiter = InputRateLimiter::new(10, 10);
        assert!(limiter.should_log_throttle());
    }

    #[test]
    fn input_rate_limiter_should_log_throttle_rate_limits() {
        let mut limiter = InputRateLimiter::new(10, 10);
        // First call returns true
        assert!(limiter.should_log_throttle());
        // Immediately subsequent call should return false
        assert!(!limiter.should_log_throttle());
    }

    // ── ClientState default tests ────────────────────────────────────

    #[test]
    fn client_state_default_values() {
        let state = ClientState::default();
        assert!(!state.known_walls_sent);
        assert!(state.pending_initial_state_bytes.is_none());
        assert!(state.pending_initial_state_chunks.is_empty());
        assert!(state.last_known_player_states.is_empty());
        assert!(state.last_known_projectile_ids.is_empty());
        assert_eq!(state.last_kill_feed_count_sent, 0);
        assert_eq!(state.last_chat_message_seq_sent, 0);
        assert_eq!(state.last_broadcast_frame, 0);
        assert!(state.match_info_pending);
        assert!(!state.is_mobile);
        assert_eq!(state.mobile_delta_skip_modulus, 1);
    }

    // ── webrtc_state_label tests ────────────────────────────────────

    #[test]
    fn webrtc_state_label_maps_known_states() {
        assert_eq!(webrtc_state_label(RTCPeerConnectionState::New), "new");
        assert_eq!(
            webrtc_state_label(RTCPeerConnectionState::Connecting),
            "connecting"
        );
        assert_eq!(
            webrtc_state_label(RTCPeerConnectionState::Connected),
            "connected"
        );
        assert_eq!(
            webrtc_state_label(RTCPeerConnectionState::Disconnected),
            "disconnected"
        );
        assert_eq!(webrtc_state_label(RTCPeerConnectionState::Failed), "failed");
        assert_eq!(webrtc_state_label(RTCPeerConnectionState::Closed), "closed");
    }

    // ── env_bool tests ──────────────────────────────────────────────

    #[test]
    fn env_bool_returns_false_for_unset_variable() {
        // Using a variable name that is very unlikely to be set
        assert!(!env_bool("MGS_TEST_UNLIKELY_VAR_XYZ_123_NEVER_SET"));
    }

    // ── env_u32 tests ───────────────────────────────────────────────

    #[test]
    fn env_u32_returns_default_for_unset_variable() {
        let result = env_u32("MGS_TEST_UNLIKELY_ENV_U32_NEVER_SET", 42);
        assert_eq!(result, 42);
    }

    #[test]
    fn ws_keepalive_interval_zero_disables_keepalive() {
        assert_eq!(normalize_ws_keepalive_interval_secs(0), None);
    }

    #[test]
    fn ws_keepalive_interval_clamps_to_bounds() {
        assert_eq!(
            normalize_ws_keepalive_interval_secs(1),
            Some(5) // MIN_WS_KEEPALIVE_INTERVAL_SECS
        );
        assert_eq!(
            normalize_ws_keepalive_interval_secs(600),
            Some(300) // MAX_WS_KEEPALIVE_INTERVAL_SECS
        );
    }

    #[test]
    fn ws_keepalive_interval_keeps_in_range_values() {
        assert_eq!(normalize_ws_keepalive_interval_secs(30), Some(30));
    }

    // ── SignalingMessageJson serde tests ─────────────────────────────

    #[test]
    fn signaling_message_json_deserializes_ice_candidate() {
        let json_str = r#"{"protocol_version":1,"ice":{"candidate":"candidate:1 1 udp 2130706431 192.168.1.1 1234 typ host","sdpMid":"0","sdpMLineIndex":0}}"#;
        let parsed: SignalingMessageJson = serde_json::from_str(json_str).unwrap();
        assert_eq!(parsed.protocol_version, Some(signaling_protocol_version()));
        assert!(parsed.sdp.is_none());
        let ice = parsed.ice.unwrap();
        assert!(ice.candidate.contains("candidate:1"));
        assert_eq!(ice.sdp_mid, Some("0".to_owned()));
        assert_eq!(ice.sdp_m_line_index, Some(0));
    }

    #[test]
    fn signaling_message_json_serializes_ice_with_skip_serializing_if() {
        let msg = SignalingMessageJson {
            protocol_version: Some(signaling_protocol_version()),
            sdp: None,
            ice: Some(RTCIceCandidateInitSerde {
                candidate: "test".to_owned(),
                sdp_mid: None,
                sdp_m_line_index: None,
                username_fragment: None,
            }),
        };
        let json = serde_json::to_string(&msg).unwrap();
        // The struct uses #[serde(skip_serializing_if = "Option::is_none")] on sdp/ice
        // so the `ice` field should be present and contain the candidate
        assert!(json.contains("protocol_version"));
        assert!(json.contains("ice"));
        assert!(json.contains("candidate"));
        assert!(json.contains("test"));
    }

    #[test]
    fn signaling_message_json_round_trip() {
        let msg = SignalingMessageJson {
            protocol_version: Some(signaling_protocol_version()),
            sdp: None,
            ice: Some(RTCIceCandidateInitSerde {
                candidate: "candidate:1 1 udp 2130706431 192.168.1.1 1234 typ host".to_owned(),
                sdp_mid: Some("audio".to_owned()),
                sdp_m_line_index: Some(0),
                username_fragment: Some("frag123".to_owned()),
            }),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: SignalingMessageJson = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.protocol_version, Some(signaling_protocol_version()));
        let ice = parsed.ice.unwrap();
        assert_eq!(ice.sdp_mid, Some("audio".to_owned()));
        assert_eq!(ice.username_fragment, Some("frag123".to_owned()));
    }

    // ── TURN HMAC credential generation tests ───────────────────────

    #[test]
    fn generate_turn_hmac_credentials_returns_valid_format() {
        let (username, credential) = generate_turn_hmac_credentials("mysecret", "player123");
        // Username must be "timestamp:suffix"
        let parts: Vec<&str> = username.splitn(2, ':').collect();
        assert_eq!(parts.len(), 2, "username must be timestamp:suffix");
        let timestamp: u64 = parts[0].parse().expect("first part must be a timestamp");
        assert_eq!(parts[1], "player123");

        // Timestamp should be in the future (now + TTL)
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(timestamp > now, "expiry must be in the future");
        assert!(
            timestamp <= now + TURN_CREDENTIAL_TTL_SECS + 1,
            "expiry must not exceed TTL"
        );

        // Credential must be valid base64
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&credential)
            .expect("credential must be valid base64");
        // HMAC-SHA256 output is 32 bytes
        assert_eq!(decoded.len(), 32, "HMAC-SHA256 output must be 32 bytes");
    }

    #[test]
    fn generate_turn_hmac_credentials_deterministic_for_same_inputs() {
        // Two calls within the same second should produce the same output
        // (assuming system clock doesn't tick between calls).
        let (u1, c1) = generate_turn_hmac_credentials("secret", "session1");
        let (u2, c2) = generate_turn_hmac_credentials("secret", "session1");
        // They will match if the system clock second is the same.
        // We verify at least that the suffix and credential algorithm are consistent.
        assert!(u1.ends_with(":session1"));
        assert!(u2.ends_with(":session1"));
        // The credentials should match if timestamps match (same second)
        if u1 == u2 {
            assert_eq!(c1, c2, "same username must produce same credential");
        }
    }

    #[test]
    fn generate_turn_hmac_credentials_different_secrets_differ() {
        let (u1, c1) = generate_turn_hmac_credentials("secret_a", "player");
        let (_u2, c2) = generate_turn_hmac_credentials("secret_b", "player");
        // Even if timestamps happen to match, different secrets produce different credentials.
        // There is a negligible chance of collision, but practically impossible for HMAC-SHA256.
        if u1.split(':').next() == _u2.split(':').next() {
            assert_ne!(
                c1, c2,
                "different secrets must produce different credentials"
            );
        }
    }

    #[test]
    fn generate_turn_hmac_credentials_different_suffixes_differ() {
        let (u1, c1) = generate_turn_hmac_credentials("secret", "player_a");
        let (u2, c2) = generate_turn_hmac_credentials("secret", "player_b");
        assert_ne!(
            u1, u2,
            "different suffixes must produce different usernames"
        );
        // Credentials will differ because the username (HMAC input) differs.
        assert_ne!(
            c1, c2,
            "different usernames must produce different credentials"
        );
    }

    #[test]
    fn generate_turn_hmac_credential_verifiable() {
        // Verify that the credential is a correct HMAC-SHA256 of the username.
        let secret = "test_shared_secret";
        let (username, credential) = generate_turn_hmac_credentials(secret, "verify_me");

        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&credential)
            .unwrap();

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(username.as_bytes());
        // verify() consumes the mac and checks against the expected bytes
        mac.verify_slice(&decoded)
            .expect("HMAC verification must succeed");
    }

    #[test]
    fn turn_credential_type_from_env_defaults_to_password() {
        temp_env::with_var("MGS_TURN_CREDENTIAL_TYPE", None::<&str>, || {
            // When MGS_TURN_CREDENTIAL_TYPE is not set, it defaults to Password.
            assert_eq!(TurnCredentialType::from_env(), TurnCredentialType::Password);
        });
    }

    #[test]
    fn turn_credential_type_from_env_supports_sha256_and_legacy_sha1() {
        temp_env::with_var("MGS_TURN_CREDENTIAL_TYPE", Some("hmac"), || {
            assert_eq!(
                TurnCredentialType::from_env(),
                TurnCredentialType::HmacSha256
            );
        });
        temp_env::with_var("MGS_TURN_CREDENTIAL_TYPE", Some("hmac-sha1"), || {
            assert_eq!(
                TurnCredentialType::from_env(),
                TurnCredentialType::HmacSha1Legacy
            );
        });
    }

    #[test]
    fn client_ice_server_serialization_omits_empty_credentials() {
        let server = ClientIceServer {
            urls: vec!["stun:stun.example.com:3478".to_owned()],
            username: None,
            credential: None,
        };
        let json = serde_json::to_string(&server).unwrap();
        assert!(json.contains("urls"));
        assert!(!json.contains("username"), "null username must be omitted");
        assert!(
            !json.contains("credential"),
            "null credential must be omitted"
        );
    }

    #[test]
    fn client_ice_server_serialization_includes_credentials() {
        let server = ClientIceServer {
            urls: vec!["turn:turn.example.com:3478".to_owned()],
            username: Some("user".to_owned()),
            credential: Some("pass".to_owned()),
        };
        let json = serde_json::to_string(&server).unwrap();
        assert!(json.contains(r#""username":"user""#));
        assert!(json.contains(r#""credential":"pass""#));
    }

    #[test]
    fn build_welcome_message_bytes_serializes_expected_welcome_payload() {
        let bytes = build_welcome_message_bytes("player-123", 60);
        let game_msg = fb::root_as_game_message(bytes.as_ref())
            .expect("welcome bytes should decode into a GameMessage");
        assert_eq!(game_msg.msg_type(), fb::MessageType::Welcome);
        assert_eq!(game_msg.protocol_version(), GAME_PROTOCOL_VERSION);
        let welcome = game_msg
            .actual_message_as_welcome_message()
            .expect("welcome payload should exist");
        assert_eq!(welcome.player_id(), Some("player-123"));
        assert_eq!(welcome.message(), Some("Welcome to MassiveGameServer!"));
        assert_eq!(welcome.server_tick_rate(), 60);
        assert_eq!(
            welcome.server_protocol_version(),
            signaling_protocol_version()
        );
    }

    #[test]
    fn build_ice_servers_marks_turn_credentials_as_password() {
        let config = CachedIceConfig {
            disable_stun: true,
            stun_urls: Vec::new(),
            turn_urls: vec!["turn:127.0.0.1:3478?transport=udp".to_owned()],
            turn_credential_type: TurnCredentialType::Password,
            turn_username: Some("turn-user".to_owned()),
            turn_credential: Some("turn-password".to_owned()),
            extra_ice_servers: Vec::new(),
        };

        let ice_servers = build_ice_servers_from_config(&config);
        assert_eq!(ice_servers.len(), 1);
        let turn_server = &ice_servers[0];
        assert_eq!(turn_server.credential_type, RTCIceCredentialType::Password);
        assert_eq!(turn_server.username, "turn-user");
        assert_eq!(turn_server.credential, "turn-password");
    }

    #[test]
    fn build_client_ice_config_includes_turn_credentials() {
        let config = CachedIceConfig {
            disable_stun: true,
            stun_urls: Vec::new(),
            turn_urls: vec!["turn:127.0.0.1:3478?transport=udp".to_owned()],
            turn_credential_type: TurnCredentialType::Password,
            turn_username: Some("turn-user".to_owned()),
            turn_credential: Some("turn-password".to_owned()),
            extra_ice_servers: Vec::new(),
        };

        let client_ice = build_client_ice_config_from_config(&config, "session-1");
        assert_eq!(client_ice.len(), 1);
        let turn_entry = &client_ice[0];
        assert_eq!(
            turn_entry.username.as_deref(),
            Some("turn-user"),
            "client config should include username"
        );
        assert_eq!(
            turn_entry.credential.as_deref(),
            Some("turn-password"),
            "client config should include credential"
        );
    }
}
