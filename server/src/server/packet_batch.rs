use bytes::Bytes;
use std::sync::Arc;
use std::time::Duration;
use tracing::{trace, warn};

const PACKET_BATCH_MAGIC: [u8; 4] = *b"MGSB";
const PACKET_BATCH_VERSION: u8 = 1;
const PACKET_BATCH_HEADER_BYTES: usize = 7; // magic (4) + version (1) + packet count (u16)
const PACKET_BATCH_ENTRY_HEADER_BYTES: usize = 4; // per-packet payload length (u32)
const MAX_COALESCED_PACKET_BYTES: usize = 256 * 1024;

fn build_coalesced_packet_batch(packets: &[Bytes]) -> Option<Bytes> {
    if packets.is_empty() || packets.len() > u16::MAX as usize {
        return None;
    }

    let payload_bytes = packets.iter().map(|packet| packet.len()).sum::<usize>();
    let header_bytes = PACKET_BATCH_HEADER_BYTES
        + packets
            .len()
            .saturating_mul(PACKET_BATCH_ENTRY_HEADER_BYTES);
    let total_bytes = header_bytes.saturating_add(payload_bytes);
    if total_bytes > MAX_COALESCED_PACKET_BYTES {
        return None;
    }

    let mut out = Vec::with_capacity(total_bytes);
    out.extend_from_slice(&PACKET_BATCH_MAGIC);
    out.push(PACKET_BATCH_VERSION);
    out.extend_from_slice(&(packets.len() as u16).to_le_bytes());

    for packet in packets {
        let packet_len = packet.len();
        if packet_len > u32::MAX as usize {
            return None;
        }
        out.extend_from_slice(&(packet_len as u32).to_le_bytes());
        out.extend_from_slice(packet.as_ref());
    }

    Some(Bytes::from(out))
}

#[inline]
fn is_not_open_channel_send_error(err_text: &str) -> bool {
    let normalized = err_text.to_ascii_lowercase();
    normalized.contains("not opened")
        || normalized.contains("not open")
        || normalized.contains("datachannel is not open")
}

pub(crate) async fn send_packet_batch_over_channel(
    data_channel: &Arc<crate::core::types::RTCDataChannel>,
    packets: &[Bytes],
    timeout_ms: u64,
    packet_batching_enabled: bool,
) -> usize {
    if packets.is_empty() {
        return 0;
    }
    if !data_channel.is_open() {
        return 0;
    }

    if packet_batching_enabled {
        if let Some(coalesced_packet) = build_coalesced_packet_batch(packets) {
            let coalesced_timeout_ms =
                timeout_ms.saturating_add((packets.len() as u64).saturating_mul(4));
            match tokio::time::timeout(
                Duration::from_millis(coalesced_timeout_ms),
                data_channel.send(&coalesced_packet),
            )
            .await
            {
                Ok(Ok(_)) => {
                    return packets.len();
                }
                Ok(Err(e)) => {
                    let err_text = e.to_string();
                    if is_not_open_channel_send_error(&err_text) {
                        trace!(
                            "Coalesced packet send skipped because data channel is not open ({} logical packets).",
                            packets.len()
                        );
                        return 0;
                    }
                    warn!(
                        "Coalesced packet send error ({} logical packets): {:?}. Falling back to sequential dispatch.",
                        packets.len(),
                        e
                    );
                }
                Err(_) => {
                    warn!(
                        "Coalesced packet send timeout after {}ms ({} logical packets). Falling back to sequential dispatch.",
                        coalesced_timeout_ms,
                        packets.len()
                    );
                }
            }
        }
    }

    let mut sent_packets = 0usize;
    for packet in packets {
        match tokio::time::timeout(Duration::from_millis(timeout_ms), data_channel.send(packet))
            .await
        {
            Ok(Ok(_)) => {
                sent_packets += 1;
            }
            Ok(Err(e)) => {
                let err_text = e.to_string();
                if is_not_open_channel_send_error(&err_text) {
                    trace!("Chat/data packet send skipped because data channel is not open.");
                } else {
                    warn!("Chat/data packet send error during batch dispatch: {:?}", e);
                }
                break;
            }
            Err(_) => {
                warn!(
                    "Chat/data packet send timeout during batch dispatch after {}ms",
                    timeout_ms
                );
                break;
            }
        }
    }
    sent_packets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesced_batch_supports_single_packet() {
        let packets = vec![Bytes::from_static(b"single-payload")];
        let coalesced = build_coalesced_packet_batch(&packets).expect("coalesced packet");

        assert_eq!(&coalesced[..4], PACKET_BATCH_MAGIC.as_slice());
        assert_eq!(coalesced[4], PACKET_BATCH_VERSION);

        let packet_count = u16::from_le_bytes([coalesced[5], coalesced[6]]);
        assert_eq!(packet_count, 1);

        let payload_len =
            u32::from_le_bytes([coalesced[7], coalesced[8], coalesced[9], coalesced[10]]) as usize;
        assert_eq!(payload_len, packets[0].len());
        assert_eq!(&coalesced[11..], packets[0].as_ref());
    }
}
