use crate::operational::monitoring::metrics;
use dashmap::DashMap;
use std::net::IpAddr;
use std::sync::{atomic::AtomicBool, Arc, OnceLock};
use tracing::{error, info};
use webrtc::{
    api::{media_engine::MediaEngine, setting_engine::SettingEngine, APIBuilder, API},
    ice::udp_network::{EphemeralUDP, UDPNetwork},
    ice_transport::ice_candidate_type::RTCIceCandidateType,
    peer_connection::peer_connection_state::RTCPeerConnectionState,
};

static SHARED_WEBRTC_API: OnceLock<Result<Arc<API>, String>> = OnceLock::new();
static WEBRTC_PEER_STATES: OnceLock<DashMap<String, &'static str>> = OnceLock::new();

pub(super) const WEBRTC_STATE_LABELS: [&str; 7] = [
    "new",
    "connecting",
    "connected",
    "disconnected",
    "failed",
    "closed",
    "other",
];

/// Drop guard that ensures a WebRTC peer connection is closed even when the
/// signaling task is cancelled or panics.  Because `RTCPeerConnection::close()`
/// is async, the guard spawns a detached task to perform the close.
pub(super) struct PeerConnectionDropGuard {
    pub(super) peer_connection: Option<Arc<webrtc::peer_connection::RTCPeerConnection>>,
    pub(super) peer_id: String,
}

impl PeerConnectionDropGuard {
    pub(super) fn new(
        pc: Arc<webrtc::peer_connection::RTCPeerConnection>,
        peer_id: String,
    ) -> Self {
        Self {
            peer_connection: Some(pc),
            peer_id,
        }
    }

    /// Consume the guard without closing the connection (call this when you
    /// intend to close the connection yourself, e.g. at the normal exit path).
    pub(super) fn defuse(&mut self) {
        self.peer_connection = None;
    }
}

impl Drop for PeerConnectionDropGuard {
    fn drop(&mut self) {
        if let Some(pc) = self.peer_connection.take() {
            let pid = self.peer_id.clone();
            tokio::spawn(async move {
                if let Err(e) = pc.close().await {
                    error!(
                        "[{}]: Error closing PeerConnection in drop guard: {}",
                        pid, e
                    );
                } else {
                    info!("[{}]: PeerConnection closed via drop guard.", pid);
                }
            });
        }
    }
}

/// Decide whether a local address is worth gathering an ICE candidate on.
///
/// Returning `true` keeps the address. webrtc-ice binds one UDP socket per
/// address it is handed, so every address rejected here is one fewer socket
/// allocated — and one fewer socket that can be stranded by the
/// gather-after-close race described at the `set_ip_filter` call site.
///
/// Private IPv4 ranges are deliberately kept: behind NAT the LAN address is the
/// only candidate that can work, and `MGS_WEBRTC_NAT_1TO1_IPS` rewrites it to
/// the public IP. What is rejected is anything a remote peer provably cannot
/// reach — loopback, link-local (IPv4 169.254/16, IPv6 fe80::/10), IPv6
/// unique-local (fc00::/7, which on this host is eight rotating privacy
/// addresses), multicast, reserved, and unspecified.
fn is_gatherable_ice_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_loopback()
                || v4.is_unspecified()
                || (a == 169 && b == 254) // link-local 169.254.0.0/16
                || a == 0 // "this" network 0.0.0.0/8
                || a >= 224) // multicast 224/4, reserved 240/4, broadcast
        }
        IpAddr::V6(v6) => {
            let s0 = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s0 & 0xffc0) == 0xfe80 // fe80::/10 link-local
                || (s0 & 0xfe00) == 0xfc00) // fc00::/7 unique-local
        }
    }
}

