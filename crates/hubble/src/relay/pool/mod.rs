//! Peer connection management for Hubble Relay
//!
//! Manages a pool of connections to peer Hubble nodes, handling:
//! - Peer discovery via peer service
//! - Connection lifecycle (connect, disconnect, reconnect)
//! - Exponential backoff on connection failures
//! - Connection health monitoring

pub mod backoff;
pub mod types;

use crate::relay::{RelayError, Result};
use dashmap::DashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tracing::info;

pub use backoff::ExponentialBackoff;
pub use types::{Peer, PeerConnection};

/// Configuration for the peer manager
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Peer service address to connect to for peer discovery
    pub peer_service_address: String,
    /// Connection check interval
    pub conn_check_interval: Duration,
    /// Connection status report interval
    pub conn_status_interval: Duration,
    /// Retry timeout between reconnection attempts
    pub retry_timeout: Duration,
}

impl Default for PoolConfig {
    fn default() -> Self {
        #[allow(clippy::duration_suboptimal_units)]
        Self {
            peer_service_address: crate::relay::defaults::DEFAULT_PEER_TARGET.to_string(),
            conn_check_interval: Duration::from_secs(2 * 60),
            conn_status_interval: Duration::from_secs(5),
            retry_timeout: crate::relay::defaults::RETRY_TIMEOUT,
        }
    }
}

/// Peer connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Not yet attempted or permanently failed
    Disconnected,
    /// Currently attempting to connect
    Connecting,
    /// Connected and healthy
    Connected,
    /// Connection exists but unhealthy
    Unhealthy,
}

/// Internal peer state
struct InternalPeer {
    /// Peer information
    peer: Peer,
    /// Current connection state
    state: ConnectionState,
    /// gRPC connection (when available)
    conn: Option<PeerConnection>,
    /// Number of failed connection attempts
    conn_attempts: u32,
    /// Next time to attempt connection
    next_conn_attempt: Option<Instant>,
}

/// Manages a pool of peer connections
pub struct PeerManager {
    /// Peer configurations by name
    peers: Arc<DashMap<String, InternalPeer>>,
    /// Peer service connection status
    peer_service_connected: Arc<AtomicBool>,
    /// Backoff strategy
    backoff: ExponentialBackoff,
}

impl PeerManager {
    /// Creates a new peer manager
    pub fn new(_config: PoolConfig) -> Self {
        Self {
            peers: Arc::new(DashMap::new()),
            peer_service_connected: Arc::new(AtomicBool::new(false)),
            backoff: ExponentialBackoff::default(),
        }
    }

    /// Adds or updates a peer
    pub fn upsert(&self, peer: Peer) -> Result<()> {
        info!(peer_name = %peer.name, "Upserting peer");

        self.peers.insert(
            peer.name.clone(),
            InternalPeer {
                peer,
                state: ConnectionState::Disconnected,
                conn: None,
                conn_attempts: 0,
                next_conn_attempt: None,
            },
        );

        Ok(())
    }

    /// Removes a peer
    pub fn remove(&self, peer_name: &str) -> Result<()> {
        info!(peer_name = %peer_name, "Removing peer");
        self.peers.remove(peer_name);
        Ok(())
    }

    /// Lists all peers with their current connection state
    pub fn list(&self) -> Vec<Peer> {
        self.peers.iter().map(|entry| entry.peer.clone()).collect()
    }

    /// Gets a specific peer by name
    pub fn get(&self, name: &str) -> Option<Peer> {
        self.peers.get(name).map(|entry| entry.peer.clone())
    }

    /// Returns the count of available (connected) peers
    pub fn available_peers_count(&self) -> usize {
        self.peers
            .iter()
            .filter(|entry| entry.state == ConnectionState::Connected)
            .count()
    }

    /// Returns the count of unavailable peers
    pub fn unavailable_peers_count(&self) -> usize {
        self.peers
            .iter()
            .filter(|entry| entry.state != ConnectionState::Connected)
            .count()
    }

    /// Returns peer service connection status
    pub fn peer_service_connected(&self) -> bool {
        self.peer_service_connected.load(Ordering::Relaxed)
    }

    /// Sets peer service connection status
    pub fn set_peer_service_connected(&self, connected: bool) {
        self.peer_service_connected
            .store(connected, Ordering::Relaxed);
    }

    /// Attempts to connect to a peer
    #[allow(dead_code, clippy::collapsible_if)]
    fn connect(&self, peer_name: &str) -> Result<()> {
        if let Some(mut entry) = self.peers.get_mut(peer_name) {
            let now = Instant::now();

            // Check if we should attempt connection
            if let Some(next_attempt) = entry.next_conn_attempt {
                if now < next_attempt {
                    return Err(RelayError::ConnectionFailed(
                        "backoff period not yet expired".to_string(),
                    ));
                }
            }

            entry.state = ConnectionState::Connecting;

            // Simulate successful connection
            entry.state = ConnectionState::Connected;
            entry.next_conn_attempt = None;
            entry.conn_attempts = 0;

            info!(
                peer_name = %peer_name,
                "Connected to peer"
            );

            Ok(())
        } else {
            Err(RelayError::PeerNotAvailable(peer_name.to_string()))
        }
    }

