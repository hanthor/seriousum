//! Main Hubble Relay Server
//!
//! Coordinates peer management, flow observation, and serves the gRPC API.

use crate::relay::{
    defaults, observer::Observer, pool::PeerManager, pool::PoolConfig, queue::PriorityQueue,
    Result,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::info;

/// Configuration for the relay server
#[derive(Debug, Clone)]
pub struct RelayConfig {
    /// Address to listen on for gRPC
    pub listen_address: String,
    /// Address to listen on for health checks
    pub health_listen_address: String,
    /// Address to listen on for metrics
    pub metrics_listen_address: Option<String>,
    /// Configuration for the peer pool
    pub pool_config: PoolConfig,
    /// Sort buffer configuration
    pub sort_buffer_max_len: usize,
    /// Drain timeout for sort buffer
    pub sort_buffer_drain_timeout: Duration,
    /// Error aggregation window
    pub error_aggregation_window: Duration,
    /// Peer update interval for follow requests
    pub peer_update_interval: Duration,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            listen_address: defaults::default_listen_address(),
            health_listen_address: defaults::default_health_listen_address(),
            metrics_listen_address: None,
            pool_config: PoolConfig::default(),
            sort_buffer_max_len: defaults::SORT_BUFFER_MAX_LEN,
            sort_buffer_drain_timeout: defaults::SORT_BUFFER_DRAIN_TIMEOUT,
            error_aggregation_window: defaults::ERROR_AGGREGATION_WINDOW,
            peer_update_interval: defaults::PEER_UPDATE_INTERVAL,
        }
    }
}

/// Health status of the relay
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    /// Relay is operational
    Serving,
    /// Relay is not ready
    NotServing,
    /// Relay is shutting down
    Unknown,
}

/// Main Hubble Relay Server
pub struct RelayServer {
    /// Configuration
    config: RelayConfig,
    /// Peer manager
    peer_manager: Arc<PeerManager>,
    /// Flow observer
    observer: Arc<Observer>,
    /// Priority queue for flow sorting
    priority_queue: Arc<RwLock<PriorityQueue>>,
    /// Health status
    health_status: Arc<RwLock<HealthStatus>>,
    /// Server running flag
    running: Arc<RwLock<bool>>,
}

