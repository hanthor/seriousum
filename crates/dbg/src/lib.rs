//! # cilium-dbg CLI — Debugging tool for Cilium internals
//!
//! A comprehensive CLI for inspecting Cilium's internal state, including:
//! - Endpoint status and information
//! - BPF map contents (policy, connection tracking, etc.)
//! - Service and load balancer configuration
//! - Network policy inspection
//! - BPF program listings
//!
//! This crate ports the Go cilium-dbg CLI from:
//! https://github.com/cilium/cilium/tree/main/cilium-dbg/cmd

use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod commands;
pub mod output;

/// Error type for dbg CLI operations
#[derive(Debug, Error)]
pub enum Error {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("invalid endpoint ID: {0}")]
    InvalidEndpointId(String),

    #[error("root privilege required: {0}")]
    RootPrivilegeRequired(String),

    #[error("map operation failed: {0}")]
    MapOperationFailed(String),

    #[error("policy error: {0}")]
    PolicyError(String),

    #[error("identity resolution failed: {0}")]
    IdentityResolutionFailed(String),

    #[error("service lookup failed: {0}")]
    ServiceLookupFailed(String),

    #[error("IO error")]
    Io(#[from] io::Error),

    #[error("JSON error")]
    Json(#[from] serde_json::Error),

    #[error("parse error: {0}")]
    ParseError(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

// ============================================================================
// Common Cilium Types (newtype wrappers)
// ============================================================================

/// NumericIdentity represents a security identity
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct NumericIdentity(pub u32);

impl NumericIdentity {
    /// World identity (all traffic from outside the cluster)
    pub const WORLD: Self = Self(1);
    /// Host identity (traffic from/to the host namespace)
    pub const HOST: Self = Self(2);
    /// Unmanaged identity
    pub const UNMANAGED: Self = Self(3);
    /// Health probe identity
    pub const HEALTH: Self = Self(4);
    /// Init identity (during initialization)
    pub const INIT: Self = Self(5);
    /// Local node identity
    pub const LOCAL_NODE: Self = Self(6);
    /// Remote node identity
    pub const REMOTE_NODE: Self = Self(7);
    /// Ingress identity
    pub const INGRESS: Self = Self(8);
    /// World IPv4
    pub const WORLD_IPV4: Self = Self(9);

    /// Minimum cluster-local allocated identity
    pub const MIN_CLUSTER_LOCAL: Self = Self(256);
}

impl std::fmt::Display for NumericIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for NumericIdentity {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(NumericIdentity(s.parse()?))
    }
}

/// EndpointId represents an endpoint's identifier
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct EndpointId(pub u16);

impl std::fmt::Display for EndpointId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for EndpointId {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(EndpointId(s.parse()?))
    }
}

/// ServiceId represents a service identifier
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct ServiceId(pub u32);

impl std::fmt::Display for ServiceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ServiceId {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(ServiceId(s.parse()?))
    }
}

// ============================================================================
// BPF Map Types
// ============================================================================

/// Traffic direction for policy rules
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum TrafficDirection {
    Ingress,
    Egress,
}

impl std::fmt::Display for TrafficDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrafficDirection::Ingress => write!(f, "Ingress"),
            TrafficDirection::Egress => write!(f, "Egress"),
        }
    }
}

impl FromStr for TrafficDirection {
    type Err = Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ingress" | "in" => Ok(TrafficDirection::Ingress),
            "egress" | "eg" => Ok(TrafficDirection::Egress),
            _ => Err(Error::InvalidArgument(format!(
                "invalid traffic direction: {}",
                s
            ))),
        }
    }
}

/// Policy entry representing a rule in the policy map
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyEntry {
    pub policy_id: u32,
    pub traffic_direction: TrafficDirection,
    pub identity: NumericIdentity,
    pub port: u16,
    pub protocol: String,
    pub proxy_port: u16,
    pub bytes: u64,
    pub packets: u64,
    pub is_deny: bool,
}

/// Connection tracking entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionTrackingEntry {
    pub source_ip: String,
    pub dest_ip: String,
    pub source_port: u16,
    pub dest_port: u16,
    pub protocol: String,
    pub state: String,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