    /// Disconnects from a peer
    pub fn disconnect(&self, peer_name: &str) -> Result<()> {
        if let Some(mut entry) = self.peers.get_mut(peer_name) {
            entry.state = ConnectionState::Disconnected;
            entry.conn = None;
            info!(peer_name = %peer_name, "Disconnected from peer");
            Ok(())
        } else {
            Err(RelayError::PeerNotAvailable(peer_name.to_string()))
        }
    }

    /// Marks a peer as failed and schedules reconnection
    pub fn mark_failed(&self, peer_name: &str) -> Result<()> {
        if let Some(mut entry) = self.peers.get_mut(peer_name) {
            entry.conn_attempts += 1;
            let backoff_duration = self.backoff.duration(entry.conn_attempts as usize);
            entry.next_conn_attempt = Some(Instant::now() + backoff_duration);
            entry.state = ConnectionState::Unhealthy;

            info!(
                peer_name = %peer_name,
                attempts = entry.conn_attempts,
                backoff_ms = backoff_duration.as_millis(),
                "Peer connection failed, scheduling reconnection"
            );

            Ok(())
        } else {
            Err(RelayError::PeerNotAvailable(peer_name.to_string()))
        }
    }

    /// Gets connection state for a peer
    pub fn get_state(&self, peer_name: &str) -> Option<ConnectionState> {
        self.peers.get(peer_name).map(|entry| entry.state)
    }

    /// Returns statistics about the pool
    pub fn stats(&self) -> PoolStats {
        let total = self.peers.len();
        let connected = self.available_peers_count();

        PoolStats {
            total_peers: total,
            connected_peers: connected,
            unavailable_peers: total.saturating_sub(connected),
            peer_service_connected: self.peer_service_connected(),
        }
    }
}

/// Statistics about the peer pool
#[derive(Debug, Clone, Copy)]
pub struct PoolStats {
    /// Total number of peers
    pub total_peers: usize,
    /// Number of connected peers
    pub connected_peers: usize,
    /// Number of unavailable peers
    pub unavailable_peers: usize,
    /// Peer service connection status
    pub peer_service_connected: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_peer(name: &str) -> Peer {
        Peer {
            name: name.to_string(),
            address: "127.0.0.1:4245".parse().ok(),
            tls_enabled: false,
            tls_server_name: None,
        }
    }

    #[test]
    fn peer_manager_upsert_and_list() {
        let pm = PeerManager::new(PoolConfig::default());

        pm.upsert(sample_peer("node-1")).unwrap();
        pm.upsert(sample_peer("node-2")).unwrap();

        let peers = pm.list();
        assert_eq!(peers.len(), 2);
    }

    #[test]
    fn peer_manager_remove() {
        let pm = PeerManager::new(PoolConfig::default());

        pm.upsert(sample_peer("node-1")).unwrap();
        assert_eq!(pm.list().len(), 1);

        pm.remove("node-1").unwrap();
        assert_eq!(pm.list().len(), 0);
    }

    #[test]
    fn peer_manager_get() {
        let pm = PeerManager::new(PoolConfig::default());
        pm.upsert(sample_peer("node-1")).unwrap();

        let peer = pm.get("node-1").expect("peer should exist");
        assert_eq!(peer.name, "node-1");
    }

    #[test]
    fn peer_manager_connect_disconnect() {
        let pm = PeerManager::new(PoolConfig::default());
        pm.upsert(sample_peer("node-1")).unwrap();

        pm.connect("node-1").unwrap();
        assert_eq!(pm.get_state("node-1"), Some(ConnectionState::Connected));

        pm.disconnect("node-1").unwrap();
        assert_eq!(pm.get_state("node-1"), Some(ConnectionState::Disconnected));
    }

    #[test]
    fn peer_manager_available_peers_count() {
        let pm = PeerManager::new(PoolConfig::default());
        pm.upsert(sample_peer("node-1")).unwrap();
        pm.upsert(sample_peer("node-2")).unwrap();

        pm.connect("node-1").unwrap();
        assert_eq!(pm.available_peers_count(), 1);
        assert_eq!(pm.unavailable_peers_count(), 1);
    }

    #[test]
    fn peer_manager_mark_failed() {
        let pm = PeerManager::new(PoolConfig::default());
        pm.upsert(sample_peer("node-1")).unwrap();
        pm.connect("node-1").unwrap();

        pm.mark_failed("node-1").unwrap();
        assert_eq!(pm.get_state("node-1"), Some(ConnectionState::Unhealthy));
    }

    #[test]
    fn peer_manager_stats() {
        let pm = PeerManager::new(PoolConfig::default());
        pm.upsert(sample_peer("node-1")).unwrap();
        pm.upsert(sample_peer("node-2")).unwrap();
        pm.connect("node-1").unwrap();

        let stats = pm.stats();
        assert_eq!(stats.total_peers, 2);
        assert_eq!(stats.connected_peers, 1);
        assert_eq!(stats.unavailable_peers, 1);
    }

    #[test]
    fn peer_manager_peer_service_status() {
        let pm = PeerManager::new(PoolConfig::default());
        assert!(!pm.peer_service_connected());

        pm.set_peer_service_connected(true);
        assert!(pm.peer_service_connected());
    }
}