impl RelayServer {
    /// Creates a new relay server
    pub fn new(config: RelayConfig) -> Self {
        let peer_manager = Arc::new(PeerManager::new(config.pool_config.clone()));
        let observer = Arc::new(Observer::new(
            config.sort_buffer_max_len,
            config.sort_buffer_drain_timeout,
        ));
        let priority_queue = Arc::new(RwLock::new(PriorityQueue::new(config.sort_buffer_max_len)));

        Self {
            config,
            peer_manager,
            observer,
            priority_queue,
            health_status: Arc::new(RwLock::new(HealthStatus::NotServing)),
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Gets the peer manager
    pub fn peer_manager(&self) -> Arc<PeerManager> {
        self.peer_manager.clone()
    }

    /// Gets the observer
    pub fn observer(&self) -> Arc<Observer> {
        self.observer.clone()
    }

    /// Gets the priority queue
    pub fn priority_queue(&self) -> Arc<RwLock<PriorityQueue>> {
        self.priority_queue.clone()
    }

    /// Starts the relay server
    pub async fn start(&self) -> Result<()> {
        info!("Starting Hubble Relay Server");

        let mut health_status = self.health_status.write().await;
        *health_status = HealthStatus::Serving;

        let mut running = self.running.write().await;
        *running = true;

        info!(
            listen_address = %self.config.listen_address,
            health_address = %self.config.health_listen_address,
            "Relay server started"
        );

        Ok(())
    }

    /// Stops the relay server
    pub async fn stop(&self) -> Result<()> {
        info!("Stopping Hubble Relay Server");

        let mut health_status = self.health_status.write().await;
        *health_status = HealthStatus::NotServing;

        let mut running = self.running.write().await;
        *running = false;

        info!("Relay server stopped");

        Ok(())
    }

    /// Returns the current health status
    pub async fn health_status(&self) -> HealthStatus {
        *self.health_status.read().await
    }

    /// Returns whether the server is running
    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }

    /// Updates health status based on peer connectivity
    pub async fn update_health_status(&self) -> Result<()> {
        let stats = self.peer_manager.stats();

        let new_status = if stats.peer_service_connected && stats.connected_peers > 0 {
            HealthStatus::Serving
        } else {
            HealthStatus::NotServing
        };

        let mut health_status = self.health_status.write().await;
        *health_status = new_status;

        info!(
            status = ?new_status,
            connected_peers = stats.connected_peers,
            total_peers = stats.total_peers,
            "Updated health status"
        );

        Ok(())
    }

    /// Returns server statistics
    pub async fn stats(&self) -> ServerStats {
        let pool_stats = self.peer_manager.stats();
        let flows = match self.observer.collector().get_flows().await {
            Ok(flows) => flows.len(),
            Err(_) => 0,
        };
        let health_status = *self.health_status.read().await;

        ServerStats {
            total_peers: pool_stats.total_peers,
            connected_peers: pool_stats.connected_peers,
            unavailable_peers: pool_stats.unavailable_peers,
            total_flows_collected: flows,
            peer_service_connected: pool_stats.peer_service_connected,
            health_status,
        }
    }
}

/// Statistics about the relay server
#[derive(Debug, Clone, Copy)]
pub struct ServerStats {
    /// Total number of peers
    pub total_peers: usize,
    /// Number of connected peers
    pub connected_peers: usize,
    /// Number of unavailable peers
    pub unavailable_peers: usize,
    /// Total flows collected
    pub total_flows_collected: usize,
    /// Peer service connection status
    pub peer_service_connected: bool,
    /// Current health status
    pub health_status: HealthStatus,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_config_default() {
        let config = RelayConfig::default();
        assert!(!config.listen_address.is_empty());
        assert!(!config.health_listen_address.is_empty());
        assert_eq!(config.sort_buffer_max_len, defaults::SORT_BUFFER_MAX_LEN);
    }

    #[tokio::test]
    async fn relay_server_creation() {
        let server = RelayServer::new(RelayConfig::default());
        assert!(!server.is_running().await);
    }

    #[tokio::test]
    async fn relay_server_start_stop() {
        let server = RelayServer::new(RelayConfig::default());

        server.start().await.unwrap();
        assert!(server.is_running().await);
        assert_eq!(server.health_status().await, HealthStatus::Serving);

        server.stop().await.unwrap();
        assert!(!server.is_running().await);
        assert_eq!(server.health_status().await, HealthStatus::NotServing);
    }

    #[tokio::test]
    async fn relay_server_health_status_update() {
        let config = RelayConfig::default();
        let server = RelayServer::new(config);

        server.update_health_status().await.unwrap();
        // Without any connected peers, should be NotServing
        assert_eq!(server.health_status().await, HealthStatus::NotServing);
    }

    #[tokio::test]
    async fn relay_server_stats() {
        let server = RelayServer::new(RelayConfig::default());

        let stats = server.stats().await;
        assert_eq!(stats.total_peers, 0);
        assert_eq!(stats.connected_peers, 0);
        assert_eq!(stats.total_flows_collected, 0);
    }

    #[tokio::test]
    async fn relay_server_peer_manager_access() {
        let server = RelayServer::new(RelayConfig::default());
        let pm = server.peer_manager();

        let peers = pm.list();
        assert_eq!(peers.len(), 0);
    }

    #[test]
    fn health_status_equality() {
        assert_eq!(HealthStatus::Serving, HealthStatus::Serving);
        assert_ne!(HealthStatus::Serving, HealthStatus::NotServing);
    }
}
