//! Relay configuration defaults.
//!
//! Provides default values for relay server, peer manager, and observer configurations.

use std::time::Duration;

/// Default cluster name
pub const DEFAULT_CLUSTER_NAME: &str = "default";

/// Health check interval between peer checks
pub const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(5);

/// Port for gops debugging server
pub const GOPS_PORT: u16 = 9893;

/// Address for pprof server (profiling)
pub const PPROF_ADDRESS: &str = "localhost";

/// Port for pprof server
pub const PPROF_PORT: u16 = 6062;

/// Retry timeout between reconnection attempts
pub const RETRY_TIMEOUT: Duration = Duration::from_secs(30);

/// Default peer target (unix socket)
pub const DEFAULT_PEER_TARGET: &str = "unix:///var/run/cilium/hubble/peer.sock";

/// Peer service name
pub const PEER_SERVICE_NAME: &str = "hubble-peer";

/// Maximum number of flows to buffer for sorting before sending to client
pub const SORT_BUFFER_MAX_LEN: usize = 100;

/// Drain timeout for sort buffer when not full
pub const SORT_BUFFER_DRAIN_TIMEOUT: Duration = Duration::from_secs(1);

/// Time window for error aggregation
pub const ERROR_AGGREGATION_WINDOW: Duration = Duration::from_secs(10);

/// Interval for checking peer updates during long-running requests
pub const PEER_UPDATE_INTERVAL: Duration = Duration::from_secs(2);

/// gRPC metadata key for relay version
pub const GRPC_METADATA_RELAY_VERSION_KEY: &str = "hubble-relay-version";

/// Default gRPC listen address
pub fn default_listen_address() -> String {
    ":4245".to_string()
}

/// Default health check listen address
pub fn default_health_listen_address() -> String {
    ":4222".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_constants_are_valid() {
        assert!(!DEFAULT_CLUSTER_NAME.is_empty());
        assert!(HEALTH_CHECK_INTERVAL.as_secs() > 0);
        assert!(SORT_BUFFER_MAX_LEN > 0);
        assert!(SORT_BUFFER_DRAIN_TIMEOUT.as_millis() > 0);
        assert!(!PEER_SERVICE_NAME.is_empty());
    }

    #[test]
    fn default_addresses_are_valid() {
        let listen = default_listen_address();
        let health = default_health_listen_address();
        assert!(!listen.is_empty());
        assert!(!health.is_empty());
    }
}
