// massive_game_server/server/src/network/webrtc/signaling.rs

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SignalingFrame {
    Offer {
        sdp: String,
    },
    Answer {
        sdp: String,
    },
    IceCandidate {
        candidate: String,
        sdp_mid: Option<String>,
        sdp_mline_index: Option<u16>,
    },
}

pub fn validate_signaling_frame(frame: &SignalingFrame, max_sdp_bytes: usize) -> bool {
    match frame {
        SignalingFrame::Offer { sdp } | SignalingFrame::Answer { sdp } => {
            sdp.len() <= max_sdp_bytes
        }
        SignalingFrame::IceCandidate { candidate, .. } => candidate.len() <= 4 * 1024,
    }
}
