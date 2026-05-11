//! Types for peer connection pool

use std::net::SocketAddr;

/// Represents a peer (remote Hubble node)
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Peer {
    /// Name of the peer node
    pub name: String,
    /// Network address of the peer
    pub address: Option<SocketAddr>,
    /// Whether TLS is enabled for this peer
    pub tls_enabled: bool,
    /// TLS server name (SNI) for this peer
    pub tls_server_name: Option<String>,
}

impl Peer {
    /// Creates a new peer
    pub fn new(name: impl Into<String>, address: Option<SocketAddr>) -> Self {
        Self {
            name: name.into(),
            address,
            tls_enabled: false,
            tls_server_name: None,
        }
    }

    /// Sets TLS configuration
    pub fn with_tls(mut self, enabled: bool, server_name: Option<String>) -> Self {
        self.tls_enabled = enabled;
        self.tls_server_name = server_name;
        self
    }
}

/// Represents a gRPC connection to a peer
#[derive(Debug, Clone)]
pub struct PeerConnection {
    /// Connection ID
    pub id: String,
    /// Peer address
    pub address: SocketAddr,
}

impl PeerConnection {
    /// Creates a new peer connection
    pub fn new(id: impl Into<String>, address: SocketAddr) -> Self {
        Self {
            id: id.into(),
            address,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peer_creation() {
        let addr: SocketAddr = "127.0.0.1:4245".parse().unwrap();
        let peer = Peer::new("node-1", Some(addr));

        assert_eq!(peer.name, "node-1");
        assert_eq!(peer.address, Some(addr));
        assert!(!peer.tls_enabled);
    }

    #[test]
    fn peer_with_tls() {
        let addr: SocketAddr = "127.0.0.1:4245".parse().unwrap();
        let peer =
            Peer::new("node-1", Some(addr)).with_tls(true, Some("node-1.example.com".to_string()));

        assert!(peer.tls_enabled);
        assert_eq!(peer.tls_server_name, Some("node-1.example.com".to_string()));
    }

    #[test]
    fn peer_connection_creation() {
        let addr: SocketAddr = "127.0.0.1:4245".parse().unwrap();
        let conn = PeerConnection::new("conn-1", addr);

        assert_eq!(conn.id, "conn-1");
        assert_eq!(conn.address, addr);
    }
}
