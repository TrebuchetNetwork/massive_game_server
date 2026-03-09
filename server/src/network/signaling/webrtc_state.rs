use crate::operational::monitoring::metrics;
use dashmap::DashMap;
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

pub(super) fn shared_webrtc_api() -> Result<Arc<API>, String> {
    match SHARED_WEBRTC_API.get_or_init(|| {
        let mut media_engine = MediaEngine::default();
        media_engine
            .register_default_codecs()
            .map_err(|e| format!("register_default_codecs failed: {e}"))?;
        let runtime = super::signaling_env_config();
        let mut setting_engine = SettingEngine::default();

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