/// Endpoint information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    pub id: EndpointId,
    pub ipv4: Option<Ipv4Addr>,
    pub ipv6: Option<Ipv6Addr>,
    pub identity: Option<NumericIdentity>,
    pub state: String,
    pub labels: HashMap<String, String>,
}

impl Endpoint {
    pub fn new(id: EndpointId) -> Self {
        Self {
            id,
            ipv4: None,
            ipv6: None,
            identity: None,
            state: "created".to_string(),
            labels: HashMap::new(),
        }
    }
}

/// Service information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub id: ServiceId,
    pub frontend: String,
    pub service_type: String,
    pub backends: Vec<ServiceBackend>,
}

/// Backend for a service
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceBackend {
    pub address: String,
    pub port: u16,
    pub state: String,
    pub preferred: bool,
}

/// BPF Map information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BpfMapInfo {
    pub name: String,
    pub type_name: String,
    pub key_size: u32,
    pub value_size: u32,
    pub max_entries: u32,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Parse a port/protocol string (e.g., "8080/tcp", "80/udp", "443")
pub fn parse_port_protocol(s: &str) -> Result<(u16, String)> {
    let parts: Vec<&str> = s.split('/').collect();
    if parts.is_empty() || parts.len() > 2 {
        return Err(Error::ParseError(format!(
            "invalid port/protocol format: {}",
            s
        )));
    }

    let port: u16 = parts[0]
        .parse()
        .map_err(|_| Error::ParseError(format!("invalid port: {}", parts[0])))?;

    let protocol = if parts.len() == 2 {
        parts[1].to_lowercase()
    } else {
        "tcp".to_string()
    };

    match protocol.as_str() {
        "tcp" | "udp" | "sctp" | "icmp" | "icmpv6" | "any" => Ok((port, protocol)),
        _ => Err(Error::ParseError(format!("invalid protocol: {}", protocol))),
    }
}

/// Format labels as "source:key=value" pairs
pub fn format_label(source: &str, key: &str, value: &str) -> String {
    format!("{source}:{key}={value}")
}

/// Parse Cilium label format "source:key=value"
pub fn parse_label(label: &str) -> Result<(String, String, String)> {
    let parts: Vec<&str> = label.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(Error::ParseError(format!(
            "invalid label format: {} (expected source:key=value)",
            label
        )));
    }

    let source = parts[0].to_string();
    let kv: Vec<&str> = parts[1].splitn(2, '=').collect();

    if kv.is_empty() {
        return Err(Error::ParseError(format!(
            "invalid label format: {} (expected key=value)",
            label
        )));
    }

    let key = kv[0].to_string();
    let value = if kv.len() > 1 {
        kv[1].to_string()
    } else {
        String::new()
    };

    Ok((source, key, value))
}

/// Check if running with root privileges (Unix-specific)
#[cfg(unix)]
pub fn is_root() -> bool {
    unsafe { libc::getuid() == 0 }
}

#[cfg(not(unix))]
pub fn is_root() -> bool {
    false // Always false on non-Unix systems
}