pub(super) fn shared_webrtc_api() -> Result<Arc<API>, String> {
    match SHARED_WEBRTC_API.get_or_init(|| {
        let mut media_engine = MediaEngine::default();
        media_engine
            .register_default_codecs()
            .map_err(|e| format!("register_default_codecs failed: {e}"))?;
        let runtime = super::signaling_env_config();
        let mut setting_engine = SettingEngine::default();

        // Only gather candidates on addresses a remote peer could actually use.
        //
        // This host carries 13 non-loopback addresses: one routable LAN IPv4,
        // eight rotating IPv6 ULA privacy addresses (fc00::/7 — not
        // internet-routable), two fe80::/10 link-locals, and a 169.254.0.0/16
        // link-local USB interface. `local_interfaces()` returns every one of
        // them and the caller binds a UDP socket per address, so each peer
        // connection allocates ~10 sockets and advertises ~10 candidates that
        // can never succeed.
        //
        // That matters more than wasted candidates: the gathering task is
        // spawned detached with no cancellation check, and
        // `AgentInternal::close()` early-returns on `done_tx.is_none()` *before*
        // reaching `delete_all_candidates()` — the only code that closes those
        // sockets. Anything bound after close() is stranded for the life of the
        // process. Filtering the address list cuts that roughly tenfold.
        //
        // This deliberately does NOT switch to `UDPNetwork::Muxed`: muxing skips
        // ServerReflexive gathering entirely ("No server reflexive for muxxed
        // connections" in agent_gather.rs), which would break the
        // MGS_WEBRTC_NAT_1TO1_CANDIDATE_TYPE=srflx public-IP advertisement and
        // push every remote peer onto TURN relay.
        setting_engine.set_ip_filter(Box::new(is_gatherable_ice_address));
        info!(
            "WebRTC ICE candidate gathering restricted to routable, non-link-local addresses."
        );

        if let (Some(port_min), Some(port_max)) =
            (runtime.webrtc_udp_port_min, runtime.webrtc_udp_port_max)
        {
            let udp = EphemeralUDP::new(port_min, port_max).map_err(|e| {
                format!(
                    "invalid WebRTC UDP port range {}-{}: {}",
                    port_min, port_max, e
                )
            })?;
            setting_engine.set_udp_network(UDPNetwork::Ephemeral(udp));
            info!(
                "WebRTC UDP candidate port range constrained to {}-{}.",
                port_min, port_max
            );
        }

        if !runtime.webrtc_nat_1to1_ips.is_empty() {
            let candidate_type = match runtime.webrtc_nat_1to1_candidate_type.as_deref() {
                Some("srflx") => RTCIceCandidateType::Srflx,
                _ => RTCIceCandidateType::Host,
            };
            setting_engine.set_nat_1to1_ips(runtime.webrtc_nat_1to1_ips.clone(), candidate_type);
            info!(
                "WebRTC NAT 1:1 candidate rewriting enabled for {} IP(s) as {:?} candidates.",
                runtime.webrtc_nat_1to1_ips.len(),
                candidate_type
            );
        }

        Ok(Arc::new(
            APIBuilder::new()
                .with_media_engine(media_engine)
                .with_setting_engine(setting_engine)
                .build(),
        ))
    }) {
        Ok(api) => Ok(Arc::clone(api)),
        Err(err) => Err(err.clone()),
    }
}

fn shared_webrtc_peer_states() -> &'static DashMap<String, &'static str> {
    WEBRTC_PEER_STATES.get_or_init(DashMap::new)
}

pub(super) fn webrtc_state_label(state: RTCPeerConnectionState) -> &'static str {
    match state {
        RTCPeerConnectionState::New => "new",
        RTCPeerConnectionState::Connecting => "connecting",
        RTCPeerConnectionState::Connected => "connected",
        RTCPeerConnectionState::Disconnected => "disconnected",
        RTCPeerConnectionState::Failed => "failed",
        RTCPeerConnectionState::Closed => "closed",
        _ => "other",
    }
}

fn publish_webrtc_peer_state_gauges(states: &DashMap<String, &'static str>) {
    let mut counts = [0usize; WEBRTC_STATE_LABELS.len()];
    states.iter().for_each(|entry| {
        if let Some(index) = WEBRTC_STATE_LABELS
            .iter()
            .position(|label| label == entry.value())
        {
            counts[index] = counts[index].saturating_add(1);
        }
    });
    for (index, label) in WEBRTC_STATE_LABELS.iter().enumerate() {
        metrics::set_webrtc_peers_in_state(label, counts[index]);
    }
}

pub(super) fn record_webrtc_peer_state(peer_id: &str, state: RTCPeerConnectionState) {
    let label = webrtc_state_label(state);
    metrics::record_webrtc_peer_state_transition(label);
    let states = shared_webrtc_peer_states();
    states.insert(peer_id.to_owned(), label);
    publish_webrtc_peer_state_gauges(states);
}

