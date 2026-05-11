//! Hubble Relay: Distributed flow observation with multi-cluster aggregation.
//!
//! The relay enables centralized flow observation across multiple Cilium clusters
//! by aggregating flows from peer nodes with gRPC, flow buffering, and filtering.
//!
//! # Architecture
//!
//! - **PeerManager**: Discovers and manages connections to peer nodes
//! - **Observer**: Collects and filters flows from peers
//! - **PriorityQueue**: Sorts flows by timestamp for consistent ordering
//! - **RelayServer**: Main gRPC server coordinating all components

pub mod defaults;
pub mod observer;
pub mod pool;
pub mod queue;
pub mod server;

pub use observer::{FlowCollector, Observer};
pub use pool::PeerManager;
pub use queue::PriorityQueue;
pub use server::RelayServer;

/// Result type for relay operations
pub type Result<T> = std::result::Result<T, RelayError>;

/// Errors that can occur during relay operations
#[derive(Debug, thiserror::Error)]
pub enum RelayError {
    #[error("peer not available: {0}")]
    PeerNotAvailable(String),

    #[error("failed to collect flows: {0}")]
    FlowCollection(String),

    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_error_display() {
        let err = RelayError::PeerNotAvailable("node-1".to_string());
        assert_eq!(err.to_string(), "peer not available: node-1");
    }
}