/// Require root privilege for an operation
pub fn require_root(operation: &str) -> Result<()> {
    if !is_root() {
        return Err(Error::RootPrivilegeRequired(format!(
            "{} requires root privilege",
            operation
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_numeric_identity_constants() {
        assert_eq!(NumericIdentity::WORLD.0, 1);
        assert_eq!(NumericIdentity::HOST.0, 2);
        assert_eq!(NumericIdentity::LOCAL_NODE.0, 6);
        assert_eq!(NumericIdentity::MIN_CLUSTER_LOCAL.0, 256);
    }

    #[test]
    fn test_numeric_identity_display() {
        let id = NumericIdentity(42);
        assert_eq!(id.to_string(), "42");
    }

    #[test]
    fn test_numeric_identity_parse() {
        let id: NumericIdentity = "256".parse().unwrap();
        assert_eq!(id.0, 256);
    }

    #[test]
    fn test_endpoint_id_creation() {
        let ep_id = EndpointId(1234);
        assert_eq!(ep_id.0, 1234);
        assert_eq!(ep_id.to_string(), "1234");
    }

    #[test]
    fn test_service_id_creation() {
        let svc_id = ServiceId(5678);
        assert_eq!(svc_id.0, 5678);
        assert_eq!(svc_id.to_string(), "5678");
    }

    #[test]
    fn test_traffic_direction_parse() {
        let ingress: TrafficDirection = "ingress".parse().unwrap();
        assert_eq!(ingress, TrafficDirection::Ingress);

        let egress: TrafficDirection = "egress".parse().unwrap();
        assert_eq!(egress, TrafficDirection::Egress);

        let result: std::result::Result<TrafficDirection, _> = "invalid".parse();
        assert!(result.is_err());
    }

    #[test]
    fn test_traffic_direction_display() {
        assert_eq!(TrafficDirection::Ingress.to_string(), "Ingress");
        assert_eq!(TrafficDirection::Egress.to_string(), "Egress");
    }

    #[test]
    fn test_parse_port_protocol_with_protocol() {
        let (port, proto) = parse_port_protocol("8080/tcp").unwrap();
        assert_eq!(port, 8080);
        assert_eq!(proto, "tcp");
    }

    #[test]
    fn test_parse_port_protocol_default() {
        let (port, proto) = parse_port_protocol("443").unwrap();
        assert_eq!(port, 443);
        assert_eq!(proto, "tcp");
    }

    #[test]
    fn test_parse_port_protocol_udp() {
        let (port, proto) = parse_port_protocol("53/udp").unwrap();
        assert_eq!(port, 53);
        assert_eq!(proto, "udp");
    }

    #[test]
    fn test_parse_port_protocol_invalid_port() {
        let result = parse_port_protocol("invalid/tcp");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_port_protocol_invalid_protocol() {
        let result = parse_port_protocol("8080/xyz");
        assert!(result.is_err());
    }

    #[test]
    fn test_format_label() {
        let label = format_label("k8s", "app", "frontend");
        assert_eq!(label, "k8s:app=frontend");
    }

    #[test]
    fn test_parse_label_valid() {
        let (source, key, value) = parse_label("k8s:app=frontend").unwrap();
        assert_eq!(source, "k8s");
        assert_eq!(key, "app");
        assert_eq!(value, "frontend");
    }

    #[test]
    fn test_parse_label_no_value() {
        let (source, key, value) = parse_label("k8s:app").unwrap();
        assert_eq!(source, "k8s");
        assert_eq!(key, "app");
        assert_eq!(value, "");
    }

    #[test]
    fn test_parse_label_invalid_no_source() {
        let result = parse_label("app=frontend");
        assert!(result.is_err());
    }

    #[test]
    fn test_endpoint_creation() {
        let ep = Endpoint::new(EndpointId(42));
        assert_eq!(ep.id.0, 42);
        assert_eq!(ep.state, "created");
        assert!(ep.labels.is_empty());
        assert_eq!(ep.ipv4, None);
    }

    #[test]
    fn test_service_creation() {
        let svc = Service {
            id: ServiceId(1),
            frontend: "10.0.0.1:80".to_string(),
            service_type: "ClusterIP".to_string(),
            backends: vec![],
        };
        assert_eq!(svc.id.0, 1);
        assert_eq!(svc.service_type, "ClusterIP");
    }

    #[test]
    fn test_policy_entry_creation() {
        let entry = PolicyEntry {
            policy_id: 1,
            traffic_direction: TrafficDirection::Ingress,
            identity: NumericIdentity::WORLD,
            port: 80,
            protocol: "tcp".to_string(),
            proxy_port: 0,
            bytes: 1000,
            packets: 50,
            is_deny: false,
        };
        assert_eq!(entry.traffic_direction, TrafficDirection::Ingress);
        assert!(!entry.is_deny);
    }

    #[test]
    fn test_connection_tracking_entry() {
        let entry = ConnectionTrackingEntry {
            source_ip: "10.0.0.1".to_string(),
            dest_ip: "10.0.0.2".to_string(),
            source_port: 12345,
            dest_port: 80,
            protocol: "tcp".to_string(),
            state: "ESTABLISHED".to_string(),
            bytes_sent: 5000,
            bytes_received: 10000,
        };
        assert_eq!(entry.source_ip, "10.0.0.1");
        assert_eq!(entry.state, "ESTABLISHED");
    }
}
