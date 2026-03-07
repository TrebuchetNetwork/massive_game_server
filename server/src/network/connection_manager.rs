// massive_game_server/server/src/network/connection_manager.rs

use dashmap::DashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    WebRtc,
    Quic,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub peer_id: String,
    pub transport: TransportKind,
    pub connected_at: Instant,
    pub last_seen_at: Instant,
    pub smoothed_rtt_ms: Option<u32>,
}

impl ConnectionInfo {
    pub fn new(peer_id: impl Into<String>, transport: TransportKind) -> Self {
        let now = Instant::now();
        Self {
            peer_id: peer_id.into(),
            transport,
            connected_at: now,
            last_seen_at: now,
            smoothed_rtt_ms: None,
        }
    }
}

#[derive(Default, Clone)]
pub struct ConnectionManager {
    connections: Arc<DashMap<String, ConnectionInfo>>,
}

static SHARED_CONNECTION_MANAGER: OnceLock<ConnectionManager> = OnceLock::new();

pub fn shared_connection_manager() -> &'static ConnectionManager {
    SHARED_CONNECTION_MANAGER.get_or_init(ConnectionManager::default)
}

impl ConnectionManager {
    pub fn upsert(&self, info: ConnectionInfo) {
        self.connections.insert(info.peer_id.clone(), info);
    }

    pub fn touch(&self, peer_id: &str) {
        if let Some(mut entry) = self.connections.get_mut(peer_id) {
            entry.last_seen_at = Instant::now();
        }
    }

    pub fn set_rtt_ms(&self, peer_id: &str, rtt_ms: u32) {
        if let Some(mut entry) = self.connections.get_mut(peer_id) {
            entry.smoothed_rtt_ms = Some(rtt_ms);
            entry.last_seen_at = Instant::now();
        }
    }

    pub fn remove(&self, peer_id: &str) -> Option<ConnectionInfo> {
        self.connections.remove(peer_id).map(|(_, value)| value)
    }

    pub fn get(&self, peer_id: &str) -> Option<ConnectionInfo> {
        self.connections.get(peer_id).map(|entry| entry.clone())
    }

    pub fn contains(&self, peer_id: &str) -> bool {
        self.connections.contains_key(peer_id)
    }

    pub fn len(&self) -> usize {
        self.connections.len()
    }

    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }

    pub fn len_by_transport(&self, transport: TransportKind) -> usize {
        self.connections
            .iter()
            .filter(|entry| entry.transport == transport)
            .count()
    }

    pub fn peer_ids_by_transport(&self, transport: TransportKind) -> Vec<String> {
        self.connections
            .iter()
            .filter(|entry| entry.transport == transport)
            .map(|entry| entry.peer_id.clone())
            .collect()
    }

    pub fn stale_peer_ids(&self, stale_after: Duration) -> Vec<String> {
        let now = Instant::now();
        self.connections
            .iter()
            .filter_map(|entry| {
                let is_stale = now
                    .checked_duration_since(entry.last_seen_at)
                    .unwrap_or(Duration::from_millis(0))
                    >= stale_after;
                if is_stale {
                    Some(entry.peer_id.clone())
                } else {
                    None
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_connections() {
        let manager = ConnectionManager::default();
        manager.upsert(ConnectionInfo::new("p1", TransportKind::WebRtc));
        assert_eq!(manager.len(), 1);
        manager.remove("p1");
        assert_eq!(manager.len(), 0);
    }

    #[test]
    fn stale_peer_ids_returns_idle_connections() {
        let manager = ConnectionManager::default();

        // Insert a connection and manually backdate its last_seen_at
        let mut info = ConnectionInfo::new("stale_peer", TransportKind::WebRtc);
        info.last_seen_at = Instant::now() - Duration::from_secs(200);
        manager.upsert(info);

        // Insert a fresh connection
        manager.upsert(ConnectionInfo::new("fresh_peer", TransportKind::Quic));

        let stale = manager.stale_peer_ids(Duration::from_secs(120));
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0], "stale_peer");
    }

    #[test]
    fn stale_peer_ids_empty_when_all_fresh() {
        let manager = ConnectionManager::default();
        manager.upsert(ConnectionInfo::new("p1", TransportKind::WebRtc));
        manager.upsert(ConnectionInfo::new("p2", TransportKind::Quic));

        let stale = manager.stale_peer_ids(Duration::from_secs(120));
        assert!(stale.is_empty());
    }

    #[test]
    fn touch_resets_last_seen() {
        let manager = ConnectionManager::default();
        let mut info = ConnectionInfo::new("peer1", TransportKind::WebRtc);
        info.last_seen_at = Instant::now() - Duration::from_secs(200);
        manager.upsert(info);

        // Should be stale before touch
        assert_eq!(manager.stale_peer_ids(Duration::from_secs(120)).len(), 1);

        // After touch, should no longer be stale
        manager.touch("peer1");
        assert!(manager.stale_peer_ids(Duration::from_secs(120)).is_empty());
    }

    #[test]
    fn get_returns_cloned_connection_info() {
        let manager = ConnectionManager::default();
        manager.upsert(ConnectionInfo::new("peer1", TransportKind::WebRtc));

        let info = manager.get("peer1").expect("connection info should exist");
        assert_eq!(info.peer_id, "peer1");
        assert_eq!(info.transport, TransportKind::WebRtc);
    }
}