pub(super) fn remove_webrtc_peer_state(peer_id: &str) {
    let states = shared_webrtc_peer_states();
    states.remove(peer_id);
    publish_webrtc_peer_state_gauges(states);
}

pub fn current_webrtc_peer_state_label(peer_id: &str) -> Option<&'static str> {
    shared_webrtc_peer_states()
        .get(peer_id)
        .map(|entry| *entry.value())
}

pub(super) fn begin_cleanup_once(cleanup_once: &AtomicBool) -> bool {
    cleanup_once
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        )
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::is_gatherable_ice_address;
    use std::net::IpAddr;

    fn keeps(s: &str) -> bool {
        is_gatherable_ice_address(s.parse::<IpAddr>().expect("valid test address"))
    }

    #[test]
    fn keeps_the_lan_address_and_public_ipv4() {
        // The only address on this host that a remote peer can actually use,
        // rewritten to the public IP by MGS_WEBRTC_NAT_1TO1_IPS.
        assert!(keeps("192.168.1.242"));
        assert!(keeps("8.8.8.8"));
    }

    #[test]
    fn keeps_private_ranges_because_nat_1to1_needs_them() {
        assert!(keeps("10.0.0.5"));
        assert!(keeps("172.16.0.1"));
        assert!(keeps("192.168.0.1"));
    }

    #[test]
    fn keeps_globally_routable_ipv6() {
        assert!(keeps("2606:4700::6812:373"));
    }

    #[test]
    fn drops_ipv4_a_remote_peer_cannot_reach() {
        assert!(!keeps("127.0.0.1")); // loopback
        assert!(!keeps("0.0.0.0")); // unspecified
        assert!(!keeps("169.254.3.1")); // link-local: the USB/BMC iface on this host
        assert!(!keeps("224.0.0.1")); // multicast
        assert!(!keeps("239.255.255.250")); // multicast (SSDP)
        assert!(!keeps("240.0.0.1")); // reserved
        assert!(!keeps("255.255.255.255")); // broadcast
    }

    #[test]
    fn drops_ipv6_a_remote_peer_cannot_reach() {
        assert!(!keeps("::1")); // loopback
        assert!(!keeps("::")); // unspecified
        assert!(!keeps("ff02::1")); // multicast
        // Both link-locals actually present on this host.
        assert!(!keeps("fe80::7f1f:c4b1:273b:a633"));
        assert!(!keeps("fe80::85bc:a6db:c636:1844"));
        // ULA privacy addresses. This exact one was found still assigned to the
        // host a month after the orphaned test instance leaked a socket on it.
        assert!(!keeps("fdba:a8f9:2abc:a5ac:84e6:fa15:a3f2:29a3"));
        assert!(!keeps("fc00::1"));
        assert!(!keeps("fdff:ffff:ffff:ffff::1"));
    }

    /// Regression guard on the real-world effect: this host has 13 non-loopback
    /// addresses and exactly one of them should survive the filter. Before the
    /// filter, webrtc-ice bound a UDP socket for each, so every peer connection
    /// allocated ~10 sockets that the gather-after-close race could strand.
    #[test]
    fn exactly_one_of_this_hosts_thirteen_addresses_survives() {
        let host_addresses = [
            "192.168.1.242",
            "fdba:a8f9:2abc:a5ac:8c9f:fff6:26b8:4080",
            "fdba:a8f9:2abc:a5ac:47d9:a398:63c5:89fe",
            "fdba:a8f9:2abc:a5ac:553a:6e06:8ac5:403c",
            "fdba:a8f9:2abc:a5ac:7d7b:aa9b:519c:f54f",
            "fdba:a8f9:2abc:a5ac:84f8:dcf5:19d4:9825",
            "fdba:a8f9:2abc:a5ac:cc85:672:6807:9c17",
            "fdba:a8f9:2abc:a5ac:97a2:7cfe:cbfe:4ade",
            "fdba:a8f9:2abc:a5ac:84e6:fa15:a3f2:29a3",
            "fe80::7f1f:c4b1:273b:a633",
            "169.254.3.1",
            "fe80::85bc:a6db:c636:1844",
        ];
        let kept: Vec<&str> = host_addresses
            .iter()
            .copied()
            .filter(|a| keeps(a))
            .collect();
        assert_eq!(kept, vec!["192.168.1.242"]);
    }
}
