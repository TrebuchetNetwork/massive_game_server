// massive_game_server/server/src/network/webrtc/data_channel.rs

use std::time::Instant;

#[derive(Debug, Clone, Default)]
pub struct DataChannelStats {
    pub messages_sent: u64,
    pub messages_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub last_activity_at: Option<Instant>,
}

impl DataChannelStats {
    pub fn record_send(&mut self, bytes: usize) {
        self.messages_sent += 1;
        self.bytes_sent += bytes as u64;
        self.last_activity_at = Some(Instant::now());
    }

    pub fn record_receive(&mut self, bytes: usize) {
        self.messages_received += 1;
        self.bytes_received += bytes as u64;
        self.last_activity_at = Some(Instant::now());
    }
}
